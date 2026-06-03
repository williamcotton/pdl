# PDL v0.12 Plan

Status: Shipped
Target version: 0.12.0
Owner: PDL maintainers
Related spec: [`PDL_SPEC.md`](PDL_SPEC.md)
Predecessor plan: [`V0_11_PLAN.md`](V0_11_PLAN.md)

## Purpose

PDL v0.12 is the first multi-input expansion after the v0.11 row-manipulation
release. It promotes `join` and `union` from deferred syntax to implemented
stages so the canonical Algraf segment-summary example (spec §26.5) and the
binding-driven workflows described throughout the spec become runnable.

The release thesis is: authors should be able to combine two named pipelines
into one deterministic output through `join` or `union`, then continue into the
existing projection, mutation, grouping, aggregation, sorting, distinct, and
stdout flow.

## Must

- Promote `join` from a deferred stage to an implemented multi-input stage.

  Status: Shipped. The parser, CST views, formatter, semantic analyzer, IR, and
  runtime must support `join binding on "key"` and `join binding on ("left",
  "right")` with kinds `inner`, `left`, `right`, `full`, `semi`, and `anti`.
  The default kind is `inner`. Join key types must be checked for compatibility
  and produce `E1208` when incompatible. Invalid join kind tokens produce
  `E1223`. Non-key column collisions follow the default suffix-right policy by
  appending `_right` to the right-side column; collisions that remain after
  suffixing produce `E1207`. Row ordering preserves left input order for
  `inner` and `left`; `right` preserves right input order; `full` orders
  matched rows by left input order followed by unmatched right rows sorted by
  join key. `semi` and `anti` preserve left input order.

- Promote `union` from a deferred stage to an implemented multi-input stage.

  Status: Shipped. `union binding` combines rows from the current table and a
  named binding. The default alignment is by column position; `by_name true`
  aligns by column name. Schemas must be compatible or produce `E1209`. The
  `distinct true` option removes duplicate rows after concatenation using the
  full-row key, retaining the first occurrence in left-then-right order.
  Without `distinct true`, output preserves left rows followed by right rows.

- Add multi-input execution planning.

  Status: Shipped. The execution planner must evaluate referenced bindings in
  dependency order, share results when a binding is referenced more than once,
  and recompute a binding when the planner cannot prove a cached table is
  valid. Binding dependency cycles produce `E1501` with the cycle path in the
  diagnostic message. Unknown binding references produce `E1007` at the join
  or union source position. Binding evaluation remains lazy: a binding that is
  not referenced by the main pipeline or a selected output is not executed.

- Align editor, LSP, WASM, and browser demo behavior through shared Rust
  crates.

  Status: Shipped. Completion offers binding names at `join` and `union`
  source positions, offers `kind` after the join `on` clause, and offers the
  join kind names after `kind`. Hover on a binding reference shows the
  binding's resolved schema. Semantic tokens, schema facts, and TextMate
  highlighting must include the multi-input keywords (`join`, `union`, `on`,
  `kind`, `by_name`) and the join kind names. The WASM browser run ABI must
  accept multiple host-supplied CSV inputs in the existing format-neutral
  host-file map so the browser demo can host both join sides without changing
  the ABI shape.

- Extend the browser demo to multiple host-supplied inputs.

  Status: Shipped. The demo workbench accepts more than one editable
  host-supplied CSV input, route them through the existing host-file map, and
  render a deterministic CSV output for the join example. The demo must not
  add a TypeScript parser or analyzer.

- Add runnable examples with both data and PDL source.

  Status: Shipped. Added `examples/customers.csv`, `examples/sales.csv`, and
  `examples/segment_revenue.pdl` mirroring spec §26.5, plus a small `union`
  example combining two daily extracts. CLI integration tests run the new
  examples with `--stdout-format csv` and assert deterministic output.

- Update the normative spec and release stamps.

  Status: Shipped. `docs/PDL_SPEC.md` describes v0.12 with `join` and
  `union` removed from the "does not yet implement" list in §0 and the §11.11
  and §11.12 stage sections matching the v0.11 pattern used for `mutate` and
  `distinct`. Bump `Cargo.toml`/`Cargo.lock`, CLI version output, VS Code
  package manifests, and browser demo package manifests to `0.12.0`.

## Deferred

- Window expressions remain planned syntax for a later mutation-focused
  release.
- Arrow IPC, Parquet, JSON Lines, stdin loading, stream sniffing, and
  configurable CSV dialects remain deferred. `join` and `union` operate over
  the existing CSV-backed driver in v0.12; cross-format multi-input pipelines
  are a later release concern.
- Schema/plan CLI subcommands, CLI formatting, full LSP code actions, and
  cross-document navigation remain deferred.
- Arrow IPC browser output, virtual browser output sinks, and full multi-input
  browser controls beyond the host-file map expansion remain deferred.
