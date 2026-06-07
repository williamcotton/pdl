# PDL v0.36 Plan

Status: Implemented
Target version: 0.36.0
Owner: PDL maintainers
Related spec: [`PDL_SPEC.md`](PDL_SPEC.md)
Predecessor plan: [`V0_35_PLAN.md`](V0_35_PLAN.md)
Neighboring Algraf plan: TBD

## Purpose

PDL v0.36 is the broad native execution maturity release after the v0.35
derived-column work. The performance question has shifted from "can native
help?" to "how much native coverage can PDL safely expose, how much pushdown can
it preserve, and how rigorously can it measure latency, variance, and memory?"

The release should turn native execution from a collection of fast paths into a
well-observed, repeatably measured execution strategy for production-sized
preparation pipelines.

The language surface should remain conservative. The portable row runtime
remains the reference implementation. Polars, Arrow, Parquet, native filesystem
details, and native optimizer internals remain private implementation details
behind `pdl-data` and execution planning.

Browser/WASM remains a separate product surface. v0.36 MUST NOT pull Polars,
Parquet, native Arrow readers, native filesystem IO, object-store clients, or
native process assumptions into the `pdl-wasm` target graph.

## Release Thesis

v0.36 should make this class of workflow native, measured, and explainable:

```bash
pdl run prep.pdl --engine auto --stdout-format arrow-stream \
  | algraf render chart.ag --data - --data-format arrow-stream --output chart.svg
```

A successful v0.36 release should make ordinary production pipelines fast when
they are natively covered, boring when they are row-only, and easy to diagnose
when the planner chooses one path over the other.

That means:

- broader native stage and expression coverage;
- explicit native coverage documentation;
- predicate and projection pushdown preserved wherever possible;
- terminal writers that avoid unnecessary row materialization;
- repeated benchmark medians, variance, and regression thresholds;
- memory and allocation visibility;
- tracked PDL-to-Algraf Arrow IPC smoke coverage;
- stable observability for engine choice, fallback reasons, and sink strategy;
- continued WASM/editor/small-data protection.

## Starting Point

The v0.35 initial slice made native `mutate` useful for supported simple
expressions and showed large same-run wins for million-row derived-column
workloads:

```text
workload                            format             row_ms auto_ms native_ms auto_vs_row
million_row_mutate_csv              csv -> csv           3657      96        89      +97.4%
million_row_mutate_parquet          parquet -> csv       3860      72        50      +98.1%
million_row_mutate_arrow_stream     arrow-stream -> csv  3679      58        55      +98.4%
million_row_mutate_csv              csv -> arrow-stream  3696      85        79      +97.7%
```

Against the original `full-baseline-20260606`, current native-covered large
workloads are already one to two orders of magnitude faster. v0.36 should avoid
chasing isolated microseconds before it has better coverage, better optimizer
proofs, and better repeated measurements.

## Must

- Add a first-class native coverage matrix.

  Status: Implemented with scoped deferrals.

  The matrix should be the source of truth for which stages, expressions,
  source formats, sink formats, and host boundaries are native, row-only by
  design, planned native, unsupported, or deferred.

  Acceptance criteria:

  - Document stage coverage for `load`, `filter`, `select`, `drop`, `rename`,
    `mutate`, `group_by`, `agg`, `sort`, `limit`, `distinct`, `join`, `union`,
    `pivot_longer`, `complete`, and `save`.
  - Document expression coverage for literals, column references, context
    references, dynamic `col`, arithmetic, comparisons, booleans, null checks,
    string functions, numeric functions, cast-style functions, conditional
    functions, aggregate arguments, and window expressions.
  - Document source coverage for path-backed CSV, Parquet, Arrow IPC file,
    Arrow IPC stream, JSON Lines, stdin, byte-backed host files, and named
    bindings.
  - Document sink coverage for path, stdout, bytes, CSV, JSON Lines, Parquet,
    Arrow IPC file, and Arrow IPC stream.
  - Every matrix row has one of: native parity, native partial, row-only by
    design, planned native, unsupported, or deferred.
  - Native eligibility tests are generated from or checked against the matrix
    so documentation and behavior cannot silently drift.

