# PDL Native Coverage Matrix

Status: v0.36 source of truth
Machine-readable matrix: [`PDL_NATIVE_COVERAGE.csv`](PDL_NATIVE_COVERAGE.csv)

This matrix records what the native execution strategy may claim in v0.36. The
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
| `load` | native partial | Path-backed CSV, Parquet, and Arrow IPC stream are native; stdin and byte-backed host files are row-only. |
| `filter` | native parity | Supported scalar expressions lower to native predicates. |
| `select` | native parity | Row-preserving projection and aliasing. |
| `drop` | native parity | Row-preserving projection. |
| `rename` | native parity | Row-preserving aliasing. |
| `mutate` | native partial | Supported scalar expressions lower to native with parallel assignment semantics. |
| `group_by` | native partial | Native only when followed by supported aggregate coverage. |
| `agg` | native partial | `count`, `sum`, `mean`, `min`, `max`, and `count_distinct` over supported expressions. |
| `sort` | native parity | Blocking stage with deterministic sort options. |
| `limit` | native parity | Row-preserving limit. |
| `distinct` | native parity | Stable first-row distinct. |
| `join` | planned native | Row runtime is reference; native join parity deferred for duplicate columns, null keys, and composite keys. |
| `union` | planned native | Row runtime is reference; native union parity deferred for schema, type, and null-padding rules. |
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
| cast-style functions | row-only by design | `to_number` remains row-only until parse/null/format parity is promoted. |
| conditional functions | row-only by design | `if_else` remains row-only until branch/null semantics are promoted. |
| aggregate arguments | native partial | Supported scalar expressions lower for `count`, `sum`, `mean`, `min`, `max`, and `count_distinct`. |
| window expressions | row-only by design | Window parity is deferred. |

## Source Coverage

| Item | Status | Notes |
| --- | --- | --- |
| path-backed CSV | native parity | Polars lazy CSV scan is eligible. |
| path-backed Parquet | native parity | Polars lazy Parquet scan is eligible. |
| path-backed Arrow IPC file | unsupported | Arrow IPC file scan is row-only until native file reader parity is added. |
| path-backed Arrow IPC stream | native partial | Stream file is read into a native dataframe, then lazy pipeline continues. |
| JSON Lines | row-only by design | Schema inference and text semantics stay on the row runtime. |
| stdin | row-only by design | Keeps stdout purity and bounded buffering policy simple. |
| byte-backed host files | row-only by design | Browser and host byte boundaries stay native-free. |
| named bindings | row-only by design | Native starts from a single path-backed main load only. |

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
| WASM | row-only by design | Polars, Arrow native readers, Parquet, object-store, and native filesystem assumptions are excluded. |
| LSP/editor | row-only by design | Language services expose no native dataframe internals. |
| PDL-to-Algraf Arrow IPC | native partial | PDL emits Arrow IPC stream; Algraf consumes it across the process boundary. |
