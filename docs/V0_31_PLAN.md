# PDL v0.31 Plan

Status: Planned
Target version: 0.31.0
Owner: PDL maintainers
Related spec: [`PDL_SPEC.md`](PDL_SPEC.md)
Predecessor plan: [`V0_30_PLAN.md`](V0_30_PLAN.md)
Related Algraf plan: [`V0_68_PLAN.md`](../../algraf/docs/V0_68_PLAN.md)
Promoted Algraf note: [`ARROW_PERFORMANCE.md`](../../algraf/docs/ARROW_PERFORMANCE.md)
Cross-repo coordination: `../algraf/` for Arrow IPC stream visualization
handoff smoke tests, reader-oriented Arrow ingestion, aggregate-first
rendering, and bounded output expectations.

## Purpose

PDL v0.31 starts the native execution performance work by adding an
engine-oriented `pdl-data` facade and a first Polars-backed fast path for
semantically safe native pipelines. The current row runtime remains the
portable reference implementation for WASM, editor previews, tests, small data,
and unsupported operations.

PDL currently declares Polars as an optional native data dependency, but the
execution path still mostly runs through `Table`, `Row`, and `Value`:

```text
source bytes -> Table<Vec<Row>> -> row operations -> Table -> output bytes
```

That shape is simple, deterministic, and portable, but it prevents native builds
from using Polars for lazy scans, projection pushdown, predicate pushdown,
vectorized expression evaluation, parallel operators, and direct typed output.
The v0.31 release should introduce the layer that lets native execution keep
work lazy and columnar until materialization or writing is actually required:

```text
source path -> lazy dataframe plan -> vectorized operations -> collect/write
```

The release must protect the PDL-to-Algraf visualization workflow:

```bash
pdl run prep.pdl --stdout-format arrow-stream \
  | algraf render chart.ag --data - --data-format arrow-stream --output chart.svg
```

For large visualization inputs, PDL should reduce and type the data before
handoff, avoid CSV formatting/parsing, and preserve Arrow IPC streams as the
preferred Unix-pipe boundary.

The neighboring Algraf v0.68 performance plan owns the consumer-side work:
reading caller-provided Arrow streams through a reader path, keeping
Arrow/Parquet internals private to `algraf-data`, moving stats and scale
training toward column-oriented scans, and rendering bounded aggregate outputs
rather than millions of raw SVG marks. This PDL plan owns the producer side:
native tabular reduction, typed Arrow IPC stream stdout, row fallback, and
backend privacy.

This plan is not normative until behavior is promoted into
[`PDL_SPEC.md`](PDL_SPEC.md) with concrete API, CLI, format, and diagnostic
language. Implementation must keep examples runnable against the accepted PDL
syntax in the working tree.

## Must

- Add an opaque data-plan facade beside the existing row API.

  Status: Planned.

  `pdl-data` MUST keep the existing public `Table`, `Row`, `Value`,
  `read_table_from_bytes`, and `write_table_to_bytes` APIs. It must add
  engine-neutral planning types that can represent either portable row work or
  native Polars work without exposing concrete Polars types above `pdl-data`.

  The exact names may change during implementation, but the API shape should
  preserve these properties:

  ```rust
  pub enum DataBackend {
      PortableRows,
      NativePolars,
  }

  pub struct DataPlan {
      inner: DataPlanInner,
  }

  pub enum DataSource<'a> {
      Path {
          path: &'a std::path::Path,
          format: DataFormat,
      },
      Bytes {
          logical_path: &'a std::path::Path,
          format: DataFormat,
          bytes: &'a [u8],
      },
  }

  pub enum DataSink<'a> {
      Path {
          path: &'a std::path::Path,
          format: DataFormat,
      },
      Writer {
          format: DataFormat,
          writer: &'a mut dyn std::io::Write,
      },
      Bytes {
          format: DataFormat,
      },
  }
  ```

  Acceptance criteria:

  - Existing row APIs remain source-compatible.
  - `DataPlan` can report its selected `DataBackend`.
  - `DataSource::Path` and `DataSource::Bytes` are both supported.
  - `DataSink::Writer` exists so stdout formats can stream through an
    `io::Write` boundary instead of requiring a whole output `Vec<u8>`.
  - Public APIs above `pdl-data` do not expose `polars::DataFrame`,
    `polars::LazyFrame`, Polars expressions, Arrow reader internals, or Parquet
    reader internals.

- Keep dependency layering and feature gates intact.

  Status: Planned.

  Polars must remain private to `pdl-data` behind the native feature set.
  `pdl-exec`, `pdl-driver`, `pdl-semantics`, CLI, LSP, editor services, and
  WASM must stay free of concrete Polars imports.

  Acceptance criteria:

  - No crate above `pdl-data` imports or mentions Polars symbols.
  - `pdl-wasm` does not enable `native-formats` or `polars-engine`.
  - Native format support remains feature-gated.
  - Backend selection and unsupported-operation handling use normal PDL
    diagnostics when execution cannot continue.