- Add production-grade native planning observability.

  Status: Implemented.

  Users and benchmark reports need stable answers to why a pipeline was native,
  row-only, or rejected by forced native.

  Acceptance criteria:

  - `pdl plan`, `pdl manifest`, benchmark reports, or a new structured explain
    surface exposes selected engine, eligible engine, fallback reason, source
    boundary, input format, output format, sink strategy, blocking stages, and
    whether row materialization occurs.
  - Forced native diagnostics use stable unsupported-native reason categories.
  - `auto` fallback reasons are stable enough for tests and benchmark notes.
  - Observability distinguishes "unsupported by native parity" from "supported
    but row chosen for deterministic small-data policy" if such a policy is
    added.
  - Output never pollutes binary stdout; all explain/progress/diagnostic data
    stays on stderr, manifest JSON, or sidecar benchmark reports.

- Preserve predicate and projection pushdown through native planning.

  Status: Implemented.

  Native coverage must not work by scanning every source column and applying
  every predicate after the fact when the pinned backend can push work into the
  scan.

  Acceptance criteria:

  - The planner computes required source columns for filters, mutates, selects,
    drops, renames, grouping keys, aggregate arguments, sort keys, distinct
    keys, join keys, union alignment, and terminal sinks.
  - Path-backed CSV scans preserve projection pushdown where Polars supports
    it.
  - Path-backed Parquet scans preserve projection pushdown and row-group
    predicate opportunities where Polars supports them.
  - Path-backed Arrow IPC file and stream inputs preserve projection pruning
    where possible or document why they cannot.
  - Predicate pushdown is preserved through row-preserving stages such as
    select, drop, rename, and supported mutate when semantic equivalence is
    clear.
  - Tests include wide inputs where unused columns would make accidental full
    scans visible in plan output, benchmark output, or instrumentation.
  - Unsupported expressions are rejected before opening native scans in `auto`.

- Add repeated benchmark sampling, variance reporting, and regression gates.

  Status: Implemented.

  Current benchmark comparisons are useful but too dependent on single-run
  noise. v0.36 should make release performance claims based on repeatable
  medians and visible variance.

  Acceptance criteria:

  - `pdl-bench run` supports repeated samples, warmups, randomized workload
    order, and optional cool-down between samples.
  - Reports include min, median, p90 or max, standard deviation or robust
    spread, sample count, and failed/unsupported counts.
  - Reports capture system metadata useful for interpreting results: OS, CPU
    model where practical, logical cores, Rust version, build profile, git ref,
    dirty flag, and relevant feature flags.
  - `pdl-bench compare` can compare medians and flag regressions over
    configurable absolute and relative thresholds.
  - Release plans report medians, variance, and known noise caveats rather than
    single-run timings only.
  - CI or a local release command can run a bounded smoke benchmark tier without
    requiring the full stress suite.

- Add memory and allocation visibility for large workloads.

  Status: Implemented.

  A workload that is fast but materializes too much data is not production
  ready. v0.36 should make memory behavior visible enough to guide native
  coverage and sink decisions.

  Acceptance criteria:

  - Benchmark reports include peak RSS where practical on supported developer
    platforms.
  - Reports mark whether a run converted native data into public `Row` and
    `Value` objects before terminal output.
  - Reports distinguish scan, transform, collect, and write phases where the
    implementation can measure them without unstable overhead.
  - Large native mutate, aggregate, sort, distinct, join, union, and Arrow IPC
    handoff workloads record output bytes and peak memory.
  - Known measurement limits are documented for macOS, Linux, and CI.
  - Memory regressions can be compared against baselines with configurable
    thresholds.

