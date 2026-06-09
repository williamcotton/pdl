# PDL v0.43 Plan

Status: Shipped in 0.43.0
Target version: 0.43.0
Owner: PDL maintainers
Related spec: [`PDL_SPEC.md`](PDL_SPEC.md)
Coverage matrix: [`PDL_NATIVE_COVERAGE.md`](PDL_NATIVE_COVERAGE.md), [`PDL_NATIVE_COVERAGE.csv`](PDL_NATIVE_COVERAGE.csv)
Predecessor plan: [`V0_42_PLAN.md`](V0_42_PLAN.md)
Successor plan: [`V0_44_PLAN.md`](V0_44_PLAN.md)

## Purpose

PDL v0.43 lays the foundation for the native coverage expansion that
runs through v0.44 – v0.49. It introduces no new native promotions and
flips no coverage matrix cells. Instead it ships the test
infrastructure, regression guards, and typed observability surface that
every subsequent release will rely on to flip cells safely.

Three foundations land in this release:

1. A row-vs-native parity test harness in `crates/pdl-parity-tests/`
   that runs every example in `examples/` on both engines and diffs
   `pdl run` byte output across CSV, JSON Lines, Arrow IPC file, Arrow
   IPC stream, and Parquet. The row engine is the byte-parity spec.
2. A `selected_engine` regression guard. Each example carries an
   expected `PlanObservability.selected_engine` fixture under
   `--engine auto`. Tests fail if any pipeline flips from
   `NativePolars` back to `PortableRows` outside of a plan-documented
   change. This is the silent-demotion canary.
3. A typed `NativeUnsupportedReason` surface populated for every
   runnable `row-only by design` cell, replacing free-form fallback
   strings. `pdl plan`, `pdl plan --json`, and `pdl manifest` surface
   the variant. Coarse categories from v0.40–v0.42 are split into
   coverage-boundary variants (`WasmTargetGraph`, `EditorService`,
   `DataDependentColIndirection`, `DataDependentReplacePattern`,
   `UnsupportedNumericCoercion`, `UnionNullPadding`, `NonEquiJoin`,
   plus a small reserve set for the v0.44–v0.49 promotions).

The coverage matrix vocabulary is unchanged in this release. Cells
remain `native parity`, `native partial`, `planned native`, or
`row-only by design`. The vocabulary cleanup (deleting `native partial`
and `planned native`) is v0.49 work, after every promotable cell has
flipped through v0.44–v0.48.

The release is gated by the v0.42 module split
(`runtime/native_lowering.rs`, `runtime/native_planning.rs`,
`runtime/row_eval.rs`, `runtime/stages.rs`), which is already shipped.

## Implemented Scope

This release adds testing and observability surface only. Three rules
hold at every commit:

1. No native promotions. The set of cells that report
   `selected_engine = NativePolars` under `--engine auto` is
   identical to v0.42. The parity harness must report green for every
   example today before any cell can flip in v0.44.
2. WASM target graph is unchanged. `pdl-wasm` Cargo manifest,
   dependency tree, and `wasm32-unknown-unknown` target graph are
   identical to v0.42. The parity-tests crate is `dev-dependencies`
   only and is not reachable from `pdl-wasm`.
3. Honest fallback for already-row-only cells. Where v0.42 returned a
   coarse `NativeUnsupportedReason` variant, v0.43 may refine it into
   a more specific variant, but only for cells that v0.42 already
   reported as row-only. No new cell becomes row-only and no
   `selected_engine` flips.

The release does not introduce new PDL language surface, new stage
keywords, new functions, or new diagnostic codes. New
`NativeUnsupportedReason` variants are internal observability values,
not diagnostic codes; they are documented in `PDL_SPEC.md`'s native
execution section as part of the v0.43 history line.

## Must

- Build the row-vs-native parity test harness.

  Status: Shipped in 0.43.0. One refinement against the proposal: CSV and
  JSON Lines payloads are diffed byte-for-byte (the row engine is the byte
  spec), while Arrow IPC file, Arrow IPC stream, and Parquet payloads are
  compared as decoded tables. The v0.42-era native direct writers emit
  semantically equal but not byte-identical binary encodings; unifying those
  bytes is the v0.44 native sink writer work, and the harness tightens to
  full byte parity when it lands.

  The harness lives in `crates/pdl-parity-tests/` (test-only crate, not
  in the wasm target graph). It enumerates `examples/`, runs each
  example through `pdl run` on both engines with stdin fixtures
  supplied per example, captures stdout, and diffs saved files and
  named-output files. It compares output bytes for every format the
  example uses: CSV, JSON Lines, Arrow IPC file, Arrow IPC stream,
  Parquet. Snapshot reference is the row engine. The harness exposes a
  `cargo test -p pdl-parity-tests parity_examples` entry point. CI
  runs the harness on every PR. The harness catches silent demotion
  and silent byte drift; without it no subsequent release can safely
  flip a cell.

