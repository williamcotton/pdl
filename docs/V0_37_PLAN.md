# PDL v0.37 Plan

Status: Implemented
Target version: 0.37.0
Owner: PDL maintainers
Related spec: [`PDL_SPEC.md`](PDL_SPEC.md)
Predecessor plan: [`V0_36_PLAN.md`](V0_36_PLAN.md)
Successor plan: [`V0_38_PLAN.md`](V0_38_PLAN.md)
Neighboring Algraf plan: TBD

## Purpose

PDL v0.37 is the native language gap closure release after the v0.36 native
execution maturity release. v0.36 made native execution fast, observable, and
measured for the already-covered core. v0.37 should now tackle the language
features that still keep otherwise columnar preparation pipelines on the row
runtime.

The release should not treat the remaining gaps as vague future work. Each
current non-native language gap should either get a native implementation slice,
an evidence-backed row-only decision, or a separately scoped successor plan with
clear reasons why it is too large or too risky for v0.37.

The current gaps are the places where otherwise columnar pipelines still drop
to the row runtime:

- joins and unions;
- byte-backed, stdin, and Arrow IPC file inputs;
- scalar expressions such as `to_number` and `if_else`;
- window expressions used inside `mutate`;
- reshaping stages such as `pivot_longer` and `complete`;
- text sinks that still require public `Row`/`Value` materialization;
- named bindings that currently block native planning.

## Release Thesis

A successful v0.37 release should make the remaining high-value PDL language
gaps native enough that ordinary analytical preparation programs can stay in the
native engine from load through terminal output. Where full native parity is not
shippable, v0.37 should leave a precise documented decision rather than another
open-ended "planned native" bucket.

That means:

- native join and union slices for the tracked large workloads;
- native window analytics for the customer/region revenue ranking pipeline;
- native expression coverage for `to_number` and `if_else` if parity is
  tractable;
- native Arrow IPC source-boundary expansion where stdout purity and WASM
  boundaries remain intact;
- terminal writer decisions that quantify and reduce row materialization;
- explicit disposition for `pivot_longer`, `complete`, JSON Lines, and broader
  window/function shapes.

## Release Themes

- Native gap accounting: rank row-only language features by elapsed time, peak
  RSS, row-materialization cost, fallback reason, and frequency in examples or
  benchmark fixtures.
- Native join and union promotion: target the tracked v0.36 workloads
  `million_row_join_dimension` and `million_row_union_partitions` before adding
  broader shapes.
- Native source expansion: decide and implement or explicitly close
  path-backed Arrow IPC file scans and CLI-only byte-backed or stdin Arrow IPC
  readers without changing the browser API boundary.
- Native expression expansion: prioritize `to_number`, `if_else`, and other
  scalar functions that commonly appear before filters, aggregates, joins, and
  Arrow IPC handoff.
- Native window analytics: make a full customer/region revenue ranking pipeline
  native before attempting every window function and frame shape.
- Native binding planning: support binding-backed inputs only where dependency
  order, source-column requirements, and observability remain deterministic.
- Row-materialization avoidance: decide whether CSV and JSON Lines terminal
  writers can stream from native data without changing PDL-visible formatting,
  and expose the cost when they cannot.
- Window and reshaping feasibility: decide whether a narrow native subset is
  worth promoting or whether these should remain explicitly row-only.
- Native optimizer guardrails: preserve predicate pushdown, projection pushdown,
  aggregate pushdown, Parquet row-group pruning, and streaming or batched
  collection where the pinned backend supports them.
- Production workload families: keep the benchmark suite focused on complete
  preparation pipelines, not only single-stage smoke tests.
- Auto-engine policy: decide whether small native-covered files should still
  prefer native in `--engine auto`, or whether startup overhead should bias them
  toward rows.
- Benchmark gate placement: decide how benchmark regression gates should be
  wired into CI, local release scripts, or both.
- PDL-to-Algraf smoke coverage: expand routine Arrow IPC smoke coverage only if
  benchmark cost stays bounded.

