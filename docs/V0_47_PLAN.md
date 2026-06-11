# PDL v0.47 Plan

Status: Shipped
Target version: 0.47.0
Owner: PDL maintainers
Related spec: [`PDL_SPEC.md`](PDL_SPEC.md)
Coverage matrix: [`PDL_NATIVE_COVERAGE.md`](PDL_NATIVE_COVERAGE.md), [`PDL_NATIVE_COVERAGE.csv`](PDL_NATIVE_COVERAGE.csv)
Predecessor plan: [`V0_46_PLAN.md`](V0_46_PLAN.md)
Successor plan: [`V0_48_PLAN.md`](V0_48_PLAN.md)

## Purpose

PDL v0.47 finishes expression-level native coverage. Three areas
flip:

1. Window frames and ordering. The four named frames that v0.43.5
   left on the row engine — `frame remaining`, `frame trailing N`,
   `frame leading N`, and `frame centered N` — promote to
   `native parity`. Mixed multi-key window order groups (a `mutate`
   stage whose window expressions use more than one distinct
   composite order group) promote as well. The ranking, aggregate,
   offset, value, and distribution window functions already lower
   natively for the supported shapes as of v0.43.5; v0.47 removes
   the frame and order-group restrictions that keep their matrix
   rows at `native partial`.
2. Aggregate arguments. `agg` argument expressions over expression
   families that are already `native parity` flip to native. Any
   aggregate call whose argument depends on a still-row-only
   expression family remains row-only with the same expression
   fallback reason.
3. Uncertain numeric coercions. `to_number`, `to_string`, and
   `to_boolean` gain a spec-final contract for edge inputs (overflow,
   mixed text, leading whitespace, locale-specific numerics,
   scientific notation). The contracted subset promotes to `native
   parity`; the remaining cases are explicitly `row-only by design`
   with `UnsupportedNumericCoercion` from v0.43.

The release is gated by the v0.43 parity harness and the v0.43.5
named-frame surface. The numeric coercion contract changes land in
`PDL_SPEC.md` in the same commit as the lowering, per the
spec/plan/code discipline.

## Implemented Scope

Three rules hold at every commit:

1. Row-runtime byte parity. Native window evaluation produces output
   bytes byte-identical to the row runtime over the parity test
   corpus, including the bounded-frame examples
   (`examples/window_frame_bounded.pdl`,
   `examples/window_frame_named.pdl`). Tie-breaking, partition
   emptiness, frame truncation at partition edges, and `N = 0`
   degenerate frames match the row runtime.
2. Contract before code. The numeric coercion contract for
   `to_number`, `to_string`, and `to_boolean` is decided in
   `PDL_SPEC.md` in the same commit as the lowering. The row engine
   remains the byte-parity spec for the contracted subset.
   Out-of-contract inputs are not silently demoted: they either
   produce a deterministic error (with an `E2xxx` code reserved in
   `PDL_SPEC.md`) or are split into a row-only subcase with
   `UnsupportedNumericCoercion`.
3. WASM stays Polars-free. Window and aggregate lowering uses Polars
   features already enabled at the `pdl-exec` level; if bounded-frame
   lowering needs a new Polars feature (e.g. rolling windows), it
   lands behind the existing native-only gates and the wasm target
   graph audit proves it is not reachable from `pdl-wasm`.

The release introduces no new PDL language surface, no new stage
keywords, and no new function names. It may reserve new `E2xxx` codes
for native-only coercion errors that were previously expressed
through row-runtime errors with different messages.

## Promotion Scope

### Stages

- `mutate` with windows. The four bounded named frames lower
  natively: `frame remaining` via the reverse-running idiom
  (current row through end of partition), and `frame trailing N` /
  `frame leading N` / `frame centered N` via fixed-size rolling
  windows truncated at partition edges to match the row runtime.
  Mixed multi-key order groups lower by emitting one Polars `over`
  expression per group. Existing native window functions gain the
  new frames wherever the row runtime allows the combination. Any
  window subcase that cannot preserve row semantics is split into a
  named `row-only by design` row with a refined
  `NativeUnsupportedReason` variant.
- `agg`. Aggregate-argument expression coverage finishes for every
  expression family that is itself `native parity`. Aggregate calls
  whose arguments depend on row-only expression families remain
  row-only with the same expression fallback reason.

### Expressions

- Uncertain numeric coercions. `to_number`, `to_string`, and
  `to_boolean` gain a spec-final edge contract. The contracted subset
  promotes to `native parity`. The uncontracted subset is split into
  `row-only by design` rows with `UnsupportedNumericCoercion`.