- Add the `selected_engine` regression guard.

  Status: Shipped in 0.43.0.

  Each `examples/*.pdl` gains a fixture under
  `crates/pdl-parity-tests/fixtures/selected_engine/` recording the
  expected `PlanObservability.selected_engine` under `--engine auto`
  as of v0.42. Tests fail if any fixture flips from `NativePolars`
  back to `PortableRows`, or from `PortableRows` to `NativePolars`,
  outside of an explicit, plan-documented update. Updating a fixture
  requires the same commit to update a plan's promotion entry. This
  is the silent-demotion canary that v0.44–v0.49 rely on to prove
  promotions land where the plan says they do.

- Refine the `NativeUnsupportedReason` surface.

  Status: Shipped in 0.43.0.

  `NativeUnsupportedReason` lives in `crates/pdl-exec/src/planning.rs`
  (or its v0.42 split sibling). v0.43 adds the variants needed for
  v0.44–v0.49 to attach honest fallback reasons:

  - `DataDependentColIndirection` — `col(...)` argument is not a
    string literal or string-typed context default.
  - `DataDependentReplacePattern` — `replace` pattern or replacement
    is not a string literal or string-typed context default.
  - `UnsupportedNumericCoercion` — `to_number` / `to_string` /
    `to_boolean` input falls outside the v0.47 contract.
  - `UnionNullPadding` — `union` participants have heterogeneous
    schemas and require null-padding alignment (v0.41 row-only).
  - `NonEquiJoin` — join predicate is not an equality on columns
    (v0.41 row-only).
  - `BindingStartNotEligible` — pipeline-start binding references a
    binding the native planner cannot lower (v0.48 reserve).
  - `NamedOutputMixedEngines` — multi-output program has at least one
    row-only output and per-output observability is not enabled
    (v0.48 reserve).
  - `NonTerminalSaveFanout` — non-terminal `save` requires fan-out
    the native planner does not yet support (v0.48 reserve).
  - `StdinBytesBackedScan` — stdin-backed format requires a
    byte-backed scan adapter the native data facade does not yet
    support (v0.46 reserve).
  - `HostBytesBackedScan` — host-supplied bytes for a CSV / Parquet /
    NDJSON path require a byte-backed scan adapter (v0.46 reserve).
  - `NativeSinkWriter` — sink format is not yet wired to
    `NativeDirectWriter` (v0.44 reserve, drops out of use when
    writers ship).
  - `RowOnlyStage` — stage is `row-only by design` with no narrower
    variant. Catch-all for stages the matrix declares row-only.
  - `WasmTargetGraph` — non-execution observability boundary for
    documentation purposes. Not produced by the planner at runtime;
    documents the WASM contract in matrix and tests.
  - `EditorService` — non-execution observability boundary for the
    LSP / editor-services boundary. Same use as `WasmTargetGraph`.

  Existing coarse variants are mapped to the closest new variant in
  the same commit. `pdl plan` text rendering names the variant; `pdl
  plan --json` and `pdl manifest` serialize it under
  `execution.observability.fallback_reason`. No runnable stage or
  expression that falls off the allowlist may omit the field. The
  parity harness asserts every row-only cell carries a variant.

- Apply `DataDependentColIndirection` and `DataDependentReplacePattern`
  as audits.

  Status: Shipped in 0.43.0.

  The v0.40 spec and code already make `col($name)` and
  `replace($pat, $rep)` native when their argument resolves to a
  string literal or string-typed context default. v0.43 verifies the
  coverage matrix and tests still reflect that, then attaches the new
  fallback reasons to the row-only subset: row-dependent `col(...)`
  yields `DataDependentColIndirection`; row-dependent `replace`
  pattern or replacement yields `DataDependentReplacePattern`. No
  matrix cell flips and no `selected_engine` flips. The audit is a
  test-only and observability change.

- Hold the WASM target graph.

  Status: Shipped in 0.43.0.

  `pdl-wasm` Cargo manifest is unchanged. `cargo check -p pdl-wasm
  --target wasm32-unknown-unknown` remains green. The wasm dependency
  tree continues to exclude Polars, Arrow, and Parquet. The
  parity-tests crate is dev-only and is not reachable from
  `pdl-wasm`. `cargo tree -p pdl-wasm --target wasm32-unknown-unknown
  | grep -E 'polars|arrow|parquet'` returns empty.

- Update the spec and release stamps.

  Status: Shipped in 0.43.0. Consumer npm pins stay on the latest verified
  published `pdl-wasm@0.39.0` and `pdl-editor@0.39.0`.

  `docs/PDL_SPEC.md` records the v0.43 history line and documents the
  refined `NativeUnsupportedReason` surface in the native-execution
  section. Workspace `Cargo.toml`, `Cargo.lock`,
  `editors/vscode/package.json`, `editors/vscode/package-lock.json`,
  and any demo manifests bump to `0.43.0`. NPM consumer pins follow
  `CLAUDE.md` "NPM package version checks" — do not point any
  consumer at `pdl-wasm@0.43.0` or `pdl-editor@0.43.0` unless that
  exact version exists on npm.

