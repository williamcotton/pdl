# PDL v0.46 Plan

Status: Implemented (0.46.0)
Target version: 0.46.0
Owner: PDL maintainers
Related spec: [`PDL_SPEC.md`](PDL_SPEC.md)
Coverage matrix: [`PDL_NATIVE_COVERAGE.md`](PDL_NATIVE_COVERAGE.md), [`PDL_NATIVE_COVERAGE.csv`](PDL_NATIVE_COVERAGE.csv)
Predecessor plan: [`V0_45_PLAN.md`](V0_45_PLAN.md)
Successor plan: [`V0_47_PLAN.md`](V0_47_PLAN.md)

## Purpose

PDL v0.46 promotes the remaining row-only sources. Today the native
engine scans path-backed CSV and Parquet lazily, and accepts Arrow
IPC bytes from stdin and host files, but every other non-path input
demotes the whole pipeline to the row engine. Three areas flip:

1. Stdin byte-backed scans. CSV and Parquet bytes arriving on stdin
   become native through byte-backed scan adapters in `pdl-data`.
   This retires `StdinBytesBackedScan` (reserved in v0.43) for the
   promoted formats.
2. Host byte-backed scans. CSV and Parquet contents supplied as
   in-memory host files through `DriverIo` (no real filesystem path)
   become native through the same byte-backed adapters. This retires
   `HostBytesBackedScan` (reserved in v0.43) for the promoted
   formats.
3. Path-backed Arrow IPC. The Arrow IPC file and Arrow IPC stream
   source rows flip from `native partial` (eager whole-file read into
   the native dataframe) to `native parity`, using a lazy IPC scan
   where Polars supports it and keeping the eager read where it does
   not. Output bytes are unchanged either way.

JSON Lines sources are explicitly not promoted: schema inference and
text semantics stay on the row runtime, so the JSON Lines source row
(path, stdin, and host bytes) keeps its `row-only by design` status
with an explicit v0.46 disposition note.

The release is gated by the v0.43 parity harness and the v0.44
native CSV / NDJSON writers. With v0.46 in place, a `load`-from-stdin
pipeline that writes any supported format can run end-to-end on the
native engine, which is what the v0.47 expression and v0.48
pipeline-shape promotions assume.

## Implemented Scope

Three rules hold at every commit:

1. Row-runtime byte parity. A byte-backed native scan produces output
   bytes byte-identical to the row runtime over the parity corpus for
   every promoted format. Schema inference, type coercion on read,
   null parsing, and row order match the row engine exactly. Where a
   Polars byte-backed reader diverges from the row reader, the
   adapter configures or normalizes it to match; if a divergence
   cannot be removed, the affected subcase stays row-only with the
   existing reserve reason rather than shipping approximate parity.
2. Sniffing bytes are preserved. Stdin format resolution follows the
   spec order (explicit format, CLI override, extension, magic-byte
   sniffing, text sniffing, CSV fallback) and the byte-backed adapter
   receives the full stream including any bytes the sniffer consumed.
   The CSV-fallback path uses the same byte-backed CSV scan as
   explicit CSV.
3. WASM stays Polars-free. The byte-backed adapters live in
   `pdl-data` behind the native-only modules or feature gates that
   already isolate Polars. `pdl-wasm` continues to execute host-byte
   inputs on the row engine; the wasm target graph audit remains
   empty.

The release does not introduce new PDL language surface, new stage
keywords, new functions, new diagnostic codes, or new format support.
It widens which existing source reads execute on the native engine.

## Promotion Scope

### Sources

- Stdin CSV. CSV bytes on stdin lower to a byte-backed Polars lazy
  CSV scan. Explicitly declared, extension-resolved, sniffed, and
  CSV-fallback stdin all route through the same adapter.
- Stdin Parquet. Parquet bytes on stdin are buffered (Parquet
  requires the footer before reading, matching what the row engine
  already does) and lower to a byte-backed Parquet read.
- Host byte-backed CSV and Parquet. The same adapters accept
  host-supplied in-memory file contents routed through `DriverIo`
  when no real filesystem path is available.
- Path-backed Arrow IPC file and stream. Status flips from
  `native partial` to `native parity`; the file form moves to a lazy
  IPC scan where Polars supports it, the stream form keeps the eager
  read into the native dataframe. Stdin and host-byte Arrow IPC are
  already native (unchanged).

### Coverage matrix

- `source,stdin` flips: CSV and Parquet stdin bytes become
  `native parity`; JSON Lines stdin stays `row-only by design`
  (inheriting the JSON Lines source disposition); the note no longer
  lists CSV and Parquet as row-only.
- `source,byte-backed host files` flips: CSV and Parquet host bytes
  become `native parity`; JSON Lines host bytes stay
  `row-only by design`.
