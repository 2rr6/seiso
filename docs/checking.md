---
kind: howto
---

# Check Markdown

Run checks from the document workspace:

```sh
seiso init
seiso check
```

Review the suggested kind mappings in `seiso.toml`. Add `kind` frontmatter to
documents that need a different role. Use `seiso rule KND001` for an example.

Stable rules are enabled by default. Add `--preview` or `preview = true` in the
configuration to opt into selected preview rules. `seiso rule <CODE>` shows a
rule's status; `seiso policy` shows the rules enabled for each file. The
[development guide](development.md) records the evaluation gates.

## Select files and rules

```sh
seiso check docs README.md
seiso check --select KND,LNK,SUP
seiso check --preview --extend-select VOX001
seiso rule VOX001
```

Paths are relative to the calling directory. Reported filenames are relative to
the workspace root. Explicit paths still respect `.gitignore`, `include`, and
`exclude`; the nearest configuration determines each file's policy.

Every check reads the included workspace documents. Single-file rules run on
the selected files; cross-file rules use the full index and report a diagnosis
when its primary or related location is selected. Checking a renamed heading's
file can therefore report a broken anchor in a document that links to it.

Use `seiso check --help` for command options and `seiso rule --all` for the
implemented rules, their examples, and exceptions. `--select` replaces the
configured selection; `--extend-select` adds to it.

## Check unsaved content

Send UTF-8 Markdown through stdin and name its workspace path:

```sh
seiso check --stdin-filename docs/guide.md < draft.md
```

The content replaces that file for this check. Relative links and configuration
use the named path; the command leaves the file on disk unchanged. The path may
be new, but it must be inside the workspace and included by its policy.

## Consume results

```sh
seiso check --output-format concise
seiso check --output-format json
seiso check --output-format sarif > seiso.sarif
seiso check --output-format github
seiso check --statistics
seiso policy > policy.json
seiso index --dump > index.json
```

Text includes source excerpts. Concise output puts each diagnostic and its
suggestion on one line. JSON is a sorted array of diagnostics; tool errors go to
stderr. Policy JSON includes effective settings, kind resolution, enabled rules,
configuration exclusions, and suppression records. The index dump includes
effective kind, language, domain, anchors, and outgoing links.

SARIF includes primary and related locations and available safe fixes. GitHub
output uses workflow annotations. With `--statistics`, JSON becomes an object
with `diagnostics` and `statistics`; SARIF stores statistics in run properties.
Statistics include rule counts and suppression reasons and states. GitHub
statistics go to stderr so stdout contains only annotation commands.

| Exit code | Meaning |
| --- | --- |
| `0` | The check completed without violations, or `--exit-zero` was used. |
| `1` | The check completed and found violations. |
| `2` | A tool error or incomplete check occurred; any collected diagnostics are still reported. |

`--exit-zero` preserves exit code `2` for errors. A check with no enabled rules
prints a notice to stderr; inspect the policy before using it as a gate.

## Explain an exception

Place a suppression before the relevant block and give a reviewable reason:

```markdown
<!-- seiso: allow LNK001 -- The site generator creates this page. -->
[Generated API](generated/api.md)
```

Use complete rule codes. `seiso rule SUP001` explains accepted syntax and
`seiso rule SUP002` explains how unused suppressions are reported.

## Apply safe fixes

```sh
seiso check --fix
```

The safe fixer removes confirmed unused suppression codes and checks the
result again. It preserves codes that are active, disabled, invalid, or
undetermined, including preview rules that were not enabled. A partially
stale declaration keeps its remaining codes and reason. No fixes are applied
after an incomplete check, and stdin checks cannot use `--fix`.

## Inspect cached results

Checks store content-derived parse data in `.seiso_cache/`. Each run reads and
hashes the sources, then resolves path policy and links against the current
workspace. Moving a file, editing configuration, or deleting a link target
takes effect even when source content was cached.

Use `--no-cache` to bypass cache reads and writes. Missing, corrupt, outdated,
or unwritable cache entries fall back to parsing; diagnostic output is the
same with a cold, warm, or disabled cache.

For editor and submission gates, follow [Integrate checks](integrations.md).
