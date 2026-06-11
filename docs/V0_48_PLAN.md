# PDL v0.48 Plan

Status: Shipped
Target version: 0.48.0
Owner: PDL maintainers
Related spec: [`PDL_SPEC.md`](PDL_SPEC.md)
Coverage matrix: [`PDL_NATIVE_COVERAGE.md`](PDL_NATIVE_COVERAGE.md), [`PDL_NATIVE_COVERAGE.csv`](PDL_NATIVE_COVERAGE.csv)
Predecessor plan: [`V0_47_PLAN.md`](V0_47_PLAN.md)
Successor plan: [`V0_49_PLAN.md`](V0_49_PLAN.md)

## Purpose

PDL v0.48 promotes the pipeline-shape constructs that today force
whole programs to the row engine even when every stage, source,
expression, and sink inside the program is itself native-eligible:

1. Native binding-start pipelines. `PipelineStartIr::Binding` becomes
   native-eligible when the referenced binding is itself
   native-eligible. The native planner lowers the binding to a
   `DataPlan` first and threads its `LazyFrame` as the start of the
   main pipeline.
2. Native named outputs. `ir.outputs` becomes eligible when every
   output pipeline and referenced binding is native-eligible. Each
   output becomes a `LazyFrame` sink with deterministic emission
   order matching the row runtime.
3. Native non-terminal `save`. `save` is no longer required to be the
   terminal stage. Side-effecting writes inside a pipeline execute
   via `LazyFrame.cache()` (or equivalent fan-out) so the saved
   frame writes its bytes while the pipeline continues.

This is the "largest blast radius" release in the v0.43–v0.49 arc.
After v0.48, a pipeline composed only of `native parity` cells
(including bindings, named outputs, and non-terminal saves) reaches
the native engine under `--engine auto`.

The release is gated by the v0.43 parity harness, the v0.44 writers,
the v0.45 stage promotions, the v0.46 source promotions, and the
v0.47 expression promotions. All of those are prerequisites because
binding-start pipelines, named outputs, and non-terminal saves
recursively require every inner construct to be native-eligible.

## Implemented Scope

Three rules hold at every commit:

1. Recursive eligibility. Binding-start pipelines and named outputs
   are native-eligible iff every referenced construct is itself
   native-eligible. Existing semantic-layer cycle detection covers
   cycles. The eligibility walker is the same surface that v0.43
   built and v0.44–v0.47 incrementally widened.
2. Deterministic emission order. Cross-output collection order and
   non-terminal save write order match the row runtime exactly.
   Where Polars `LazyFrame.cache()` or fan-out semantics could
   produce a different observable order, the lowering inserts an
   explicit sequencing step.
3. Per-output observability or whole-program demotion. Mixed
   native/row execution across named outputs is allowed only if
   `PlanObservability` carries per-output `selected_engine` and
   `fallback_reason` fields. Without per-output observability, a
   single row-only output forces the whole multi-output program to
   the row engine.

The release introduces no new PDL language surface, no new stage
keywords, and no new functions.

## Promotion Scope

### Pipeline shape

- Native binding-start pipelines. `PipelineStartIr::Binding` becomes
  native-eligible when the referenced binding is itself
  native-eligible. The native planner lowers the binding to a
  `DataPlan` first and threads its `LazyFrame` as the start of the
  main pipeline.
- Native named outputs. `ir.outputs` becomes eligible when every
  output pipeline and referenced binding is native-eligible. Each
  output becomes a `LazyFrame` sink; collection order across outputs
  is deterministic and matches the row runtime's output emission
  order.
- Native non-terminal `save`. `save` is no longer required to be the
  terminal stage. Side-effecting writes inside a pipeline execute
  via `LazyFrame.cache()` (or equivalent fan-out) so the saved frame
  writes its bytes while the pipeline continues. Write order across
  saves is deterministic.

### Observability

