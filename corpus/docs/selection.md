---
kind: reference
---

# Corpus selection

`sources.json` declares documentation scopes before parsing. All regular
`.md` and `.markdown` files matching those scopes enter `corpus.lock.json`;
MDX, symlinks, and non-Markdown files are outside this baseline. Empty scopes
are visible in each source's `matched_patterns`, including repositories
that publish most documentation in MDX. The lock is the authoritative file
list; no sampling, file limit, or size exclusion is applied during verification.

Sources are assigned at repository level to tuning and holdout splits.
`sources.json` owns repository membership and split assignments.
`language_focus` describes the selection intent, not a human label for every
sentence. Agent-tooling sources cover agent development and workflow
documentation; their inclusion does not establish whether any particular
page was written by an agent.

Parsing and source mapping checks may run on both splits. Rule thresholds
and semantic heuristics must be developed using the tuning split; diagnostic
precision is evaluated separately on the holdout split. Parser success is
not a rule precision or performance result.

## Source integrity

Downloads read the locked commits on GitHub. Existing files are reused only
when their hashes match. Source files live in the ignored `data/blobs/`
directory under content hashes, preserving original bytes independently of
Windows filename restrictions and checkout line endings. Nothing from the
source repositories is executed.

Each document's lock entry contains its repository-relative path, byte size,
Git blob ID, and SHA-256. The seiso runner checks the bytes again before
parsing and records its parser source and executable fingerprints.

## Upstream licenses

Upstream documents retain their original licenses. The lock records the
upstream license/notice paths and their hashes, including explicit README
paths when licensing is stated inline. GitHub's `license_hint` is advisory;
the pinned upstream license text is the source. Downloaded third-party
documents are kept out of this repository's Git history.

The `claude-code` source is supplementary publicly visible documentation
under the commercial terms identified by its pinned `LICENSE.md`; it is
not classified as an open-source project. Availability and licensing are
recorded separately.
