# PDL v0.16 Plan

Status: Complete / shipped
Target version: 0.16.0
Owner: PDL maintainers
Related spec: [`PDL_SPEC.md`](PDL_SPEC.md)
Predecessor plan: [`V0_15_PLAN.md`](V0_15_PLAN.md)

## Purpose

PDL v0.16 is the window analytics release after the v0.15 native tabular
format parity release. It promotes row-preserving window expressions into the
existing parser, semantic IR, runtime, editor-service, CLI JSON, examples, and
documentation surfaces without adding a separate query block or merging window
state with `group_by`.

The release thesis is: authors should be able to compute partitioned totals,
running frame metrics, rank/distribution values, row numbers, offsets, and
first/last values inside `mutate` while preserving input row count, stable row
order, stdout discipline, and the existing table-stage model.

## Must

- Promote `CallExpr over (WindowSpec)` syntax.

  Status: Shipped in 0.16.0. The parser accepts window expressions with
  `partition_by`, `order_by`, and `rows between ... and ...` clauses. The
  formatter emits stable single-line `over (...)` specs, AST/CST views preserve
  spans, and malformed clauses recover with stable diagnostics.

- Implement window semantic validation and IR.

  Status: Shipped in 0.16.0. Window expressions are valid in `mutate`
  assignments, are rejected elsewhere with `E1226`, and cannot be nested.
  Analyzer validation covers function arity, rank/distribution `order_by`
  requirements, lag/lead integer offsets, partition/order columns, output
  schema, and parallel mutate assignment semantics. IR and CLI JSON expose
  window specs and frame bounds deterministically.

- Execute a broad first window function set.

  Status: Shipped in 0.16.0. Runtime supports `row_number`, `rank`,
  `dense_rank`, `percent_rank`, `cume_dist`, `lag`, `lead`, `first_value`,
  `last_value`, `count`, `sum`, `mean`, `min`, and `max`. It evaluates windows
  per current table, partitions by explicit keys, uses stable order semantics,
  preserves original row order, and supports explicit row frames for running
  calculations.

- Keep editor and static assets aligned.

  Status: Shipped in 0.16.0. The semantic registry includes window functions
  and clause keywords. Editor completions, semantic tokens, hover traversal,
  optimistic schema hints, and VS Code TextMate highlighting recognize the new
  syntax without moving language logic into the VS Code client.

- Update normative spec, examples, README, release stamps, and tests.

  Status: Shipped in 0.16.0. `docs/PDL_SPEC.md` documents v0.16 window
  semantics and remaining deferrals. Workspace, lockfile, CLI version output,
  VS Code package manifests, browser demo manifests, README release text, and
  manifest versions are aligned to `0.16.0`. Added
  `examples/customer_window_metrics.pdl` plus parser, analyzer, runtime, and
  CLI integration tests.

## Should

- Keep `group_by` and windows separate.

  Status: Shipped in 0.16.0. `group_by` remains state for `agg`; window
  partitions are explicit inside each window expression. Window expressions do
  not consume or mutate grouping state.

- Prefer deterministic defaults over implicit optimizer behavior.

  Status: Shipped in 0.16.0. Omitted `partition_by` means the whole table.
  Omitted `order_by` uses current partition order for functions that allow it.
  Omitted frames use the whole partition; running metrics require explicit
  `rows between ... and ...`. Ties preserve stable input order unless authors
  add tie-breaker columns.

## Deferred

- Configurable CSV dialects.
- Full LSP code actions and cross-document navigation.
- Arrow IPC browser output, virtual browser output sinks, and richer browser
  output controls.
- Window optimizations such as shared window evaluation caches, pushdown, and
  streaming window execution.
- Additional window frame modes such as range frames and exclude clauses.
- Later mutation-focused language work beyond row-preserving window
  expressions.
