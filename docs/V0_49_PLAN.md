# PDL v0.49 Plan

Status: Proposed
Target version: 0.49.0
Owner: PDL maintainers
Related spec: [`PDL_SPEC.md`](PDL_SPEC.md)
Coverage matrix: [`PDL_NATIVE_COVERAGE.md`](PDL_NATIVE_COVERAGE.md), [`PDL_NATIVE_COVERAGE.csv`](PDL_NATIVE_COVERAGE.csv)
Predecessor plan: [`V0_48_PLAN.md`](V0_48_PLAN.md)

## Purpose

PDL v0.49 closes the native-coverage expansion arc started in v0.43.
By this release every promotable cell has flipped through
v0.44–v0.48; v0.49 makes the accounting honest:

1. The coverage matrix status vocabulary drops `native partial` and
   `planned native`. Every cell in
   `PDL_NATIVE_COVERAGE.md` and `PDL_NATIVE_COVERAGE.csv` becomes
   `native parity` or `row-only by design`. No intermediate status
   survives.
2. Remaining unflipped rows that have not been promoted or split in
   v0.44–v0.48 (`load`, `group_by`, `join`, `union`, context
   references, Arrow IPC source rows, named bindings, path / stdout
   / bytes sink boundaries, and PDL-to-Algraf Arrow IPC) get an
   explicit v0.49 disposition: each is fully promoted, or split into
   narrower `native parity` and `row-only by design` rows.
3. `--engine native-strict` lands. The flag errors on any fallback,
   including side pipelines (binding, named output, non-terminal
   save). It is the symmetric "loud demote" counterpart to today's
   silent `--engine auto` plus observability surface.
4. The eligibility tests treat the CSV as the compile-time oracle.
   If `PDL_NATIVE_COVERAGE.csv` claims `native parity` for a cell
   that the eligibility check still rejects, or vice versa, the
   tests fail.

After v0.49 the `Auto` engine reaches the native path for every
pipeline composed only of `native parity` cells. Pipelines that cross
a `row-only by design` boundary continue to use the row engine with
an observable, typed `NativeUnsupportedReason`.

The release is gated by v0.43–v0.48. Every promotion that lands here
is a remainder that did not fit a more-specific release theme.

## Implemented Scope

Three rules hold at every commit:

1. Status vocabulary cleanup. The matrix header drops `native
   partial` and `planned native`. Every cell flips to `native parity`
   or `row-only by design`. The CSV column for status uses the new
   two-value vocabulary; the eligibility test fails compilation if
   any other value appears.
2. Eligibility-CSV agreement. The eligibility check in
   `crates/pdl-exec/src/runtime/native_planning.rs` and the matrix
   CSV must agree at compile time. The matrix becomes the spec for
   eligibility; the eligibility test reads it and asserts every cell
   the CSV calls `native parity` is accepted by the planner and every
   cell the CSV calls `row-only by design` is rejected with the
   listed `NativeUnsupportedReason` variant.
3. WASM stays Polars-free. `pdl-wasm` Cargo manifest, dependency
   tree, and `wasm32-unknown-unknown` target graph are unchanged.

The release introduces no new PDL language surface, no new stage
keywords, and no new functions. It may reserve no new `E2xxx` codes
beyond what v0.43–v0.48 introduced.

## Promotion Scope

### Coverage matrix

- `native partial` and `planned native` are removed from the matrix
  header's status vocabulary.
- Every row in `PDL_NATIVE_COVERAGE.md` and `PDL_NATIVE_COVERAGE.csv`
  carries `native parity` or `row-only by design`.
