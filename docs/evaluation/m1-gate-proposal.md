---
kind: adr
---

# M1 protocol-rule acceptance decision

Status: accepted by the project owner on 2026-09-28. The acceptance applies
only to the three-rule exception below; each promotion still needs its
recorded evidence.

The decision gives `KND002`, `SUP001`, and `SUP002` a protocol-conformance
acceptance route. Their inputs are declarations introduced by seiso itself;
the frozen external corpus contains none. The
[syntax audit](../../corpus/results/m1/syntax-audit.json) records the input
counts, parser fingerprint, and verification method.

The [existing design](../design/history.md#quality-assurance) requires at least
100 labeled natural holdout diagnostics and precision of at least 95% for
each stable rule. M1 also requires every M1 consistency rule to reach stable.
Zero occurrences cannot establish precision or satisfy that sample threshold.

## Accepted decision

Allow only `KND002`, `SUP001`, and `SUP002` to become stable when all of these
conditions hold:

1. A reviewer accepts the rule's explicit protocol and the conformance matrix
   below. Every applicable positive, negative, malformed-input, scope,
   disabled-rule, and incomplete-execution case must pass.
2. Complete diagnostic and suppression results are deterministic; source
   spans and Unicode positions match the original bytes; published examples
   and diagnostic snapshots pass.
3. The unmodified natural holdout emits no unwanted diagnostics for these
   rules. The report preserves the actual sample count and records precision
   as unavailable when no diagnostic is emitted.
4. Constructed cases are reported as conformance tests, separately from
   natural precision. They do not increase its numerator or denominator.

`KND001`, `LNK001`, and every convention rule retain the existing minimum of
100 labeled natural holdout diagnostics and precision of at least 95%.
Each rule still needs its own result; scores cannot be pooled across rules.
This exception does not change the M1 requirement that all M1 consistency
rules reach stable, so approval alone does not complete M1.

## Conformance matrix

| Contract | Existing evidence | Added evidence or remaining condition |
| --- | --- | --- |
| KND002 accepts supported kinds and rejects unknown kinds or frontmatter `generated` | `engine::kind_errors_do_not_fall_back_to_a_configured_exemption`; executable rule examples | `acceptance_contract::kind_vocabulary_and_invalid_yaml_have_distinct_outcomes` covers every supported frontmatter kind, case-sensitive and Unicode invalid values, and distinct KND001 handling for malformed or wrongly typed YAML |
| YAML in examples and ordinary comments is inert | Parser model tests | `acceptance_contract::kind_examples_and_unrelated_comments_do_not_declare_a_kind` checks the resulting rule behavior |
| SUP001 validates full codes, separators, unique codes, one-line reasons, command, and location | `suppression::malformed_declarations_never_suppress_even_when_sup001_is_disabled`; `file_scope_must_precede_all_content`; `missing_or_unsupported_following_block_is_invalid` | Covered by existing protocol fixtures |
| File, next-block, paragraph, nested-list, and table-cell scope | `suppression::file_scope_accepts_frontmatter_and_leading_comments`; `standalone_binds_only_the_next_block`; `inline_scopes_cover_paragraph_list_item_and_table_cell`; `nested_standalone_scope_stays_inside_its_list_item` | Covered by existing protocol fixtures |
| Scope priority, primary locations, and nonrecursive meta-suppression | `suppression::block_scope_takes_credit_before_file_scope`; `only_primary_location_can_match`; `meta_suppressions_do_not_recurse_or_hide_themselves` | `acceptance_contract::overlapping_block_scopes_choose_smallest_then_earliest` checks nested and equal block scopes, one winning hit, and one stale declaration; `equal_file_meta_suppressions_receive_no_circular_credit` locks the nonrecursive result |
| SUP002 distinguishes active, stale, disabled, and incomplete states per code | `suppression::state_is_per_code_and_disabled_or_incomplete_rules_are_not_stale`; `incomplete_rules_can_suppress_known_diagnostics_without_claiming_completion` | `acceptance_contract::suppression_completion_uses_effective_engine_policy` adds selector, per-file ignore, unknown-kind, and generated-kind integration checks |
| Code examples and unrelated comments do not become suppression declarations | `suppression::code_examples_and_unrelated_comments_are_inert` | Natural corpus audit also checks for declaration syntax; complete natural rule evaluation remains required |
| Diagnostic byte ranges, Unicode columns, line endings, and no M1 edits | Rule output snapshots | `acceptance_contract::suppression_diagnostics_preserve_unicode_columns_and_line_endings` checks Chinese, Japanese, emoji, LF, and CRLF; KND002 cases assert the exact frontmatter span |
| Repeated results and public examples | `engine::rule_documentation_examples_execute_the_published_contract`; corpus robustness test | Every added contract case compares the complete serialized result of two evaluations |

The matrix references tests in
[`engine.rs`](../../tests/engine.rs),
[`suppression.rs`](../../tests/suppression.rs), and
[`acceptance_contract.rs`](../../tests/acceptance_contract.rs).
On 2026-09-28, their 27 Windows tests passed. Unix-only filesystem tests were
not executed in that run. This run supports the matrix; it is not a natural
precision result. The [protocol acceptance receipt](../../corpus/results/m1/protocol-acceptance.json)
records the reviewed test sources and validation command. The
[M1 acceptance record](m1-2026-09-28.md) records the natural regression result.

## Alternative considered

Keeping the original gate would leave M1 pending until authentic adopter
repositories provide
enough naturally occurring diagnostics for the required holdout evaluation.
Adding declarations to external documents, creating deliberate mistakes, or
repeating equivalent variants produces constructed cases and cannot fill
that natural sample requirement.

## Application

The canonical design includes the accepted exception. Each promotion remains
bound to its recorded evidence; natural evaluation for all other rules keeps
the existing criterion.
