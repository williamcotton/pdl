# PDL v0.35 Plan

Status: In progress
Target version: 0.35.0
Owner: PDL maintainers
Related spec: [`PDL_SPEC.md`](PDL_SPEC.md)
Predecessor plan: [`V0_34_PLAN.md`](V0_34_PLAN.md)
Neighboring Algraf plan: [`V0_69_PLAN.md`](../../algraf/docs/V0_69_PLAN.md)

## Purpose

PDL v0.35 carries forward the v0.34 production native-pipeline work that did
not ship in the initial v0.34 implementation slice. v0.34 fixed the measured
path-backed Arrow IPC stream input outlier, but large production preparation
pipelines still fall back to rows whenever they need native `mutate`, richer
expression lowering, byte-backed Arrow input, deeper sink coverage, or
end-to-end PDL-to-Algraf validation. v0.35 must make ordinary derived-column
work fast by lowering supported `mutate` assignments to Polars lazy expressions
rather than evaluating them row by row.

The v0.35 goal is to make this pipeline shape native and production-ready:

```bash
pdl run prep.pdl --engine auto --stdout-format arrow-stream \
  | algraf render chart.ag --data - --data-format arrow-stream --output chart.svg
```

The language surface does not change. The portable row runtime remains the
reference implementation. Polars and Arrow remain private implementation
details behind `pdl-data`, and browser/WASM builds MUST keep Polars, Parquet,
native Arrow readers, and native filesystem assumptions out of the target
dependency graph.

## v0.34 Carry-Forward Inventory

The following v0.34 plan items were not completed by the initial v0.34 slice
and are promoted into v0.35:

- Fast native Polars-backed `mutate` support for simple row-expression parity.
- Native expression lowering beyond raw column references.
- Projection and predicate pushdown through select/drop/rename/mutate/filter.
- Columnar terminal writers for native plans across `save` and
  `--stdout-format` paths.
- Production PDL-to-Algraf Arrow IPC stream smoke coverage.
- Native planning observability and benchmark attribution.
- A native coverage matrix that drives eligibility tests.
- Deliberate Polars lazy optimization and schema fidelity work.
- Repeated benchmark and variance reporting, including PDL-to-Algraf timing.
- Memory and allocation visibility for large native workloads.
- Continued small-data, editor, and WASM responsiveness protection.
- Evaluation of byte-backed/stdin Arrow-stream native readers.
- Evaluation of conservative native join and union coverage.

## Release Thesis

v0.35 should make native execution useful for ordinary data-prep programs, not
only projection, sorting, distinct, and grouped aggregate benchmark shapes. The
critical unlock is a fast native Polars-backed `mutate`, because realistic
pipelines derive fields before filtering, grouping, writing Arrow IPC, or
handing the table to Algraf.

PDL should classify native eligibility before opening scans, lower only the
expression subset with proven row/native parity, and keep unsupported programs
on the row runtime in `auto`. Forced `--engine native` should fail early with a
stable diagnostic when native parity is not available.

## Initial Implementation Slice

Status: In progress.

The first v0.35 implementation slice adds native Polars-backed `mutate` for the
supported simple expression subset, shares that lowering with native filter and
aggregate arguments, and expands direct native binary terminal writers for
Parquet, Arrow IPC file, and Arrow IPC stream sinks. CSV and JSON Lines native
plans still collect through the public row table before writing so their
user-visible text formatting remains unchanged.

This slice intentionally does not bump repository or package version stamps to
`0.35.0`. The release remains open until the deferred plan items below are
either implemented, explicitly deferred, or removed from the v0.35 release
scope.

## Must

