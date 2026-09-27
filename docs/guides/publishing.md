---
kind: howto
---

# Publish seiso

Use [Build and publish distributions](../../.github/workflows/publish.yml) to
publish `seiso` on crates.io, `@scarletkc/seiso` on npm, `seiso` on PyPI, and
a GitHub Release from one release commit. All three package installations
provide the `seiso` executable. The workflow runs only through
`workflow_dispatch`; all upload inputs default to false.

## Prepare the version and release notes

From the repository root, run:

```sh
python scripts/bump_version.py patch --note "Release title"
```

The version argument accepts `patch` (also the default), `minor`, `major`,
or an explicit version such as `1.2.3`, `v1.2.3`, or `1.2.3-rc.1`. Add `--dry-run` to preview
the affected files. The script rejects equal or lower versions, checks the
existing versions for consistency, and plans all edits before writing.

The script updates `package.version` in the root `Cargo.toml`, the `seiso`
entry in `Cargo.lock`, and `npm/seiso/package.json`. Third-party dependency
versions remain unchanged. clap reads the package version for the CLI's
`--version`, and maturin reads it for PyPI. No separate CLI or Python version
literal needs editing. Release versions use `MAJOR.MINOR.PATCH`, optionally
followed by `-alpha.N`, `-beta.N`, or `-rc.N`. Python distribution metadata uses
the corresponding PEP 440 spelling (`1.2.3a1`, `1.2.3b1`, or `1.2.3rc1`);
Cargo, npm, and the executable retain the Cargo spelling. Other suffixes,
including build metadata, are outside this shared registry version format.
From a prerelease, `patch` promotes its version to the final release; use an
explicit version to advance the prerelease number.

Prereleases publish under npm's `next` tag and are marked as prereleases on
GitHub. Stable versions publish under npm's `latest` tag. Version parsing and
Python normalization are shared in [`scripts/versions.py`](../../scripts/versions.py).

`--note` creates `docs/release-notes/VERSION.md` with a `## Release title`
heading. Fill in its body with user-facing changes and migration instructions
before committing. Existing notes are never overwritten. The note is optional,
but a supplied file with an invalid heading or no body fails preflight.

CI appends an automatic `Changelog` to the handwritten note. It lists commits
from the nearest reachable previous version tag through the release commit,
plus a full diff link. When no earlier version tag exists, it lists the full
commit history. Preview the resulting body locally:

```sh
python scripts/github_release.py notes --output target/release-notes.md
```

An existing `vVERSION` tag must point to the selected release commit; a
conflicting tag fails preflight before any upload. Commit the version changes
and completed note together, then validate that release ref.

## Validate and build without publishing

Use Python 3.12 or later and current stable Rust:

```sh
python scripts/release.py check
python -m unittest discover -s scripts -p 'test_*.py'
cargo test --locked
python scripts/release.py crates
```

The last command runs `cargo package --package seiso --locked --registry
crates-io`, including compilation of the packaged sources without uploading. The package
contains the CLI, library modules, embedded rule documentation, and MIT license.
Release validation identifies the root `seiso` package; other workspace packages
may have their own versions and publication settings. Cargo's package verification
checks dependency publishability and compilation, including versioned local dependencies.
The root package's crates.io publication setting is checked only on the crates
path; it does not block Python, npm, or GitHub distribution.
Already published versions are immutable; bump the version before releasing
changed code.

In GitHub Actions, select **Build and publish distributions → Run workflow**
and choose the release branch or tag. Leave all `publish_*` checkboxes unchecked.

The workflow checks version consistency and release notes, runs script and
Rust tests, verifies the Cargo package, builds and exercises Linux x64 and
Windows x64 wheels, creates a source archive, and packs and exercises the npm
executable. Build jobs have no publishing secrets or OIDC permissions and do
not enter publishing environments. This is the complete validation path. A
selected registry upload waits for shared preflight and its required artifacts:

| Selection | Builds and verifies |
| --- | --- |
| `publish_crates` | Cargo package |
| `publish_pypi` | Wheels and source archive |
| `publish_npm` | Wheels and source archive, then the npm package using those binaries |
| `publish_github`, `publish_all`, or no upload selection | All distributions and release notes |

Selections are additive. Script tests and Linux Rust tests run once in shared
preflight; the Windows wheel job also runs Rust tests on Windows. Release notes
and tag checks apply to the full validation and GitHub Release paths.

Download the artifacts and generated release body:

```sh
gh run download RUN_ID -p 'distributions-*' -p npm-package -p cargo-package -p release-notes -D dist/downloaded
```

The npm package requires Node.js 18 or later and contains both native binaries
from the verified wheels, with version and checksum checks before packing.
It has no install-time download or build step. Source installations require Rust.

## Authentication

The workflow uses these GitHub environments and credentials:

| Destination | Environment | Authentication |
| --- | --- | --- |
| npm | `npm` | Trusted Publishing (OIDC), with direct `npm publish` allowed |
| crates.io | `crates-io` | Trusted Publishing (OIDC) for `seiso` |
| PyPI | `pypi` | Trusted Publishing (OIDC) |
| GitHub Release | `github-release` | Built-in `GITHUB_TOKEN` with `contents: write` |

Trusted Publisher configurations must match the repository, `publish.yml`,
and the environment. Registry upload jobs alone receive `id-token: write`.
Build jobs require no registry credentials. Environment deployment rules must
permit the selected release ref.

crates.io authentication uses a temporary token from
`rust-lang/crates-io-auth-action`. No stored registry publishing token is needed.

## Publish and recover a partial release

Run the workflow at the tested release ref and enable **`publish_all`** to
publish npm, crates.io, PyPI, and a GitHub Release in one run. For selected
registries, leave it off and enable `publish_npm`, `publish_crates`, or
`publish_pypi`. Select `publish_github` as well to create the GitHub Release
after the selected uploads succeed. On its own, `publish_github` builds all
distributions and creates the GitHub Release without registry uploads.

The GitHub Release is named `seiso vVERSION`, tags the checked-out commit, and
includes the generated body, wheels, source archive, npm archive, and the
`seiso` crate archive. Registry uploads are independent after shared validation; a failure
in one cannot roll back another. GitHub Release creation waits for all selected
registries to succeed.

Re-run the same release commit to finish a partial release, or select only the
failed destination:

- npm checks the exact package version and skips one that exists. A new version
  uses the tested archive without repacking it.
- crates.io checks the exact `seiso` version in its sparse index and skips an
  existing version. Yanked versions stop the release.
- PyPI retains `skip-existing: true` and uploads missing distribution files.
- An existing GitHub Release keeps its body and assets; retrying uploads only
  missing attachments. A draft release requires manual review before retrying.

Only HTTP 404 means an absent registry resource. Network, authorization,
rate-limit, malformed-response, and publication errors fail the job. If Cargo
times out after uploading, check crates.io before retrying: the upload may have
succeeded. Skipping a duplicate confirms the version exists; it does not prove
its contents match changed local source.

Verify each registry exposes the intended version, check the GitHub tag's
commit and attachments, then install the exact version in a clean environment
and run `seiso --version` and `seiso parse`. Registry publication alone does not
establish the milestone's corpus acceptance criteria.