## Language Gap Closure Targets

The v0.36 coverage matrix and planner currently identify these language gaps as
not native, planned native, or native partial. v0.37 should close the loop on
each one.

| Gap | v0.37 target outcome |
| --- | --- |
| `join` | Promote conservative native `inner`/`left` equi-join coverage, then document deferred shapes. |
| `union` | Promote conservative native compatible-schema union coverage, then document deferred shapes. |
| window expressions | Promote the customer/region revenue ranking slice; evaluate broader windows after that slice. |
| `to_number` and `if_else` | Promote if row/native null, parse, coercion, and branch semantics match; otherwise record stable fallback reasons. |
| path-backed Arrow IPC file | Decide whether native scan parity ships in v0.37 or stays unsupported with measured impact. |
| stdin and byte-backed Arrow IPC | Decide whether CLI-only native readers ship without changing browser boundaries. |
| named bindings | Support native binding inputs where needed for join/union, or document why binding starts remain row-only. |
| CSV/JSON Lines terminal sinks | Either prove native text writer byte parity or expose the row-materialization cost clearly. |
| `pivot_longer` and `complete` | Promote narrow native subsets only if deterministic row parity is tractable; otherwise reclassify with evidence. |
| JSON Lines input | Keep row-only unless deterministic schema and text semantics can be preserved natively. |
| data-dependent `col(...)` and uncertain coercions | Keep row-only unless eligibility can reject ambiguity before scans open. |

## v0.37 Release Results

v0.37 promotes the high-value native slices that could preserve the whole-plan
native contract without exposing backend internals:

| Gap | Final v0.37 disposition |
| --- | --- |
| `join` | Native partial. `auto` and forced native support main inputs joined to native-safe binding inputs for `inner`, `left`, `semi`, and `anti` single-key equi-joins. Null keys do not match, duplicate right non-key columns use the row `_right` suffix rule where right columns are emitted, and row/native parity is covered in `pdl-exec` tests. Right, full, non-equi, and true composite-key syntax remain row-only and move to `V0_38_PLAN.md`. |
| `union` | Native partial. Compatible-schema binding inputs can union by name or by position, with optional native `distinct`; row/native parity is covered in `pdl-exec` tests. Incompatible schemas, uncertain type coercions, and browser byte-backed sources remain row-only. |
| window expressions | Row-only by design for v0.37. The row runtime already covers the target analytics shape, but native parity needs a dedicated lowering and tie/frame audit. Deferred to `V0_38_PLAN.md`. |
| `to_number` | Native partial. `to_number(expr)` now lowers natively for supported native expressions and matches row whitespace, parse-failure, numeric pass-through, and null behavior across CSV, Parquet, and Arrow IPC stream path inputs. |
| `if_else` | Native partial. `if_else(condition, when_true, when_false)` now lowers natively for supported native condition and branch expressions. Null conditions produce null results, and row/native parity is covered for compatible numeric and string branch outputs. Mixed `Value` branch outputs remain row-only because native output columns are typed. |
| path-backed Arrow IPC file | Native partial. Arrow IPC file input is read into the native dataframe path and then lazy transforms continue. The implementation keeps Arrow reader internals private and preserves native-free WASM. |
| stdin and byte-backed Arrow IPC | Native partial. CLI stdin and host byte readers support Arrow IPC file/stream bytes in the native engine without changing browser/WASM boundaries. Non-Arrow stdin/byte formats remain row-only. |
| named bindings | Native partial. Binding-backed right-hand inputs are native for supported join/union slices, including `inner`, `left`, `semi`, and `anti` joins. Binding starts, named outputs, and browser byte-backed bindings remain row-only. |
| CSV/JSON Lines terminal sinks | Row-format writer retained. Binary Parquet and Arrow sinks stay native direct; CSV and JSON Lines still use row-format conversion because text formatting is PDL-visible. Observability continues to report row materialization. |
| `pivot_longer` and `complete` | Row-only by design. Their deterministic output order, fill, and mixed-value semantics remain better served by the row runtime until a dedicated parity plan exists. Deferred to `V0_38_PLAN.md`. |
| JSON Lines input | Row-only by design. Deterministic schema inference and text semantics remain row-runtime responsibilities. Deferred to `V0_38_PLAN.md`. |
| data-dependent `col(...)` and uncertain coercions | Row-only by design. Literal and context-string `col(...)` stay native-capable; data-dependent indirection and uncertain coercions remain rejected before native scans where detectable. |

