# PDL v0.50 Plan

Status: Shipped
Target version: 0.50.0
Owner: PDL maintainers
Related spec: [`PDL_SPEC.md`](PDL_SPEC.md)
Coverage matrix: [`PDL_NATIVE_COVERAGE.md`](PDL_NATIVE_COVERAGE.md), [`PDL_NATIVE_COVERAGE.csv`](PDL_NATIVE_COVERAGE.csv)
Predecessor plan: [`V0_49_PLAN.md`](V0_49_PLAN.md)
Successor plan: [`V0_51_PLAN.md`](V0_51_PLAN.md)

## Purpose

PDL v0.50 is the post-parity performance release.

v0.49 deliberately chose semantic closure over raw speed: every
language-level feature became native-eligible, and where Polars could
not represent PDL's row-visible semantics exactly, `pdl-data` crossed a
native orchestration boundary into the public `Table`/`Row`/`Value`
representation and evaluated the feature with row semantics. That was
the right v0.49 tradeoff because byte parity was non-negotiable.

v0.50 keeps the same semantic contract but stops accepting those bridges
as the final implementation. The release target is:

1. Identify every place selected-native execution collects native data
   into rows before the terminal sink.
2. Remove the bridges that can be replaced with row-identical Polars
   lowerings.
3. Replace unavoidable row-semantic bridges with algorithms that are
   linear or near-linear, not quadratic.
4. Make benchmark reporting loud enough that a selected-native pipeline
   that secretly spends most of its time in row objects is obvious.

The main problem class is not "native unsupported" anymore. It is
selected-native execution that is semantically native but operationally
row-heavy.

## Release Thesis

After v0.50, a selected-native pipeline should have one of three clear
performance stories:

1. It stays in Polars through the expensive portion of the plan.
2. It crosses to row semantics once, with a bounded and measured reason.
3. It uses a custom PDL algorithm that preserves row semantics without
   repeatedly rebuilding the same partitions, orders, or value maps.

The release should make the following class of bug unacceptable:

```text
for each output row:
  rebuild the current partition
  sort the partition
  find the current row
  evaluate a window expression
```

That is the dynamic-window path introduced by v0.49 parity work. It is
byte-correct, but it can be quadratic on large partitions. v0.50 must
turn that into a cached partition/order plan or a native lowering.

## Starting Point

The latest tracked stress reports before v0.49 showed two old slow
classes:

```text
v0.36 same-release row-vs-auto stress report:

workload                         selected engine  elapsed
million_row_join_dimension        row              4013ms
million_row_union_partitions      row              3011ms
```

v0.49 promotes those shapes to native eligibility, so they are no longer
the primary class of concern. A current v0.49 smoke-scale release run
instead showed the new standout:

```text
workload                              elapsed   selected engine
million_row_dynamic_window_offsets     2685ms   native
million_row_segment_summary             288ms   native
million_row_segment_summary_parquet     265ms   native
```

The absolute numbers above came from smoke data (`input_rows=1000`), not
the intended million-row stress files. That makes the finding more
important, not less: dynamic offset windows dominate even when the input
is small.

The relevant implementation shape is:

- `DataPlan::mutate` collects a native plan when
  `data_expr_requires_row_semantics` says an expression needs row
  semantics.
- `data_expr_requires_row_semantics` currently treats dynamic
  `col(value)`, temporal functions, mixed `if_else`, dynamic
  `replace`, multi-key window ordering, and non-literal `lag`/`lead`
  offsets as row-semantic boundaries.
- `eval_data_window_expr` calls `data_ordered_partition_indices` per
  row.
- `data_ordered_partition_indices` scans the table and sorts the current
  partition each time it is called.

That last pair is the immediate quadratic risk.

## Performance Model

v0.50 treats "native parity" as a semantic label, not a performance
guarantee. Performance work is tracked with more specific implementation
labels:

| Label | Meaning |
| --- | --- |
| Polars-native | The stage/expression is represented as Polars expressions or lazy operations through the expensive part of the plan. |
| Native bridge | The pipeline starts native but collects to `Table` before continuing. |
| Cached row bridge | The bridge is required for PDL semantics, but shared work is indexed/cached and the algorithm is not quadratic. |
| Writer-bound | The dominant cost is byte-identical output encoding or filesystem writes, not transformation. |
| Host-bound | The dominant cost is process, browser, LSP, or external handoff overhead. |