### Coverage matrix

- `window bounded frames` flips from `row-only by design` to
  `native parity` for the promoted frames; any unlowered subcase is
  split into a named row with a refined reason.
- `window multi-key ordering` flips to `native parity` for mixed
  multi-key groups.
- The window function rows (`ranking`, `whole-partition aggregates`,
  `running aggregates`, `offset`, `value`, `distribution`) update
  their notes to drop the retired frame and order-group
  restrictions; rows whose remaining restrictions all clear flip to
  `native parity`.
- `agg` argument rows flip to `native parity` for arguments over
  native-parity expression families; remaining rows stay row-only
  inheriting the argument expression's fallback reason.
- `to_number`, `to_string`, `to_boolean` rows split:
  contracted-subset `native parity`, uncontracted-subset
  `row-only by design`.

## Must

- Promote the bounded named frames to native parity.

  Status: Complete (v0.47.0).

  Lowering in `crates/pdl-exec/src/runtime/native_lowering.rs`
  (`lower_data_window_frame` grows arms for the bounded
  `FrameBoundIr` pairs the named frames desugar to). Eligibility
  flip in `crates/pdl-exec/src/runtime/native_planning.rs` retires
  the bounded-frame rejection for the promoted forms; the
  native-engine bounded-frame diagnostic drops out of use for them.
  Parity tests cover: empty partitions, single-row partitions,
  `N = 0` degenerate frames, `N` larger than the partition, ties in
  order keys, and all-null order keys. The
  `examples/window_frame_bounded.pdl` `selected_engine` fixture
  flips from `PortableRows` to `NativePolars` in the same commit.

- Promote mixed multi-key window order groups.

  Status: Complete (v0.47.0).

  Eligibility flip in `native_planning.rs`: a `mutate` stage with
  more than one distinct composite order group becomes eligible, and
  the lowering emits one `over` expression per group. Parity tests
  cover two and three distinct order groups in one stage, with mixed
  null/non-null partition keys, against the existing window
  examples.

- Finish `agg` argument coverage.

  Status: Complete (v0.47.0).

  Aggregate calls whose argument expression family is `native parity`
  (as of v0.46 and earlier) flip to native. The eligibility check in
  `native_planning.rs` walks the argument expression and accepts it
  when every reachable node is native-parity. Aggregate calls whose
  argument depends on a still-row-only expression family stay
  row-only and inherit the argument's `NativeUnsupportedReason`
  variant.

- Define and lower uncertain numeric coercions.

  Status: Complete (v0.47.0).

  `docs/PDL_SPEC.md` gains a new normative subsection (in the
  existing function-set section) for `to_number`, `to_string`, and
  `to_boolean` that pins down: scientific notation handling
  (`1e6`, `1.5E-3`), leading and trailing whitespace, overflow on
  `i64` / `f64` bounds, locale neutrality (no locale-dependent
  decimal separators or thousands separators), and the behavior on
  values that do not match the contracted forms (deterministic
  error vs propagated null). Any new `E2xxx` codes reserved for
  coercion errors land in the spec before the implementation
  commit. Lowering for the contracted subset is added to
  `lower_data_expr` in `native_lowering.rs`. Out-of-contract inputs
  split into `row-only by design` rows carrying
  `UnsupportedNumericCoercion` from v0.43.

- Update the coverage matrix in lockstep.

  Status: Complete (v0.47.0).

  `docs/PDL_NATIVE_COVERAGE.md` and `docs/PDL_NATIVE_COVERAGE.csv`
  update in the same commit as each promotion or split. The CSV is
  the machine-readable source of truth for the native eligibility
  tests. `selected_engine` fixtures for examples that use the
  promoted frames, order groups, aggregate arguments, or coercions
  flip from `PortableRows` to `NativePolars`.

- Hold the WASM target graph.

  Status: Complete (v0.47.0).

  `pdl-wasm` Cargo manifest is unchanged. `cargo check -p pdl-wasm
  --target wasm32-unknown-unknown` remains green. Any new Polars
  feature needed for rolling-window lowering is native-only and the
  wasm target graph audit proves it is not reachable from
  `pdl-wasm`.

- Update the spec, examples, and release stamps.

  Status: Complete (v0.47.0).

  `docs/PDL_SPEC.md` records the v0.47 history line, documents the
  numeric coercion contract, notes that all six named frames now
  execute natively, and reserves any new `E2xxx` codes introduced.
  Every example that exercises window functions, `agg` argument
  expressions, or numeric coercions runs on both engines through
  the parity harness with byte-identical output. Workspace
  `Cargo.toml`, `Cargo.lock`, `editors/vscode/package.json`,
  `editors/vscode/package-lock.json`, and any demo manifests bump
  to `0.47.0`. NPM consumer pins follow `AGENTS.md` "NPM package
  version checks".

