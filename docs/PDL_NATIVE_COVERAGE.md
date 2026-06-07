# PDL Native Coverage Matrix

Status: v0.39 source of truth
Machine-readable matrix: [`PDL_NATIVE_COVERAGE.csv`](PDL_NATIVE_COVERAGE.csv)

This matrix records what the native execution strategy may claim in v0.39. The
portable row runtime remains the semantic reference. A matrix row may use only
one of these statuses:

- `native parity`
- `native partial`
- `row-only by design`
- `planned native`
- `unsupported`
- `deferred`

Native planning and tests include the CSV matrix so documentation and behavior
cannot silently drift. If a stage, expression family, source, sink, or host
boundary changes status, update the CSV and the tests in the same change.

## Stage Coverage

| Item | Status | Notes |
| --- | --- | --- |
| `load` | native partial | Path-backed CSV, Parquet, Arrow IPC file, and Arrow IPC stream are native. Stdin and byte-backed host Arrow IPC file/stream inputs are native; other stdin/byte formats are row-only. |
| `filter` | native parity | Supported scalar expressions lower to native predicates. |
| `select` | native parity | Row-preserving projection and aliasing. |
| `drop` | native parity | Row-preserving projection. |
| `rename` | native parity | Row-preserving aliasing. |
| `mutate` | native partial | Supported scalar expressions and supported row-preserving window expressions lower to native with parallel assignment semantics. |
| `group_by` | native partial | Native only when followed by supported aggregate coverage. |
| `agg` | native partial | `count`, `sum`, `mean`, `min`, `max`, and `count_distinct` over supported expressions. |
| `sort` | native parity | Blocking stage with deterministic sort options. |
| `limit` | native parity | Row-preserving limit. |
| `distinct` | native parity | Stable first-row distinct. |
| `join` | native partial | Native covers path-backed main inputs joined to native-safe binding inputs for `inner`/`left`/`right`/`full`/`semi`/`anti` single-key and composite-key equi-joins; non-equi joins stay row-only by design. |
| `union` | native partial | Native covers compatible-schema binding inputs by name or position with optional `distinct`; incompatible schemas, language-level null padding, and browser byte boundaries stay row-only. |
| `pivot_longer` | row-only by design | Row runtime preserves deterministic long output and mixed value behavior. |
| `complete` | row-only by design | Row runtime preserves key expansion and fill expression semantics. |
| `save` | native partial | Binary Parquet and Arrow sinks use native direct writers; CSV/JSON Lines use the row-format writer. |

## Expression Coverage

| Item | Status | Notes |
| --- | --- | --- |
| literals | native parity | String, numeric, boolean, and null literals lower natively. |
| column references | native parity | Static column references lower natively. |
| context references | native partial | Scalar contexts lower as literals; context column positions resolve before native planning. |
| dynamic `col` | native partial | String literal or string context only; data-dependent indirection is row-only. |
| arithmetic | native parity | `+`, `-`, `*`, `/`, and `%` over numeric values lower natively. |
| comparisons | native parity | `==`, `!=`, `<`, `<=`, `>`, and `>=` lower natively. |
| booleans | native parity | `and`, `or`, and `not` lower natively. |
| null checks | native parity | `is_null` and `not_null` lower natively. |
| string functions | native partial | `concat`, `lower`, `upper`, and `trim` lower natively; other string functions are row-only. |
| numeric functions | native partial | `abs` and `round` lower natively; uncertain coercions are row-only. |
| cast-style functions | native partial | `to_number` lowers natively with row-identical whitespace, parse-failure, and null behavior; other cast-style functions remain row-only. |
| conditional functions | native partial | `if_else` lowers natively for supported native condition and branch expressions; typed native branch output must remain compatible. |
| aggregate arguments | native partial | Supported scalar expressions lower for `count`, `sum`, `mean`, `min`, `max`, and `count_distinct`. |
| window ranking functions | native partial | `row_number`, `rank`, and `dense_rank` lower natively for row-preserving mutate windows; `rank` and `dense_rank` require exactly one order key and `row_number` supports zero or one order key. |
| window whole-partition aggregates | native partial | `count`, `sum`, `mean`, `min`, and `max` lower natively for whole-partition row-preserving mutate windows over supported native expressions with zero or one order key. |
| window running aggregates | native partial | `count`, `sum`, `mean`, `min`, and `max` lower natively for `rows between unbounded_preceding and current_row` over supported native expressions with zero or one order key. |
| window offset functions | native partial | `lag` and `lead` lower natively with one order key, a non-negative integer literal offset, and an omitted or `null` default; non-null defaults remain row-only. |
| window value functions | native partial | `first_value` and `last_value` lower natively for whole-partition and unbounded-preceding-to-current-row frames over supported native expressions with zero or one order key. |
| window distribution functions | native partial | `percent_rank` and `cume_dist` lower natively with exactly one order key. |
| window multi-key ordering | row-only by design | Multi-key window order stays on the row runtime until per-key direction, null placement, and tie behavior are proven identical in native execution. |

## Source Coverage

| Item | Status | Notes |
| --- | --- | --- |
| path-backed CSV | native parity | Polars lazy CSV scan is eligible. |
| path-backed Parquet | native parity | Polars lazy Parquet scan is eligible. |
| path-backed Arrow IPC file | native partial | Arrow IPC file is read into the native dataframe path, then the lazy pipeline continues. |
| path-backed Arrow IPC stream | native partial | Stream file is read into a native dataframe, then lazy pipeline continues. |
| JSON Lines | row-only by design | Schema inference and text semantics stay on the row runtime. |
| stdin | native partial | Arrow IPC file/stream stdin bytes are native; CSV, JSON Lines, Parquet, and unknown stdin bytes stay row-only. |
| byte-backed host files | native partial | Arrow IPC file/stream host bytes are native when no real filesystem path is available; other host byte formats stay row-only. |
| named bindings | native partial | Binding-backed inputs are native for supported join/union right sides; binding starts and named outputs remain row-only. |

## Sink Coverage

| Item | Status | Notes |
| --- | --- | --- |
| path | native partial | Binary formats direct; CSV/JSON Lines row-format fallback. |
| stdout | native partial | Binary formats byte-clean; CSV/JSON Lines row-format fallback. |
| bytes | native partial | Binary formats direct; CSV/JSON Lines row-format fallback. |
| CSV | row-only by design | Text formatting remains PDL-visible. |
| JSON Lines | row-only by design | Text formatting remains PDL-visible. |
| Parquet | native parity | Native direct writer. |
| Arrow IPC file | native parity | Native direct writer. |
| Arrow IPC stream | native parity | Native direct writer. |

## Host Boundary Coverage

| Item | Status | Notes |
| --- | --- | --- |
| WASM | row-only by design | Polars, Parquet, object-store, and native filesystem assumptions are excluded from the wasm target graph; Arrow IPC byte support remains behind browser-safe row/WASM APIs. |
| LSP/editor | row-only by design | Language services expose no native dataframe internals. |
| PDL-to-Algraf Arrow IPC | native partial | PDL emits Arrow IPC stream; Algraf consumes it across the process boundary. |