- Implement the facade with the row runtime first.

  Status: Planned.

  The first `DataPlan` implementation MUST be able to wrap the current row
  engine so the new API can land without changing source-language semantics.
  The row engine remains the semantic reference for native parity tests and the
  default portable fallback.

  Acceptance criteria:

  - Row-backed `DataPlan` supports the operations needed by existing execution:
    scan, filter, select, drop, rename, mutate, sort, limit, distinct,
    group/aggregate where already supported by the row runtime, collect to
    `Table`, and write to supported sinks.
  - Existing runtime tests pass through the row-backed facade or an equivalent
    compatibility path.
  - WASM continues to use the portable row backend by default.
  - Browser dry-run save behavior remains row/text based.

- Add conservative expression lowering into `pdl_data::DataExpr`.

  Status: Planned.

  `pdl-exec` SHOULD translate semantic expressions into a smaller data-layer
  expression type. `pdl-data` then maps that expression to either the row
  evaluator or Polars expressions.

  Initial native expression support MUST stay conservative:

  - column references;
  - string, number, bool, and null literals;
  - boolean `and`, `or`, and `not`;
  - comparisons;
  - numeric arithmetic where type compatibility is clear;
  - scalar functions that have direct Polars equivalents and matching PDL
    semantics.

  Acceptance criteria:

  - `pdl-exec` does not import Polars.
  - Unsupported native expressions cause whole-pipeline row fallback until
    mid-pipeline materialization exists.
  - Expression parity tests compare row and native results before a native
    expression is enabled.

- Add the first native Polars fast path for path-backed pipelines.

  Status: Planned.

  Native execution MUST prefer path sources when the driver has a real resolved
  path, allowing Polars lazy scanners to push work toward the source:

  ```text
  DataSource::Path
    -> Polars lazy plan
    -> vectorized operations
    -> collect or write
  ```

  The v0.31 native fast path should cover the lowest-risk stages:

  - `load` from real paths;
  - `filter`;
  - `select`;
  - `drop`;
  - `rename`;
  - `limit`;
  - `sort`;
  - `distinct` for simple column sets.

  Acceptance criteria:

  - Backend selection is explicit and testable.
  - The first implementation chooses the backend for the whole pipeline before
    execution.
  - Unsupported stages, byte-backed inputs, and uncertain semantics fall back to
    the row runtime.
  - Native path-backed pipelines can avoid `Table<Vec<Row>>` materialization
    until collect or write.
  - Benchmarks show a meaningful improvement for medium path-backed inputs.

- Preserve Arrow IPC stream stdout as a first-class output target.

  Status: Planned.

  The native backend MUST treat Arrow IPC stream output as a primary typed
  handoff, not as a compatibility afterthought. `--stdout-format arrow-stream`
  and `save stdout format "arrow-stream"` should share the same writer-oriented
  path where practical.

  The ideal native path is:

  ```text
  DataSource::Path
    -> Polars lazy plan
    -> Polars/Arrow batches
    -> Arrow IPC stream writer on stdout
    -> Algraf caller data reader
    -> Algraf render
  ```

  Acceptance criteria:

  - Stdout contains only Arrow IPC stream bytes.
  - Diagnostics, progress messages, and human-readable logs go to stderr.
  - Output schema preserves PDL column order.
  - Arrow batch order and row order are deterministic for deterministic PDL
    plans.
  - Unsupported Arrow column types or unsupported native output plans fail
    before writing partial stdout bytes where practical.
  - Native Arrow output avoids converting through `Table<Vec<Row>>` when the
    active native plan can write directly.

- Define the native compatibility policy before enabling each operation.

  Status: Planned.

  The row engine is the reference behavior. Polars defaults will not always
  match it, especially for CSV type inference, mixed dynamic values, null
  ordering, null equality, joins, distinct operations, grouped output ordering,
  floating-point formatting, integer-to-float normalization, and lazy error
  timing.

  Acceptance criteria:

  - If parity is uncertain, execution uses the row engine.
  - Native materialization normalizes Polars output back into PDL `Value`
    semantics where a public `Table` is required.
  - Any intentional behavior change is documented in `PDL_SPEC.md` before it is
    enabled.
  - CSV native scans have an explicit policy for PDL-compatible parsing versus
    Polars type inference.

- Prove native behavior with parity tests and benchmarks.

  Status: Planned.

  Every native-backed operation MUST have parity coverage against the row
  runtime. Tests should run the same PDL program through both engines and
  compare user-visible results.

  Acceptance criteria:

  - Parity tests compare CSV output bytes, final `Table` values, diagnostics,
    named output ordering, saved text outputs, and Arrow IPC stream output where
    applicable.
  - Fixtures cover empty input, nulls, booleans, numbers, strings, mixed CSV
    columns, duplicate columns, rename collisions, sorted output, grouped
    output, joins with unmatched rows where supported, save stages, and stdout
    formats.
  - Boundary tests assert that Polars does not leak above `pdl-data`.
- Benchmarks compare row and native runtime for CSV filter/select, CSV
  group/aggregate where supported, Parquet filter/projection, sort/limit,
  distinct, and the PDL-to-Algraf Arrow stream handoff.
