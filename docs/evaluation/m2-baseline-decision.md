---
kind: adr
---

# Establish the M2 performance baseline

Accepted by the project owner on 2026-09-28.

## Context

M1's hook checks the selected document. M2 follows the planned workspace
semantics: even a path-scoped check discovers, reads, hashes, and indexes all
included documents before filtering reported diagnostics. Comparing these
two workloads under a relative regression limit would reject the required
change in scope.

## Decision

The first M2 revision that passes the absolute performance gates establishes
the M2 baseline. The M1 comparison remains in the benchmark evidence but does
not enforce the relative limit across this transition. Subsequent M2 relative
changes are reported for review under the
[manual evaluation policy](../development.md#performance-and-ecosystem-checks).

The absolute gates are unchanged: cold full checks below one second, warm
full checks below 500 milliseconds, and warm hooks including process startup
below 50 milliseconds. Acceptance uses the standard Linux CI runner and the
fixed 1,000-file, approximately 10 MB workload. Local timings do not establish
CI acceptance.

## Consequences

The manual evaluation workflow enforces the absolute gates and reports
relative changes when a baseline revision is supplied. Preview measurements
remain separate from the stable-rule latency gates. A timeout must remain
visible in the evidence.
