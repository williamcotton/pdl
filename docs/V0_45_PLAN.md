# PDL v0.45 Plan

Status: Implemented (0.45.0)
Target version: 0.45.0
Owner: PDL maintainers
Related spec: [`PDL_SPEC.md`](PDL_SPEC.md)
Coverage matrix: [`PDL_NATIVE_COVERAGE.md`](PDL_NATIVE_COVERAGE.md), [`PDL_NATIVE_COVERAGE.csv`](PDL_NATIVE_COVERAGE.csv)
Predecessor plan: [`V0_44_PLAN.md`](V0_44_PLAN.md)
Successor plan: [`V0_46_PLAN.md`](V0_46_PLAN.md)

## Purpose

PDL v0.45 promotes `pivot_longer` and `complete` to native parity.
Both stages are purely local lowerings — they translate to Polars
`LazyFrame` operations without changing pipeline shape, sourcing, or
sinking. They are the smallest-blast-radius native promotions in the
v0.43–v0.49 arc and serve as the first stage-level exercise of the
v0.43 parity harness.

The release is gated by the v0.43 parity harness and the v0.44 native
CSV / NDJSON writers. Pipelines that pivot or complete and then write
CSV or NDJSON now execute end-to-end on the native engine.

## Implemented Scope

Two rules hold at every commit:

1. Row-runtime byte parity. The native lowering of `pivot_longer`
   and `complete` produces output bytes byte-identical to the row
   runtime over the parity test corpus. Column ordering, value
   ordering within groups, null rendering, and mixed-type behavior
   match the row runtime exactly. Where Polars unpivot or
   `cross_join` defaults diverge from the row runtime, the lowering
   normalizes through an additional projection or sort to match.
   Mixed-class subcases that a typed engine cannot reproduce keep
   byte parity by refusing the lowering, which demotes the pipeline
   to the row engine in automatic mode.
2. WASM stays Polars-free. The lowerings landed as
   `DataPlan::pivot_longer` and `DataPlan::complete` in
   `crates/pdl-data/src/engine.rs` (behind the `polars-engine`
   feature), with stage wiring and eligibility flips in
   `crates/pdl-exec/src/runtime/native_planning.rs` and the plan
   observability walk in `crates/pdl-exec/src/planning.rs`. None of
   these paths are in the wasm target graph. The lowerings required
   enabling the `pivot` and `cross_join` features on the workspace
   Polars dependency (the proposal assumed they were already
   enabled); Polars remains entirely absent from the wasm graph, so
   the wasm boundary is unchanged.

The release does not introduce new PDL language surface. `pivot_longer`
and `complete` already exist with row-runtime semantics; v0.45 widens
their execution path without changing their syntax, semantics, or
diagnostics.

## Promotion Scope

### Stages

- `pivot_longer` — lower to Polars `melt` with deterministic output
  order matching the row runtime. Mixed-value behavior, fill
  semantics, and null handling preserved byte-for-byte. Empty input,
  single-row input, all-null input, and mixed numeric / string value
  columns are all part of the parity corpus.
- `complete` — lower to cross-join across key domains plus
  left-anti join for missing tuples plus fill-expression projection
  plus ordered concat with the original frame. Key expansion order
  matches the row runtime. Fill-expression evaluation reuses
  `lower_data_expr` for the static subset.

### Coverage matrix

- `pivot_longer` row flips to `native partial` (implementation
  finding: a typed column engine cannot keep the row runtime's
  per-cell value types, so mixed-class value column sets stay
  row-only by design; the class-homogeneous subset is byte-identical
  and the matrix note records the boundary).
- `complete` row flips to `native partial` (same finding for
  class-changing fill expressions; window-bearing fills are also
  row-only).

## Must

