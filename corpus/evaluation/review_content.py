# /// script
# requires-python = ">=3.12"
# dependencies = ["PyYAML==6.0.2"]
# ///
"""Independently check KND001 evidence from locked bytes and kind profiles."""

from __future__ import annotations

import argparse
import gzip
import hashlib
import json
from pathlib import Path
import re

import yaml


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def mapping_matches(pattern: str, path: str) -> bool:
    # The frozen profile uses literals and single-component stars only.
    if any(char in pattern for char in "?[]{}\\") or "**" in pattern:
        raise ValueError(f"Unsupported oracle path pattern: {pattern!r}")
    return re.fullmatch(re.escape(pattern).replace(r"\*", "[^/]*"), path) is not None


def frontmatter_kind(raw: bytes) -> tuple[str, str | None, str]:
    lines = raw.decode("utf-8-sig").splitlines()
    if not lines or lines[0] != "---":
        return "absent", None, "The original bytes have no opening YAML frontmatter delimiter."
    end = next((i for i, line in enumerate(lines[1:], 1) if line in {"---", "..."}), None)
    if end is None:
        return "invalid", None, "The opening frontmatter has no closing delimiter."
    try:
        value = yaml.safe_load("\n".join(lines[1:end]))
    except yaml.YAMLError as error:
        mark = getattr(error, "problem_mark", None)
        location = f" at frontmatter line {mark.line + 1}, column {mark.column + 1}" if mark else ""
        return "invalid", None, f"Independent PyYAML parsing rejects the frontmatter{location}."
    if value is None:
        return "missing", None, "The original frontmatter is empty and has no kind field."
    if not isinstance(value, dict):
        return "invalid", None, "The original frontmatter is not a YAML mapping."
    if "kind" not in value:
        return "missing", None, "The original frontmatter has no top-level kind field."
    kind = value["kind"]
    if not isinstance(kind, str) or kind not in {
        "readme", "howto", "reference", "runbook", "adr", "plan", "changelog"
    }:
        return "invalid_kind", None, "The kind field is present but invalid; this requires KND002 review."
    return "resolved", kind, f"The original frontmatter declares the supported kind {kind!r}."


def review(corpus: Path, report_path: Path, profile_path: Path) -> dict:
    packed = report_path.read_bytes()
    report_bytes = gzip.decompress(packed)
    report = json.loads(report_bytes)
    lock_bytes = (corpus / "corpus.lock.json").read_bytes()
    lock = json.loads(lock_bytes)
    profile_bytes = profile_path.read_bytes()
    profile = json.loads(profile_bytes)
    if digest(lock_bytes) != report["corpus_sha256"] or digest(lock_bytes) != profile["corpus_sha256"]:
        raise ValueError("Corpus lock hash does not match report and profile")
    if digest(profile_bytes) != report["kind_profile_sha256"]:
        raise ValueError("Kind profile hash does not match report")
    documents = {(s["id"], d["path"]): (s, d) for s in lock["sources"] for d in s["documents"]}
    labels = []
    counts: dict[str, int] = {}
    for diagnostic in report["diagnostics"]:
        if diagnostic["code"] != "KND001":
            continue
        source, document = documents[(diagnostic["source"], diagnostic["path"])]
        raw = (corpus / "data" / "blobs" / document["git_blob"]).read_bytes()
        if digest(raw) != document["sha256"] or digest(raw) != diagnostic["input_sha256"]:
            raise ValueError(f"Input hash mismatch: {source['id']}/{document['path']}")
        if source["commit"] != diagnostic["commit"]:
            raise ValueError("Diagnostic source commit does not match the lock")
        mappings = [
            entry for entry in profile["profiles"][source["id"]]["mappings"]
            if mapping_matches(entry["path"], document["path"])
        ]
        status, kind, evidence = frontmatter_kind(raw)
        if status == "invalid":
            label = "tp"
            evidence += " Invalid frontmatter prevents a kind from resolving."
        elif status == "invalid_kind":
            label = "fp"
        elif kind is not None or mappings:
            label = "fp"
            if mappings:
                evidence += f" A frozen mapping resolves the kind: {mappings[-1]['path']!r}."
        else:
            label = "tp"
            evidence += " No frozen profile mapping matches this repository-relative path."
        counts[status] = counts.get(status, 0) + 1
        labels.append({
            "id": diagnostic["id"],
            "label": label,
            "evidence": evidence,
            "review_method": "independent_kind_oracle",
            "frontmatter_status": status,
        })
    return {
        "schema_version": 1,
        "report_sha256": digest(packed),
        "report_json_sha256": digest(report_bytes),
        "reviewer_kind": "independent_agent_with_oracle",
        "method": "KND001 consistency check from original bytes and frozen configuration; no seiso parser, rule implementation, or reported effective kind is used as the oracle.",
        "oracle": {
            "script_sha256": digest(Path(__file__).read_bytes()),
            "yaml_parser": f"PyYAML {yaml.__version__} safe_load",
            "mapping_semantics": "Case-sensitive literals and single-component stars; unsupported syntax fails closed.",
            "kind_profile_sha256": digest(profile_bytes),
            "frontmatter_counts": counts,
            "scope": "Field presence and effective mapping consistency only; no judgment of an upstream author's intended kind or the quality of their documentation.",
        },
        "manually_reviewed_sample_ids": [],
        "labels": sorted(labels, key=lambda item: item["id"]),
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--corpus", type=Path, required=True)
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--profile", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    result = review(args.corpus, args.report, args.profile)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n", encoding="utf-8", newline="\n")
    print(f"Reviewed {len(result['labels'])} KND001 diagnostics; manual review is recorded separately.")


if __name__ == "__main__":
    main()