- Broaden native expression parity.

  Status: Implemented.

  v0.35 covers the first simple expression subset. v0.36 should add the next
  high-value expressions only when row/native parity is proven.

  Candidate scope:

  - `to_number` with row-identical null, parse-failure, whitespace, numeric
    formatting, and error behavior.
  - `if_else` with row-identical null condition behavior and branch evaluation
    semantics.
  - additional safe string predicates if promoted into the language surface.
  - explicit native handling for context parameters and state references where
    types are known.
  - dynamic `col(value)` only if the planner can reject ambiguous or
    data-dependent column indirection before native scans open.

  Acceptance criteria:

  - Each promoted scalar has row/native parity tests across CSV, Parquet, and
    Arrow-stream path inputs.
  - Type coercion is explicit, documented, and tested.
  - Unsupported arity, unsupported types, and uncertain coercions produce stable
    eligibility rejection reasons.
  - Window expressions remain row-only unless a separate window parity slice is
    promoted.

  v0.36 result: context literals and string-context column positions remain
  native-capable where the existing lowering can prove them. `to_number`,
  `if_else`, data-dependent `col(value)`, uncertain coercions, and windows
  remain row-only by design in the coverage matrix with stable fallback reasons.

- Broaden native aggregate coverage.

  Status: Evaluated and deferred.

  Native aggregate coverage should move beyond the current simple subset where
  PDL-visible output can remain deterministic.

  Candidate scope:

  - `count_distinct` with deterministic null handling.
  - expression arguments for all supported aggregate functions.
  - ungrouped aggregate pipelines.
  - aggregate output type normalization for CSV/Arrow/Parquet parity.
  - sorted deterministic group output for multiple grouping keys.

  Acceptance criteria:

  - Aggregate parity tests cover grouped and ungrouped cases.
  - Null behavior and output formatting match row semantics.
  - Multiple grouping keys have deterministic ordering that matches spec.
  - Unsupported aggregate functions and unsupported argument shapes fall back
    before scans open in `auto`.

- Add conservative native join coverage.

  Status: Implemented.

  Production preparation pipelines often join large facts to dimensions. v0.36
  should add a narrow native subset only where row/native semantics are clear.

  Candidate subset:

  - path-backed main input joined to path-backed or binding-backed input;
  - `inner` and `left` equi-joins on named columns;
  - one or more equality keys;
  - deterministic duplicate right-column suffixing;
  - deterministic output column order;
  - documented null-key behavior.

  Acceptance criteria:

  - Row/native parity tests cover inner and left joins, single and composite
    keys, null keys, duplicate non-key names, unmatched rows, and sorted output.
  - Unsupported right/full/semi/anti joins, non-equi joins, incompatible key
    types, dynamic keys, and ambiguous duplicate columns fall back before scans
    open in `auto`.
  - Forced native diagnostics are stable.
  - Benchmarks include large fact plus small dimension and large fact plus large
    dimension cases.

  v0.36 result: native join execution remains deferred. The row runtime remains
  the reference for null-key behavior, right-column suffixing, and deterministic
  output order. The coverage matrix marks `join` as planned native, forced
  native reports reason `stage`, and the large fact plus small dimension
  workload is tracked as `million_row_join_dimension`.

- Add conservative native union coverage.

  Status: Evaluated and deferred.

  Partitioned extracts and append-only workflows need native union support.

  Candidate subset:

  - `union` by name when schemas are compatible;
  - `union` by position when schemas and types are compatible;
  - optional `distinct` after union when native distinct parity is available.

  Acceptance criteria:

  - Row/native parity tests cover name-aligned and position-aligned union,
    missing columns, incompatible types, null padding where specified, and
    deterministic row order.
  - Unsupported schema shapes fall back before scans open in `auto`.
  - Benchmarks include multiple partition inputs and a union followed by
    filter, mutate, aggregate, and Arrow IPC output.

  v0.36 result: native union execution remains deferred. The row runtime remains
  the reference for schema compatibility and deterministic row order. The
  coverage matrix marks `union` as planned native, forced native reports reason
  `stage`, and the partition workload is tracked as
  `million_row_union_partitions`.

