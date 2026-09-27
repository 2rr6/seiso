---
kind: howto
---

# Verify the pinned corpus

The [corpus directory](../corpus/README.md) contains the pinned source
manifest and fetch tool. Use Python 3.12 or later and a Rust toolchain
matching this checkout's requirements. Run from the repository root:

```sh
python corpus/corpus.py fetch
python corpus/corpus.py verify
python scripts/run_corpus.py --corpus corpus --report target/corpus-forward.json
python scripts/run_corpus.py --corpus corpus --report target/corpus-reverse.json --reverse
python -c "from pathlib import Path; assert Path('target/corpus-forward.json').read_bytes() == Path('target/corpus-reverse.json').read_bytes()"
```

The runner builds the current parser from the locked Cargo dependencies.
Every locked file is checked against its original bytes and hashes, then
parsed in a separate process. Both CommonMark and GFM are exercised, with
two parses per flavor. The probe validates source ranges, fragment mappings,
section/block references, and serialized-model equality.

The command exits with 0 only when every listed file passes. Missing or
changed inputs, panics, process failures, timeouts, model inconsistencies,
and nondeterministic output are failures. Individual failures are collected
while the remaining files are processed. `--jobs` controls concurrent
processes; `--timeout` bounds each process in seconds.

The JSON report identifies the corpus lock, parser source files, probe
binary, and per-document results by SHA-256. It contains no elapsed times or
timestamps, so reversed discovery order and process scheduling can be
checked by comparing complete reports from the same build.

The corpus directory owns [selection scopes, repository splits, and upstream
license references](../corpus/docs/selection.md), alongside acceptance
artifacts. Source files are fetched by full commit and stored as verified
content blobs in the ignored `corpus/data/` directory. The parser does not
load or execute source-repository configuration or code.

See the [M0 acceptance record](evaluation/m0-2026-09-27.md) for the initial
run. Parser acceptance does not measure rule precision or performance.