- Remaining unflipped rows are promoted or split:
  - `load` — promote the file-resolution + format-dispatch subset
    that is already covered by v0.44/v0.46; split off any subset
    that depends on object-store / remote paths into a row-only row
    with a refined reason (e.g. `RemotePathNotSupported`, reserved
    here).
  - `group_by` — promote the subset whose key expressions are
    `native parity` (after v0.47); split key expressions that
    depend on row-only expression families into a row-only row
    inheriting the expression's reason.
  - `join` — promote the equi-join subset over `native parity`
    columns; row-only subset (non-equi joins, joins on row-only
    expression families) carries `NonEquiJoin` or the appropriate
    inherited reason.
  - `union` — promote the schema-matched subset; row-only subset
    (heterogeneous schemas requiring null-padding alignment) carries
    `UnionNullPadding`.
  - Context references — promote the subset where the resolved
    context value is a string literal or string-typed default
    (matching the v0.43 audits for `col` and `replace`); row-only
    subset carries `DataDependentColIndirection` or
    `DataDependentReplacePattern` as already populated by v0.43.
  - Arrow IPC source rows — confirm `native parity` for path, file,
    and stream (already covered earlier); no split expected.
  - Named bindings — confirm `native parity` for the subset covered
    by v0.48; row-only subset carries `BindingStartNotEligible`.
  - Path / stdout / bytes sink boundaries — confirm `native parity`
    for CSV / NDJSON (from v0.44) and Arrow IPC / Parquet (already
    parity); no split expected.
  - PDL-to-Algraf Arrow IPC — confirm `native parity` for the
    typed-stream handoff path; document the boundary if it stays
    row-only by design.

### CLI

- `--engine native-strict` lands as a new mode of the existing
  `--engine` flag. Distinct from `--engine native` (which errors on
  ineligibility), `native-strict` also errors if any side pipeline
  (binding, named output, non-terminal save) falls back. Useful for
  CI parity gating.

## Must

- Flip the coverage matrix to the two-value status vocabulary.

  Status: Proposed.

  `docs/PDL_NATIVE_COVERAGE.md` header drops `native partial` and
  `planned native` from the documented status set.
  `docs/PDL_NATIVE_COVERAGE.csv` status column is restricted to
  `native parity` and `row-only by design`. Every existing cell
  carrying the deleted statuses receives a v0.49 disposition. The
  matrix update is a single commit alongside the eligibility-CSV
  agreement test.

- Promote or split every unflipped remainder row.

  Status: Proposed.

  Process: walk each row in the matrix that v0.43–v0.48 left
  unflipped, check the v0.47 expression coverage and v0.48 pipeline
  shape, and either flip the row to `native parity` (with a
  `selected_engine` fixture update) or split it into a narrower
  `native parity` row and a `row-only by design` row carrying a
  typed `NativeUnsupportedReason` variant. The Promotion Scope
  section above enumerates the expected rows.

- Implement the CSV-as-oracle eligibility test.

  Status: Proposed.

  A new test in `crates/pdl-exec/tests/native_coverage.rs` (or
  similar) reads `PDL_NATIVE_COVERAGE.csv` at compile time (via
  `include_str!`), parses each row, and asserts the
  `native_planning.rs` eligibility check agrees: every
  `native parity` row must be accepted; every `row-only by design`
  row must be rejected with the listed
  `NativeUnsupportedReason` variant. The CSV becomes the
  load-bearing spec for native eligibility; drift between matrix and
  planner becomes a compile-time test failure.

- Add `--engine native-strict`.

  Status: Proposed.

  Flag handling in `crates/pdl-cli/`. The mode errors if any side
  pipeline (binding, named output, non-terminal save) falls back to
  the row engine. The per-output observability surface from v0.48
  makes the side-pipeline check straightforward: walk the per-output
  `selected_engine` fields and fail if any equals `PortableRows`.
  The exit code matches today's `--engine native` ineligibility
  exit code.

- Populate every remaining runnable `row-only by design` cell's
  `fallback_reason`.

  Status: Proposed.

  Final pass after the matrix flip: walk every `row-only by design`
  row and verify the planner attaches the typed
  `NativeUnsupportedReason` variant the matrix declares. Any row
  whose runtime path does not attach a variant fails the
  CSV-as-oracle test from the previous Must item.

- Hold the WASM target graph.

  Status: Proposed.

  `pdl-wasm` Cargo manifest is unchanged. `cargo check -p pdl-wasm
  --target wasm32-unknown-unknown` remains green. The matrix flip,
  eligibility test, and `--engine native-strict` flag introduce no
  new wasm-reachable dependencies.

