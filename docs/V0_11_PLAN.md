# PDL v0.11 Plan

Status: Complete
Target version: 0.11.0
Owner: PDL maintainers
Related spec: [`PDL_SPEC.md`](PDL_SPEC.md)
Predecessor plan: [`V0_10_PLAN.md`](V0_10_PLAN.md)

## Purpose

PDL v0.11 is the first data-manipulation expansion after the v0.10 CLI
diagnostic presentation and editor hover preview release. It promotes
row-preserving transformations that make CSV cleaning examples useful without
introducing multi-input planning yet.

The release thesis is: authors should be able to clean, derive, and deduplicate
columns in one deterministic pipeline, then continue into the existing
projection, grouping, aggregation, sorting, and stdout flow.

## Must

- Promote `mutate` from a deferred stage to an implemented row-expression stage.

  Status: Complete. The parser, CST views, formatter, semantic analyzer, IR, and
  runtime now support `mutate "name" = expression` assignments. Assignments in a
  single stage are evaluated against the input schema in parallel. Replacements
  keep existing column positions; new columns append in assignment order.
  Duplicate mutate targets produce `E1207`.

- Promote `distinct` from a deferred stage to an implemented duplicate-removal
  stage.

  Status: Complete. `distinct` keeps the first row for each unique full-row key,
  and `distinct "a", "b"` keeps the first row for each listed key tuple. Row
  order follows the retained input rows, and unknown key columns produce
  `E1005`.

- Add a scalar function set for practical cleaning and derivation.

  Status: Complete. Row expressions now support `col`, `lit`, `is_null`,
  `not_null`, `coalesce`, `concat`, `lower`, `upper`, `trim`, `to_number`,
  `abs`, `round`, and `if_else`. The semantic registry drives validation,
  completions, hovers, semantic tokens, and TextMate highlighting. Unknown
  scalar functions produce `E1401`; invalid arity produces `E1402`.

- Add runnable examples with both data and PDL source.

  Status: Complete. Added `examples/orders_raw.csv`,
  `examples/orders_cleaned.pdl`, and `examples/order_region_summary.pdl`.
  CLI integration tests run the new examples with `--stdout-format csv` and
  assert deterministic output.

- Keep editor, LSP, WASM, and browser demo behavior aligned through shared Rust
  crates.

  Status: Complete. Editor services now include the promoted stages and scalar
  functions in completions, semantic tokens, schema facts, and hover metadata.
  WASM execution uses the same runtime path and inherits the new stages without
  adding TypeScript language logic. The VS Code static grammar highlights the
  promoted scalar functions.

- Update the normative spec and release stamps.

  Status: Complete. `docs/PDL_SPEC.md` now describes the v0.11 implementation,
  `Cargo.toml`/`Cargo.lock`, CLI version output, VS Code package manifests, and
  browser demo package manifests are aligned to `0.11.0`.

## Deferred

- `join` and `union` remain deferred because they require multi-input execution
  planning, binding dependency diagnostics, and collision policy decisions.
- Arrow IPC, Parquet, JSON Lines, stdin loading, stream sniffing, configurable
  CSV dialects, schema/plan subcommands, and Arrow browser output remain
  deferred.
- Window expressions remain planned syntax for a later mutation-focused release.
