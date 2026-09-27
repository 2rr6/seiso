---
kind: reference
---

# Evaluation kind profiles

[`evaluation/kinds.json`](../evaluation/kinds.json) assigns document
responsibilities for rule evaluation against the pinned corpus. These are
evaluation assumptions reviewed by an agent, not declarations accepted by
the upstream projects or human annotations.

The profile is tied to the SHA-256 of `corpus.lock.json`. Each source has
ordered path mappings, a responsibility rationale, and the paths of complete
documents read during review. A glob mapping extends the sampled responsibility
to that documentation scope; it does not claim every matching file received
an individual review. `reviewed_unknown_examples` records inspected documents
whose educational or mixed purpose could not be assigned confidently.

## Resolution

Mappings use repository-relative paths and seiso's kind-mapping semantics:
the last matching mapping wins. Original frontmatter takes precedence,
including the existing error behavior for invalid declarations. Evaluation
does not add frontmatter, remove comments, translate text, render site
components, or change the locked source bytes.

An unmapped path stays unknown. It receives only the rule families permitted
for an unknown kind by seiso. Unknown also covers files that have not received
a responsibility review; it is not a verdict on documentation quality.

## Review criteria

The same criteria apply to tuning and holdout sources:

- Project introductions and documentation entry points can be `readme`.
  A chapter named `README.md` does not automatically qualify.
- Concrete installation, configuration, release, and usage procedures can
  be `howto`. A tutorial or guide directory alone does not establish that
  responsibility.
- Symbol definitions, argument contracts, type descriptions, and catalogs
  can be `reference`. Examples may illustrate those definitions.
- Dated release records can be `changelog`.
- Conceptual textbooks, interview-preparation essays, exercises, prompt
  templates, and mixed or unreviewed pages remain unknown unless a narrower
  inspected scope supports one of the existing kinds.

Assignment identifies the page's intended responsibility. It does not
require the page to comply already with every restriction of that kind;
otherwise the selection would exclude the violations being measured.
API rendering stubs remain reference source documents. Their generated
website content is outside this evaluation, and no `generated` exemption
is inferred from the presence of a rendering directive.

## Freeze boundary

The profile was frozen before this review inspected normative diagnostic
output. Evaluation receipts must record its hash alongside the corpus and
rule-source hashes. Changing a mapping after inspecting holdout diagnostics
requires a new, disclosed evaluation; it must not silently replace the
profile behind an existing score.

Agent-reviewed responsibilities and diagnostic judgments must be identified
as such in results. They do not satisfy a human-review requirement or prove
coverage for the unmapped corpus. Kind and language coverage belong in each
evaluation result, where they can be derived from effective resolution.