- `source,path-backed Arrow IPC file` and
  `source,path-backed Arrow IPC stream` flip from `native partial`
  to `native parity`.
- `source,JSON Lines` keeps `row-only by design` with an updated
  note recording the v0.46 disposition.
- `stage,load` note updates to reflect the widened native source
  set.

## Must

- Promote stdin CSV to native parity via a byte-backed scan adapter.

  Status: Implemented.

  The adapter lives in `crates/pdl-data/src/engine.rs` behind the
  `polars-engine` gate and wraps in-memory bytes in
  `ScanSources::Buffers`, so byte-backed CSV runs through the same
  Polars lazy CSV scan as path-backed CSV. Two row-reader divergences
  were normalized in the adapter: the native reader keeps doubled-quote
  escapes inside quoted header cells verbatim, so scanned column names
  are renamed positionally to the row reader's header parse; and the
  native reader rejects empty input that the row reader accepts as a
  zero-column table, so empty bytes scan as an empty native frame. The
  eligibility flip in `crates/pdl-exec/src/runtime/native_planning.rs`
  removed `StdinBytesBackedScan` for CSV stdin. The parity corpus in
  `pdl-data` covers empty input, header-only input, embedded
  delimiters / quotes / newlines, multibyte UTF-8 in headers and
  cells, null parsing, numeric edge values, and a 50k-row input that
  crosses reader buffer and schema-inference boundaries;
  sniffed-versus-explicit resolution is covered by `pdl-exec` runtime
  tests. The `examples/stdin_orders_csv.pdl` `selected_engine`
  fixture flipped from row to native in this change.

- Promote stdin Parquet to native parity via a byte-backed read.

  Status: Implemented.

  Same shape as stdin CSV. The stream is buffered to completion
  before the footer-driven read, matching row-engine behavior. The
  adapter uses the eager Polars `ParquetReader` over the buffered
  bytes rather than `scan_parquet_sources`: the lazy buffer scan's
  hive-partitioning path is unimplemented in Polars 0.53 for
  in-memory buffers, and the bytes are already fully resident.
  Parity corpus covers empty tables, single-row-group and
  multi-row-group files, and nullable columns.

- Promote host byte-backed CSV and Parquet scans.

  Status: Implemented.

  The stdin adapters are reused for host-supplied in-memory file
  contents routed through `DriverIo`. The eligibility flip removed
  `HostBytesBackedScan` for the promoted formats. Parity coverage
  reuses the stdin corpus through the host-bytes entry point plus
  `pdl-exec` runtime tests over `InMemoryDriverIo`. The promotion
  exposed one latent native-lowering divergence: native `round`
  returned `-0` where the row runtime normalizes to `0`; the native
  lowering now remaps negative-zero results to `0`.

- Flip path-backed Arrow IPC file and stream rows to native parity.

  Status: Implemented.

  The Arrow IPC file source moved to `LazyFrame::scan_ipc` (the
  `ipc_streaming` feature already enables the lazy IPC scan); the
  stream source keeps the existing eager read into the native
  dataframe. Both rows flipped to `native parity`; the parity
  harness proves byte/table equality for every Arrow-sourced
  example.

- Record the JSON Lines source disposition.

  Status: Implemented. The planner reports `input-format` for JSON
  Lines stdin and host bytes, matching path-backed JSON Lines.

  JSON Lines sources (path, stdin, host bytes) stay
  `row-only by design`: schema inference and text semantics remain
  row-runtime behavior. The matrix note records that v0.46
  considered and declined the promotion; the planner attaches the
  same fallback reason it attaches to path-backed JSON Lines today
  rather than the retired scan-adapter reserves.

- Update the coverage matrix in lockstep.

  Status: Implemented.

  `docs/PDL_NATIVE_COVERAGE.md` and `docs/PDL_NATIVE_COVERAGE.csv`
  updated with the promotions. `examples/stdin_orders_csv.pdl` is the
  only stdin- or host-byte-sourced example, and its `selected_engine`
  fixture flipped from row to native per the v0.43 protocol.

- Hold the WASM target graph.

  Status: Implemented.

  `pdl-wasm` Cargo manifest is unchanged. `cargo check -p pdl-wasm
  --target wasm32-unknown-unknown` remains green. The byte-backed
  adapters are native-only; `pdl-wasm` host-byte execution stays on
  the row engine. One new dependency landed behind the existing
  `polars-engine` gate: `polars-buffer` (version-locked to the
  `polars` pin) supplies the buffer type `ScanSources::Buffers`
  takes. The wasm target graph audit stays empty.