- Implement fast native Polars-backed `mutate` for the simple expression subset.

  Status: Partially implemented.

  Initial slice status:

  - Native eligibility now accepts `mutate` when all assignment expressions
    lower to the supported native expression subset.
  - Native execution applies assignments through a single Polars
    `with_columns` projection, preserving parallel assignment semantics.
  - Row/native parity tests cover replacement, append order, parallel
    assignment, nulls, booleans, strings, numbers, and mixed expression
    pipelines over CSV, Parquet, and path-backed Arrow-stream inputs.
  - Unsupported scalar functions such as `if_else` still reject forced native
    execution and fall back to rows in `auto`.
  - Native `mutate` benchmark workloads were added for million-row CSV,
    Parquet, and path-backed Arrow-stream inputs.
  - Plan/manifest observability and release variance reporting remain open.

  Current row `mutate` semantics are already specified and implemented.
  Native execution still rejects `mutate`, which prevents a production pipeline
  such as `mutate score_per_latency = score / latency_ms` from staying in
  Polars before Arrow IPC handoff. The v0.35 implementation should lower
  supported assignments to Polars lazy expressions, apply them with the
  equivalent of a native `with_columns` projection, and avoid collecting into
  public `Row` and `Value` objects.

  Acceptance criteria:

  - Native eligibility accepts `mutate` only when every assignment expression is
    in the supported native expression subset.
  - Native `mutate` preserves row semantics for parallel assignment: a target
    created earlier in the same `mutate` stage is not visible to later
    assignments in that same stage.
  - Replacing an existing column preserves that column's position. New columns
    append in assignment order.
  - Duplicate mutate targets keep producing the existing `E1207` diagnostic.
  - Unsupported mutate expressions are rejected before native scans are opened
    in `auto`; forced `native` reports the existing unsupported-native
    diagnostic path.
  - Supported native `mutate` pipelines stay lazy until the terminal sink unless
    the pinned Polars version requires a documented materialization point.
  - Native `mutate` uses Polars vectorized kernels for arithmetic, comparison,
    boolean, string, and null operations instead of row iteration.
  - The implementation records native `mutate` in plan/manifest observability
    so benchmark output can prove that the accelerated path was selected.
  - Row/native parity tests cover replacement, append order, parallel
    assignment, nulls, booleans, strings, numbers, and mixed expression
    pipelines.
  - Release-profile benchmark medians show native `mutate` is materially faster
    than forced row on million-row CSV, Parquet, and path-backed Arrow-stream
    workloads. The minimum target is 50% faster than row for path-backed
    Parquet and Arrow-stream inputs; the stretch target is 80% faster.

- Add shared native lowering for simple PDL expressions.

  Status: Partially implemented.

  Initial slice status:

  - The shared native expression subset now covers column references,
    numeric/string/boolean/null literals, arithmetic, comparisons, boolean
    `and`/`or`/`not`, `is_null`, `not_null`, `coalesce`, `concat`, `lower`,
    `upper`, `trim`, `abs`, and `round`.
  - Native aggregate arguments now accept supported simple expressions, not
    only raw column references.
  - `to_number`, `if_else`, window expressions, dynamic/uncertain coercions,
    and unsupported arities remain row-only unless separately promoted with
    parity tests.

  The supported expression subset should be shared by native `filter`,
  `mutate`, and aggregate arguments where row/native parity is clear.

  Acceptance criteria:

  - The first native subset supports column references, numeric/string/boolean
    literals, null literals, arithmetic `+`, `-`, `*`, `/`, comparisons,
    boolean `and`, `or`, `not`, parentheses, `is_null`, `not_null`, `coalesce`,
    `lower`, `upper`, `trim`, `concat`, `abs`, and `round` where Polars
    behavior matches row semantics.
  - Cast-style functions such as `to_number` are included only after null,
    parse-failure, and numeric formatting behavior match row output.
  - Window expressions remain row-only unless native window parity is added in
    a separate tested slice.
  - Aggregate arguments can use supported simple expressions, not only raw
    column references, once aliases, null behavior, and numeric normalization
    match row output.
  - Unsupported scalar functions, unsupported arity, dynamic column
    indirection, and uncertain type coercions have deterministic native
    eligibility rejection reasons.

- Preserve predicate and projection pushdown through native planning.

  Status: Planned.

  Native coverage should not work by reading every source column and throwing
  most of them away later.

  Acceptance criteria:

  - Native planning computes the source columns required by filters, mutates,
    selects, drops, renames, groups, aggregates, sorts, and terminal sinks.
  - CSV, Parquet, and path-backed Arrow stream inputs preserve projection
    pushdown wherever the pinned Polars version supports it.
  - Filters that appear before row-preserving stages keep predicate pushdown
    opportunities.
  - Tests cover pipelines with wide unused columns and ensure unsupported
    expressions do not open native scans before falling back.