`PDL_NATIVE_COVERAGE.csv` remains the language coverage matrix. v0.50
adds performance observability in benchmark reports rather than
reintroducing intermediate language-support statuses.

## Must

- Add a native materialization inventory and make it testable.

  Status: Shipped in 0.50.0.

  Every call to `native_collect_to_table` in `pdl-data` must have a
  reason category. The initial categories are:

  - `terminal_collect`
  - `dynamic_column_lookup`
  - `dynamic_replace_text`
  - `mixed_class_conditional`
  - `temporal_scalar`
  - `window_dynamic_offset`
  - `window_multi_order`
  - `union_alignment`
  - `pivot_longer_order_or_mixed_value`
  - `complete_key_expansion_or_fill`
  - `json_lines_scan`
  - `native_writer_text_bridge`

  Acceptance criteria:

  - Internal observability records whether a selected-native run
    collected to row objects before the terminal sink.
  - Benchmark reports include the materialization reason categories,
    not just a boolean.
  - `native-strict` still means "no engine fallback"; a materialization
    reason is a performance fact, not a language fallback.
  - Tests fail if a new native-to-row collection site is added without
    assigning a reason.

- Eliminate the quadratic dynamic offset window path.

  Status: Shipped in 0.50.0.

  The dynamic `lag(value, offset, default)` and
  `lead(value, offset, default)` semantics are row-visible because
  `offset` may be computed from the current row. A literal offset can
  lower to Polars `shift`, but a per-row offset cannot be represented as
  one static `shift(N)`.

  v0.50 must implement one of these strategies:

  1. A cached row bridge:
     - group row indices by partition key once;
     - sort each partition once;
     - build `row_index -> partition position` once;
     - evaluate all dynamic-offset window assignments against that cache.
  2. A Polars-compatible target-position lowering:
     - compute per-row partition position;
     - compute target position as `position +/- offset`;
     - join back to the value column inside the partition;
     - apply default expressions for out-of-bounds rows.

  The cached row bridge is acceptable if it is linear or
  `O(n log n)` per partition group and byte-identical. The existing
  per-row partition rebuild/sort is not acceptable.

  Acceptance criteria:

  - `million_row_dynamic_window_offsets` becomes a stress benchmark
    gate.
  - Runtime grows sub-quadratically as row count scales from smoke to
    stress tiers.
  - The implementation handles multiple dynamic offset window
    assignments sharing the same partition/order spec without rebuilding
    the cache.
  - Error behavior for null, negative, fractional, text, and boolean
    offsets stays row-identical.
  - The benchmark report identifies the path as either
    `Polars-native` or `Cached row bridge`, never unclassified
    row materialization.

- Replace avoidable row bridges with homogeneous Polars lowerings.

  Status: Shipped in 0.50.0.

  Some v0.49 bridges exist because mixed PDL value classes cannot be
  represented in one Polars column without changing row-visible bytes.
  That does not mean every use of the feature must bridge.

  Candidate homogeneous fast paths:

  - `if_else` when both branches have the same proven value class.
  - `replace` when pattern and replacement are literals or scalar
    contexts.
  - temporal extraction and formatting subsets Polars can express
    byte-identically.
  - `pivot_longer` when value columns have one value class and row order
    restoration can stay native.
  - `complete` when fill expressions preserve column value classes.
  - `union by_name` and positional `union` when schemas align and no
    null-padding class ambiguity is present.

  Acceptance criteria:

  - Each fast path has row/native parity tests.
  - Each fallback keeps the v0.49 byte-parity behavior.
  - Benchmark reports show whether the homogeneous fast path was used.
  - No lowering may change CSV, JSON Lines, Arrow IPC, or Parquet bytes.

