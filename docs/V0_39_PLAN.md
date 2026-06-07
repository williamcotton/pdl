# PDL v0.39 Plan

Status: Complete
Target version: 0.39.0
Owner: PDL maintainers
Related spec: [`PDL_SPEC.md`](PDL_SPEC.md)
Predecessor plan: [`V0_38_PLAN.md`](V0_38_PLAN.md)
Successor plan: [`V0_40_PLAN.md`](V0_40_PLAN.md)
Neighboring Algraf plan: TBD

## Purpose

PDL v0.39 is the native windowing and composite-join release after the v0.38
native-parity release. The portable row runtime remains the semantic reference.
Polars, Arrow, Parquet, native filesystem details, and optimizer internals stay
private behind `pdl-data` and execution planning. Browser/WASM builds remain
native-free.

The release deliberately separates language support from native acceleration:
PDL supports the full row/WASM window function surface, while native execution
promotes only the subsets whose ordering, null handling, frames, and output
types match rows before native scans open.

## Release Results

- Advanced window functions now have native coverage for the high-value exact
  subset: `percent_rank`, `cume_dist`, `lag`, `lead`, `first_value`,
  `last_value`, whole-partition aggregate windows, and
  `rows between unbounded_preceding and current_row` aggregate windows.
- Native composite-key equi-joins now cover `inner`, `left`, `right`, `full`,
  `semi`, and `anti` at the existing path-backed main input plus native-safe
  binding input boundary.
- Existing CLI commands, public crate boundaries, WASM APIs, and old single-key
  join JSON remain compatible. Composite join syntax adds fields only for new
  syntax.
- Text formatting, JSON Lines input/output, incompatible union schema extension,
  `pivot_longer`, `complete`, binding starts, named outputs, non-equi joins,
  multi-key window ordering, and non-null native `lag`/`lead` defaults remain
  row-runtime surfaces by design or are deferred to v0.40.

## Carry-Forward Rules

- A promoted feature must update `docs/PDL_SPEC.md`,
  `docs/PDL_NATIVE_COVERAGE.csv`, `docs/PDL_NATIVE_COVERAGE.md`, eligibility
  checks, row/native parity tests, examples where useful, and benchmark
  workloads in the same change.
- A deferred feature must keep a stable `auto` fallback reason and a forced
  native diagnostic before native scans open.
- Native text writers ship only with byte-for-byte row-output parity.
- Any new public language policy for null padding, type coercion, CSV dialects,
  frames, ordering, or browser bytes must be specified before implementation.
- Version stamps and npm package pins follow the repository rules. Rust/CLI,
  spec, demo, VS Code, `pdl-wasm`, and `pdl-editor` release stamps move to
  `0.39.0`. `npm view` confirmed no existing `0.39.0` browser packages, so this
  change prepares the new v0.39 browser package publication.

## Must

- Close the advanced native window backlog.

  Status: Complete.

  Native execution now lowers the exact v0.39 window subset:
  `row_number`, `rank`, `dense_rank`, `percent_rank`, `cume_dist`, `lag`,
  `lead`, `first_value`, `last_value`, and `count`/`sum`/`mean`/`min`/`max`
  windows for whole partitions and `rows between unbounded_preceding and
  current_row`. Multiple window assignments in one `mutate` remain parallel.

  The native subset requires supported native argument expressions and at most
  one order key. Ranking, distribution, and offset functions that require order
  must have exactly one order key. `lag` and `lead` support omitted or `null`
  defaults natively; arbitrary non-null defaults remain on rows because PDL row
  values may be mixed while Polars columns must keep a stable dtype.

  Multi-key window ordering stays row-only until per-key direction, null
  placement, and tie behavior are proven identical to `sort`. Other bounded
  frames remain row-only. Parity tests cover CSV, Parquet, Arrow IPC file, Arrow
  IPC stream, chained window mutates, and WASM in-memory CSV execution.
  Benchmarks now include `windowed_sales_rank`,
  `million_row_window_running`, and `million_row_window_offsets_values`.

- Promote composite-key equi-join coverage and decide non-equi joins.

  Status: Complete.

  Parser, AST, formatter, semantic IR, analyzer, editor services, row runtime,
  and native runtime now support comma-separated join keys:
  `on customer_id, order_date` and
  `on (sku, product_sku), (region, market)`.

  Native composite-key equi-joins cover `inner`, `left`, `right`, `full`,
  `semi`, and `anti`. Row/native parity covers null-key non-matches,
  duplicate right-column suffixing, coalesced key output, unmatched rows,
  deterministic output order, and binding-backed right inputs. Benchmarks now
  include a small-rollup composite join and a large lookup composite join.

  Non-equi joins remain row-only by design for v0.39. They need a dedicated
  syntax and semantics plan before any backend behavior is exposed.