- Keep native terminal writers columnar.

  Status: Partially implemented.

  Initial slice status:

  - Native plans now write Parquet, Arrow IPC file, and Arrow IPC stream sinks
    directly from collected Polars dataframes instead of converting through
    public `Row` and `Value` objects.
  - Native Arrow IPC stream stdout remains valid stream bytes.
  - Parquet and Arrow IPC file sink tests cover a native mutate pipeline and
    read the result back through PDL's supported table readers.
  - CSV and JSON Lines output intentionally keep the row-format fallback so
    current text formatting semantics remain stable.
  - Sink-strategy observability remains open.

  A native plan should not collect into public `Row` and `Value` objects just
  to write terminal CSV, Parquet, Arrow IPC file, or Arrow IPC stream output.

  Acceptance criteria:

  - Terminal `save` and `--stdout-format` paths write directly from native
    plans whenever the active plan is native.
  - Arrow IPC stream stdout remains valid data bytes with all diagnostics,
    progress output, and benchmark notes on stderr or sidecar files.
  - Arrow IPC stream stdout is readable after projection, mutation, grouped
    aggregate, distinct, sort, and limit stages.
  - Parquet and Arrow file sinks preserve deterministic schema names, field
    order, nullability metadata where available, and logical types that PDL
    supports.
  - CSV output preserves current row-visible formatting semantics.

- Establish tracked PDL-to-Algraf Arrow IPC smoke coverage.

  Status: Planned.

  The root-level prototype proved that PDL can stream a large Arrow IPC table to
  Algraf quickly, but v0.35 should make the contract reproducible and tracked.

  Acceptance criteria:

  - Add or promote a cross-repo smoke command equivalent to:

    ```bash
    pdl run prep.pdl --stdout-format arrow-stream \
      | algraf render chart.ag --data - --data-format arrow-stream --output chart.svg
    ```

  - The PDL fixture performs real work with `filter`, native `mutate`,
    projection, and a deterministic ordering or grouping step before the handoff.
  - The Arrow IPC handoff moves a large table, not only a tiny aggregate, so IPC
    throughput remains visible.
  - The Algraf side performs visual aggregation such as `SummaryBin` so
    rendering remains bounded even when input rows are large.
  - The smoke records input rows, output Arrow bytes, elapsed PDL time, elapsed
    Algraf time where practical, SVG byte size, and whether PDL selected the
    native backend.
  - PDL does not parse `.ag`, and Algraf does not parse `.pdl`; Arrow IPC is the
    only process boundary.

- Add production-grade native planning observability.

  Status: Planned.

  Users and benchmark reports need to know whether a run was native, why it was
  row-only, and which sink strategy wrote the final bytes.

  Acceptance criteria:

  - `pdl plan`, run manifests, or both expose selected execution engine,
    native eligibility, fallback reason, input format, output format, source
    boundary, and terminal sink strategy.
  - Native eligibility reasons are stable enough for tests and benchmark notes.
  - `pdl-bench` records row/auto/native status, elapsed time, output rows,
    output bytes, engine notes, and unsupported-native cases without treating
    expected unsupported-native workloads as failed benchmark runs.
  - Release benchmark summaries compare `auto` against forced `row` and forced
    `native` wherever forced native is supported.

- Keep Polars and native formats out of WASM.

  Status: Validated for initial slice.

  Initial slice status:

  - `cargo check -p pdl-wasm --target wasm32-unknown-unknown` passes with the
    stable toolchain command listed below.
  - `cargo tree -p pdl-wasm --target wasm32-unknown-unknown` was searched
    case-insensitively for `polars`, `parquet`, and `arrow`; no matches were
    found.

  v0.35 should deepen native execution without changing the browser product
  boundary.

  Acceptance criteria:

  - `pdl-wasm` keeps depending on workspace crates with `default-features =
    false` wherever required.
  - `cargo check -p pdl-wasm --target wasm32-unknown-unknown` passes.
  - `cargo tree -p pdl-wasm --target wasm32-unknown-unknown` has no `polars`,
    `parquet`, or native Arrow reader entries.
  - Parser, syntax, semantics, editor-services, LSP, CLI public command models,
    and WASM public APIs expose no concrete Polars types.
  - Any future browser Arrow byte support is scoped separately and does not
    enable `polars-engine`, native filesystem IO, or Parquet.