Native coverage changes are reflected in `docs/PDL_NATIVE_COVERAGE.csv`,
`docs/PDL_NATIVE_COVERAGE.md`, `docs/PDL_SPEC.md`, eligibility checks, and
`pdl-exec` parity tests. The workspace, Cargo lockfile, CLI version output,
manifest/language versions, VS Code package metadata, and private demo package
version are bumped to `0.37.0`. npm was checked for browser packages:
`pdl-wasm` publishes `0.30.0`; `pdl-editor` publishes `0.30.0` and `0.30.1`.
No `0.37.0` browser packages are published, so browser package manifests and
consumer dependency pins remain on the verified 0.30.x line.

## Inherited Should/May Backlog

Recent plans left a few recurring `Should` or `May` threads that are still
relevant to v0.37. Some were implemented as v0.36 infrastructure but should
remain active guardrails; others were evaluated and deferred because they need a
more focused native-performance thesis.

Carry forward these items when they support the release thesis:

- cross-repo PDL-to-Algraf Arrow IPC smoke coverage sized for routine runs;
- small-data, editor-preview, schema-inspection, and diagnostic responsiveness;
- stable PDL-level optimizer facts without exposing Polars physical plans;
- deliberate use of predicate/projection/aggregate pushdown, Parquet row-group
  pruning, and streaming or batched collection;
- memory metric policy beyond peak RSS if allocator or phase-local metrics are
  practical;
- production-style benchmark families for wide CSV projection/filtering,
  Parquet pruning, Arrow IPC handoff, joins, unions, windows, unsupported
  native fallback, and small editor previews;
- benchmark artifact hygiene and generated release-performance narratives;
- native JSON Lines scan feasibility;
- configurable CSV dialect feasibility only where it affects native readers or
  text sink parity.

## Native Performance Gaps

The v0.36 coverage matrix leaves several performance-relevant rows outside
native parity. v0.37 should treat these as a ranked backlog, not as an
all-or-nothing native rewrite.

### Join

Native `join` ships in v0.37 for `inner`, `left`, `semi`, and `anti`
single-key equi-joins from a native-safe main input to a native-safe
binding-backed right input. This covers the common fact-to-dimension and
existence-filter shapes before aggregation or Arrow IPC output. The promoted
slice pins deterministic duplicate right-column suffixing where right columns
are emitted, null-key non-matching, output order, and row/native parity.

Right, full, non-equi, and true composite-key joins remain the successor work.
The large benchmark target remains `million_row_join_dimension`.

### Union

Native `union` ships in v0.37 for compatible-schema binding inputs by name or
by position, with optional native `distinct`. This covers partitioned extracts
and append-only workflows that continue into filter, mutate, aggregate, and
Arrow IPC output. Incompatible schemas, uncertain type coercions, and broader
projection-pruning work remain successor items.

The large benchmark target remains `million_row_union_partitions`, including a
union followed by filter, mutate,
aggregate, and Arrow IPC output.

### Source Boundaries

Path-backed CSV, Parquet, Arrow IPC file, and Arrow IPC stream inputs are
native-capable in v0.37. CLI stdin and host byte inputs are native-capable for
Arrow IPC file/stream bytes, and binding-backed right-hand inputs are
native-capable for supported join and union slices. Non-Arrow byte/stdin
formats, named outputs, and binding starts remain row-only.

WASM remains native-free. The CLI-only byte-backed promotion is a host-boundary
exception, not a browser API change.