- `PlanObservability` gains per-output `selected_engine` and
  `fallback_reason` fields. `pdl plan`, `pdl plan --json`, and `pdl
  manifest` surface them. Today's single-engine field is preserved
  as the program-level engine (matching the row runtime's
  whole-program view when all outputs agree) for backward
  compatibility.

### Coverage matrix

- Binding-start pipeline row flips to `native parity` for the
  recursively-eligible subset; the remaining subset is split into a
  row-only row with `BindingStartNotEligible` from v0.43.
- Named-output program row flips to `native parity` for the
  recursively-eligible subset; the remaining subset is split into a
  row-only row with `NamedOutputMixedEngines` for the
  no-per-output-observability path.
- Non-terminal `save` row flips to `native parity`; any subcase that
  cannot use `LazyFrame.cache()` fan-out splits into a row-only row
  with `NonTerminalSaveFanout`.

## Must

- Promote native binding-start pipelines.

  Status: Complete.

  Eligibility flip in
  `crates/pdl-exec/src/runtime/native_planning.rs`. The native
  planner walks the referenced binding's pipeline and accepts it
  when every reachable construct is native-eligible. The lowering
  in `native_lowering.rs` emits a `DataPlan` for the binding first
  and threads its `LazyFrame` into the main pipeline's start. Cycle
  detection reuses the existing semantic-layer pass; no new cycle
  check is added.

- Promote native named outputs.

  Status: Complete.

  Eligibility for `ir.outputs` recursively requires every output and
  every referenced binding to be native-eligible. Each output becomes
  a `LazyFrame` sink. Emission order matches the row runtime's
  output emission order (existing IR enumeration order). The
  collect-and-write step is sequenced so that observable side
  effects (file writes, stdout writes) occur in the same order on
  both engines.

- Add per-output `selected_engine` and `fallback_reason` to
  `PlanObservability`.

  Status: Complete.

  Fields land in `crates/pdl-exec/src/planning.rs` (or its v0.42
  split sibling). `pdl plan` text rendering shows one line per
  output. `pdl plan --json` and `pdl manifest` serialize them under
  `execution.observability.outputs[].selected_engine` and
  `execution.observability.outputs[].fallback_reason`. The top-level
  `selected_engine` field reports the program-level engine
  (`NativePolars` if all outputs are native, `PortableRows` if any
  output demotes and per-output observability is disabled, or
  `Mixed` if per-output observability is enabled and outputs
  disagree).

- Promote non-terminal `save` via `LazyFrame.cache()` fan-out.

  Status: Complete.

  Lowering in `native_lowering.rs` rewrites a pipeline containing
  non-terminal `save` into a `LazyFrame.cache()` fan-out: the cached
  frame writes its bytes to the save target while the pipeline
  continues with the same cached frame. Write order across multiple
  non-terminal saves is deterministic and matches the row runtime
  (existing in-order semantics). Any subcase that cannot use the
  fan-out (e.g. a save followed by a stage that reorders the frame
  in a way that would change the saved bytes) splits into a
  row-only row with `NonTerminalSaveFanout`.

- Update the coverage matrix in lockstep.

  Status: Complete.

  `docs/PDL_NATIVE_COVERAGE.md` and `docs/PDL_NATIVE_COVERAGE.csv`
  update in the same commit as each promotion or split.
  `selected_engine` fixtures for every multi-output example, every
  binding-start example, and every non-terminal-save example update
  from `PortableRows` to `NativePolars` per the v0.43 protocol. The
  per-output fixture surface lands in this release so future
  releases can reason about per-output engines.

- Hold the WASM target graph.

  Status: Complete.

  `pdl-wasm` Cargo manifest is unchanged. `cargo check -p pdl-wasm
  --target wasm32-unknown-unknown` remains green. The
  per-output-observability field is a plain Rust struct field with
  no Polars or Arrow dependency; `pdl-wasm` continues to populate
  it with `PortableRows` for every output.