- `scripts/run-large-demos.sh` appends timestamped rows to
  `bench-output/large-demos/report.tsv` so before/after large-demo runs can be
  compared with the same workflow used by Algraf's large-demo script.

- Keep spec, versions, and release documents aligned when implementation lands.

  Status: Planned.

  Acceptance criteria:

  - `PDL_SPEC.md` records any promoted data backend, stdout, CLI, format,
    diagnostics, and release-table behavior before this plan is marked
    implemented.
  - Workspace/package version stamps that track the release are aligned to
    `0.31.0` when this numbered plan is implemented.
  - If the release closes with all Must items implemented, the next minor plan
    is started.

## Should

- Add a debug-visible backend selection mode.

  Status: Planned.

  A CLI flag such as `--engine row`, `--engine native`, or equivalent test-only
  hooks should make backend selection observable enough for parity tests and
  performance diagnosis without exposing concrete backend internals as normal
  user-facing API.

- Include a PDL-to-Algraf smoke test where practical.

  Status: Planned.

  The canonical pipe should be covered when a local Algraf binary is available:

  ```bash
  pdl run prep.pdl --stdout-format arrow-stream \
    | algraf render chart.ag --data - --data-format arrow-stream --output chart.svg
  ```

  Normal PDL tests may use a fake Arrow stream consumer when cross-repo binaries
  are unavailable.

- Pilot grouped aggregate native coverage if the core fast path is stable.

  Status: Planned.

  Simple `group_by` plus `agg` for `count`, `sum`, `mean`, `min`, and `max` can
  be included in v0.31 only after null behavior, output ordering, and type
  normalization are covered by parity tests.

- Keep small and browser-style workloads from regressing noticeably.

  Status: Planned.

  Performance work should not be considered successful only because one large
  benchmark improves. The native path should be beneficial for medium and large
  path-backed inputs without adding meaningful overhead to small row-runtime
  workloads.

## Non-Goals

- Enabling Polars in the default browser/WASM runtime.
- Removing the existing `Table`, `Row`, or `Value` API.
- Changing source-language syntax or user-visible dataframe semantics as part
  of the performance foundation.
- Requiring every PDL stage to be Polars-backed before the first native speedup
  lands.
- Building a distributed execution engine or general plugin API.
- Making Algraf incrementally render unbounded raw streams as part of this PDL
  release.
- Solving Algraf-side reader buffering, column-view stat paths, mark-budget
  diagnostics, or aggregate-first rendering UX inside the PDL repository.

## Validation

Required repository checks before this plan can be marked implemented:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets
cargo test --workspace
```

WASM validation when shared APIs change:

```bash
cargo check -p pdl-wasm --target wasm32-unknown-unknown
```

Focused validation should cover:

- Row-backed `DataPlan` behavior for existing runtime operations.
- Native backend selection for supported and unsupported pipelines.
- Path-backed native scans for CSV, Parquet, and Arrow formats where supported
  by the feature set.
- Whole-pipeline fallback for unsupported stages and expressions.
- Writer-sink stdout behavior for Arrow IPC stream output.
- Stderr-only diagnostics when stdout is used as a data stream.
- Boundary checks proving Polars remains private to `pdl-data`.
- PDL-to-Algraf smoke testing aligned with
  [`V0_68_PLAN.md`](../../algraf/docs/V0_68_PLAN.md) when a local Algraf binary
  is available.
- Benchmarks at small, medium, and large data sizes:
  1,000 rows, 100,000 rows, and 1,000,000 or more rows.
- `scripts/run-large-demos.sh` exercises checked-in large `.pdl` demo programs
  against ignored generated fixtures and writes report rows with status,
  output path, row count where available, byte count, elapsed milliseconds, run
  timestamp, and git ref.

## Deferred

- Mid-pipeline fallback that materializes a native `DataPlan` into `Table` at
  the first unsupported stage and continues through the row engine.
- Byte-backed Polars readers for stdin, in-memory tests, and browser-hosted
  files.
- Native coverage for `mutate` beyond simple expressions, `union`, and simple
  inner/left joins.
- Native coverage for full, right, semi, and anti joins.
- Native coverage for window expressions, `pivot_longer`, `complete`, and
  functions with PDL-specific stringification or null behavior.
- Direct native writes for every output format beyond the initial Arrow stream
  and simplest supported sinks.
- Algraf streaming stats, scale training, GPU/raster large-mark paths, or other
  cross-repo rendering changes.

## Open Questions

- Should native CSV scans use Polars type inference, or a PDL-compatible
  string-first strategy that casts during expressions?
- Should `RunResult` grow a mode that omits materialized named-output tables for
  native CLI execution?
- Should backend selection be user-visible through a stable CLI flag, or only
  exposed through tests and debug diagnostics?
- How should parity tests force a specific backend without exposing backend
  internals in public APIs?
- Which output-order differences, if any, are acceptable enough to specify?
- Which Arrow logical types should PDL commit to emitting for native Polars
  columns that are richer than current `Value` types?