### Expressions

Native `filter`, `mutate`, and aggregate arguments use the shared native
expression subset, but several common scalar functions still force row
execution.

Potential v0.37 slice:

- `to_number` with row-identical whitespace, parse-failure, null, and numeric
  formatting behavior;
- `if_else` with row-identical null condition and branch evaluation semantics;
- additional string or numeric predicates only when type coercion is explicit;
- static `col(...)` coverage remains native, while data-dependent dynamic
  indirection stays row-only unless ambiguity can be rejected before scans open.

The goal is not a large function grab bag. The goal is to remove expression
fallbacks that block otherwise native pipelines before joins, aggregates, and
Arrow IPC handoffs.

### Windows And Reshaping

Window expressions, `pivot_longer`, and `complete` are row-only by design in
v0.36. They may still matter for performance when they appear late in a large
pipeline.

The first native window target should be this row-supported analytics shape:

```pdl
load "sales.csv"
  | filter status == "completed"
  | mutate customer_sale_number = row_number() over (partition_by customer_id order_by amount desc), customer_revenue = sum(amount) over (partition_by customer_id), region_revenue = sum(amount) over (partition_by region)
  | mutate region_revenue_rank = dense_rank() over (order_by region_revenue desc)
  | select region, customer_id, amount, customer_sale_number, customer_revenue, region_revenue_rank
  | sort region_revenue_rank, customer_id, amount desc
```

This workload is a good native candidate because it is otherwise a straight
columnar pipeline. The native blocker is window execution, especially the
combination of:

- `row_number()` over a partition with descending order;
- aggregate-style `sum(amount)` over whole partitions;
- `dense_rank()` over a derived window column from an earlier `mutate` stage;
- parallel assignment semantics inside the first `mutate`;
- deterministic final sorting after generated window columns.

Potential v0.37 closure paths:

- window subset for `row_number`, `rank`, `dense_rank`, `lag`, `lead`, and
  aggregate windows over explicit partitions and orderings;
- `pivot_longer` for homogeneous value columns with deterministic generated
  columns and output order;
- `complete` for bounded key expansion with deterministic fill semantics.

These should remain row-only if parity requires exposing backend-specific
ordering, null, or frame behavior.

### Terminal Text Sinks

Binary Parquet and Arrow sinks can write directly from native plans. CSV and
JSON Lines still use row-format writers because text formatting is PDL-visible.
For large native pipelines ending in text output, this can erase part of the
native gain through public row materialization.

Potential v0.37 closure paths:

- native-to-CSV writer only if field formatting, nulls, booleans, numbers,
  quoting, escaping, and line endings match row output exactly;
- native-to-JSON Lines writer only if object key order, nulls, booleans,
  numbers, strings, and unsupported values match row output exactly;
- observability that clearly separates native transform time from terminal
  row-format conversion time.

## Must

- Define the v0.37 release thesis before implementation work lands.

  Status: Defined by this plan.

- Close the native language gap map.

  Status: Implemented with scoped deferrals.

  Acceptance criteria:

  - Every row in "Language Gap Closure Targets" has a final v0.37 disposition:
    native parity, native partial, row-only by design, unsupported with reason,
    or deferred to a named successor plan.
  - The final disposition is reflected in `docs/PDL_NATIVE_COVERAGE.csv`,
    `docs/PDL_NATIVE_COVERAGE.md`, `docs/PDL_SPEC.md`, eligibility tests, plan
    observability, and benchmark notes.
  - No gap remains described only as "maybe", "planned", or "evaluate" without
    measured impact and a release decision.
  - All unsupported native shapes continue to fall back before native scans
    open in `auto`, and forced `native` reports stable reason categories.

