# seiso

seiso is a Markdown linter for documentation structure and fact ownership,
written in Rust. Its [design](docs/seiso%20设计与实施方案.md) covers stale
snapshots, duplicated definitions, broken local links, and documents that mix
responsibilities.

Development builds expose `seiso parse` to inspect the document model and
configuration resolution. Lint rules and `seiso check` are not available yet.
See [development](docs/development.md) for the implementation boundary and
validation commands.

```sh
cargo run -p seiso_cli -- parse README.md
cargo run -p seiso_cli -- parse --output-format json
```

Run `cargo run -p seiso_cli -- parse --help` for available options.

Licensed under [MIT](LICENSE).