- Define the union schema-extension policy before promoting more native union
  behavior.

  Status: Complete decision, no v0.39 behavior change.

  PDL v0.39 keeps union compatible-only. `by_name` still requires the same
  column-name set, and position-aligned union still requires the same column
  count and compatible positions. Missing-column null padding and implicit type
  widening are deferred to v0.40 until the language specifies column order,
  mixed-value behavior, and diagnostics.

- Revisit JSON Lines input and terminal text writer strategy.

  Status: Complete decision, no native text promotion.

  CSV and JSON Lines output bytes remain PDL-visible and stay on the row-format
  writer. JSON Lines input stays row-only because deterministic inference,
  mixed values, missing keys, object-only rows, diagnostics, and ordering are
  language semantics rather than backend conveniences. Native binary Parquet
  and Arrow IPC sinks remain direct.

- Revisit native `pivot_longer` and `complete` feasibility.

  Status: Complete decision, row-only by design.

  `pivot_longer` keeps row-runtime behavior for deterministic long output and
  mixed values. `complete` keeps row-runtime behavior for observed key order,
  fill expressions, duplicate-key diagnostics, and bounded key expansion.

- Decide the native planning policy for binding starts, named outputs, and
  multi-output execution.

  Status: Complete decision, no segmented execution.

  Native execution remains a whole-pipeline strategy. Binding-backed inputs may
  be executed natively when used as supported join or union right sides. A
  pipeline that starts from a binding, materializes named outputs, or executes
  multiple outputs remains row-runtime work until a segmented planning design
  covers observability, diagnostics, cache boundaries, memory behavior, stdout
  purity, and parity tests.

## Should

- Decide whether `--engine auto` needs a small-data policy.

  Status: Complete decision, no policy change.

  `auto` continues to choose native whenever the whole pipeline is natively
  eligible, regardless of small input size. Adding a data-size threshold would
  create a second semantic branch for little gain and would complicate
  deterministic engine observability.

- Keep native coverage, observability, and benchmark reports synchronized.

  Status: Complete.

  The coverage CSV and Markdown now distinguish ranking, whole-partition
  aggregate, running aggregate, offset, value, distribution, and multi-key
  window ordering. The benchmark suite now includes v0.39 window and composite
  join workloads while preserving plan/manifest observability fields.

- Refresh routine PDL-to-Algraf Arrow IPC smoke coverage.

  Status: Complete.

  `bench/workloads/large/pdl_to_algraf_arrow_handoff.pdl` now exercises filter,
  mutate, `dense_rank` over `partition_by`, projection, deterministic ordering,
  limit, and Arrow IPC stream stdout before Algraf consumes the stream.

- Preserve native-free WASM, editor, and small-data responsiveness.

  Status: Complete.

  `pdl-wasm` remains default-features=false through `pdl-data`, `pdl-driver`,
  `pdl-exec`, and editor-service crates. WASM tests cover in-memory CSV
  execution for advanced windows. Parser, semantics, editor services, LSP, CLI
  JSON, and WASM ABI expose no Polars types.

- Evaluate high-leverage scalar and type-system expression gaps.

  Status: Deferred to v0.40.

  The v0.39 implementation focuses on window functions and composite joins.
  Additional scalar/type promotions should use measured fallback cost and
  explicit row/native null, type, parse, and formatting parity.

## Could

- Evaluate configurable CSV dialect support where it affects native input or
  text sink parity.

  Status: Deferred to v0.40.

- Evaluate a byte-oriented browser IO ABI.

  Status: Deferred to v0.40.

  Browser Arrow IPC output and binary host-file contents need a browser-specific
  byte ABI before changing package or WASM contracts.

- Evaluate object-store and remote path support.

  Status: Deferred to v0.40.

  Object stores carry credential, security, reproducibility, and browser-boundary
  risks and need a dedicated security and IO plan.

## Validation Notes

Implementation validation for v0.39 includes:

- row/auto/native parity tests for advanced windows and composite-key joins;
- WASM in-memory CSV execution for advanced windows;
- coverage matrix updates and native eligibility checks;
- benchmark workloads for promoted windows, composite joins, and PDL-to-Algraf
  Arrow handoff;
- wasm target dependency checks to keep native dependencies out of `pdl-wasm`;
- repository-required Rust checks.

Full release validation should run from the PDL root:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets
cargo test --workspace
cargo check -p pdl-wasm --target wasm32-unknown-unknown
```

## Non-Goals

- Do not expose Polars expressions, lazy plans, Arrow arrays, Parquet metadata,
  optimizer hints, or physical plans as PDL syntax or public API.
- Do not make browser/WASM execution native.
- Do not change CSV or JSON Lines output bytes to gain native speed.
- Do not add hidden mid-pipeline native-to-row fallback.
- Do not promote broad implicit type coercion for union, joins, or expressions
  without normative spec rules and diagnostics.
- Do not add distributed execution, service runtime, object-store credentials,
  or remote path support as part of this release.
