---
kind: reference
---

# Repository tree inventory

The inventory records complete repository paths at the revisions in
[`corpus.lock.json`](../corpus.lock.json). Link evaluation uses it to distinguish
missing targets from files outside the selected Markdown scopes.

From the repository root:

```sh
python corpus/inventory.py fetch
python corpus/inventory.py verify
```

Fetching requires an authenticated GitHub CLI. It requests each locked tree by
SHA and rejects truncated responses. Existing inventory locks are immutable:
missing archives are reconstructed against their recorded checksums, and
mismatches fail verification. The operation neither resolves a newer branch
revision nor downloads or executes repository code.

## Stored metadata

[`inventory.lock.json`](../inventory/inventory.lock.json) binds the exact corpus
lock bytes to every source's repository, commit, tree SHA, archive name, SHA-256,
and entry count. Each adjacent deterministic gzip archive contains the source
identity and an `entries` array with `path`, `type`, `mode`, and `sha` fields.
Entries are sorted by those fields and preserve exact spelling and case.

All returned paths are retained, including directories, symlinks, submodules,
duplicate names, and names unsupported by Windows filesystems. Paths remain
metadata and are never materialized as upstream files. Consumers must treat
ambiguous paths and targets below symlinks or submodules as uncertain rather
than infer that the target is absent. Symlink target contents and submodule
trees are outside this inventory.

Every corpus document and license blob must appear in its repository inventory
with the locked Git blob ID. Raw API response caches live under ignored
`data/trees/`. Archive checksums detect changes to the recorded metadata; the
inventory does not independently reconstruct Git tree object hashes from the
API response.