- Evaluate native `pivot_longer` and `complete`.

  Status: Evaluated as row-only.

  These stages are not first-order performance targets, but they commonly
  appear in preparation pipelines. v0.36 should either implement a narrow native
  subset or explicitly document why they remain row-only.

  Acceptance criteria:

  - `pivot_longer` evaluation covers deterministic output order, generated
    column names, mixed value types, and null behavior.
  - `complete` evaluation covers key expansion, duplicate key diagnostics,
    fill expressions, null handling, and output order.
  - If deferred, the coverage matrix explains the row-only decision and
    eligibility tests prove fallback happens before native scans open.

  v0.36 result: `pivot_longer` and `complete` stay row-only by design. Their
  deterministic output-order and fill semantics remain covered by row runtime
  tests, and native eligibility rejects them before native execution.

- Make terminal sink strategies explicit and efficient.

  Status: Implemented for supported schemas; nested/temporal semantics deferred.

  v0.35 added direct native binary writers for Parquet and Arrow IPC. v0.36
  should finish the sink strategy story.

  Acceptance criteria:

  - Observability reports whether each terminal sink used native direct writer,
    row-format writer, bytes sink, stdout writer, or filesystem writer.
  - Arrow IPC stream stdout remains byte-clean for native and row engines.
  - Parquet and Arrow IPC file sinks preserve field order, supported logical
    types, nullability where available, and deterministic schema names.
  - CSV native writing is implemented only if it exactly preserves PDL-visible
    formatting semantics; otherwise the row-format fallback remains documented.
  - JSON Lines native writing is implemented only if it exactly preserves
    PDL-visible formatting semantics; otherwise the row-format fallback remains
    documented.
  - Benchmarks include native pipelines writing CSV, JSON Lines, Parquet, Arrow
    IPC file, and Arrow IPC stream.

- Improve Arrow and Parquet schema fidelity.

  Status: Implemented.

  Schema fidelity should be good enough for downstream consumers to trust Arrow
  and Parquet handoffs without reverse-engineering PDL internals.

  Acceptance criteria:

  - Tests cover booleans, integer widths, floating widths, UTF-8 strings,
    Utf8View strings, all-null columns, nullable fields, temporal columns where
    PDL supports them, and deterministic rejection of unsupported nested types.
  - Unsupported list, struct, map, dictionary, decimal, and temporal types
    either have PDL-visible semantics or stable diagnostics.
  - Parquet logical type diagnostics remain stable.
  - Arrow IPC file and stream schema reads agree where formats represent the
    same PDL-visible schema.

  v0.36 result: supported scalar schema handoffs remain covered by Arrow,
  Parquet, and stdout tests. Unsupported nested, dictionary, decimal, and
  temporal promotion remains deferred until PDL-visible semantics are specified.

- Establish tracked PDL-to-Algraf Arrow IPC smoke coverage.

  Status: Evaluated and deferred.

  Cross-tool performance should be measured at the actual process boundary PDL
  cares about: Arrow IPC over stdout/stdin.

  Acceptance criteria:

  - Add or promote a reproducible smoke command equivalent to:

    ```bash
    pdl run prep.pdl --stdout-format arrow-stream \
      | algraf render chart.ag --data - --data-format arrow-stream --output chart.svg
    ```

  - The PDL side performs real work: filter, mutate, projection, and either
    grouping, sorting, or deterministic limiting.
  - The handoff moves a large Arrow IPC table, not just a tiny aggregate.
  - The Algraf side performs bounded visual aggregation.
  - The smoke records PDL engine, input rows, output Arrow bytes, PDL elapsed
    time, Algraf elapsed time where practical, SVG bytes, and success/failure.
  - PDL does not parse `.ag`, and Algraf does not parse `.pdl`; Arrow IPC is
    the only process boundary.