- Add a bridge-local performance cache for unavoidable row semantics.

  Status: Shipped in 0.50.0.

  Some PDL semantics are intentionally row-visible and do not map cleanly
  to Polars' typed column model:

  - data-dependent `col(value)`;
  - mixed-class `if_else`;
  - mixed-class `pivot_longer`;
  - class-changing `complete` fills;
  - dynamic string pattern/replacement expressions;
  - JSON Lines schema and missing-field inference where the row reader is
    the spec.

  These must not become slow merely because they are not Polars-native.
  v0.50 should introduce reusable caches for:

  - column-name lookup maps;
  - expression evaluation over repeated row contexts;
  - partition keys and ordered partition indices;
  - union output-column mapping;
  - complete key-domain expansion;
  - pivot output row ordering.

  Acceptance criteria:

  - Row bridges are single-pass or cache-backed for repeated work.
  - The same cache can serve multiple assignments in one stage when the
    partition/order/schema context is identical.
  - Caches are scoped to one stage or bridge and do not alter observable
    mutation parallelism.

- Reduce row-engine allocation and clone pressure.

  Status: Shipped in 0.50.0.

  v0.49 focused on making the row engine the byte-parity reference while
  selected-native execution became semantically complete. v0.50 must also
  treat the row engine as production code, because it is still:

  - the WASM execution path;
  - the fallback/opt-in `--engine row` path;
  - the semantic bridge used by selected-native features that cannot
    stay Polars-native;
  - the reference implementation used by parity tests.

  The current row representation is allocation-heavy:

  - `Row` owns `Vec<Value>`, and `Table` owns `Vec<Row>`.
  - Row filters clone every passing `Row`.
  - Row mutates clone `row.values` for every input row before assigning
    appended or replacement values.
  - Row unions allocate remapped output rows for both left and right
    inputs, even when the left side could be moved or shared.
  - Projection, drop, distinct, sort, preview, and aggregate helpers
    repeatedly build column index maps and string keys.

  v0.50 should not jump straight to one representation without
  measurement. The implementation plan is staged:

  1. Remove avoidable clones with ownership-aware APIs. Stages that take
     `Table` by value should move rows instead of iterating by reference
     and cloning when row shape is unchanged.
  2. Introduce shared row storage only where it wins. `Arc<Row>` or
     `Arc<[Value]>` can make filters, limits, and renames cheap, but it
     can make mutates and appends worse if every row immediately needs a
     copy-on-write allocation.
  3. Add a `TableView` or row-index selection layer for filter/limit/
     sort-like operations so they can defer physical row copies until a
     sink or mutation requires materialization.
  4. Evaluate a columnar internal row-runtime representation, ideally
     Arrow-array-backed or Arrow-like, for bridge-heavy and WASM
     workloads. This is the bigger win for mutates, aggregates, and
     vectorizable scalar functions, but it is a larger compatibility
     project because PDL `Value` preserves per-cell classes.

  Acceptance criteria:

  - Add allocation/clone instrumentation or heap profiling notes for
    representative row, bridge-heavy, and WASM workloads.
  - `filter` and `limit` avoid cloning unchanged rows in the common
    case.
  - `rename` avoids cloning rows entirely.
  - `mutate` avoids repeated column-name lookups and preallocates output
    width; replacement-only mutates avoid reallocating when a move-based
    path is possible.
  - `union` moves or shares left rows when schemas already align, and
    uses precomputed index maps when remapping is required.
  - `distinct` and grouped aggregate avoid stringifying key values when
    typed keys can be compared directly without changing semantics.
  - Any `Arc<Row>`/`Arc<[Value]>` experiment is benchmarked against the
    move-based and columnar alternatives before it becomes the default
    representation.
  - The final v0.50 update records which representation changes landed
    and which remain deferred to a larger row-runtime rewrite.

- Fix and track multi-output fanout performance.

  Status: Shipped in 0.50.0.

  `million_row_multi_output_fanout` failed in the current local run
  because its non-terminal `save` writes to a generated-data path whose
  parent did not exist in that execution context. v0.50 must make this
  workload reliable before using it as evidence.

  Acceptance criteria:

  - The workload writes to a run-local output directory or the benchmark
    harness creates the parent directory before execution.
  - Benchmark reports separately identify non-terminal save time,
    named-output execution time, and terminal write time where possible.
  - Native fanout remains selected-native and byte-identical to row.