- Promote `pivot_longer` to native execution via Polars unpivot.

  Status: Implemented.

  Lowering landed as `DataPlan::pivot_longer` in
  `crates/pdl-data/src/engine.rs`: a hidden input row index, the
  unpivot, a stable sort on the index, and an index drop reproduce
  the row runtime's interleaved output order exactly. Eligibility
  flipped in `crates/pdl-exec/src/runtime/native_planning.rs` and
  `crates/pdl-exec/src/planning.rs`. Implementation finding: a typed
  values column cannot keep per-cell value types, so value column
  sets whose observed classes mix numbers, strings, or booleans
  refuse the lowering and demote to rows in automatic mode with
  byte-identical output (`native partial`, not `native parity`).
  The parity coverage landed as
  `native_engine_pivot_longer_matches_rows_for_path_formats` in
  `crates/pdl-exec/src/runtime.rs` (mixed value classes, all-null
  columns, single-id, multi-id, and empty input across CSV, Parquet,
  and Arrow IPC stream inputs) plus unit tests in
  `crates/pdl-data/src/engine.rs`, and the new
  `examples/pivot_longer_basics.pdl` runs through the parity harness
  on all engine legs. No existing example flipped engine; the new
  example's `selected_engine` fixture pins `native`.

- Promote `complete` to native execution via cross-join + left join +
  fill.

  Status: Implemented.

  Lowering landed as `DataPlan::complete` in
  `crates/pdl-data/src/engine.rs`: the input materializes once,
  duplicate key tuples report `E1208` (matching the row runtime),
  first-appearance key domains come from a stable distinct, an
  order-preserving cross join builds the tuple frame in the row
  runtime's nested expansion order, a null-matching left join
  attaches existing rows, and a `when`/`otherwise` projection applies
  fill expressions to inserted rows only (all fills see the pre-fill
  frame, matching row semantics). Fill-expression lowering reuses
  `lower_data_expr`. Implementation finding: the anticipated
  `row-only by design` subcase is class-changing fill expressions
  (e.g. a string fill over a numeric column), which would re-render
  existing values on a typed engine; the lowering refuses them at
  execution time and automatic mode demotes with byte-identical
  output. Window-bearing fills are rejected at plan time with
  `RowOnlyStage`; no refined `NativeUnsupportedReason` variant was
  added because the class boundary is only observable after scan
  schema resolution, which plan-time eligibility cannot see. Parity
  coverage landed as
  `native_engine_complete_matches_rows_for_path_formats` in
  `crates/pdl-exec/src/runtime.rs` (single-key, composite-key,
  explicit-fill, and empty-input cases across CSV, Parquet, and
  Arrow IPC stream inputs) plus unit tests in
  `crates/pdl-data/src/engine.rs`, and the new
  `examples/complete_keys.pdl` runs through the parity harness on
  all engine legs with a `selected_engine` fixture pinning `native`.

- Add a runnable example for each promotion.

  Status: Implemented.

  `examples/pivot_longer_basics.pdl` (with
  `examples/monthly_sales.csv`) and `examples/complete_keys.pdl`
  (with `examples/daily_visits.csv`) exercise the promoted lowerings
  end-to-end. Both run on both engines through the parity harness
  with byte-identical output, plan as `native` under `--engine
  auto`, and are listed in the README tour (section 6, "Reshape").

- Hold the WASM target graph.

  Status: Implemented.

  `pdl-wasm` Cargo manifest is unchanged and `cargo check -p pdl-wasm
  --target wasm32-unknown-unknown` stays green. Correction to the
  proposal: the `pivot` and `cross_join` Polars features were not
  previously enabled; they are now enabled on the workspace Polars
  dependency, which is reachable only behind `pdl-data`'s
  `polars-engine` feature and remains absent from the wasm target
  graph (`cargo tree -p pdl-wasm --target wasm32-unknown-unknown`
  shows no polars/arrow/parquet).

- Update the spec and release stamps.

  Status: Implemented.

  `docs/PDL_SPEC.md` records the v0.45 implementation-status entry
  and the "Since version 0.45.0" native-execution paragraph
  (section 15.5), including the row-only-by-design subcases.
  Workspace `Cargo.toml`, `Cargo.lock`, the CLI `version` draft
  string, the manifest-version CLI test, `editors/vscode/package.json`
  + lockfile, and `demo/package.json` + lockfile bump to `0.45.0`.
  npm was re-checked on June 10, 2026: the latest published browser
  packages remain `pdl-wasm@0.43.5` / `pdl-editor@0.43.6`, so
  consumer pins stay there per `CLAUDE.md` "NPM package version
  checks" (see `docs/NPM_PACKAGES.md`).