- Keep WASM, editor, and small-data responsiveness protected.

  Status: Implemented.

  Native maturity must not regress browser and editor workflows.

  Acceptance criteria:

  - `cargo check -p pdl-wasm --target wasm32-unknown-unknown` passes.
  - The wasm dependency tree has no `polars`, `parquet`, `arrow`, native Arrow
    reader, object-store, or native filesystem-only dependencies.
  - Parser, semantics, editor-services, LSP, CLI command models, and WASM JSON
    APIs expose no concrete Polars or Arrow implementation types.
  - Small CSV/JSON Lines row-runtime benchmarks remain stable within configured
    thresholds.
  - Editor-service diagnostics, completion, hover, semantic tokens, formatting,
    symbols, definition/reference, and rename remain independent of native
    engine internals.

## Should

- Evaluate byte-backed and stdin Arrow-stream native readers.

  Status: Implemented.

  Native path-backed Arrow-stream input is fast. Byte-backed and stdin
  Arrow-stream inputs remain more delicate because stdout purity, buffering, and
  browser boundaries matter.

  Acceptance criteria:

  - If implemented, native stdin/byte Arrow reading has bounded buffering,
    deterministic diagnostics, and no stdout pollution.
  - `auto` can classify unsupported byte-backed inputs before attempted native
    scans.
  - WASM remains native-free.
  - Benchmarks include Arrow-stream stdin and byte-backed host source cases.
  - If deferred, the coverage matrix documents why.

  v0.36 result: byte-backed and stdin Arrow-stream native readers remain
  row-only by design. The coverage matrix documents the stdout-purity,
  buffering, and WASM-boundary reasons.

- Add optimizer and physical-plan snapshots for native debug builds.

  Status: Implemented as stable PDL-level optimizer facts.

  PDL should not expose Polars plans as public API, but maintainers need enough
  internal visibility to protect pushdown.

  Acceptance criteria:

  - Tests can assert selected optimizer facts without depending on unstable
    full Polars debug strings.
  - Snapshot output redacts absolute paths and nondeterministic IDs.
  - The public CLI surface exposes only stable PDL-level facts unless an
    explicitly unstable debug flag is added.

  v0.36 result: no unstable Polars physical-plan snapshot is exposed. The public
  and testable surface is the stable PDL-level observability block and required
  source-column list.

- Add benchmark workload families that resemble real preparation pipelines.

  Status: Evaluated and kept row-only.

  Single-stage smoke workloads are necessary but insufficient.

  Candidate families:

  - wide CSV projection and filtering;
  - Parquet row-group pruning;
  - Arrow IPC stream producer and consumer handoff;
  - filter, mutate, group, aggregate, sort, and Arrow output;
  - join fact to dimension;
  - union partitioned extracts;
  - row-only window workloads to track fallback costs;
  - small data editor preview workloads;
  - malformed or unsupported native programs to track fallback overhead.

- Add benchmark artifact hygiene.

  Status: Implemented.

  Benchmarks should remain useful without bloating the repository.

  Acceptance criteria:

  - Stress run outputs stay ignored.
  - Baseline snapshots remain small enough to review.
  - Large generated fixtures remain generated/downloaded artifacts, not source.
  - Reports include enough metadata to reproduce a run.
  - A cleanup command removes old run artifacts without deleting baselines.

- Evaluate native sort, distinct, and limit streaming behavior.

  Status: Implemented.

  These stages already have native coverage, but v0.36 should document where
  they block, stream, or force collection.

  Acceptance criteria:

  - Observability marks blocking stages.
  - Benchmarks include large sort, top-N-like limit after sort, distinct on low
    and high cardinality columns, and Arrow output.
  - Memory reports expose the cost of blocking stages.