- Update the spec and release stamps.

  Status: Complete.

  `docs/PDL_SPEC.md` records the v0.48 history line and updates the
  native-execution and observability sections to document
  binding-start eligibility, named-output eligibility,
  non-terminal-save fan-out, and per-output observability fields.
  Workspace `Cargo.toml`, `Cargo.lock`,
  `editors/vscode/package.json`, `editors/vscode/package-lock.json`,
  and any demo manifests bump to `0.48.0`. NPM consumer pins follow
  `CLAUDE.md` "NPM package version checks".

## Should

- Land each pipeline-shape promotion in its own commit.

  Status: Complete in implementation order. The repository-local
  `AGENTS.md` forbids agents from creating commits, so the suggested
  per-promotion commit split remains a human review/commit task.

  Suggested order: binding-start first (smallest blast radius,
  reuses existing eligibility walker); then non-terminal `save`
  (purely local fan-out lowering); then per-output observability as
  a standalone observability commit; then native named outputs (the
  largest delta, depends on per-output observability). Each commit
  passes `cargo fmt --all --check`, `cargo clippy --workspace
  --all-targets`, `cargo test --workspace`, and the parity harness
  independently.

- Add a `pdl-bench` benchmark for a representative multi-output
  pipeline.

  Status: Complete.

  Multi-output programs are common end-user pipelines. A benchmark
  in `crates/pdl-bench/` that uses two named outputs, one binding,
  and a non-terminal save gives the v0.43–v0.49 performance thesis
  a realistic upper-bound number.

- Audit `NativeUnsupportedReason` retirement.

  Status: Complete. `BindingStartNotEligible` and
  `NamedOutputMixedEngines` are populated by planner/forced-native subcases.
  `NonTerminalSaveFanout` remains reserved because the v0.48 implementation
  supports every current non-terminal-save fan-out subcase without demotion.

  The v0.43 reserve variants `BindingStartNotEligible`,
  `NamedOutputMixedEngines`, and `NonTerminalSaveFanout` are
  populated in v0.48. Verify they appear in `pdl plan --json`
  output for example pipelines that exercise the row-only subcases.

## Could

- Surface a `--engine native-strict` flag.

  Status: Deferred to v0.49.

  Distinct from `--engine native`, which already errors on
  ineligibility. `native-strict` would also error if any side
  pipeline (binding, named output) falls back. v0.48 enables the
  per-output observability the flag needs; v0.49 adds the flag.

- Allow mixed native/row engine selection per output through a CLI
  override.

  Status: Deferred.

  Today the planner picks the engine. A CLI flag that overrides the
  selection per output is interesting for debugging but out of scope
  for v0.48.

## Validation Notes

Repository-required Rust checks are authoritative:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets
cargo test --workspace
cargo check -p pdl-wasm --target wasm32-unknown-unknown
```

Parity harness (must pass green at every commit on the v0.48 branch):

```bash
cargo test -p pdl-parity-tests parity_examples
cargo test -p pdl-parity-tests selected_engine_fixtures
```

Selected-engine confirmation for binding-start and multi-output
pipelines composed only of `native parity` cells:

```bash
cargo run -p pdl-cli -- plan examples/binding_start.pdl --json | \
  jq '.execution.observability.selected_engine'
# Expected: "NativePolars"

cargo run -p pdl-cli -- plan examples/named_outputs.pdl --json | \
  jq '.execution.observability.outputs[].selected_engine'
# Expected: every element is "NativePolars"

cargo run -p pdl-cli -- plan examples/non_terminal_save.pdl --json | \
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
  new functions. v0.48 widens native execution of the existing
  pipeline-shape surface.
- Do not change the observable order of named outputs or
  non-terminal saves. The row runtime's order is the spec.
- Do not delete `native partial` or `planned native` from the matrix
  status vocabulary. That cleanup is v0.49 work.
- Do not silently demote any pipeline that runs natively today.
- Do not add `--engine native-strict` here. It lands in v0.49.
