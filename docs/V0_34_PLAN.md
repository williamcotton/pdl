# PDL v0.34 Plan

Status: In progress
Target version: 0.34.0
Owner: PDL maintainers
Related spec: [`PDL_SPEC.md`](PDL_SPEC.md)
Predecessor plan: [`V0_33_PLAN.md`](V0_33_PLAN.md)
Neighboring Algraf plan: [`V0_69_PLAN.md`](../../algraf/docs/V0_69_PLAN.md)

## Purpose

PDL v0.34 is the production native-pipeline release after v0.33. The v0.33
release benchmarks show broad wins for path-backed CSV and Parquet workloads,
with Arrow-stream input remaining the only measured large-suite outlier.

The v0.34 goal is to turn PDL into a stronger producer for production data
pipelines and Algraf visualization workflows:

```bash
pdl run prep.pdl --stdout-format arrow-stream \
  | algraf render chart.ag --data - --data-format arrow-stream --output chart.svg
```

PDL should use Polars where it provides native columnar execution, lazy scans,
predicate/projection pushdown, typed kernels, grouping, sorting, and efficient
writers. PDL should use Arrow IPC streams as the typed process boundary for
downstream consumers such as Algraf. The portable row runtime remains the
reference behavior and the fallback when native parity is uncertain.

Browser/WASM builds remain a separate product surface. v0.34 MUST keep Polars
completely out of the `pdl-wasm` target dependency graph.

## Release Thesis

v0.34 should make this shape fast and dependable:

```text
large CSV, Parquet, or Arrow input
  -> PDL native Polars plan
  -> typed filtered, projected, joined, grouped, or summarized table
  -> Arrow IPC stream stdout or path-backed artifact
  -> Algraf typed caller data
  -> bounded visual aggregate output
```

The release is not about exposing Polars or Arrow implementation types to the
PDL language. It is about using them behind `pdl-data` to remove row conversion,
avoid unnecessary full-buffer materialization, preserve deterministic semantics,
and produce an Arrow stream that downstream tools can consume without guessing.

## Starting Point

v0.33 release-profile benchmarks produced these `auto` versus forced `row`
results:

```text
workload                                    format             row_ms auto_ms     auto_vs_row
million_row_segment_summary                 csv -> csv           2681    1161         +56.7%
million_row_segment_summary_parquet         parquet -> csv       1900     385         +79.7%
million_row_segment_summary_arrow_stream    arrow-stream -> csv  2104    2243          -6.6%
million_row_segment_summary                 csv -> arrow-stream  2101     108         +94.9%
million_row_top_scores                      csv -> csv           1232     166         +86.5%
million_row_projection_smoke                csv -> csv            536      22         +95.9%
million_row_distinct_segments               csv -> csv            456      66         +85.5%
```

The v0.34 plan treats the Arrow-stream input regression as the first measured
problem to fix, then expands native coverage so production preparation
pipelines can stay in Polars until the final Arrow handoff.

## Initial v0.34 Slice Results

The first implementation slice makes path-backed Arrow IPC stream input
eligible for native execution when the rest of the pipeline is natively
supported. The large-suite release benchmarks were run on 2026-06-06:

```text
workload                                    format             row_ms auto_ms native_ms auto_vs_row
million_row_segment_summary                 csv -> csv           3449     963       897      +72.1%
million_row_segment_summary_parquet         parquet -> csv       2126      44        30      +97.9%
million_row_segment_summary_arrow_stream    arrow-stream -> csv  2060      64        36      +96.9%
million_row_segment_summary                 csv -> arrow-stream  2019      75        74      +96.3%
million_row_top_scores                      csv -> csv           1355      99        85      +92.7%
million_row_projection_smoke                csv -> csv            628      21        19      +96.7%
million_row_distinct_segments               csv -> csv            466      68        69      +85.4%
```

Against the checked-in `full-baseline-20260606` report, the v0.34 `auto`
release run improved all large-suite workloads:

```text
million_row_segment_summary                 csv -> csv          +89.9%
million_row_segment_summary_parquet         parquet -> csv      +99.5%
million_row_segment_summary_arrow_stream    arrow-stream -> csv +99.3%
million_row_segment_summary                 csv -> arrow-stream +99.2%
million_row_top_scores                      csv -> csv          +97.8%
million_row_projection_smoke                csv -> csv          +98.4%
million_row_distinct_segments               csv -> csv          +94.8%
```