- Update the spec, examples, and release stamps.

  Status: Implemented.

  `docs/PDL_SPEC.md` records the v0.46 history line and updates the
  native-execution section's source coverage description. Every
  stdin and host-byte example round-trips identically on both
  engines through the parity harness. Workspace `Cargo.toml`,
  `Cargo.lock`, `editors/vscode/package.json`,
  `editors/vscode/package-lock.json`, and the demo manifests bumped
  to `0.46.0`. Per `CLAUDE.md` "NPM package version checks", the
  `pdl-wasm` / `pdl-editor` consumer pins stay at the published
  `0.43.5` / `0.43.6`; no `0.46.x` browser packages are prepared
  (recorded in `docs/NPM_PACKAGES.md`).

## Should

- Land each source promotion in its own commit.

  Status: Adapted.

  The promotions were implemented and validated as one working-tree
  change; commit slicing is left to the human author per the
  repository commit policy. The full check set (`cargo fmt --all
  --check`, `cargo clippy --workspace --all-targets`, `cargo test
  --workspace`, the parity harness, and the wasm target audit)
  passes on the combined change.

- Audit retirement of the v0.43 scan-adapter reserves.

  Status: Implemented.

  `pdl plan --json` over every example reports only
  `binding-start-not-eligible`, `input-format`,
  `named-output-mixed-engines`, and `window-expression` fallback
  reasons; no producer site for `StdinBytesBackedScan` or
  `HostBytesBackedScan` remains. The variants stay in the
  `NativeUnsupportedReason` vocabulary until the v0.49 cleanup.

- Add a `pdl-bench` row-vs-native benchmark for a stdin-sourced
  pipeline.

  Status: Implemented.

  `bench/workloads/large/million_row_mutate_csv_stdin.pdl` mirrors
  `million_row_mutate_csv` with `load stdin format "csv"`, and the
  large suite feeds the generated million-row CSV to the workload on
  stdin (a `stdin_path` field on the bench `Workload` descriptor),
  giving the v0.43–v0.49 performance thesis a number for the
  byte-backed scan path.

## Could

- Promote JSON Lines sources to native parity.

  Status: Deferred indefinitely.

  Requires pinning the row engine's schema-inference and text
  semantics as a spec-final contract first, mirroring the v0.47
  numeric-coercion approach. No release in the v0.43–v0.49 arc
  takes this on; the row is `row-only by design` until a dedicated
  plan exists.

- Evaluate object-store and remote path support with a dedicated
  security and IO plan.

  Status: Deferred.

  Remote inputs are a separate plan with its own security model.
  v0.49 reserves `RemotePathNotSupported` for the split.

## Validation Notes

Repository-required Rust checks are authoritative:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets
cargo test --workspace
cargo check -p pdl-wasm --target wasm32-unknown-unknown
```

Parity harness (must pass green at every commit on the v0.46 branch;
the harness supplies each stdin example's fixture on stdin for both
engines):

```bash
cargo test -p pdl-parity-tests parity_examples
cargo test -p pdl-parity-tests selected_engine_fixtures
```

Selected-engine confirmation for a stdin-sourced example (the harness
stdin fixture matches the example's expected schema):

```bash
cargo run -p pdl-cli -- plan examples/stdin_orders_csv.pdl --json \
  < crates/pdl-parity-tests/fixtures/stdin/stdin_orders_csv.csv | \
  jq '.execution.observability.selected_engine'
# Expected: "native"
```

Row-vs-native byte check for the same example:

```bash
FIXTURE=crates/pdl-parity-tests/fixtures/stdin/stdin_orders_csv.csv
cargo run -p pdl-cli -- run examples/stdin_orders_csv.pdl \
  --stdout-format csv --engine row < "$FIXTURE" > /tmp/row.out
cargo run -p pdl-cli -- run examples/stdin_orders_csv.pdl \
  --stdout-format csv --engine native < "$FIXTURE" > /tmp/native.out
diff /tmp/row.out /tmp/native.out
# Expected: empty
```

WASM target graph audit:

```bash
cargo tree -p pdl-wasm --target wasm32-unknown-unknown | grep -E 'polars|arrow|parquet'
# must be empty
```

## Non-Goals

- Do not change CSV, JSON Lines, Arrow IPC, or Parquet output bytes
  on the row engine. The row reader and writer are the spec.
- Do not introduce Polars, Arrow, or Parquet into the `pdl-wasm`
  dependency graph.
- Do not introduce new PDL language surface, new stage keywords, new
  functions, or new diagnostic codes.
- Do not change the stdin format-resolution order or consume sniffing
  bytes. The spec's resolution order and the preserved-bytes contract
  are unchanged.
- Do not promote JSON Lines source semantics. The row stays
  `row-only by design`.
- Do not promote stages, expressions, or pipeline shape. Stages
  landed in v0.45; expressions land in v0.47; pipeline-shape changes
  land in v0.48.
- Do not introduce object-store, network, or remote-path support.
- Do not delete `native partial` or `planned native` from the matrix
  status vocabulary. That cleanup is v0.49 work.
- Do not silently demote any pipeline that runs natively today.
