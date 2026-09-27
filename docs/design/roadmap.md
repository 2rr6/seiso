---
kind: plan
---

# Roadmap

This page contains proposed work. Implemented behavior is described by the
[convention](../reference/convention.md), [configuration reference](../reference/configuration.md),
[architecture](../reference/architecture.md), and `seiso rule --all`. The original phased
proposal is preserved through the [design history](history.md).
Milestone numbers describe evaluation stages and do not establish a published
version or require a particular Cargo package layout.

## Milestone evidence

The [M0 record](../evaluation/m0-2026-09-27.md),
[M1 record](../evaluation/m1-2026-09-28.md), and
[M2 record](../evaluation/m2-2026-09-28.md) retain their measured outcomes,
limitations, and decisions. Stage completion and stable-rule promotion remain
separate. The [evaluation policy](../evaluation/policy.md) owns acceptance criteria.

## M3: Heuristic rules and section classification

M3 delivers a heuristic section classifier and section-type annotations, the
preview rules below, edit-history replay, and repair tests. It also recalibrates
existing rule thresholds on tuning data. These identifiers are reserved in this
proposal only; configuration and suppression accept implemented rules from the registry.

| Proposed code | Candidate behavior |
| --- | --- |
| STL002 | Detect deployment-state assertions in long-lived pages, excluding conditional instructions |
| STL004 | Detect unconstrained bare versions in long-lived pages |
| RAT001 | Detect argument-heavy prose before the first procedure step in how-to/reference content |
| ORD001 | Detect a long preamble before a runnable example or ordered procedure; the original candidate thresholds were 30 lines or a table of more than 10 lines |
| ORD002 | Detect troubleshooting, FAQ, or exceptions placed before the main flow in how-to/runbook content |
| MIX001 | Detect section responsibilities that conflict with the document kind |
| VOX002 | Detect headings framed around an excluded scope |
| VOX003 | Detect production-process headings and self-narration outside plans and ADRs |
| EVD001 | Detect evaluative claims without nearby evidence or a source |

Thresholds and phrase lists are hypotheses to calibrate on tuning data. Section
annotations should cover steps, references, rationale, background, and other
content, including false negatives. Every heuristic starts in preview and needs
holdout precision and usage-noise evidence before promotion. Recalibration of
existing rule defaults requires the same separation of tuning and holdout.

M3 is complete when every proposed heuristic has a holdout precision report and
usage-noise report. Each rule's results determine whether it can leave preview
under the [evaluation policy](../evaluation/policy.md); completing the stage does
not promote all rules together.

## M4: Ecosystem

M4 delivers editor and agent integrations and evaluates the optional
classification backend below. Each subproject is accepted independently.

### Editors and agent hooks

Planned integrations include `seiso server` (LSP), a thin VS Code extension,
and hook adapters for Codex CLI, Cursor, and other agents. Each integration
needs a defined input/output contract, passing diagnostic refresh tests,
and latency evidence before acceptance.

### Classification backend experiment

An external classifier may be evaluated after a section-labeled baseline
exists. [Jev](https://openrouter.ai/typesafe/jev-1.13) was a candidate in the
original proposal. The experiment would classify candidate sections, with
deterministic code deciding whether evidence meets a calibrated rule threshold.
Model confidence alone is not a validated threshold. It would not rewrite prose.

Evaluation would record backend/model identity and every input affecting the
answer: paragraph, heading, kind, context, and question. A frozen mode for CI
would consume only accepted recorded answers; absent answers would remain
undetermined. Compare precision, cost, and latency against the same heuristic
baseline before adopting a backend. A trait, new crate, configuration syntax,
or `--judge` option is chosen after the experiment establishes a useful result.
An experiment can conclude without adopting a model backend; retain its
measured result and decision as its acceptance record.

## Open design questions

- Whether projects need custom kinds such as tutorials or specifications.
- Whether versioned documentation trees need automatic comparison domains.
- Whether MDX support justifies an additional parser surface.
- Whether rule explanations should be distributed in Chinese and Japanese.
- Target dates and ownership for future deliverables.

The project's license is recorded in [LICENSE](../../LICENSE). Parser replacement
remains possible behind the document model if measured correctness, maintenance,
or performance needs justify it. Adoption should remain incremental through
path mappings and selectors. Policy review must keep exemptions, ignored paths,
and generated mappings visible so passing checks still have a meaningful scope.