- Add a native-performance gap report for row-only or planned-native coverage
  rows.

  Status: Implemented with scoped deferrals.

  Acceptance criteria:

  - Report the current coverage status, fallback reason, elapsed median, peak
    RSS, output bytes, row-materialization status, and blocking stages for each
    promoted candidate workload.
  - Include join, union, byte/stdin Arrow IPC input, Arrow IPC file input,
    named bindings, `to_number`, `if_else`, window, `pivot_longer`,
    `complete`, JSON Lines, dynamic `col(...)`, uncertain coercions, and
    terminal text sink candidates.
  - Use repeated benchmark samples for any claim that native promotion is worth
    shipping.
  - Rank candidates by expected user-visible gain and implementation risk.

- Promote high-value native language slices behind separate parity checkpoints.

  Status: Implemented with scoped deferrals.

  Acceptance criteria:

  - `join`, `union`, windows, source boundaries, expression expansion, text
    sinks, and reshaping each need their own parity matrix before
    implementation.
  - A promoted slice must update `docs/PDL_NATIVE_COVERAGE.csv`,
    `docs/PDL_NATIVE_COVERAGE.md`, `docs/PDL_SPEC.md`, tests, examples, and
    benchmark workloads in the same change.
  - Forced `--engine native` diagnostics must use stable unsupported-native
    reason categories for unpromoted shapes.
  - `auto` must reject unsupported shapes before native scans open.

- Promote or explicitly close the high-value stage gaps: `join`, `union`, and
  native window analytics.

  Status: Implemented with scoped deferrals.

  Acceptance criteria:

  - `million_row_join_dimension`, `million_row_union_partitions`, and
    `windowed_sales_rank` each have row, auto, and native benchmark results or
    explicit unsupported-native results with stable reasons.
  - Any promoted native slice proves row/native byte parity and deterministic
    output order.
  - Deferred shapes list the exact missing parity concerns, such as right/full
    joins, non-equi joins, ambiguous duplicate columns, incompatible union
    schemas, explicit window frames, offset windows, or unstable tie behavior.

- Promote or explicitly close the high-value expression and source gaps.

  Status: Implemented with scoped deferrals.

  Acceptance criteria:

  - `to_number`, `if_else`, path-backed Arrow IPC file input, CLI stdin Arrow
    IPC input, CLI byte-backed Arrow IPC input, and named binding inputs each
    have a v0.37 disposition.
  - Promoted expression coverage has row/native parity across CSV, Parquet, and
    Arrow IPC stream path inputs.
  - Promoted source coverage preserves byte-clean stdout, bounded buffering,
    source-column pruning where practical, deterministic diagnostics, and
    native-free WASM.

- Preserve native-free WASM and public API boundaries.

  Status: Implemented with scoped deferrals.

  Acceptance criteria:

  - No Polars, Parquet, Arrow native reader, object-store, or native
    filesystem-only dependency is added to the wasm target graph.
  - Parser, syntax, semantics, editor services, LSP, CLI public command models,
    and WASM ABI do not expose concrete dataframe internals.
  - Any native byte-backed or stdin reader is explicitly scoped to native CLI
    hosts.

- Preserve small-data, editor, and unsupported-fallback responsiveness.

  Status: Implemented with scoped deferrals.

  Acceptance criteria:

  - Tiny files, row-only browser workflows, editor previews, schema inspection,
    and diagnostics do not gain meaningful native-planning overhead.
  - Unsupported native programs still route directly to rows in `auto` before
    native scans open.
  - If native startup overhead dominates small native-covered inputs, v0.37
    either documents why `auto` still selects native or adds a deterministic
    row-selection policy.

## Should

- Keep v0.36 coverage matrix, benchmark reporting, and plan observability in
  sync with any promoted behavior.

  Status: Implemented with scoped deferrals.

- Promote conservative native join coverage if parity and benchmarks justify
  it.

  Status: Implemented with scoped deferrals.

  Acceptance criteria:

  - Cover `inner` and `left` equi-joins on named single and composite keys.
  - Cover null-key behavior, duplicate right-column suffixing, unmatched rows,
    deterministic output order, and projection pruning across both inputs.
  - Track large fact plus small dimension and large fact plus large dimension
    benchmarks.