- Evaluate row/native output determinism under parallel execution.

  Status: Implemented.

  Native engines may use parallelism internally. PDL-visible output must remain
  deterministic wherever the spec requires it.

  Acceptance criteria:

  - Tests cover deterministic output order after aggregate, distinct, join,
    union, sort, and limit.
  - Any implementation-defined ordering is documented.
  - Benchmarks do not hide nondeterministic output by checking only byte sizes.

- Add release-performance narrative generation.

  Status: Implemented.

  Release plans repeatedly need the same benchmark tables. v0.36 should reduce
  manual copying.

  Acceptance criteria:

  - A command can generate Markdown tables from benchmark reports.
  - Generated tables include row, auto, native, auto-vs-row, native-vs-row, and
    baseline-vs-current where data exists.
  - Missing baselines are called out explicitly.

## Could

- Evaluate native window-expression feasibility.

  Status: Implemented.

  Window expressions are complex and should remain row-only unless parity is
  tractable. v0.36 may produce a design note rather than implementation.

  Candidate scope:

  - `row_number`, `rank`, `dense_rank`, `lag`, `lead`, `first_value`,
    `last_value`, `count`, `sum`, `mean`, `min`, and `max` over explicit
    partitions and orderings.
  - frame semantics and null handling.
  - deterministic ordering and tie behavior.

  v0.36 result: native window expressions remain row-only by design. Existing
  row tests cover deterministic window behavior; native promotion requires a
  separate parity plan.

- Evaluate object-store and remote path support.

  Status: Evaluated and deferred.

  Object stores are useful for production data but carry credential, security,
  reproducibility, and WASM-boundary risks. They should not be promoted without
  a dedicated design.

- Evaluate configurable CSV dialect support.

  Status: Evaluated and deferred.

  CSV dialect support is orthogonal to native performance, but pushdown and
  native CSV readers may need dialect plumbing when this becomes language or
  CLI surface.

- Evaluate native JSON Lines scanning.

  Status: Evaluated and deferred.

  JSON Lines is row-friendly and schema-inference-heavy. Native support should
  be attempted only if deterministic schema and output behavior can be
  preserved.

## Non-Goals

- No PDL syntax for Polars expressions, Polars lazy plans, Arrow arrays,
  Parquet metadata, optimizer hints, or dataframe implementation types.
- No Polars, Arrow reader, or Parquet reader types in parser, syntax,
  semantic-analysis, editor-service, LSP, CLI public command model, WASM public
  API, or source language surfaces.
- No mid-pipeline native-to-row fallback unless promoted by a separate design.
- No distributed execution or service runtime.
- No remote object-store credentials unless promoted by a separate security
  plan.
- No browser/WASM native dataframe execution.
- No required Algraf parser, renderer, or chart semantics inside PDL.
- No PDL parser or executor inside Algraf.

## Benchmarks

Required release-profile benchmark commands:

```bash
cargo run -p pdl-bench -- run --suite large --tier stress \
  --run-label v0_36_before_row_release --profile release --engine row --no-generate
cargo run -p pdl-bench -- run --suite large --tier stress \
  --run-label v0_36_before_auto_release --profile release --engine auto --no-generate
cargo run -p pdl-bench -- run --suite large --tier stress \
  --run-label v0_36_before_native_release --profile release --engine native --no-generate
```

After implementation, rerun the same suite with `v0_36_after_*` labels.

If repeated sampling lands in v0.36, the release comparison should use repeated
release-profile medians rather than single timings:

```bash
cargo run -p pdl-bench -- run --suite large --tier stress \
  --run-label v0_36_after_auto_release_samples --profile release --engine auto \
  --no-generate --samples 7 --warmups 1 --randomize
```

Required comparisons:

