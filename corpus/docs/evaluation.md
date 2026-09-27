---
kind: howto
---

# Evaluate rules

Run from the seiso repository root with the Rust toolchain and Python 3.12
or later. Restore the pinned document bytes and complete file inventories:

```sh
python corpus/corpus.py fetch
python corpus/inventory.py fetch
```

Review document responsibilities before inspecting diagnostic output.
[Kind profiles](kinds.md) record the scope and provenance of those decisions.
Do not assign a genre merely to enable a rule.

## Freeze a run

```sh
python scripts/evaluate_m1.py --output corpus/reports/m1-candidate
```

The evaluator refuses to overwrite a run. It verifies the inventories,
checks each original document hash, builds the locked rule engine, and runs
the same inputs twice. The saved report binds diagnostics to source,
configuration, corpus, executable, and inventory hashes. Link existence
uses case-sensitive Git paths; symlinks and submodule contents are
undetermined. Upstream programs and configuration files are not executed.

## Review diagnostics

Annotate each diagnosis as `tp`, `fp`, or `uncertain`, with a reason and its
diagnostic ID. Store annotation files as `labels-*.json` beside the frozen
report. Bind them to the compressed report's SHA-256 in `report_sha256`.
Record the review method and the IDs inspected individually. Agent review
and independent consistency oracles are identified separately from human
labels.

Use only tuning findings to develop rule behavior. A holdout finding can
prevent promotion; changing a rule in response requires a fresh holdout
evaluation before claiming independent validation.

## Summarize the gates

```sh
python scripts/summarize_m1.py corpus/reports/m1-candidate
```

The summarizer rejects missing, duplicate, and unbound annotations. It
reports tuning and holdout separately, including language/kind groups and
unresolved judgments. Unknown labels cannot improve the promotion score.
Natural precision is unavailable when there are no samples.

An accepted protocol exception is supplied explicitly with `--protocol`.
The acceptance receipt must match the syntax audit and conformance test
sources. Constructed protocol cases never enter natural precision counts.
See the [M1 protocol decision](../../docs/evaluation/m1-gate-proposal.md).

## Cross-file evaluation

```sh
python scripts/evaluate_m2.py --output corpus/reports/m2-candidate
```

Each upstream repository forms its own workspace. The index uses the pinned
Git inventory for physical paths and the original selected Markdown for
anchors and duplicate content. An existing file outside the document sample
has unknown anchors. Symlinks and submodules remain undetermined.

The evaluator reverses source, document, and inventory order for its second
run and requires identical output. It applies single-file and cross-file
diagnostics together before suppression. It records all implemented preview
rules; new rules remain preview until their independent acceptance gates pass.

Review every new rule's diagnoses with the original primary and related
source ranges. Report TP, FP, uncertain, sample count, precision, and
language/kind groups for each split. A rule with no holdout diagnoses has
unavailable precision, rather than a perfect score.

Stage completion and stable promotion are separate decisions. A complete
report may establish that natural samples are absent or that a rule is too
noisy. Keep that rule in preview; constructed cases do not fill the natural
sample requirement. Promotion also needs a review of source diversity and
annotation quality, not just a numeric score.

Compare two reports over the same locked inputs:

```sh
python scripts/ecosystem_diff.py before/diagnostics.json.gz after/diagnostics.json.gz --output corpus/reports/diff
```

The JSON retains every added, removed, and changed diagnosis, including
related locations. The Markdown view limits individual entries for workflow
summaries; the artifact contains the full result. Changing corpus locks or
kind profiles requires a separate review before comparing rule behavior.
