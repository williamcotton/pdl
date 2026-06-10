# PDL v0.44 Plan

Status: Implemented (shipped as 0.44.0)
Target version: 0.44.0
Owner: PDL maintainers
Related spec: [`PDL_SPEC.md`](PDL_SPEC.md)
Coverage matrix: [`PDL_NATIVE_COVERAGE.md`](PDL_NATIVE_COVERAGE.md), [`PDL_NATIVE_COVERAGE.csv`](PDL_NATIVE_COVERAGE.csv)
Predecessor plan: [`V0_43_5_PLAN.md`](V0_43_5_PLAN.md)
Successor plan: [`V0_45_PLAN.md`](V0_45_PLAN.md)

## Purpose

PDL v0.44 promotes the CSV and JSON Lines sink writers to native
parity. These writers are the first promotion in the v0.43–v0.49 arc
because they unblock every downstream sink-eligible promotion: once a
pipeline can write CSV or NDJSON natively, the stages and sources that
feed those sinks can flip without leaving a `RowFormatWriter` tail at
the end of every pipeline. Arrow IPC and Parquet sinks are already
native parity as of v0.42; v0.44 completes the sink matrix.

The release is gated by the v0.43 parity harness. Every byte-parity
claim made in v0.44 is verified by `cargo test -p pdl-parity-tests
parity_examples` running both engines and diffing output bytes for
every format each example uses.

## Implemented Scope

Two rules hold at every commit:

1. Byte-for-byte writer parity. The native CSV and JSON Lines
   writers produce output bytes byte-identical to the row writers
   across the parity corpus. Header row, quoting, escaping, line
   terminators, null rendering, field ordering, numeric formatting,
   and trailing newline are unchanged from the row writer. Where
   Polars writer defaults diverge from the row writer, the native
   writer normalizes before emission. If Polars cannot be configured
   for row-identical bytes, the implementation uses native-side text
   emission in `pdl-data` while keeping the selected engine
   `NativePolars` and avoiding row-engine execution.
2. WASM stays Polars-free. The native CSV/NDJSON writer plumbing
   lives behind native-only modules or feature gates. The wasm
   target graph audit remains empty.

The release does not introduce new PDL language surface, new stage
keywords, new functions, new diagnostic codes, or new format support.
It widens which existing format writes execute on the native engine.

## Promotion Scope

### Sinks

- Native CSV writer. `SinkStrategy::RowFormatWriter` is replaced with
  `SinkStrategy::NativeDirectWriter` for CSV across path, stdout, and
  bytes sinks. The writer may use Polars writer configuration or
  native-side text emission; emitted bytes must be byte-identical to
  the row writer.
- Native JSON Lines writer. Same shape: `RowFormatWriter` is replaced
  with `NativeDirectWriter` for NDJSON across path, stdout, and bytes
  sinks. Field ordering, null encoding, trailing newline, and numeric
  formatting match the row writer byte-for-byte.

### Coverage matrix

- CSV sink rows (path, stdout, bytes) flip from their current
  partial / row-only status to `native parity`.
- JSON Lines sink rows (path, stdout, bytes) flip from their current
  partial / row-only status to `native parity`.
- Arrow IPC file, Arrow IPC stream, and Parquet sink rows remain
  `native parity` (unchanged).

## Must

- Promote CSV sink to native parity across path, stdout, and bytes.

  Status: Implemented.

  Implemented as native-side text emission in `pdl-data`: Polars
  writer configuration cannot guarantee row-identical bytes, so
  `write_native_csv` in `crates/pdl-data/src/engine.rs` streams
  collected dataframe rows through the row writer's own dialect and
  cell encoder (`CsvStreamWriter` in `crates/pdl-data/src/csv.rs`),
  making byte parity hold by construction without materializing a
  row table. Writer routing in
  `crates/pdl-exec/src/planning.rs` (`sink_strategy`) reports
  `NativeDirectWriter` for native CSV saves and `BytesSink` for
  native CSV stdout payloads, matching the binary-format precedent.
  The parity corpus (unit tests in `engine.rs` plus the
  `pdl-parity-tests` example harness) covers empty input, embedded
  delimiters, quotes, and newlines, multibyte UTF-8 in headers and
  cells, numeric edges (int64-scale magnitudes, f64 subnormals),
  explicit null rendering, and booleans, across bytes, writer, and
  path sinks.

- Promote JSON Lines sink to native parity across path, stdout, and
  bytes.

  Status: Implemented.

  Same shape as the CSV writer promotion: `write_native_json_lines`
  in `crates/pdl-data/src/engine.rs` streams dataframe rows through
  the row writer's record encoder (`write_json_lines_record` in
  `crates/pdl-data/src/jsonl.rs`), preserving stable field ordering,
  integral-number narrowing, null encoding, Unicode escaping, and
  the trailing-newline contract byte-for-byte. Nested struct
  flattening does not apply: the native value model rejects nested
  dtypes before emission, exactly as the previous row-format
  fallback did.

