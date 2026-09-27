# Development

The Cargo workspace implements the local M0 foundation described in the
[implementation plan](seiso%20设计与实施方案.md#实施路线图). `seiso_md` owns
content-derived document data, `seiso_config` owns configuration and path
policy, `seiso_diagnostics` owns source locations and diagnostic rendering,
and the `seiso` package in `crates/seiso_cli` joins them in the `seiso parse`
inspection command.

## Build and validate

Use a stable Rust toolchain with Cargo, rustfmt, and Clippy.

```sh
cargo build --workspace --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

Tests include source spans in English, Chinese, and Japanese; Markdown
structure; configuration precedence and inheritance; diagnostic snapshots;
and repeatable CLI output. Snapshot changes are reviewed as ordinary Git
diffs. `INSTA_UPDATE=always cargo test --workspace` regenerates snapshots
when an output change is intentional.

## Inspect documents

`seiso parse` discovers the workspace, respects include/exclude patterns and
Git ignore files, and parses selected Markdown files. Each file uses its
nearest configuration. Paths printed in reports are relative to the workspace
root. JSON is written to stdout; input and filesystem errors are reported
separately and return exit code 2. This command inspects structure; it does
not apply lint rules or return lint exit code 1.

```sh
cargo run -p seiso -- parse docs/ --output-format json
cargo run -p seiso -- parse --config seiso.toml README.md
```

`--stdin-filename PATH` reads stdin instead of the file at PATH. PATH must
be inside the workspace and selected by the active configuration. The file
does not need to exist, and its contents are never written to disk.

The document model records frontmatter errors as content facts. A failed
frontmatter declaration has no effective kind, even if a path mapping exists.
The forthcoming KND rules will turn these facts into lint diagnostics.

## Parser boundaries

The parser accepts CommonMark and GFM with YAML frontmatter. TOML-style
`+++` fences remain ordinary Markdown. Source mappings mark synthesized or
ambiguous decoded text with `exact: false`; consumers must use the containing
source range for those fragments.

Some reference labels with leading whitespace remain plain text in the
upstream parser even when a matching definition exists. For example,
`[the docs][ A  B ]` does not resolve to `[a b]: target.md`. The original
text is preserved, but the link is absent from the link collection. The
`markdown_rs_spaced_reference_limitation_retains_original_text` regression
in [document tests](../crates/seiso_md/tests/document.rs) records this boundary.

## Milestone acceptance

The [M0 acceptance record](evaluation/m0-2026-09-27.md) records the pinned
real-document corpus run and its evidence. Use the [corpus procedure](corpus.md)
to repeat it against parser changes. Rule precision and performance are
evaluated separately under the implementation plan's later gates. See
[publishing](publishing.md) for the packaging and release procedure.

The rule engine, suppression evaluation, repository index, parse cache,
and integrations follow the plan's later milestones.
All rule metadata remains preview until the required evaluation establishes
eligibility for stable status.
