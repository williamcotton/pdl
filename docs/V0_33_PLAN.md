# PDL v0.33 Plan

Status: Implemented
Target version: 0.33.0
Owner: PDL maintainers
Related spec: [`PDL_SPEC.md`](PDL_SPEC.md)
Predecessor plan: [`V0_32_PLAN.md`](V0_32_PLAN.md)
Successor plan: [`V0_34_PLAN.md`](V0_34_PLAN.md)

## Purpose

PDL v0.33 follows the v0.32 native execution foundation with targeted
performance fixes and broader native coverage. The v0.32 benchmark pass showed
large wins for native-covered path-backed workloads, but it also exposed
regressions for grouped aggregate workloads that still fall back to the row
runtime.

The first v0.33 goal is to remove that fallback overhead. The second goal is to
turn grouped aggregate pipelines into a native fast path with row-runtime parity
for PDL-visible behavior.

## v0.32 Benchmark Findings

The v0.32 large-suite benchmark compared the checked-in
`full-baseline-20260606` report with two current `auto` engine runs:

```text
workload                                 format             result
million_row_projection_smoke             csv -> csv          +84.6%
million_row_top_scores                   csv -> csv          +80.0%
million_row_distinct_segments            csv -> csv          +58.0%
million_row_segment_summary              csv -> arrow-stream  -1.3%
million_row_segment_summary_parquet      parquet -> csv       -5.4%
million_row_segment_summary_arrow_stream arrow-stream -> csv  -6.2%
million_row_segment_summary              csv -> csv          -27.7%
```

Direct timing showed the aggregate regression was not a row-runtime slowdown:

```text
million_row_segment_summary csv -> csv
baseline:        9.575s
current row:     9.56s
current auto:   11.15s direct / 12.23s benchmark average
current native:  fails quickly because aggregate is unsupported
```

The regression comes from `auto` attempting native execution, discovering that
`group_by`/`agg` cannot run natively, and then executing the full row pipeline.

## v0.33 Benchmark Results

Release-profile benchmarks were run after implementation on 2026-06-06. The
primary comparison is `auto` versus forced `row` in the same release profile:

```text
workload                                    format             row_ms auto_ms native_ms     auto_vs_row
million_row_segment_summary                 csv -> csv           2681    1161      1215         +56.7%
million_row_segment_summary_parquet         parquet -> csv       1900     385       392         +79.7%
million_row_segment_summary_arrow_stream    arrow-stream -> csv  2104    2243 unsupported       -6.6%
million_row_segment_summary                 csv -> arrow-stream  2101     108        78         +94.9%
million_row_top_scores                      csv -> csv           1232     166        79         +86.5%
million_row_projection_smoke                csv -> csv            536      22        17         +95.9%
million_row_distinct_segments               csv -> csv            456      66        57         +85.5%
```

Against the checked-in `full-baseline-20260606` report, the v0.33 release
`auto` run improved every large-suite workload:

```text
million_row_segment_summary                 csv -> csv          +87.9%
million_row_segment_summary_parquet         parquet -> csv      +95.6%
million_row_segment_summary_arrow_stream    arrow-stream -> csv +74.1%
million_row_segment_summary                 csv -> arrow-stream +98.8%
million_row_top_scores                      csv -> csv          +96.3%
million_row_projection_smoke                csv -> csv          +98.3%
million_row_distinct_segments               csv -> csv          +95.0%
```

Interpretation:

- Native grouped aggregate lowering fixed the CSV and Parquet aggregate
  regressions and made those path-backed workloads faster than forced row.
- Existing native wins for projection, top scores, and distinct were preserved.
- The remaining measured outlier is Arrow-stream input. Forced native correctly
  records that workload as unsupported, and `auto` takes the row-compatible
  route, but the release run was still 6.6% slower than forced row. v0.34 should
  target native Arrow-stream input support or eliminate that row-only overhead.

## Must

- Skip native attempts for pipelines that are known to be unsupported.

  Status: Implemented.

  `auto` should classify the main pipeline before opening a native scan or
  constructing a native plan. If the pipeline contains an unsupported stage,
  expression, source shape, sink, or output mode, `auto` should go directly to
  the row runtime. Forced `--engine native` should keep returning the normal PDL
  diagnostic for unsupported native execution.

  Acceptance criteria:

  - Grouped aggregate workloads no longer regress merely because `auto` tries
    native before row fallback.
  - Unsupported `group_by`, `agg`, joins, unions, windows, `pivot_longer`,
    `complete`, non-terminal saves, byte-backed inputs, and unsupported scalar
    functions are rejected by eligibility checks before native scans are
    opened.
  - Eligibility checks are tested independently from execution.
  - `--engine row`, `--engine auto`, and `--engine native` keep their current
    user-visible semantics.

