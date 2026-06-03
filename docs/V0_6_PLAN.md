# PDL v0.6 Plan

Status: Complete
Target version: 0.6.0
Owner: PDL maintainers
Related spec: [`PDL_SPEC.md`](PDL_SPEC.md)
Predecessor plan: [`V0_5_PLAN.md`](V0_5_PLAN.md)

## Purpose

PDL v0.6 starts with focused maintenance after the v0.5 architecture hardening
release. The immediate goal is to keep editor, LSP, CLI, and WASM diagnostics
aligned with the existing normative spec before larger language features land.

## Must

### LSP Schema Diagnostics Regression

Status: Complete.

Restore schema-aware unknown-column diagnostics in editor and LSP analysis.

Acceptance criteria:

- `filter "sttus" == "completed"` in a pipeline loaded from a CSV with a
  `status` column produces `E1005` on the misspelled column.
- Correct column references from the loaded CSV schema remain diagnostic-free.
- The fix goes through shared parser, driver/schema facts, semantic analysis,
  editor diagnostics, LSP protocol conversion, and WASM JSON ABI rather than
  VS Code-specific behavior.
- Add regression coverage for the editor diagnostic path, the LSP diagnostic
  conversion surface, and the schema-aware WASM check path touched by the fix.

## Deferred

- New language stages, formats, and commands remain outside this maintenance
  item unless promoted by a later v0.6 plan entry.