Interpretation:

- The v0.33 Arrow-stream input outlier is fixed for path-backed streams used by
  the large benchmark. It moved from slower than forced row to 96.9% faster than
  forced row in this release run.
- Forced `native` now supports the Arrow-stream aggregate workload instead of
  reporting it as unsupported.
- Byte-backed and stdin Arrow streams still intentionally take the row path in
  `auto`; native reader support for those host boundaries remains open.

## Must

- Make Arrow-stream input a first-class native execution path.

  Status: Partially implemented.

  PDL already writes Arrow IPC streams efficiently in native paths. v0.34 should
  make Arrow IPC stream input participate in the same native execution strategy
  wherever the source and stages have parity coverage.

  Implemented slice:

  - Path-backed Arrow IPC stream inputs are eligible for `auto` native execution
    when the rest of the pipeline is natively supported.
  - Forced `native` supports the large-suite Arrow-stream grouped aggregate
    workload.
  - The native data path reads Arrow stream files into Polars without converting
    through public row `Value` objects.
  - `load stdin format "arrow-stream"` continues to go directly to the row
    runtime in `auto` because byte-backed native readers remain deferred.
  - Row/native/auto parity tests cover CSV, Parquet, and path-backed
    Arrow-stream grouped aggregate input.

  Acceptance criteria:

  - Path-backed Arrow IPC stream inputs are eligible for `auto` native execution
    when the rest of the pipeline is natively supported.
  - `load stdin format "arrow-stream"` has an explicit eligibility decision:
    either it takes a native reader path with bounded buffering or it goes
    directly to the row runtime without failed-native overhead.
  - If the pinned Polars version supports a lazy Arrow IPC scan for the source
    shape, `pdl-data` should use it through the existing opaque facade.
    Otherwise, Arrow record batches should enter the native table path without
    converting through public row `Value` objects.
  - Multi-batch streams, null-heavy arrays, strings, booleans, integers,
    floats, and temporal columns have parity tests against the row runtime.
  - Unsupported Arrow physical or logical types produce deterministic PDL
    diagnostics. `auto` falls back to rows only when the row runtime supports
    the type; forced `native` reports unsupported native execution.
  - `million_row_segment_summary_arrow_stream` is no more than 5% slower than
    forced row by release-profile median. The stretch target is at least 50%
    faster than forced row for path-backed Arrow-stream input.

- Expand native Polars expression and stage coverage.

  Status: Planned.

  v0.33 proved grouped aggregate parity for simple column references. v0.34
  should broaden the subset of PDL pipelines that can stay in Polars without
  changing the language surface.

  Acceptance criteria:

  - Native `mutate` supports simple existing PDL expressions: column
    references, literals, casts where semantics are specified, arithmetic,
    comparisons, boolean combinations, null checks, and string functions whose
    Polars behavior matches row semantics.
  - Native aggregate arguments can use supported simple expressions, not only
    raw column references, when aliasing, null behavior, and numeric
    normalization match row output.
  - Native filters preserve predicate pushdown opportunities for CSV, Parquet,
    and Arrow-backed inputs.
  - Native select/drop/rename/mutate/filter planning preserves projection
    pushdown so large unused columns are not read just to be discarded.
  - Unsupported expressions are rejected by eligibility checks before native
    scans are opened.
  - Parity tests cover row/native equality for each newly lowered expression
    family and at least one combined production-style pipeline.

- Keep native writers columnar through the terminal sink.

  Status: Planned.

  Production pipelines should not collect a native table into public rows just
  to write Arrow, Parquet, or CSV at the end.

  Acceptance criteria:

  - Terminal `save` and `--stdout-format` paths write from native plans through
    writer-oriented sinks whenever the active plan is native.
  - Arrow IPC stream stdout is valid, deterministic, and readable by Arrow IPC
    consumers after projection, mutation, grouped aggregate, distinct, sort,
    and limit stages.
  - Parquet and Arrow file sinks preserve logical schema, field order, null
    metadata, and deterministic output where the format permits it.
  - CSV output preserves current row-visible formatting semantics even when
    produced by the native backend.
  - Diagnostics, progress output, and benchmark notes stay on stderr or sidecar
    files. Stdout remains data bytes only.