- Promote conservative native union coverage if parity and benchmarks justify
  it.

  Status: Implemented with scoped deferrals.

  Acceptance criteria:

  - Cover name-aligned and position-aligned compatible schemas.
  - Cover null padding, incompatible schema rejection, optional native
    `distinct`, and deterministic row order.
  - Track partitioned extract workloads followed by filter, mutate, aggregate,
    and Arrow IPC output.

- Close native source-boundary expansion.

  Status: Implemented with scoped deferrals.

  Acceptance criteria:

  - Path-backed Arrow IPC file native scans ship in v0.37.
  - CLI-only stdin and byte-backed Arrow IPC file/stream readers ship with
    byte-clean stdout behavior.
  - Keep browser-hosted byte sources row-only unless a future browser-specific
    plan changes the product boundary.

- Use native optimizer opportunities deliberately.

  Status: Implemented with scoped deferrals.

  Acceptance criteria:

  - Native plans preserve predicate pushdown, projection pushdown, aggregate
    pushdown, Parquet row-group pruning, and streaming or batched collection
    where the pinned backend supports them.
  - The PDL facade keeps backend optimizer internals private.
  - Plan, manifest, benchmark, or test observability exposes stable PDL-level
    optimizer facts such as required source columns and blocking stages.
  - Any unstable debug output is explicitly marked unstable and redacts
    absolute paths and nondeterministic IDs.

- Carry forward production-style benchmark families.

  Status: Implemented with scoped deferrals.

  Acceptance criteria:

  - Benchmarks include wide CSV projection/filtering, Parquet row-group
    pruning, Arrow IPC producer/consumer handoff, native-covered filter/mutate/
    aggregate/sort/Arrow output, join fact-to-dimension, union partitioned
    extracts, windowed sales ranking, unsupported-native fallback, malformed
    native programs, and small editor-preview workloads.
  - Reports continue to include row/auto/native timings, output rows, output
    bytes, selected/eligible engine, fallback reason, sink strategy,
    row-materialization status, required source columns, unsupported-native
    counts, and peak RSS where practical.
  - Benchmark reports remain reproducible without checking large generated
    fixtures or stress outputs into source.

- Settle the v0.37 memory metric policy.

  Status: Implemented with scoped deferrals.

  Acceptance criteria:

  - Peak RSS remains reported where available.
  - Decide whether allocator stats, phase-local bytes, or phase timings are
    stable enough to add to release comparisons.
  - Memory comparisons have configurable thresholds and documented platform
    caveats.

- Keep PDL-to-Algraf Arrow IPC smoke coverage routine-sized.

  Status: Implemented with scoped deferrals.

  Acceptance criteria:

  - The smoke fixture is large enough to catch throughput regressions but small
    enough for routine local or CI-adjacent runs.
  - Reports include PDL engine, input rows, Arrow bytes, PDL elapsed time,
    Algraf elapsed time where practical, SVG bytes, and status.
  - Logs and diagnostics never pollute Arrow IPC stdout.

- Close high-leverage native expression expansion.

  Status: Implemented with scoped deferrals.

  Acceptance criteria:

  - Evaluate `to_number` and `if_else` first.
  - Require row/native parity across CSV, Parquet, and Arrow-stream path
    inputs.
  - Keep uncertain coercions and data-dependent `col(...)` row-only unless
    eligibility can reject ambiguity before scans open.

