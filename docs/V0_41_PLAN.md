# PDL v0.41 Plan

Status: Planned
Target version: 0.41.0
Owner: PDL maintainers
Related spec: [`PDL_SPEC.md`](PDL_SPEC.md)
Predecessor plan: [`V0_40_PLAN.md`](V0_40_PLAN.md)

## Purpose

PDL v0.41 should pick up the post-v0.40 design backlog only where the language
surface, diagnostics, and row/native/WASM parity rules are explicit before code
promotion.

## Must

- Specify union schema extension before implementation.

  Status: Planned.

  Decide whether missing-column null padding, explicit type widening, or both
  belong in PDL. Specify column order, mixed values, diagnostics, and row,
  native, and WASM parity before changing execution.

- Design non-equi joins.

  Status: Planned.

  Define syntax, ordering, null semantics, cardinality, diagnostics, and native
  eligibility. Do not expose incidental backend non-equi behavior.

## Should

- Revisit native text writer and JSON Lines input parity.

  Status: Planned.

  Promote only with byte-for-byte row output parity for CSV/JSON Lines writers
  and deterministic row-equivalent JSON Lines schema inference.

- Revisit `pivot_longer` and `complete` native subsets.

  Status: Planned.

  Promote only narrow subsets with deterministic output order, mixed-value
  behavior, fill semantics, diagnostics, and row/native parity tests.

- Design segmented native planning for binding starts, named outputs, and
  multi-output execution.

  Status: Planned.

  Cover observability, diagnostics, cache boundaries, memory behavior, stdout
  purity, and parity tests before implementation.

## Could

- Evaluate configurable CSV dialect support.
- Evaluate a browser byte IO ABI for binary host-file contents and Arrow IPC
  output.
- Evaluate object-store and remote path support with a dedicated security and IO
  plan.