- Establish a production PDL-to-Algraf Arrow contract.

  Status: Planned.

  PDL owns data preparation and typed stream production. Algraf owns chart
  semantics, aggregation for visual marks, and rendering. The boundary between
  them is Arrow IPC stream bytes, not PDL syntax or Polars objects.

  Acceptance criteria:

  - Add a cross-repo smoke path equivalent to:

    ```bash
    pdl run prep.pdl --stdout-format arrow-stream \
      | algraf render chart.ag --data - --data-format arrow-stream --output chart.svg
    ```

  - The smoke fixture covers a realistic chart-prep table with numeric,
    categorical string, boolean or flag, temporal, and nullable fields.
  - PDL output schema order and names are deterministic and match what Algraf
    receives.
  - PDL can produce visual-ready aggregate tables that let Algraf render bounded
    marks instead of raw million-row mark sets.
  - The handoff works with explicit `--data-format arrow-stream` and with
    Algraf's sniffed caller-data path when that path is available.
  - Malformed streams, unsupported Arrow types, and downstream consumer failures
    are diagnosable without mixing logs into the Arrow stdout stream.
  - The plan stays aligned with Algraf
    [`V0_69_PLAN.md`](../../algraf/docs/V0_69_PLAN.md). Algraf must not parse
    `.pdl`, and PDL must not import Algraf chart semantics.

- Add production-grade native planning observability.

  Status: Planned.

  Users need to know whether a pipeline ran natively, why it fell back, and
  whether output was written through a columnar sink.

  Acceptance criteria:

  - `pdl plan` or the run manifest reports the selected execution engine,
    native eligibility, fallback reason, input format, output format, and
    terminal sink strategy.
  - Native eligibility reasons are stable enough for tests and benchmark notes.
  - `pdl-bench` records row/auto/native status, elapsed time, output rows,
    output bytes, engine notes, and unsupported-native cases without treating
    expected unsupported native workloads as failed benchmark runs.
  - Release benchmark summaries compare `auto` against both forced `row` and
    forced `native` where forced native is supported.

- Keep Polars and native formats out of WASM.

  Status: Planned.

  v0.34 should expand native execution without bloating browser builds or
  accidentally pulling Polars into `pdl-wasm`.

  Acceptance criteria:

  - `pdl-wasm` depends on workspace crates with `default-features = false`.
  - `cargo check -p pdl-wasm --target wasm32-unknown-unknown` passes.
  - `cargo tree -p pdl-wasm --target wasm32-unknown-unknown` has no `polars`
    entries.
  - Any future browser Arrow byte support is scoped separately and must not
    enable `polars-engine`, `native-formats`, Parquet, or native filesystem IO.
  - Workspace boundary tests fail if parser, syntax, semantics,
    editor-services, LSP protocol, CLI command models, or WASM public APIs
    expose concrete Polars types.

## Should

- Add a native coverage matrix.

  Status: Planned.

  Document each stage, expression family, source format, sink format, and host
  boundary as one of: native parity, row-only by design, planned native, or
  unsupported. This matrix should live near the spec or benchmark docs and
  should drive both eligibility tests and release notes.

- Evaluate native equi-join and union coverage.

  Status: Planned.

  Production preparation pipelines often join fact tables to small dimensions
  or union partitioned extracts. v0.34 should evaluate a conservative Polars
  native subset:

  - inner and left equi-joins on named columns;
  - deterministic duplicate-column naming;
  - schema-compatible `union` or concat;
  - explicit row fallback for ambiguous joins, non-equi joins, and uncertain
    null-key semantics.

  These should ship only if row/native parity tests cover output order, null
  behavior, duplicate names, and diagnostics.

- Use Polars lazy optimization more deliberately.

  Status: Planned.

  Native plans should preserve opportunities for predicate pushdown, projection
  pushdown, aggregate pushdown, row-group pruning, and streaming or batched
  collection where the pinned Polars version supports them. The PDL facade
  should keep this implementation detail private while making the benchmark
  benefit visible.