- Restore true stress benchmark discipline.

  Status: Shipped in 0.50.0.

  A release run that says `tier=stress` but records `input_rows=1000`
  is not useful for performance decisions. The harness currently avoids
  regenerating existing generated data, which can leave smoke data in
  place for a stress run.

  Acceptance criteria:

  - Generated dataset metadata records row count and tier.
  - `pdl-bench run --tier stress` refuses to use smoke-sized generated
    data unless an explicit override is passed.
  - `pdl-bench generate --tier stress` and `prepare --tier stress`
    produce all v0.49/v0.50 workload inputs consistently.
  - Release-performance commands use release binaries, warmups, repeated
    samples, randomization, and median comparisons.

- Add performance gates for the slowest query families.

  Status: Shipped in 0.50.0.

  v0.50 should gate these workload families:

  | Family | Representative workload | Expected result |
  | --- | --- | --- |
  | Dynamic offset windows | `million_row_dynamic_window_offsets` | No quadratic growth; classified bridge or native lowering. |
  | Joins followed by aggregates | `million_row_join_dimension`, `million_row_composite_join_rollup` | Stays native and materially faster than row. |
  | Union followed by aggregate | `million_row_union_partitions`, `million_row_union_null_padding` | Avoids full row bridge when schemas permit. |
  | Reshape/key expansion | `million_row_pivot_longer`, `million_row_complete_buckets` | Homogeneous cases use native or cached bridge. |
  | Writer-dominated text output | `million_row_text_emission` | Classified as writer-bound when transform cost is low. |
  | Multi-output fanout | `million_row_multi_output_fanout` | Reliable and measured. |
  | Dynamic expressions | `million_row_dynamic_text_and_col` | Avoids repeated lookup/evaluation overhead. |
  | Temporal/JSON Lines | `million_row_temporal_buckets`, `million_row_jsonl_temporal_buckets` | Separates scan, parse, transform, and write costs. |
  | Row-runtime allocation | row-engine variants of filter, mutate, union, distinct, and aggregate workloads | Fewer clones/allocations with unchanged bytes. |

  Acceptance criteria:

  - The slowest five rows in the release report have an explanation
    field that names the dominant cost class.
  - Any workload slower than same-run row execution by more than the
    configured threshold must either fail the gate or be explicitly
    marked writer-bound/host-bound with evidence.

## Should

- Preserve v0.49 native parity and the two-status coverage vocabulary.

  Status: Shipped in 0.50.0.

  v0.50 performance work must not reopen language coverage statuses.
  `native parity` remains true even when implementation uses a cached row
  bridge. Performance classifications live in benchmark observability,
  not the language coverage matrix.

- Add plan and manifest performance facts.

  Status: Shipped in 0.50.0.

  `pdl plan`, `pdl manifest`, or benchmark-only plan JSON should expose:

  - selected engine;
  - eligible engine;
  - materialization reason categories;
  - native bridge count;
  - estimated row bridge stages;
  - sink strategy;
  - required source columns;
  - whether dynamic windows use cached or per-expression execution.

  These facts must not pollute stdout data streams.

- Improve writer-bound attribution.

  Status: Shipped in 0.50.0.

  Text sinks often become the bottleneck after native transforms become
  fast. v0.50 should avoid chasing Polars rewrites when the actual cost
  is byte-identical CSV or JSON Lines formatting.

  Acceptance criteria:

  - Benchmarks separate transform and write phases when practical.
  - CSV and JSON Lines writers keep row-writer byte semantics.
  - The report can say "writer-bound" for workloads where Polars is no
    longer the limiting factor.

- Document "why Polars cannot speed this up" for every remaining bridge.

  Status: Shipped in 0.50.0.

  The final v0.50 plan update should include a table with one row per
  remaining bridge:

  | Bridge | Why direct Polars lowering is unsafe | v0.50 mitigation |
  | --- | --- | --- |
  | dynamic offset windows | per-row offset is not one static shift | cached row bridge or target-position join |
  | mixed-class conditionals | Polars column has one dtype; PDL cells keep value classes | homogeneous fast path plus bridge |
  | dynamic `col(value)` | selected column varies per row | lookup cache or generated case expression where finite |
  | dynamic `replace` | pattern/replacement may vary per row | literal/context fast path plus bridge cache |
  | mixed `pivot_longer` | one output value column may contain multiple PDL classes | homogeneous native fast path plus bridge |
  | class-changing `complete` | inserted row cells may change class per column | class-preserving native fast path plus bridge |
  | JSON Lines scan | row reader defines schema/missing-field semantics | classify as scan bridge; optimize reader |

