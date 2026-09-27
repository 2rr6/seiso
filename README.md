# seiso

seiso is a Markdown linter for documentation structure and fact ownership,
written in Rust. Its [design](https://github.com/scarletkc/seiso/blob/main/docs/seiso%20设计与实施方案.md) covers stale
snapshots, duplicated definitions, broken local links, and documents that mix
responsibilities.

`seiso parse` inspects the document model and
configuration resolution. Lint rules and `seiso check` are not available yet.
See [development](https://github.com/scarletkc/seiso/blob/main/docs/development.md) for the implementation boundary and
validation commands.

```sh
cargo run -p seiso -- parse README.md
cargo run -p seiso -- parse --output-format json
```

Run `cargo run -p seiso -- parse --help` for available options.

Licensed under [MIT](https://github.com/scarletkc/seiso/blob/main/LICENSE).