## Should

- Add a native coverage matrix.

  Status: Planned.

  Document each stage, expression family, source format, sink format, and host
  boundary as native parity, row-only by design, planned native, or unsupported.
  This matrix should drive eligibility tests and release notes.

- Evaluate byte-backed and stdin Arrow-stream native readers.

  Status: Planned.

  v0.34 intentionally left `load stdin format "arrow-stream"` on the row
  runtime. v0.35 should evaluate whether bounded native Arrow IPC stream reading
  from stdin or byte-backed host sources can preserve the stdout purity and WASM
  boundaries.

  Acceptance criteria:

  - If implemented, byte-backed native Arrow reading has bounded buffering,
    deterministic diagnostics, and row/native parity tests.
  - If deferred again, `auto` continues to route directly to rows without
    failed-native overhead, and the coverage matrix explains why.

- Improve Arrow and Parquet schema fidelity.

  Status: Planned.

  Tests should cover nullable fields, temporal units, integer and float widths,
  booleans, Utf8 strings, and deterministic rejection of unsupported nested,
  dictionary, list, or struct types until those types have PDL-visible
  semantics.

- Use Polars lazy optimization more deliberately.

  Status: Planned.

  Native plans should preserve opportunities for predicate pushdown, projection
  pushdown, aggregate pushdown, row-group pruning, and streaming or batched
  collection where the pinned Polars version supports them. The PDL facade must
  keep those implementation details private.

- Add repeated release benchmarking and variance reporting.

  Status: Planned.

  `pdl-bench` should make release-profile repeated samples easy to capture and
  summarize. Release notes should report medians, any high variance, and known
  hardware or background-load caveats.

- Add memory and allocation visibility.

  Status: Planned.

  Large native workloads need stable memory behavior. v0.35 should record peak
  RSS where practical and call out workloads that collect native data into row
  values.

- Preserve small-data and editor responsiveness.

  Status: Planned.

  Native planning should not add meaningful overhead to tiny files, row-only
  browser workflows, editor previews, schema inspection, completions, or
  diagnostics. If native startup overhead dominates small inputs, `auto` may
  choose rows when that decision is deterministic and documented.

- Evaluate conservative native join and union coverage.

  Status: Planned.

  Production pipelines often join facts to small dimensions or union partitioned
  extracts. v0.35 may implement a narrow native subset only if parity tests
  cover output order, null-key behavior, duplicate column naming, schema
  compatibility, and diagnostics.

  Candidate subset:

  - inner and left equi-joins on named columns;
  - deterministic duplicate-column naming;
  - schema-compatible position- or name-aligned `union`;
  - explicit row fallback for ambiguous joins, non-equi joins, right/full joins,
    and uncertain null-key semantics.

## Non-Goals

- No PDL syntax for Polars expressions, Polars lazy plans, Arrow arrays, or
  dataframe implementation types.
- No Polars types in parser, syntax, semantic analysis, editor-services, LSP,
  CLI public command models, WASM public APIs, or PDL source syntax.
- No Algraf parser, renderer, or chart semantics inside PDL.
- No PDL parser or executor inside Algraf.
- No required browser/WASM Polars, Parquet, native Arrow reader, or native
  filesystem support.
- No mid-pipeline fallback from native plans to the row runtime.
- No native window-expression support unless promoted by a separate scoped plan
  update with parity tests.
- No distributed execution, SQL planner, remote object-store credentials, or
  long-running service runtime.

## Deferred Beyond v0.35 Unless Promoted

