# seiso

A Markdown convention and linter for project docs written by AI and read by
humans and agents.

Hardly anyone writes project docs by hand anymore. AI writes most of them,
and coding agents read them as context as often as people do. Both kinds of
reader take the page at its word, so a stale version number or a field list
updated in only one of its three copies misleads all of them. AI writes docs
faster than anyone can review them, and it makes the same few mistakes every
time.

seiso defines one convention for how Markdown in a repository is organized,
so projects can follow the same rules without agreeing on them first, the
way rustfmt settled formatting for Rust code.

## The convention

- Every document declares one kind, such as `howto`, `reference`, or `adr`,
  and holds only what that kind is for. A how-to gives the steps. Why the
  design looks this way belongs in an ADR.
- Each fact has one home. Other pages link to it instead of retelling it.
- Long-lived pages don't record values that change faster than the page,
  such as versions, deployment status, or counts.
- A pointer names a file or symbol, so the reader doesn't have to search for
  what the sentence promised.
- The finished page doesn't address whoever asked for it or narrate how it
  was made.
- Judgment calls a tool can't make are written down with a reason. An
  exception without one is itself a violation.

seiso checks documents against this convention. Each diagnostic says where
the problem is and how to fix it, so an agent can repair the page from
seiso's output alone. seiso doesn't guess whether prose sounds
machine-written, and it leaves formatting and spelling to other tools. The
[convention](https://github.com/scarletkc/seiso/blob/main/docs/convention.md)
defines each kind's contract and the evidence a diagnostic can claim.

## Try it

Run the linter from a source checkout:

```sh
cargo run -p seiso -- check README.md
cargo run -p seiso -- rule KND001
```

Default checks use accepted stable rules; `--preview` adds selected preview
rules. See [checking documents](https://github.com/scarletkc/seiso/blob/main/docs/checking.md)
for configuration and output, [integrations](https://github.com/scarletkc/seiso/blob/main/docs/integrations.md)
for editor hooks and pre-commit, and [development](https://github.com/scarletkc/seiso/blob/main/docs/development.md)
for validation commands. `seiso parse` inspects the document model without
running rules.

The [architecture](https://github.com/scarletkc/seiso/blob/main/docs/architecture.md)
describes command execution; the
[roadmap](https://github.com/scarletkc/seiso/blob/main/docs/roadmap.md)
contains proposed features and links to milestone evidence.

Licensed under [MIT](https://github.com/scarletkc/seiso/blob/main/LICENSE).
