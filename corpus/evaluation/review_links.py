"""Independently label LNK001 diagnostics against pinned Git tree metadata."""

import argparse
from collections import Counter, defaultdict
import gzip
import hashlib
import html
import json
from pathlib import Path
import posixpath
import re
from urllib.parse import unquote, urlsplit

CORPUS = Path(__file__).resolve().parents[1]


def sha256(content):
    return hashlib.sha256(content).hexdigest()


def markdown_text(value):
    return html.unescape(re.sub(r"\\([!\"#$%&'()*+,\-./:;<=>?@\[\\\]^_`{|}~])", r"\1", value))


def label_key(value):
    return " ".join(value.split()).casefold()


def source_link(raw, link):
    span, destination_span = link["span"], link["destination_span"]
    if destination_span is None:
        return False, "No original destination span is available."
    try:
        use = raw[span["start"]:span["end"]].decode("utf-8")
        destination = raw[destination_span["start"]:destination_span["end"]].decode("utf-8")
        if markdown_text(destination) != link["destination"]:
            return False, "Original destination bytes disagree with the parsed destination."
        if link["reference"] is None:
            before = raw[span["start"]:destination_span["start"]].decode("utf-8")
            after = raw[destination_span["end"]:span["end"]].decode("utf-8")
            if not re.match(r"!?\[", use) or not re.search(r"\]\(\s*<?$", before) or not re.match(r"(?:>|\s|\))", after):
                return False, "Original inline-link delimiters need manual interpretation."
            return True, "Original inline link and destination bytes verified."
        line_start = raw.rfind(b"\n", 0, destination_span["start"]) + 1
        prefix = raw[line_start:destination_span["start"]].decode("utf-8")
        if not prefix.strip() and line_start:
            previous_line = raw.rfind(b"\n", 0, line_start - 1) + 1
            prefix = raw[previous_line:destination_span["start"]].decode("utf-8")
        definition = re.fullmatch(r" {0,3}\[([^\]\n]+)\]:[ \t]*(?:\r?\n[ \t]*)?<?", prefix)
        if not definition:
            return False, "Reference definition needs manual interpretation."
        suffix_label = re.search(r"\[([^\]\n]*)\]$", use)
        if not suffix_label:
            return False, "Reference use needs manual interpretation."
        use_label = suffix_label[1]
        if not use_label:
            full_label = re.fullmatch(r"!?\[([^\]\n]+)\]\[\]", use)
            if not full_label:
                return False, "Collapsed reference label needs manual interpretation."
            use_label = full_label[1]
        if label_key(use_label) != label_key(definition[1]):
            return False, "Reference use and definition labels disagree."
        if label_key(use_label) != label_key(link["reference"]):
            return True, "Original reference use and definition match; parsed reference-key spelling differs, but destination bytes agree."
        return True, "Original reference use, definition label, and destination bytes verified."
    except UnicodeDecodeError:
        return False, "A recorded span cuts a UTF-8 sequence."


def resolve_target(document, destination, entries):
    """Apply the written physical-path contract without the Rust link resolver."""
    if any(character in destination for character in "{}$<>\\"):
        return "uncertain", None, "Template or platform-dependent destination."
    if re.search(r"%(?![0-9A-Fa-f]{2})", destination):
        return "uncertain", None, "Malformed percent escape."
    try:
        url = urlsplit(destination)
        if url.scheme or url.netloc or destination.startswith("//"):
            return "fp", None, "External URL is outside LNK001."
        path = unquote(url.path, encoding="utf-8", errors="strict")
    except (ValueError, UnicodeDecodeError):
        return "uncertain", None, "Destination cannot be decoded unambiguously."
    if not path:
        return "fp", None, "Fragment-only or empty path has no missing file target."
    if any(character in path for character in "\0{}$<>\\"):
        return "uncertain", None, "Decoded destination is dynamic or platform-dependent."
    by_path = defaultdict(list)
    for entry in entries:
        by_path[entry["path"]].append(entry)
    parts = [] if path.startswith("/") else posixpath.dirname(document).split("/")
    parts = [part for part in parts if part]
    for part in path.split("/"):
        prefix = "/".join(parts)
        ancestors = by_path.get(prefix, [])
        if len(ancestors) > 1 or any(entry["mode"] in {"120000", "160000"} for entry in ancestors):
            return "uncertain", prefix, "Traversal encounters a duplicate path, symlink, or submodule."
        if part in {"", "."}:
            continue
        if part == "..":
            if not parts:
                return "uncertain", None, "Traversal leaves the repository workspace."
            parts.pop()
        else:
            parts.append(part)
    target = posixpath.normpath("/".join(parts))
    found = by_path.get(target, [])
    if len(found) > 1 or any(entry["mode"] in {"120000", "160000"} for entry in found):
        return "uncertain", target, "Target is ambiguous, a symlink, or a submodule."
    if target == "." or found:
        return "fp", target, "An exact case-sensitive repository entry exists."
    return "tp", target, "No exact physical target exists in the complete pinned repository tree."