## Should

- Document the silent-demotion canary update protocol.

  Status: Shipped in 0.43.0. See CLAUDE.md "selected_engine fixture update
  protocol".

  Add a short subsection to `CLAUDE.md` describing the protocol for
  changing a `selected_engine` fixture: any flip must travel in the
  same commit as a plan's promotion entry update, with a one-line
  reference to the plan section. This is process documentation, not
  code; it prevents future drift.

- Wire the parity harness into the existing `cargo test --workspace`
  default run.

  Status: Shipped in 0.43.0. `pdl-parity-tests` is a workspace member, so CI
  runs the harness on every PR through the existing `cargo test --workspace`
  job. Run cost stayed acceptable; no feature-flag gating was needed.

  Adding `pdl-parity-tests` to the workspace `members` makes the
  harness run on every workspace test. If the run cost becomes
  prohibitive, gate the expensive examples behind a feature flag and
  keep a quick subset in the default test set.

## Could

- Add a `--engine row-strict` flag for CI parity.

  Status: Shipped in 0.43.0 (pulled forward from deferred). `row-strict`
  plans and executes like `row` and fails the run if the result reports any
  backend other than the portable row runtime. The violation is a CLI-level
  error on stderr, not a new diagnostic code. The parity harness runs a
  row-strict leg for every example. The `--engine native-strict` counterpart
  remains deferred to v0.49.

  Symmetric with the `--engine native-strict` flag deferred to v0.49.
  `row-strict` would error if any side pipeline silently uses native
  lowering. Useful for proving the row engine still handles a
  pipeline end-to-end. Not required to ship v0.43.

- Promoting remaining `row-only by design` boundaries (WASM, LSP) is
  explicitly out of scope.

  Status: Deferred indefinitely.

  These cells stay row-only by design. The matrix is their
  documentation; execution fallback reasons apply only to runnable
  pipelines that can actually demote between engines. Promoting WASM
  would require Polars in the wasm target graph, which the WASM
  contract forbids.

- Evaluate configurable CSV dialect support.

  Status: Deferred.

- Evaluate a browser byte IO ABI for binary host-file contents and
  Arrow IPC output.

  Status: Deferred.

- Evaluate object-store and remote path support with a dedicated
  security and IO plan.

  Status: Deferred.

## Validation Notes

Repository-required Rust checks are authoritative:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets
cargo test --workspace
cargo check -p pdl-wasm --target wasm32-unknown-unknown
```

Parity harness baseline (must pass green at the v0.43 commit, before
any v0.44 cell flips):

```bash
cargo test -p pdl-parity-tests parity_examples
cargo test -p pdl-parity-tests selected_engine_fixtures
```

Output-byte parity against v0.42 for representative examples (the
existing examples that v0.42 already runs natively must continue to
produce the same bytes):

```bash
cargo run -p pdl-cli -- run examples/top_regions.pdl
cargo run -p pdl-cli -- run examples/top_regions.pdl --stdout-format arrow-stream > /tmp/out.arrow
cargo run -p pdl-cli -- manifest examples/top_regions.pdl
cargo run -p pdl-cli -- plan examples/top_regions.pdl --json
```

`plan --json` output must surface
`execution.observability.fallback_reason` for every runnable row-only
cell, and that field must be a member of the refined
`NativeUnsupportedReason` enum.

WASM target graph audit:

```bash
cargo tree -p pdl-wasm --target wasm32-unknown-unknown | grep -E 'polars|arrow|parquet'
# must be empty
```

## Non-Goals

- Do not promote any cell. The set of `selected_engine = NativePolars`
  pipelines is identical to v0.42.
- Do not delete `native partial` or `planned native` from the matrix
  status vocabulary. That cleanup is v0.49 work.
- Do not introduce new PDL language surface, new stage keywords, or
  new functions.
- Do not introduce Polars, Arrow, or Parquet into the `pdl-wasm`
  dependency graph.
- Do not change CSV, JSON Lines, Arrow IPC, or Parquet output bytes
  on the row engine. The row writer is the spec.
- Do not change diagnostic codes. New `NativeUnsupportedReason`
  variants are internal observability values, not diagnostic codes.
- Do not promote v0.41 deferred items (union schema extension,
  non-equi joins). They stay v0.41 scope or are picked up by the
  v0.44–v0.49 native-coverage expansion if v0.41 shipped them as
  row-only.
- Do not promote WASM or the LSP/editor surface to native. Those
  boundaries are `row-only by design` and stay so.