- Update the coverage matrix in lockstep.

  Status: Implemented. CSV and JSON Lines sink rows and the
  path/stdout/bytes sink rows are `native parity`; the `save` stage
  row stays `native partial` only for the non-terminal fan-out
  boundary (v0.48).

  `docs/PDL_NATIVE_COVERAGE.md` and `docs/PDL_NATIVE_COVERAGE.csv`
  update in the same commit as each promotion. The CSV is the
  machine-readable source of truth for the native eligibility tests;
  if a sink row flips to `native parity` in the CSV, the eligibility
  check must agree.

- Update the `selected_engine` regression fixtures.

  Status: Implemented as a no-op, verified by the canary.

  No fixture flips: CSV/JSON Lines sinks never demoted engine
  selection — the planner already selected `NativePolars` for those
  pipelines and routed only the terminal write through the
  row-format fallback inside `pdl-data`. v0.44 changes the write
  path and the reported `sink_strategy` / `row_materialization`
  observability, not engine selection, so every existing fixture
  stays as committed and `selected_engine_fixtures` passes
  unchanged. The remaining `row` fixtures are row-only for source
  (v0.46) and window-frame (v0.47) reasons, not sink reasons.

- Hold the WASM target graph.

  Status: Implemented.

  `pdl-wasm` Cargo manifest is unchanged and no Polars writer
  feature was added: native text emission reuses the Polars-free row
  writer encoders in `pdl-data`, and the native plumbing stays
  behind the existing `polars-engine` gate. `cargo check -p pdl-wasm
  --target wasm32-unknown-unknown` is green and the target graph
  audit stays empty.

- Update the spec, examples, and release stamps.

  Status: Implemented.

  `docs/PDL_SPEC.md` records the v0.44 history line and updates the
  native-execution section's sink coverage description. Every
  example that exercises CSV or JSON Lines output round-trips
  identically on both engines through the parity harness. Workspace
  `Cargo.toml`, `Cargo.lock`, `editors/vscode/package.json`,
  `editors/vscode/package-lock.json`, and the demo manifest bump to
  `0.44.0`. NPM consumer pins do not move: v0.44 is a native
  Rust/CLI release with no browser-visible behavior change, so
  `pdl-wasm`/`pdl-editor` package versions and consumer pins stay on
  their previously verified values per `CLAUDE.md` "NPM package
  version checks".

## Should

- Land each writer promotion in its own commit.

  Status: Adjusted. The two writers share one streaming emission
  path in `pdl-data` (frame-row iteration feeding the row writers'
  encoders), so splitting them would have meant landing the shared
  plumbing twice. They land together; the change passes `cargo fmt
  --all --check`, `cargo clippy --workspace --all-targets`, `cargo
  test --workspace`, and the parity harness as one unit. Commits are
  authored manually by the maintainer per repository policy.

- Add a `pdl-bench` row-vs-native benchmark for CSV and NDJSON
  emission.

  Status: Implemented.

  v0.44 is the first release where a sink can claim "several times
  faster than the row writer". The writer-dominated
  `million_row_text_emission` workload
  (`bench/workloads/large/million_row_text_emission.pdl`) runs with
  `csv` and `jsonl` output formats next to the existing large-suite
  workloads in `crates/pdl-bench/`.

## Could

- Evaluate configurable CSV dialect support.

  Status: Deferred.

  Native CSV emission with delimiter/quote/escape configuration is
  out of scope for v0.44. The row writer's default dialect is the
  spec; native emission must match.

- Promote remaining row-only sinks (e.g. a hypothetical TSV) to
  native parity.

  Status: Deferred indefinitely.

  PDL supports CSV, JSON Lines, Arrow IPC file, Arrow IPC stream,
  and Parquet. There are no other sink formats to promote.

## Validation Notes

Repository-required Rust checks are authoritative:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets
cargo test --workspace
cargo check -p pdl-wasm --target wasm32-unknown-unknown
```

Parity harness (must pass green at every commit on the v0.44 branch):

```bash
cargo test -p pdl-parity-tests parity_examples
cargo test -p pdl-parity-tests selected_engine_fixtures
```

Selected-engine and sink-strategy confirmation for CSV/NDJSON-terminated
pipelines (`selected_engine` serializes as `"native"`; CSV stdout payloads
report `"bytes-sink"` and CSV/NDJSON saves report `"native-direct-writer"`):

```bash
cargo run -p pdl-cli -- plan examples/top_regions.pdl --json --stdout-format csv | \
  jq '.execution.observability | {selected_engine, sink_strategy}'
# Expected: {"selected_engine": "native", "sink_strategy": "bytes-sink"}
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
- Do not introduce new PDL language surface, new stage keywords, new
  functions, or new diagnostic codes.
- Do not promote sources, stages, or expressions. v0.44 is scoped to
  sink writers only. Source and stage promotions land in v0.45–v0.48.
- Do not delete `native partial` or `planned native` from the matrix
  status vocabulary. That cleanup is v0.49 work.
- Do not silently demote any pipeline that runs natively today.
