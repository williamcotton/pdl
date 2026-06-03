# PDL v0.8 Plan

Status: Complete
Target version: 0.8.0
Owner: PDL maintainers
Related spec: [`PDL_SPEC.md`](PDL_SPEC.md)
Predecessor plan: [`V0_7_PLAN.md`](V0_7_PLAN.md)

## Purpose

PDL v0.8 is a maintenance release after the v0.7 browser demo release. It keeps
the language surface stable and improves recoverable diagnostics for malformed
stage arguments.

## Must

### Syntax Diagnostic Recovery

Status: Complete.

Repair parser diagnostics for malformed stage arguments that previously left
tokens unconsumed without an author-facing error.

Acceptance criteria:

- `sort "total_revenue" des` reports `E1210` on `des` instead of silently
  defaulting to ascending order.
- `filter "status" "completed"` reports a syntax diagnostic on the adjacent
  operand missing its operator.
- `filter "staus" = "completed"` reports the recoverable operator syntax
  diagnostic and, when a schema is available, also reports `E1005` for
  `"staus"`.
- `agg sum("amount") a "total_revenue"` reports the missing `as` diagnostic
  without a secondary quoted-column-name error, recovers the alias, and still
  allows schema-backed diagnostics from earlier recoverable stages.
- Other non-boundary tokens that remain after a parsed main pipeline report
  `E0021`.
- Regression tests cover the parser diagnostics and recovery.

## Deferred

- New stages, formats, commands, editor features, and browser demo expansions
  remain deferred until a maintainer promotes them into a later plan with matching
  spec and test scope.