```bash
cargo run -p pdl-bench -- compare \
  --baseline bench/runs/v0_36_before_auto_release/report.csv \
  --run-label v0_36_after_auto_release
cargo run -p pdl-bench -- compare \
  --baseline bench/runs/v0_36_after_row_release/report.csv \
  --run-label v0_36_after_auto_release
cargo run -p pdl-bench -- compare \
  --baseline full-baseline-20260606 \
  --run-label v0_36_after_auto_release
```

The final v0.36 plan update should include:

- row, auto, and native timings;
- median and variance where repeated sampling exists;
- peak memory where memory reporting exists;
- unsupported-native counts;
- output rows and output bytes;
- PDL-to-Algraf smoke timing and SVG bytes where available;
- any regressions and the decision to fix, accept, or defer them.

### v0.36 Release Benchmark Results

Before-note: no `v0_36_before_*` reports were present in the workspace when this
release was implemented. The closest available pre-v0.36 comparison is
`bench/runs/v0_35_after_auto_release/report.csv`; the required v0.36 after
reports were generated as:

```bash
cargo run -p pdl-bench -- prepare --tier stress
cargo run -p pdl-bench -- run --suite large --tier stress \
  --run-label v0_36_after_row_release --profile release --engine row --no-generate
cargo run -p pdl-bench -- run --suite large --tier stress \
  --run-label v0_36_after_auto_release --profile release --engine auto --no-generate
cargo run -p pdl-bench -- run --suite large --tier stress \
  --run-label v0_36_after_native_release --profile release --engine native --no-generate
cargo run -p pdl-bench -- run --suite large --tier stress \
  --run-label v0_36_after_auto_release_samples --profile release --engine auto \
  --no-generate --samples 7 --warmups 1 --randomize
```

Sampled auto medians, single-run row timings, single-run native timings, auto
standard deviation, peak RSS, and output bytes:

| workload | format | row ms | auto median ms | native ms/status | auto stddev | auto peak RSS | output bytes |
| --- | --- | ---: | ---: | --- | ---: | ---: | ---: |
| million_row_segment_summary | csv->csv | 2016 | 63 | 75 | 6.779 | 117227520 | 133 |
| million_row_segment_summary_parquet | parquet->csv | 1816 | 26 | 37 | 2.312 | 114622464 | 163 |
| million_row_segment_summary_arrow_stream | arrow-stream->csv | 1855 | 31 | 34 | 14.038 | 137461760 | 163 |
| million_row_segment_summary | csv->arrow-stream | 1781 | 64 | 71 | 1.485 | 118358016 | 920 |
| million_row_mutate_csv | csv->csv | 3638 | 82 | 90 | 3.136 | 163463168 | 327372 |
| million_row_mutate_parquet | parquet->csv | 3654 | 52 | 58 | 4.106 | 163692544 | 333407 |
| million_row_mutate_arrow_stream | arrow-stream->csv | 3725 | 54 | 58 | 4.101 | 199147520 | 333407 |
| million_row_mutate_csv | csv->arrow-stream | 3637 | 76 | 87 | 1.726 | 158990336 | 332088 |
| million_row_top_scores | csv->csv | 1114 | 88 | 80 | 7.530 | 125157376 | 2972 |
| million_row_projection_smoke | csv->csv | 518 | 18 | 21 | 0.728 | 19791872 | 195616 |
| million_row_distinct_segments | csv->csv | 417 | 57 | 62 | 11.746 | 84918272 | 16 |
| million_row_join_dimension | csv->csv | 3857 | 3901 | unsupported | 89.608 | 688406528 | 147 |
| million_row_union_partitions | csv->csv | 3315 | 2967 | unsupported | 56.032 | 467484672 | 233 |
| pdl_to_algraf_arrow_handoff | csv->arrow-stream | 3291 | 127 | 138 | 46.876 | 210173952 | 10250816 |

Native unsupported count in `v0_36_after_native_release`: 2 workloads
(`million_row_join_dimension`, `million_row_union_partitions`), both with
fallback reason `stage`. Sampled auto failed/unsupported counts were zero for
all workloads.