## Should

- Land each stage promotion in its own commit.

  Status: Adapted.

  Both promotions were implemented and validated together in one
  working tree; per repository policy commits are authored manually
  by the human author after review, so commit slicing is left to the
  committer. The full required check set (`cargo fmt --all --check`,
  `cargo clippy --workspace --all-targets`, `cargo test
  --workspace`, the wasm target check, and the parity harness) was
  run green over the combined change.

- Add a `pdl-bench` row-vs-native benchmark for `pivot_longer` and
  `complete`.

  Status: Implemented.

  `bench/workloads/large/million_row_pivot_longer.pdl`
  (reshape-dominated) and
  `bench/workloads/large/million_row_complete_buckets.pdl`
  (key-expansion-dominated, 1M-row × 4-segment domain) are registered
  in `crates/pdl-bench/src/main.rs`. Both produce byte-identical
  output on the two engines (the workloads deliberately avoid the
  pre-existing float-accumulation and float-group-key rendering gaps
  in `agg`, which predate v0.45). Debug-build spot check: native ran
  the pivot workload ~13x and the complete workload ~9x faster than
  the row engine.

## Could

- Promote `pivot_wider` (or whatever the inverse pivot is named in
  PDL today) to native parity.

  Status: Deferred to a later release if and when the stage exists.

  The current language surface includes `pivot_longer` (covered here)
  and `complete`. If `pivot_wider` lands later, its native promotion
  follows the same template as `pivot_longer`.

- Split `complete` into narrower row-only subcases with refined
  `NativeUnsupportedReason` variants.

  Status: Decided in this release.

  The row-only subcases are class-changing fill expressions
  (`complete`) and mixed-class value column sets (`pivot_longer`).
  Both are documented in the coverage matrix notes and the spec. No
  refined `NativeUnsupportedReason` variant was added: the class
  boundary depends on scan-resolved dtypes, which plan-time
  eligibility cannot observe, so the demotion happens inside the
  native lowering at execution time (automatic mode falls back to
  rows with byte-identical output; forced native reports `E1211`).
  Plan-time observability gains `RowOnlyStage` only for the
  statically visible subcases (empty column/key lists and
  window-bearing fills).

## Validation Notes

Repository-required Rust checks are authoritative:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets
cargo test --workspace
cargo check -p pdl-wasm --target wasm32-unknown-unknown
```

Parity harness (must pass green at every commit on the v0.45 branch):

```bash
cargo test -p pdl-parity-tests parity_examples
cargo test -p pdl-parity-tests selected_engine_fixtures
```

Selected-engine confirmation for `pivot_longer` / `complete`
pipelines composed only of natively covered cells (the observability
surface serializes engines as `native` / `row`):

```bash
cargo run -p pdl-cli -- plan examples/pivot_longer_basics.pdl --json | \
  jq '.execution.observability.selected_engine'
# Expected: "native"

cargo run -p pdl-cli -- plan examples/complete_keys.pdl --json | \
  jq '.execution.observability.selected_engine'
# Expected: "native"
```

WASM target graph audit:

```bash
cargo tree -p pdl-wasm --target wasm32-unknown-unknown | grep -E 'polars|arrow|parquet'
# must be empty
```

## Non-Goals

- Do not change CSV, JSON Lines, Arrow IPC, or Parquet output bytes
  on the row engine. The row writer is the spec.
- Do not introduce Polars, Arrow, or Parquet into the `pdl-wasm`
  dependency graph.
- Do not introduce new PDL language surface, new stage keywords, or
  new functions.
- Do not change the syntax, semantics, or diagnostics for
  `pivot_longer` or `complete`. v0.45 widens their execution only.
- Do not promote sources, sinks, or other stages. Sources land in
  v0.46, expressions in v0.47, pipeline-shape changes in v0.48.
- Do not delete `native partial` or `planned native` from the matrix
  status vocabulary. That cleanup is v0.49 work.
- Do not silently demote any pipeline that runs natively today.