## Should

- Land each promotion in its own commit.

  Status: Not applicable in single-commit implementation (v0.47.0).

  Suggested order: `frame remaining` first (single reverse-running
  idiom, no new Polars surface); then `frame trailing N` /
  `frame leading N` / `frame centered N` (shared rolling-window
  lowering); then mixed multi-key order groups (eligibility walker
  change); then `agg` argument coverage; then numeric coercions
  (spec change in the same commit as lowering). Each commit passes
  `cargo fmt --all --check`, `cargo clippy --workspace
  --all-targets`, `cargo test --workspace`, and the parity harness
  independently.

- Add a `pdl-bench` window workload and row-vs-native benchmark.

  Status: Complete (v0.47.0).

  `bench/workloads/large/` has no window workload today. Add a
  million-row workload exercising offset functions and bounded
  frames, with a row-vs-native wrapper next to the existing
  `crates/pdl-bench/` workloads, to feed the v0.43–v0.49
  performance thesis.

- Document the post-v0.47 row-only expression families.

  Status: Complete (v0.47.0).

  After v0.47, the only row-only expression cells should be the
  documented `row-only by design` rows (dynamic `col` indirection
  from v0.43, dynamic `replace` patterns from v0.43,
  out-of-contract numeric coercions from v0.47, and any window
  subcase the implementation could not lower). Document them in the
  coverage matrix with refined `NativeUnsupportedReason` variants.

## Could

- Promote v0.41 union schema extension to native parity.

  Status: Out of scope (v0.41 remains planned).

  If v0.41 shipped union schema extension with row-runtime
  semantics, native lowering uses Polars `concat` with
  null-padding column alignment. Promotion requires byte-parity
  tests against the row writer and the `UnionNullPadding` variant
  for any remaining row-only subcase. If v0.41 did not ship this,
  it stays out of scope.

- Promote v0.41 non-equi joins to native parity.

  Status: Out of scope (v0.41 remains planned).

  If v0.41 shipped non-equi joins, native lowering uses Polars
  range / inequality join APIs. Promotion requires the v0.41
  ordering, null semantics, and cardinality contracts to be
  spec-final and the `NonEquiJoin` variant for any remaining
  row-only subcase.

- Tighten the numeric coercion contract beyond v0.47.

  Status: Deferred.

  v0.47 pins the edge contract for the cases that affect today's
  example corpus and the v0.43–v0.49 parity claims. Further
  tightening (e.g. locale-aware modes, alternate base parsing) is
  out of scope.

## Validation Notes

Repository-required Rust checks are authoritative:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets
cargo test --workspace
cargo check -p pdl-wasm --target wasm32-unknown-unknown
```

Parity harness (must pass green at every commit on the v0.47 branch):

```bash
cargo test -p pdl-parity-tests parity_examples
cargo test -p pdl-parity-tests selected_engine_fixtures
```

Bounded-frame row-vs-native parity:

```bash
cargo run -p pdl-cli -- run examples/window_frame_bounded.pdl --engine row > /tmp/row.out
cargo run -p pdl-cli -- run examples/window_frame_bounded.pdl --engine native > /tmp/native.out
diff /tmp/row.out /tmp/native.out
# Expected: empty (today the native run errors on the bounded frame)

cargo run -p pdl-cli -- plan examples/window_frame_bounded.pdl --json | \
  jq '.execution.observability.selected_engine'
# Expected: "NativePolars"
```

Numeric coercion contract spot check:

```bash
cargo run -p pdl-cli -- run examples/coerce_to_number.pdl --engine auto
cargo run -p pdl-cli -- plan examples/coerce_to_number.pdl --json | \
  jq '.execution.observability.selected_engine'
# Expected: "NativePolars"
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
  new functions. v0.47 widens native execution of the existing
  language and pins one edge contract.
- Do not change the named-frame surface grammar. The v0.43.5
  six-name vocabulary is closed; v0.47 changes which frames execute
  natively, not what parses.
- Do not silently change row-engine numeric coercion behavior. The
  row engine remains the byte-parity spec. The contract documents
  what the row engine already does and what the native engine must
  match.
- Do not promote pipeline-shape changes. Those land in v0.48.
- Do not delete `native partial` or `planned native` from the matrix
  status vocabulary. That cleanup is v0.49 work.
- Do not silently demote any pipeline that runs natively today.
