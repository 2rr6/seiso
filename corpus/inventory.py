"""Fetch or verify complete Git tree metadata at the corpus's locked revisions."""

import argparse
from concurrent.futures import ThreadPoolExecutor
import gzip
import hashlib
import io
import json
from pathlib import Path
import re
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parent
MODES = {"blob": {"100644", "100755", "120000"}, "tree": {"040000"}, "commit": {"160000"}}


def digest(content):
    return hashlib.sha256(content).hexdigest()


def json_bytes(value):
    return (json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")) + "\n").encode("utf-8")


def read_json(path):
    return json.loads(path.read_bytes())


def write_bytes(path, content):
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(dir=path.parent, delete=False) as temporary:
        temporary.write(content)
        name = Path(temporary.name)
    try:
        name.replace(path)
    finally:
        name.unlink(missing_ok=True)


def valid_hash(value):
    return isinstance(value, str) and re.fullmatch(r"[a-f0-9]{40}", value)


def valid_path(value):
    # Names stay in JSON; Windows-reserved characters are never materialized.
    if not isinstance(value, str) or "\0" in value or any(part in {"", ".", ".."} for part in value.split("/")):
        raise ValueError(f"Unsafe Git tree path: {value!r}")


def load_corpus():
    content = (ROOT / "corpus.lock.json").read_bytes()
    lock = json.loads(content)
    if lock.get("schema_version") != 1 or not lock.get("sources"):
        raise ValueError("Unsupported or empty corpus.lock.json")
    ids, repositories = set(), set()
    for source in lock["sources"]:
        if not re.fullmatch(r"[a-z0-9-]+", source["id"]) or source["id"] in ids:
            raise ValueError("Corpus source IDs must be unique safe filenames")
        if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", source["repository"]):
            raise ValueError(f"Invalid GitHub repository: {source['repository']!r}")
        if source["repository"].lower() in repositories:
            raise ValueError("Duplicate repository in corpus lock")
        if not valid_hash(source["commit"]) or not valid_hash(source["tree"]):
            raise ValueError(f"Unpinned corpus source: {source['id']}")
        ids.add(source["id"])
        repositories.add(source["repository"].lower())
    return lock, digest(content)


def identity(source):
    return {key: source[key] for key in ("id", "repository", "commit", "tree")}


def validate_entries(entries, source):
    if not isinstance(entries, list) or not entries:
        raise ValueError(f"Empty Git tree for {source['id']}")
    by_path = {}
    for entry in entries:
        if set(entry) != {"path", "type", "mode", "sha"}:
            raise ValueError(f"Invalid tree entry fields for {source['id']}")
        valid_path(entry["path"])
        if entry["mode"] not in MODES.get(entry["type"], set()) or not valid_hash(entry["sha"]):
            raise ValueError(f"Invalid tree entry for {source['id']}/{entry['path']}")
        by_path.setdefault(entry["path"], []).append(entry)
    for document in source["documents"] + source["licenses"]:
        matches = by_path.get(document["path"], [])
        if not any(entry["type"] == "blob" and entry["mode"] in {"100644", "100755"}
                   and entry["sha"] == document["git_blob"] for entry in matches):
            raise ValueError(f"Tree does not contain locked corpus blob: {source['id']}/{document['path']}")


def normalize_tree(response, source):
    if response.get("sha") != source["tree"]:
        raise ValueError(f"GitHub tree SHA mismatch for {source['id']}")
    if response.get("truncated") is not False:
        raise ValueError(f"Incomplete GitHub tree for {source['id']}; truncated must be false")
    entries = [{key: entry[key] for key in ("path", "type", "mode", "sha")} for entry in response["tree"]]
    validate_entries(entries, source)
    entries.sort(key=lambda entry: (entry["path"], entry["type"], entry["mode"], entry["sha"]))
    return {"schema_version": 1, **identity(source), "entries": entries}


def archive_bytes(payload):
    buffer = io.BytesIO()
    with gzip.GzipFile(fileobj=buffer, filename="", mode="wb", compresslevel=9, mtime=0) as archive:
        archive.write(json_bytes(payload))
    return buffer.getvalue()


def validate_archive(content, source):
    payload = json.loads(gzip.decompress(content))
    if payload.get("schema_version") != 1 or any(payload.get(key) != value for key, value in identity(source).items()):
        raise ValueError(f"Inventory identity mismatch for {source['id']}")
    validate_entries(payload["entries"], source)
    if payload["entries"] != sorted(payload["entries"], key=lambda entry: (entry["path"], entry["type"], entry["mode"], entry["sha"])):
        raise ValueError(f"Unsorted inventory for {source['id']}")
    return payload


def load_inventory_lock(corpus_lock, corpus_hash):
    path = ROOT / "inventory/inventory.lock.json"
    if not path.exists():
        return None
    lock = read_json(path)
    if lock.get("schema_version") != 1 or lock.get("corpus_lock_sha256") != corpus_hash:
        raise ValueError("Inventory lock does not match corpus.lock.json; review the snapshot before changing it")
    expected = {source["id"]: source for source in corpus_lock["sources"]}
    records = lock.get("sources", [])
    if len(records) != len(expected) or {record["id"] for record in records} != expected.keys():
        raise ValueError("Inventory sources differ from corpus.lock.json")
    for record in records:
        source = expected[record["id"]]
        if any(record.get(key) != value for key, value in identity(source).items()):
            raise ValueError(f"Inventory lock identity mismatch for {source['id']}")
        if record.get("archive") != f"{source['id']}.json.gz" or not re.fullmatch(r"[a-f0-9]{64}", record.get("sha256", "")):
            raise ValueError(f"Invalid inventory archive for {source['id']}")
        if type(record.get("entries")) is not int or record["entries"] < 1:
            raise ValueError(f"Invalid inventory entry count for {source['id']}")
    return lock


def check_record(content, source, record):
    if digest(content) != record["sha256"]:
        raise ValueError(f"Inventory SHA-256 mismatch for {source['id']}")
    payload = validate_archive(content, source)
    if len(payload["entries"]) != record["entries"]:
        raise ValueError(f"Inventory entry count mismatch for {source['id']}")
    return payload


def fetch_source(source, record=None):
    path = ROOT / "inventory" / f"{source['id']}.json.gz"
    if path.exists():
        content = path.read_bytes()
        payload = check_record(content, source, record) if record else validate_archive(content, source)
    else:
        cache = ROOT / "data/trees" / f"{source['tree']}.json"
        if cache.exists():
            response = read_json(cache)
        else:
            route = f"repos/{source['repository']}/git/trees/{source['tree']}?recursive=1"
            process = subprocess.run(["gh", "api", route], capture_output=True, check=True, timeout=120)
            response = json.loads(process.stdout)
        payload = normalize_tree(response, source)
        content = archive_bytes(payload)
        if record:
            check_record(content, source, record)
        write_bytes(cache, json_bytes(response))
        write_bytes(path, content)
    return {**identity(source), "archive": path.name, "sha256": digest(content), "entries": len(payload["entries"])}


def fetch():
    corpus_lock, corpus_hash = load_corpus()
    previous = load_inventory_lock(corpus_lock, corpus_hash)
    records = {record["id"]: record for record in previous["sources"]} if previous else {}
    def retrieve(source):
        return fetch_source(source, records.get(source["id"]))
    with ThreadPoolExecutor(max_workers=4) as executor:
        fetched = []
        for record in executor.map(retrieve, corpus_lock["sources"]):
            fetched.append(record)
            print(f"Verified tree {record['id']}: {record['entries']} entries", flush=True)
    if previous is None:
        lock = {"schema_version": 1, "corpus_lock_sha256": corpus_hash, "sources": fetched}
        write_bytes(ROOT / "inventory/inventory.lock.json", (json.dumps(lock, ensure_ascii=False, indent=2) + "\n").encode("utf-8"))
    verify()


def verify():
    corpus_lock, corpus_hash = load_corpus()
    lock = load_inventory_lock(corpus_lock, corpus_hash)
    if lock is None:
        raise ValueError("Missing inventory/inventory.lock.json; run python corpus/inventory.py fetch")
    sources = {source["id"]: source for source in corpus_lock["sources"]}
    count = 0
    for record in lock["sources"]:
        content = (ROOT / "inventory" / record["archive"]).read_bytes()
        count += len(check_record(content, sources[record["id"]], record)["entries"])
    print(f"Verified {count} Git tree entries from {len(sources)} pinned sources", flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["fetch", "verify"])
    args = parser.parse_args()
    try:
        {"fetch": fetch, "verify": verify}[args.command]()
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as error:
        print(f"Inventory verification failed: {error}", file=sys.stderr)
        raise SystemExit(1) from error