- Keep WASM/editor boundaries Polars-free.

  Status: Shipped in 0.50.0.

  Browser and editor surfaces still use the row engine. Any cache or
  algorithm added for native bridges should be reusable by the row engine
  where it helps, but v0.50 must not add Polars, Parquet, object-store,
  or native filesystem assumptions to `pdl-wasm`.

## Could

- Evaluate configurable CSV dialect support.

  Status: Deferred from v0.49.

  This remains future language/source-sink contract work. It should not
  distract from v0.50 performance cleanup unless a benchmark proves the
  current CSV path is blocked by dialect assumptions.

- Evaluate a browser byte IO ABI for binary host-file contents and Arrow
  IPC output.

  Status: Deferred from v0.49.

  Browser byte IO may be useful for product reasons, but it is not a
  native-host performance optimization.

- Evaluate object-store and remote path support with a dedicated security
  and IO plan.

  Status: Deferred from v0.49.

  Remote IO could dominate query time and needs a separate security,
  caching, and credential model before implementation.

- Explore generated finite-case Polars expressions for dynamic `col`.

  Status: Evaluated in 0.50.0; deferred to v0.51+.

  If the set of possible column names can be proven finite from schema
  and expression analysis, a dynamic `col(value)` may lower to a Polars
  `when`/`then` chain instead of a row lookup. This is optional because
  the expression can become large and may be slower than a cached row
  lookup for wide schemas.

- Explore native self-join lowering for dynamic offset windows.

  Status: Evaluated in 0.50.0; cached row bridge shipped instead.

  A target-position self join may keep dynamic offsets Polars-native, but
  it is more complex than the cached row bridge. It should land only if
  benchmarks prove it is faster and parity tests cover null/default edge
  cases.

## Final v0.50 Update

v0.50 shipped the performance-observability and bridge-cache layer without
changing PDL syntax or row-visible bytes.

Materialization inventory:

| Category | v0.50 handling |
| --- | --- |
| `terminal_collect` | Tagged at explicit terminal collection. |
| `dynamic_column_lookup` | Classified in plan/manifest/bench observability and kept as a row-visible bridge. |
| `dynamic_replace_text` | Classified when pattern or replacement is expression-valued. |
| `mixed_class_conditional` | Classified for non-homogeneous `if_else`; homogeneous proven branch classes use the native lowering. |
| `temporal_scalar` | Classified for temporal scalar row-visible semantics. |
| `window_dynamic_offset` | Uses the cached row bridge. |
| `window_multi_order` | Classified as a row-visible bridge boundary. |
| `union_alignment` | Schema-compatible unions stay native; alignment/null-padding bridges are tagged. |
| `pivot_longer_order_or_mixed_value` | Homogeneous native path is attempted first; mixed/order bridge is tagged. |
| `complete_key_expansion_or_fill` | Class-preserving native path is attempted first; fill/key bridge is tagged. |
| `json_lines_scan` | Classified as a scan bridge in plan and benchmark observability. |
| `native_writer_text_bridge` | Reserved for writer-bound text attribution; text writers still avoid row-table materialization. |

Remaining bridges and mitigations:

| Bridge | Why direct Polars lowering is unsafe | v0.50 mitigation |
| --- | --- | --- |
| dynamic offset windows | per-row offset is not one static shift | cached partition/order/position bridge shared by matching specs |
| mixed-class conditionals | Polars columns have one dtype while PDL cells preserve value classes | homogeneous fast path plus classified bridge |
| dynamic `col(value)` | selected column can vary per row | classified row-visible lookup bridge |
| dynamic `replace` | pattern/replacement can vary per row | literal fast path plus classified bridge |
| mixed `pivot_longer` | one output value column may contain multiple PDL classes | homogeneous native fast path plus classified bridge |
| class-changing `complete` | inserted row cells may change class per column | class-preserving native fast path plus classified bridge |
| JSON Lines scan | row reader defines schema and missing-field semantics | classified scan bridge |
| temporal scalars | row parser/formatter defines accepted bytes and output strings | classified scalar bridge |
| union alignment/null padding | padded cells and class compatibility are row-visible | schema-compatible native fast path plus classified bridge |