- Update the spec, examples, and release stamps.

  Status: Proposed.

  `docs/PDL_SPEC.md` records the v0.49 history line, updates the
  native-execution section to describe the closed coverage arc,
  documents `--engine native-strict`, and notes the two-value
  status vocabulary. Every example continues to round-trip
  identically on both engines through the parity harness. Workspace
  `Cargo.toml`, `Cargo.lock`, `editors/vscode/package.json`,
  `editors/vscode/package-lock.json`, and any demo manifests bump
  to `0.49.0`. NPM consumer pins follow `CLAUDE.md` "NPM package
  version checks".

## Should

- Land the matrix flip, the CSV-as-oracle test, and the remainder
  promotions in separate commits.

  Status: Proposed.

  Suggested order: remainder promotions / splits first (one commit
  per row group); CSV-as-oracle test second (locks in the matrix);
  status vocabulary cleanup third (deletes `native partial` and
  `planned native` from headers and CSV); `--engine native-strict`
  fourth (purely CLI, depends on per-output observability from
  v0.48). Each commit passes `cargo fmt --all --check`, `cargo
  clippy --workspace --all-targets`, `cargo test --workspace`, and
  the parity harness independently.

- Add a CI gate that runs `--engine native-strict` on a curated
  example subset.

  Status: Proposed.

  The strict gate proves that the curated subset never silently
  demotes, even partially. Distinct from the parity harness, which
  proves byte parity; strict proves engine selection.

- Document the post-v0.49 row-only catalog.

  Status: Proposed.

  After v0.49 the row-only cell set is small and stable. Add a
  short subsection to `PDL_SPEC.md` (or a sibling
  `docs/ROW_ONLY_CATALOG.md`) listing every row-only row with its
  variant and motivation. This is documentation of the closed arc,
  not policy.

## Could

- Surface a `--engine row-strict` flag for CI parity.

  Status: Already shipped in 0.43.0 (pulled forward from this arc's
  deferred set). No v0.49 work remains; `--engine native-strict` in
  this release completes the symmetric strict pair.

- Promoting remaining `row-only by design` boundaries (WASM, LSP) is
  explicitly out of scope.

  Status: Deferred indefinitely.

  These cells stay row-only by design. The matrix is their
  documentation; execution fallback reasons apply only to runnable
  pipelines that can actually demote between engines. Promoting
  WASM would require Polars in the wasm target graph, which the
  WASM contract forbids.

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

Parity harness (must pass green at every commit on the v0.49 branch):

```bash
cargo test -p pdl-parity-tests parity_examples
cargo test -p pdl-parity-tests selected_engine_fixtures
```

Coverage matrix consistency (the CSV is now the compile-time oracle):

```bash
cargo test -p pdl-exec native_coverage
# Asserts: every `native parity` row in PDL_NATIVE_COVERAGE.csv is
# accepted by the eligibility check; every `row-only by design` row
# is rejected with the declared NativeUnsupportedReason variant.
```

`--engine native-strict` smoke test on a curated example subset:

```bash
cargo run -p pdl-cli -- run examples/top_regions.pdl --engine native-strict
# Expected: exit 0, runs on native, no fallback.
```

WASM target graph audit:

```bash
cargo tree -p pdl-wasm --target wasm32-unknown-unknown | grep -E 'polars|arrow|parquet'
# must be empty
```

## Non-Goals

- Do not promote WASM or the LSP/editor surface to native. Those
  boundaries are `row-only by design` and stay so.
- Do not remove the row engine. It remains the parity reference, the
  WASM execution path, and the `--engine row` opt-in.
- Do not introduce Polars, Arrow, or Parquet into the `pdl-wasm`
  dependency graph.
- Do not change CSV, JSON Lines, Arrow IPC, or Parquet output bytes
  on the row engine. The row writer is the spec.
- Do not introduce new PDL language surface, new stage keywords, or
  new functions.
- Do not reintroduce `native partial` or `planned native` as
  matrix statuses for future releases. After v0.49 the vocabulary
  is closed; any future promotion either flips a `row-only by
  design` row to `native parity` (with a plan-documented commit) or
  splits an existing row, but no intermediate status returns.
- Do not silently demote any pipeline that runs natively today.
- Do not introduce object-store, network, or remote-path support;
  that is a separate plan with its own security model.