- Add native grouped aggregate coverage for the benchmark-critical case.

  Status: Implemented.

  Implement native lowering for path-backed `group_by` followed by `agg` using
  the aggregate functions that appear in the large workloads first: `count`,
  `sum`, `mean`, `min`, and `max`.

  Acceptance criteria:

  - Native aggregate output matches row-runtime output for grouping key order,
    aggregate column aliases, null handling, numeric normalization, and
    deterministic sorting after aggregation.
  - Parity tests cover CSV and Parquet inputs.
  - The CSV and Parquet `million_row_segment_summary` workloads improve versus
    both the checked-in baseline and the forced row engine.
  - If any aggregate parity rule is uncertain, `auto` uses row execution and
    forced `native` reports a diagnostic.

- Extend `pdl-bench` so performance attribution is explicit.

  Status: Implemented.

  The benchmark harness should make it easy to compare row, auto, and native
  behavior without ad hoc shell timing.

  Acceptance criteria:

  - `pdl-bench run` can record an engine mode in the report, either by running
    one selected engine or by producing row/auto/native rows for each workload.
  - Reports include enough notes to distinguish native success, planned row
    execution, and unsupported-native fallback.
  - The large suite can emit a comparison summary against a baseline report.
  - Release benchmark reports use a consistent profile policy. Debug runs may
    remain useful for development, but release-profile numbers should be
    captured before marking the plan implemented.

- Preserve and expand the v0.32 native wins.

  Status: Implemented.

  Native execution should keep improving the workloads that already benefit:
  projection, filter/select/sort/limit, and distinct.

  Acceptance criteria:

  - `million_row_projection_smoke`, `million_row_top_scores`, and
    `million_row_distinct_segments` stay faster than the row engine.
  - Row/native parity checks continue comparing output bytes for these
    workloads.
  - Any change that makes these workloads slower must be explained in the plan
    or fixed before release.

## Should

- Follow a staged implementation order.

  Status: Implemented.

  Recommended order:

  1. Add `pdl-bench` engine-mode reporting and baseline comparison output.
  2. Add native eligibility checks so `auto` avoids known unsupported pipelines.
  3. Re-run the large suite and confirm aggregate workloads return to row-level
     timings while native-covered workloads keep their v0.32 wins.
  4. Implement native grouped aggregate lowering with parity tests.
  5. Re-run release-profile benchmarks and promote the results into the plan
     before marking it implemented.

- Add cross-repo PDL-to-Algraf Arrow stream smoke coverage.

  Status: Planned.

  Validate the producer/consumer path when local Algraf tooling is available:

  ```bash
  pdl run prep.pdl --stdout-format arrow-stream \
    | algraf render chart.ag --data - --data-format arrow-stream --output chart.svg
  ```

- Evaluate additional native path-backed operations.

  Status: Planned.

  Candidate operations include `mutate`, non-grouped aggregate forms if they
  emerge, and terminal save paths that can write typed output without collecting
  to row values.

- Keep small and browser-style workloads from regressing.

  Status: Planned.

  Native optimization should not add meaningful overhead to row-only workloads,
  editor/WASM paths, or small data runs where the row engine is already the
  better fit.

## Deferred From v0.32

- Mid-pipeline fallback from native plans to the row runtime.
- Byte-backed native readers for stdin and browser-hosted files.
- Native coverage for `mutate`, joins, unions, windows, `pivot_longer`, and
  `complete`.

## Validation

Required repository checks before this plan can be marked implemented:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets
cargo test --workspace
RUSTC=$(rustup which --toolchain stable rustc) \
  rustup run stable cargo check -p pdl-wasm --target wasm32-unknown-unknown
cargo tree -p pdl-wasm --target wasm32-unknown-unknown
```

Required benchmark validation:

```bash
cargo run -p pdl-bench -- run --suite large --tier stress \
  --run-label v0_33_row_release --profile release --engine row --no-generate
cargo run -p pdl-bench -- run --suite large --tier stress \
  --run-label v0_33_auto_release --profile release --engine auto --no-generate
cargo run -p pdl-bench -- run --suite large --tier stress \
  --run-label v0_33_native_release --profile release --engine native --no-generate
cargo run -p pdl-bench -- compare \
  --baseline bench/runs/v0_33_row_release/report.csv \
  --run-label v0_33_auto_release
cargo run -p pdl-bench -- compare \
  --baseline bench/runs/v0_33_row_release/report.csv \
  --run-label v0_33_native_release
cargo run -p pdl-bench -- compare --baseline full-baseline-20260606 \
  --run-label v0_33_auto_release
```

Completed validation:

- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets`
- `cargo test --workspace`
- `RUSTC=$(rustup which --toolchain stable rustc) rustup run stable cargo check -p pdl-wasm --target wasm32-unknown-unknown`
- `cargo tree -p pdl-wasm --target wasm32-unknown-unknown` followed by a
  case-insensitive search for `polars`, `parquet`, and `arrow`.

The WASM dependency tree check must show no `polars` packages in the
`pdl-wasm` target graph. The completed check produced no matches for `polars`,
`parquet`, or `arrow`.