Row-runtime representation changes landed:

- `filter` evaluates keep decisions first and then moves kept rows instead of
  cloning every passing row.
- `limit` truncates the owned row vector.
- `rename` moves rows unchanged.
- `mutate` precomputes output indexes, shares a stage-local window cache, and
  moves row storage before applying replacement/appended values.
- `union` moves aligned left rows and precomputes right-side remap indexes.
- `Table::column_index` no longer allocates an index map per lookup.

Deferred row-runtime work:

- A larger `TableView`, `Arc<Row>`, or columnar row-runtime rewrite remains
  future work because v0.50 favored targeted move-based changes and cache
  locality over a representation swap.
- Generated finite-case Polars expressions for dynamic `col(value)` and a
  target-position self-join for dynamic offset windows remain v0.51+ research.

Benchmark and reporting changes:

- `pdl-bench` generated datasets now write metadata with tier and row count.
- `pdl-bench run --tier <tier>` refuses mismatched generated data unless
  `--allow-tier-mismatch` is passed.
- Reports include `materialization_reasons`, `native_bridge_count`,
  `estimated_row_bridge_stages`, `dynamic_window_strategy`, and
  `performance_classification`.
- The multi-output fanout workload is reliable when the generated-data
  preparation step creates the parent directory; run-local benchmark output
  remains under `bench/runs/<run-label>/`.

## Risks

- The fastest Polars expression is still wrong if it changes row-visible
  bytes. v0.50 must keep the v0.49 parity bar.
- A cached row bridge can hide row materialization cost while still
  allocating too much memory. Benchmarks need peak RSS and bridge counts.
- Polars-native rewrites for homogeneous cases can accidentally change
  type normalization, null behavior, string formatting, or output order.
- Dynamic window optimizations are easy to make correct for one
  assignment and wrong for multiple assignments sharing a mutate stage.
  Mutate assignments remain parallel against the input schema.
- Stress benchmarks are expensive. The release needs a bounded smoke gate
  for development and a real stress gate before closure.

## Validation Notes

Repository-required checks remain authoritative:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets
cargo test --workspace
```

WASM target graph must stay clean:

```bash
RUSTC=$(rustup which --toolchain stable rustc) \
  rustup run stable cargo check -p pdl-wasm --target wasm32-unknown-unknown

RUSTC=$(rustup which --toolchain stable rustc) \
  rustup run stable cargo tree -p pdl-wasm --target wasm32-unknown-unknown --edges normal
```

Performance validation should include:

```bash
cargo run -p pdl-bench -- generate --tier stress
cargo run -p pdl-bench -- run \
  --suite large \
  --tier stress \
  --profile release \
  --engine auto \
  --run-label v0_50_after_auto_release \
  --samples 7 \
  --warmups 1 \
  --randomize \
  --cooldown-ms 250

cargo run -p pdl-bench -- run \
  --suite large \
  --tier stress \
  --profile release \
  --engine row \
  --run-label v0_50_after_row_release \
  --samples 3 \
  --warmups 1 \
  --randomize \
  --cooldown-ms 250

cargo run -p pdl-bench -- compare \
  --baseline bench/runs/v0_50_after_row_release/report.csv \
  --run-label v0_50_after_auto_release \
  --max-relative-regression 0.05 \
  --max-absolute-regression-ms 50
```

The final release update should report:

- slowest ten workloads;
- row-vs-auto and prior-release comparisons;
- materialization reason counts;
- row-engine allocation/clone findings;
- peak RSS for bridge-heavy workloads;
- whether dynamic offset windows are Polars-native or cache-backed;
- any remaining bridge and why direct Polars lowering is unsafe.

## Non-Goals

- Do not change PDL language syntax or semantics to make Polars lowering
  easier.
- Do not relax byte parity with the row engine.
- Do not remove the row engine; it remains the reference runtime and the
  WASM execution path.
- Do not reintroduce `native partial` or `planned native` into the
  language coverage matrix.
- Do not pull Polars or native file-format dependencies into
  `pdl-wasm`.
- Do not optimize by changing CSV, JSON Lines, Arrow IPC, or Parquet
  output bytes.