def review(report_path, corpus=CORPUS):
    compressed = report_path.read_bytes()
    report = json.loads(gzip.decompress(compressed))
    corpus_bytes = (corpus / "corpus.lock.json").read_bytes()
    inventory_bytes = (corpus / "inventory/inventory.lock.json").read_bytes()
    if report["corpus_sha256"] != sha256(corpus_bytes) or report["inventory_sha256"] != sha256(inventory_bytes):
        raise ValueError("Report and pinned inputs disagree")
    sources = {source["id"]: source for source in json.loads(corpus_bytes)["sources"]}
    manifests = {record["id"]: record for record in json.loads(inventory_bytes)["sources"]}
    trees = {}
    for source_id, record in manifests.items():
        content = (corpus / "inventory" / record["archive"]).read_bytes()
        if sha256(content) != record["sha256"]:
            raise ValueError(f"Inventory checksum mismatch: {source_id}")
        payload = json.loads(gzip.decompress(content))
        if any(payload[key] != sources[source_id][key] for key in ("id", "repository", "commit", "tree")):
            raise ValueError(f"Inventory identity mismatch: {source_id}")
        trees[source_id] = payload["entries"]
    documents = {(source_id, entry["path"]): entry for source_id, source in sources.items() for entry in source["documents"]}
    files = {(item["source"], item["path"]): item for item in report["files"]}
    labels = []
    for diagnostic in report["diagnostics"]:
        if diagnostic["code"] != "LNK001":
            continue
        key = diagnostic["source"], diagnostic["path"]
        original = documents[key]
        raw = (corpus / "data/blobs" / original["git_blob"]).read_bytes()
        if sha256(raw) != original["sha256"] or sha256(raw) != diagnostic["input_sha256"]:
            raise ValueError(f"Source checksum mismatch: {key}")
        links = [link for link in files[key]["links"] if link["span"] == diagnostic["span"]]
        if len(links) != 1:
            label, target, evidence = "uncertain", None, "Diagnostic does not identify exactly one parsed link."
            destination, verified, source_evidence = None, False, "Source link not uniquely mapped."
        else:
            destination = links[0]["destination"]
            verified, source_evidence = source_link(raw, links[0])
            label, target, evidence = resolve_target(key[1], destination, trees[key[0]])
            if not verified:
                label = "uncertain"
        labels.append({"id": diagnostic["id"], "label": label, "reviewer_kind": "independent_agent_with_oracle",
                       "evidence": source_evidence + " " + evidence, "oracle_target": target,
                       "destination": destination, "split": diagnostic["split"], "source_verified": verified})
    labels.sort(key=lambda item: item["id"])
    counts = {split: dict(sorted(Counter(item["label"] for item in labels if item["split"] == split).items()))
              for split in ("tuning", "holdout")}
    return {"schema_version": 1, "report_sha256": sha256(compressed),
            "reviewer_kind": "independent_agent_with_oracle",
            "method": "Separate Python POSIX path and urllib percent-decoding oracle; original link spans and reference definitions checked against hash-verified source bytes. Complete case-sensitive Git tree metadata is the physical-path authority. No Rust resolver is called. Oracle labels are not human review.",
            "manually_reviewed_sample_ids": [], "counts": counts, "labels": labels}


def apply_manual_review(result, receipt):
    if receipt.get("schema_version") != 1 or receipt.get("report_sha256") != result["report_sha256"]:
        raise ValueError("Manual review receipt belongs to a different diagnostic report")
    sample = receipt["manually_reviewed_sample_ids"]
    additional = receipt["additional_reviewed_ids"]
    expected = sorted(item["id"] for item in result["labels"] if item["split"] == "holdout")[:120]
    if sample != expected or len(set(additional)) != len(additional):
        raise ValueError("Manual review sample does not match the declared deterministic selection")
    by_id = {item["id"]: item for item in result["labels"]}
    if not set(additional).issubset(by_id) or set(additional).intersection(sample):
        raise ValueError("Invalid additional manual review IDs")
    if receipt.get("reviewer_kind") != "independent_agent_with_oracle" or not receipt.get("findings"):
        raise ValueError("Manual review provenance and findings are required")
    for identifier in sample + additional:
        by_id[identifier]["agent_context_reviewed"] = True
    result["manually_reviewed_sample_ids"] = sample
    result["additional_reviewed_ids"] = additional
    result["manual_review"] = {key: value for key, value in receipt.items()
                               if key not in {"schema_version", "report_sha256", "manually_reviewed_sample_ids", "additional_reviewed_ids"}}
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--report", type=Path, default=CORPUS / "results/m1/natural-v1/diagnostics.json.gz")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--manual-review", type=Path, help="Attach a completed agent review receipt bound to this report")
    args = parser.parse_args()
    result = review(args.report)
    if args.manual_review:
        result = apply_manual_review(result, json.loads(args.manual_review.read_bytes()))
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n", encoding="utf-8", newline="\n")
    print(json.dumps(result["counts"], sort_keys=True))


if __name__ == "__main__":
    main()
