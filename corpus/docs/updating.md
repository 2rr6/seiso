---
kind: howto
---

# Update a corpus snapshot

Preserve the existing lock and acceptance artifacts in Git, then create a
branch in the seiso repository for the next snapshot. Edit
`corpus/sources.json` to set the selection scopes and repository splits.
Review the upstream licensing references.

Remove the working copy of the old `corpus/corpus.lock.json` only after its
revision is saved. With GitHub CLI authenticated, run from the repository root:

```sh
python corpus/corpus.py freeze
python corpus/corpus.py verify
python -m unittest discover -s corpus -p 'test_*.py'
```

Review the complete lock diff: resolved commits, file membership, content
hashes, licensing references, and split changes. Then repeat the
[parser evaluation](../../docs/corpus.md) and retain its new acceptance
artifacts alongside the parser changes in the same review.

After an interrupted initial freeze, resume without moving unchanged source
selections to newer commits:

```sh
python corpus/corpus.py freeze --resume
```

`freeze` refuses to overwrite an existing lock. `fetch` only restores the
committed snapshot and does not re-resolve upstream branch heads.