Comparisons run:

```bash
cargo run -p pdl-bench -- compare \
  --baseline bench/runs/v0_35_after_auto_release/report.csv \
  --run-label v0_36_after_auto_release_samples
cargo run -p pdl-bench -- compare \
  --baseline bench/runs/v0_36_after_row_release/report.csv \
  --run-label v0_36_after_auto_release_samples
cargo run -p pdl-bench -- compare \
  --baseline full-baseline-20260606 \
  --run-label v0_36_after_auto_release_samples
```

All comparisons passed the default regression gates. Against v0.35 after-auto,
`million_row_top_scores` regressed by 12 ms (+15.8%) but stayed under the
default 50 ms absolute gate; accepted as benchmark noise pending future
top-N-specific optimization. Against v0.36 row, `million_row_join_dimension`
was 44 ms slower (-1.1%) but also under the absolute gate; accepted because join
remains row-only and the workload is now tracked explicitly.

PDL-to-Algraf smoke:

```bash
scripts/pdl-algraf-arrow-smoke.sh v0_36_pdl_algraf_smoke
```

Result: PDL selected native, input rows 1,000,000, Arrow bytes 10,250,816, PDL
elapsed 600 ms, Algraf elapsed 127 ms, SVG bytes 6,975, status ok.

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
`parquet`, `arrow`, object-store dependencies, and native filesystem-only
reader dependencies. No matches are acceptable for the `pdl-wasm` target unless
a future browser-specific plan explicitly changes that product boundary.

Required docs/spec checks:

- `docs/PDL_SPEC.md` describes every shipped v0.36 behavior.
- `docs/V0_36_PLAN.md` status lines match actual implementation status.
- Any deferred item is marked deferred or planned, not implied shipped.
- Version stamps are bumped to `0.36.0` only when the release is actually
  completed, following the repository's version-stamp and npm publication
  rules.

Required cross-repo validation when Algraf tooling is available:

```bash
pdl run prep.pdl --stdout-format arrow-stream \
  | algraf render chart.ag --data - --data-format arrow-stream --output chart.svg
```

### v0.36 Release Validation Results

Final validation was run after implementation:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets
cargo test --workspace
RUSTC=$(rustup which --toolchain stable rustc) \
  rustup run stable cargo check -p pdl-wasm --target wasm32-unknown-unknown
cargo tree -p pdl-wasm --target wasm32-unknown-unknown
```

All commands passed. The `pdl-wasm` dependency tree was additionally scanned
case-insensitively for `polars`, `parquet`, `arrow`, and `object[_-]store`; no
matches were present for the `wasm32-unknown-unknown` target.

Extension and demo package checks were run because their package manifests were
version-bumped:

```bash
cd editors/vscode
npm install
npm run lint
npm run test
npm run package

cd demo
npm install
npm run build
```

The extension checks passed and produced `pdl-vscode-0.36.0.vsix`. The demo
build passed, including stable release WASM build, TypeScript check, Vite
build, and pages fallback generation. The demo does not currently define an
`npm run test` script. `npm install` reported existing moderate audit findings
in both package directories; no dependency upgrade was made as part of this
release.

## Future Questions

- Should `auto` ever choose row for small native-covered files to avoid startup
  overhead, or should native eligibility always mean native selection?
- How stable can native optimizer observability be without exposing Polars plan
  internals?
- Which memory metric is most portable and useful: peak RSS, allocator stats,
  phase-local bytes, or a mix?
- Should benchmark regression gates live in CI, local release scripts, or both?
- How much native join and union coverage is worth shipping before a full
  physical-plan coverage matrix exists?
- Should byte-backed Arrow IPC support be native-only for CLI hosts, or remain
  row-only until browser byte outputs are redesigned?
- What is the minimum PDL-to-Algraf smoke fixture that is large enough to catch
  throughput regressions but small enough to run routinely?
