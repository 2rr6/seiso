---
kind: readme
---

# Evaluation corpus

A commit-pinned corpus of real project documentation for seiso's parser and
rule evaluation. `corpus.lock.json` records every selected Markdown file,
its upstream commit, Git blob ID, byte length, and SHA-256 checksum.

## Reproduce the snapshot

From the repository root, use Python 3.12 or later:

```sh
python corpus/corpus.py fetch
python corpus/corpus.py verify
```

Follow the [parser verification procedure](../docs/corpus.md) to evaluate
the snapshot against the current parser.

See [selection and licensing](docs/selection.md) for the corpus contract,
[updating sources](docs/updating.md) for snapshot maintenance, and
[M0 acceptance](results/m0/acceptance.json) for the initial verification.

The [MIT license](../LICENSE) covers this directory's scripts and metadata.
Upstream documents retain their original licenses.