- Mid-pipeline native-to-row fallback.
- Native window expressions in `mutate`.
- Native `pivot_longer` and `complete`.
- Browser/WASM native dataframe execution.
- Object-store readers and remote credentials.
- SQL compatibility layers.

## Benchmarks

Required before/after release-profile benchmark coverage:

```bash
cargo run -p pdl-bench -- run --suite large --tier stress \
  --run-label v0_35_before_row_release --profile release --engine row --no-generate
cargo run -p pdl-bench -- run --suite large --tier stress \
  --run-label v0_35_before_auto_release --profile release --engine auto --no-generate
cargo run -p pdl-bench -- run --suite large --tier stress \
  --run-label v0_35_before_native_release --profile release --engine native --no-generate
```

After implementation, rerun the same suite with `v0_35_after_*` labels and
compare:

```bash
cargo run -p pdl-bench -- compare \
  --baseline bench/runs/v0_35_before_auto_release/report.csv \
  --run-label v0_35_after_auto_release
cargo run -p pdl-bench -- compare \
  --baseline bench/runs/v0_35_after_row_release/report.csv \
  --run-label v0_35_after_auto_release
cargo run -p pdl-bench -- compare \
  --baseline full-baseline-20260606 \
  --run-label v0_35_after_auto_release
```

The benchmark suite should include or add workloads for:

- native `mutate` over CSV, Parquet, and path-backed Arrow-stream inputs;
- `filter | mutate | select | sort | --stdout-format arrow-stream`;
- simple expression aggregate arguments;
- terminal CSV, Parquet, Arrow IPC file, and Arrow IPC stream writers;
- PDL-to-Algraf Arrow IPC pipe timing and output byte counts;
- row-only small data to protect startup and editor-oriented paths.

### Initial Slice Benchmark Results

Release-profile benchmarks were run on 2026-06-06 with forced row, automatic,
and forced native engines:

```text
workload                            format             row_ms auto_ms native_ms auto_vs_row
million_row_mutate_csv              csv -> csv           3657      96        89      +97.4%
million_row_mutate_parquet          parquet -> csv       3860      72        50      +98.1%
million_row_mutate_arrow_stream     arrow-stream -> csv  3679      58        55      +98.4%
million_row_mutate_csv              csv -> arrow-stream  3696      85        79      +97.7%
```

The mutate workloads have no `v0_35_before_*` baseline because they were added
by this slice. The same-run forced-row comparison shows the native mutate path
exceeds the 50% faster acceptance target for Parquet and Arrow-stream inputs.

For existing pre-slice workloads, `v0_35_after_auto_release` remained faster
than the same-run forced row baseline across the suite, ranging from +67.2% for
CSV grouped summary to +98.5% for Parquet grouped summary. Compared directly to
`v0_35_before_auto_release`, existing auto workloads were flat to slower in
this single run: CSV Arrow-stream grouped summary was unchanged at 65 ms,
others ranged from -5.6% to -17.2%, with projection smoke moving from 16 ms to
40 ms. Repeated-sample variance reporting remains open.

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

The WASM dependency tree must be searched case-insensitively for `polars`,
`parquet`, and native Arrow reader dependencies. No matches are acceptable for
the `pdl-wasm` target unless a future browser-specific plan explicitly changes
that product boundary.

Initial slice validation on 2026-06-06:

```text
cargo fmt --all --check                                      passed
cargo clippy --workspace --all-targets                       passed
cargo test --workspace                                       passed
cargo check -p pdl-wasm --target wasm32-unknown-unknown      passed
cargo tree pdl-wasm target search for polars/parquet/arrow   no matches
```

PDL-to-Algraf smoke coverage, native coverage matrix updates, memory reporting,
and repeated-sample variance reporting remain open before v0.35 can be marked
implemented.

Required cross-repo validation when Algraf tooling is available:

```bash
pdl run prep.pdl --stdout-format arrow-stream \
  | algraf render chart.ag --data - --data-format arrow-stream --output chart.svg
```

The final v0.35 plan update should record release-profile benchmark results,
PDL-to-Algraf smoke results, native coverage matrix status, and WASM dependency
tree results before the plan is marked implemented.