- Improve Arrow and Parquet schema fidelity.

  Status: Planned.

  PDL should normalize logical schemas consistently across CSV inference,
  Parquet metadata, Arrow IPC file input, Arrow IPC stream input, and native
  output. Tests should cover nullable fields, temporal units, integer and float
  widths, booleans, Utf8 strings, and deterministic rejection of unsupported
  nested or dictionary types until those types have PDL-visible semantics.

- Add repeated release benchmarking and variance reporting.

  Status: Planned.

  `pdl-bench` should support repeated samples or make repeated labels easy to
  summarize. Release notes should report medians, worst sample or p95 when
  useful, and any known run-to-run variance. Benchmarks should include:

  - PDL large suite row/auto/native;
  - Arrow-stream input aggregate;
  - CSV, Parquet, and Arrow variants of the same workload;
  - PDL-to-Algraf Arrow pipe smoke timing;
  - output byte and row counts for parity sanity checks.

- Add memory and allocation visibility.

  Status: Planned.

  Production pipelines need stable memory behavior, not only lower wall-clock
  time. v0.34 should record peak RSS where practical and add targeted tests or
  benchmark notes for workloads that previously collected native data into row
  values before writing.

- Preserve small-data and editor responsiveness.

  Status: Planned.

  Native planning should not add meaningful overhead to tiny files, row-only
  browser workflows, editor previews, schema inspection, completions, or
  diagnostics. If native startup overhead dominates, `auto` may choose the row
  runtime for small inputs when that choice is deterministic and documented.

## Non-Goals

- No Polars types in parser, syntax, semantic analysis, LSP, editor-services,
  CLI public command models, WASM public APIs, or PDL source syntax.
- No Algraf parser, renderer, or chart semantics inside PDL.
- No PDL parser or executor inside Algraf.
- No required browser/WASM Polars, Parquet, or native filesystem support.
- No distributed execution, SQL planner, remote object-store credentials, or
  long-running service runtime in this release.

## Deferred From v0.33

- Mid-pipeline fallback from native plans to the row runtime.
- Byte-backed native readers for stdin and browser-hosted files.
- Native coverage for joins, unions, windows, `pivot_longer`, and `complete`.

## Validation

Required repository checks:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets
cargo test --workspace
RUSTC=$(rustup which --toolchain stable rustc) \
  rustup run stable cargo check -p pdl-wasm --target wasm32-unknown-unknown
cargo tree -p pdl-wasm --target wasm32-unknown-unknown
```

The WASM dependency tree must show no `polars` package. The release validation
should also search for `parquet` and `arrow` in the `pdl-wasm` tree so native
format dependencies do not slip into the browser target accidentally.

Required release benchmark commands:

```bash
cargo run -p pdl-bench -- run --suite large --tier stress \
  --run-label v0_34_row_release --profile release --engine row --no-generate
cargo run -p pdl-bench -- run --suite large --tier stress \
  --run-label v0_34_auto_release --profile release --engine auto --no-generate
cargo run -p pdl-bench -- run --suite large --tier stress \
  --run-label v0_34_native_release --profile release --engine native --no-generate
cargo run -p pdl-bench -- compare \
  --baseline bench/runs/v0_34_row_release/report.csv \
  --run-label v0_34_auto_release
cargo run -p pdl-bench -- compare \
  --baseline full-baseline-20260606 \
  --run-label v0_34_auto_release
```

Required cross-repo validation when Algraf tooling is available:

```bash
pdl run prep.pdl --stdout-format arrow-stream \
  | algraf render chart.ag --data - --data-format arrow-stream --output chart.svg
```

The final v0.34 plan update should record release-profile benchmark results,
the PDL-to-Algraf smoke result, and the WASM dependency-tree result before the
plan is marked implemented.

Completed for the initial Arrow-stream slice:

- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets`
- `cargo test --workspace`
- `RUSTC=$(rustup which --toolchain stable rustc) rustup run stable cargo check -p pdl-wasm --target wasm32-unknown-unknown`
- `cargo tree -p pdl-wasm --target wasm32-unknown-unknown` followed by a
  case-insensitive search for `polars`, `parquet`, and `arrow`; no matches were
  found.
