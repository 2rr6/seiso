---
kind: howto
---

# Publish seiso

Cargo and PyPI use the package name `seiso`. npm uses `@scarletkc/seiso`.
All three installations provide the `seiso` executable.

The Cargo workspace version is the release authority. Keep the npm version
in `npm/seiso/package.json` equal to it; the packaging script rejects mismatches.
PyPI reads the version from Cargo through maturin.

## Validate the source packages

```sh
cargo test --workspace --locked
cargo publish --workspace --dry-run --locked
```

Cargo publishes `seiso` and its `seiso_config`, `seiso_diagnostics`, and
`seiso_md` dependencies. Every crate ships an MIT license file copied from
the repository license; keep those copies synchronized when changing it.

## Build the installation packages

Run the **Build and publish distributions** workflow in GitHub Actions with
`publish_pypi` disabled. It builds and tests Linux x64 and Windows x64 wheels,
a source archive, and an npm archive containing both executables. npm requires
Node.js 18 or later. Source installations require a Rust toolchain.

Download artifacts from that run:

```sh
gh run download RUN_ID -p 'distributions-*' -p npm-package -D dist/downloaded
```

The npm package has no install-time download or build step. Its native binaries
come from the wheels, with version and checksum checks before packing.

## Authorize publishing

For crates.io, sign in with GitHub, verify your email, create an API token
with permission to publish the crates, and pass it to `cargo login`.

For npm, run `npm login` and complete the browser login. Verify the account
with `npm whoami`.

For PyPI, create a pending Trusted Publisher in your account's **Publishing**
settings with these fields:

| Field | Value |
| --- | --- |
| PyPI project | `seiso` |
| GitHub owner | `scarletkc` |
| Repository | `seiso` |
| Workflow filename | `publish.yml` |
| Environment | `pypi` |

## Upload and verify

Publish from the tested release commit. Cargo resolves workspace dependency
order during publication:

```sh
cargo publish --workspace --locked
npm publish PATH_TO_NPM_ARCHIVE --access public
```

For PyPI, run the same GitHub workflow at the release commit with
`publish_pypi` enabled. The publication job waits for the wheel and npm
installation checks to pass.

Verify each registry exposes the intended version, then install that exact
version in a clean environment and run `seiso --version` and `seiso parse`.
Registry publication alone does not establish the milestone's corpus
acceptance criteria.