- Promote a native window analytics slice for the customer/region revenue
  ranking pipeline if parity and benchmarks justify it.

  Status: Implemented with scoped deferrals.

  Acceptance criteria:

  - The example pipeline in "Windows And Reshaping" is native-eligible for
    path-backed CSV, Parquet, and Arrow IPC stream inputs when the terminal
    sink is native-capable.
  - Native window lowering covers `row_number()`, `dense_rank()`, and
    aggregate-style `sum(expr)` over explicit `partition_by` and `order_by`
    clauses used by the target pipeline.
  - Aggregate-style windows preserve the row-runtime default frame of the
    whole partition when no explicit `rows between` frame is present.
  - Multiple window assignments in one `mutate` stay parallel; a later
    `mutate` stage can reference window columns produced by an earlier stage.
  - Row/native parity tests cover null partition keys, null sort keys, equal
    sort keys, stable tie behavior, descending order, and derived-column sort.
  - Observability marks window execution as blocking and reports no public
    `Row`/`Value` materialization before a native-capable terminal sink.
  - Benchmarks include a million-row `windowed_sales_rank` workload with row,
    auto, and native timings, peak RSS, output bytes, and unsupported-native
    counts.

- Close terminal text-sink materialization gaps.

  Status: Implemented with scoped deferrals.

  Acceptance criteria:

  - Measure how much CSV and JSON Lines row-format writers cost after native
    transforms.
  - Promote native text writers only if byte-for-byte row output parity is
    proven.
  - Expose terminal conversion cost in benchmark or plan observability where
    practical.

## Could

- Evaluate broader native window-expression coverage after the customer/region
  ranking slice.

  Status: Implemented with scoped deferrals.

  Candidate functions: `row_number`, `rank`, `dense_rank`, `lag`, `lead`,
  `first_value`, `last_value`, `count`, `sum`, `mean`, `min`, and `max` over
  explicit partitions and orderings.

- Evaluate native JSON Lines scanning.

  Status: Implemented with scoped deferrals.

  JSON Lines should stay row-only unless deterministic schema inference, null
  behavior, text rendering, and unsupported-type diagnostics can match row
  semantics.

- Evaluate configurable CSV dialect support.

  Status: Implemented with scoped deferrals.

  This is in scope only where dialect settings affect native CSV input,
  pushdown, or native/text sink parity. Broader CSV language design should stay
  out of v0.37.

- Evaluate narrow native `pivot_longer` and `complete` subsets.

  Status: Implemented with scoped deferrals.

  These should be promoted only if deterministic output order, null handling,
  fill expressions, and mixed-type behavior can match the row runtime.

- Decide whether `--engine auto` needs a small-data threshold.

  Status: Implemented with scoped deferrals.

  This should be based on repeated measurements of native startup overhead
  against small CSV, JSON Lines, editor-preview, and browser-style workloads.

## Validation Notes

Implementation validation for the promoted native slices includes focused
`pdl-exec` row/auto/native parity tests for:

- `to_number` across path-backed CSV, Parquet, and Arrow IPC stream inputs;
- `if_else` across path-backed CSV, Parquet, and Arrow IPC stream inputs;
- path-backed Arrow IPC file input in `auto`;
- Arrow IPC file host bytes and Arrow IPC stream stdin bytes in `auto`;
- native left join against a binding-backed input, including duplicate
  right-column suffixing and null-key non-matching;
- native semi and anti joins against a binding-backed input, including null-key
  non-matching;
- native union by name with `distinct` against a binding-backed input.

The v0.37 code also preserves existing native aggregate, mutate, Arrow stdout,
Parquet/Arrow sink, fallback, window row-runtime, reshape row-runtime, and
manifest observability tests.

Full repeated release-profile benchmark medians for the large tracked workloads
are not checked into source. The large workload names remain tracked under
`bench/workloads/large/`, and v0.38 should use the repeated benchmark
infrastructure before promoting broader windows, right/full joins, native text
writers, JSON Lines native input, or small-data auto-engine policy changes.

## Non-Goals

- Do not reopen v0.36 release scope in this plan.
- Do not expose dataframe implementation internals in the language, CLI public
  model, editor services, LSP, or WASM ABI.
- Do not add a general mid-pipeline native-to-row fallback unless a separate
  design promotes it.
- Do not make browser/WASM execution native.
- Do not expose Polars expressions, lazy plans, Arrow arrays, Parquet metadata,
  optimizer hints, or physical plans as PDL syntax.
- Do not promote native text sinks if output differs from the row runtime.
