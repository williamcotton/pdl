# PDL v0.9 Plan

Status: Complete
Target version: 0.9.0
Owner: PDL maintainers
Related spec: [`PDL_SPEC.md`](PDL_SPEC.md)
Predecessor plan: [`V0_8_PLAN.md`](V0_8_PLAN.md)

## Purpose

PDL v0.9 is a maintenance release after the v0.8 diagnostic recovery release. It
keeps the language surface stable and improves parser recovery for missing pipe
tokens before stages. It also tightens diagnostic presentation and crate-boundary
ownership for shared diagnostic primitives.

## Must

### Missing Pipe Stage Recovery

Status: Complete.

Recover when a valid stage starts after a pipeline input or stage without a
preceding `|`.

Acceptance criteria:

- `load "sales.csv"\n  filter "staus" == "completed"` reports the missing pipe
  diagnostic on `filter` instead of a trailing-token diagnostic.
- The recovered `filter` stage remains available to schema-backed semantic
  diagnostics, so `"staus"` also reports `E1005` when a schema is available.
- Regression tests cover parser recovery and driver diagnostics.

### Compact Diagnostic Rendering

Status: Complete.

Render CLI diagnostics with a compact source snippet so clustered parse and
semantic errors are easier to scan.

Acceptance criteria:

- Diagnostic output starts with `file:line:column: error[E0001]: message`.
- Output includes the source line and a caret underline for the primary span.
- Multiple diagnostic blocks are separated by one blank line.
- Caret placement counts non-ASCII source columns correctly.
- Human-readable diagnostic rendering is owned by `pdl-cli`; `pdl-core` exposes
  diagnostic values and source-position helpers, not terminal formatting.

### Core Internal Dependency Boundary

Status: Complete.

Keep `pdl-core` as the foundational crate for shared primitives.

Acceptance criteria:

- `pdl-core` has no dependencies on other PDL internal crates.
- A workspace-boundary regression test fails if an internal PDL dependency is
  added to `pdl-core`.

## Deferred

- New stages, formats, commands, editor features, and browser demo expansions
  remain deferred until a maintainer promotes them into a later plan with matching
  spec and test scope.
