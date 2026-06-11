# PDL Native Coverage Matrix

Status: v0.49.0 source of truth
Machine-readable matrix: [`PDL_NATIVE_COVERAGE.csv`](PDL_NATIVE_COVERAGE.csv)

This matrix records what the native execution strategy may claim in v0.49. The
portable row runtime remains the semantic reference. A matrix row may use only
one of these statuses:

- `native parity`
- `row-only by design`

Native planning and tests include the CSV matrix so documentation and behavior
cannot silently drift. After v0.49, every shipped language feature is
native-eligible; the only row-only rows are non-execution host boundaries.

## Stage Coverage

| Item | Status | Notes |
| --- | --- | --- |
| `load` | native parity | Path-backed, stdin, and byte-backed host CSV, JSON Lines, Parquet, Arrow IPC file, and Arrow IPC stream inputs are native-eligible. |
| `filter` | native parity | Scalar expressions lower or materialize at native orchestration boundaries with row-identical predicate semantics. |
| `select` | native parity | Row-preserving projection and aliasing. |
| `drop` | native parity | Row-preserving projection. |
| `rename` | native parity | Row-preserving aliasing. |
| `mutate` | native parity | All scalar and row-preserving window expressions are native-eligible with row-identical assignment semantics. |
| `group_by` | native parity | Group keys and following aggregate coverage are native-eligible for all shipped expression families. |
| `agg` | native parity | `count`, `sum`, `mean`, `min`, `max`, and `count_distinct` over shipped expression families are native-eligible. |
| `sort` | native parity | Blocking stage with deterministic sort options. |
| `limit` | native parity | Row-preserving limit. |
| `distinct` | native parity | Stable first-row distinct. |
| `join` | native parity | Native covers path-backed main inputs joined to native-safe binding inputs for `inner`, `left`, `right`, `full`, `semi`, and `anti` single-key and composite-key equi-joins; no non-equi join syntax is shipped. |
| `union` | native parity | Binding inputs union by name or position with null padding and optional `distinct` using row-identical column order and cell classes. |
| `pivot_longer` | native parity | Homogeneous and mixed-class value column sets preserve row-identical interleaved output order and per-cell value classes. |
| `complete` | native parity | First-appearance key expansion and all fill expression classes preserve row-identical order and values. |
| `save` | native parity | Terminal and non-terminal saves use native direct writers or row-compatible text encoders for every format; non-terminal saves cache/fan out in row-runtime write order. |

## Expression Coverage

| Item | Status | Notes |
| --- | --- | --- |
| literals | native parity | String, numeric, boolean, and null literals lower natively. |
| column references | native parity | Static column references lower natively. |
| context references | native parity | Scalar contexts lower as literals; context column positions resolve before native planning. |
| dynamic `col` | native parity | String literals, context strings, and data-dependent column-name expressions are native-eligible with per-row lookup semantics. |
| arithmetic | native parity | `+`, `-`, `*`, `/`, and `%` over numeric values are native-eligible. |
| comparisons | native parity | `==`, `!=`, `<`, `<=`, `>`, and `>=` are native-eligible. |
| booleans | native parity | `and`, `or`, and `not` are native-eligible. |
| null checks | native parity | `is_null` and `not_null` are native-eligible. |
| string functions | native parity | `concat`, `lower`, `upper`, `trim`, `contains`, `starts_with`, and `replace` with literal or expression-valued patterns are native-eligible. |
| numeric functions | native parity | `abs`, `round`, and the contracted `to_number`, `to_string`, and `to_boolean` coercion subset are native-eligible with row-identical null, parse, and formatting behavior. |
| cast-style functions | native parity | `to_number`, `to_string`, and `to_boolean` are native-eligible for supported language arguments. |
| temporal functions | native parity | `date`, `datetime`, `year`, `month`, `day`, `date_floor`, and `date_format` are native-eligible with row-identical parsing, flooring, formatting, and null behavior. |
| conditional functions | native parity | `if_else` is native-eligible for compatible and mixed-class branch outputs with row-identical selected-branch semantics. |
| aggregate arguments | native parity | `count`, `sum`, `mean`, `min`, `max`, and `count_distinct` walk their argument expressions and are native-eligible for every shipped expression family. |
| window ranking functions | native parity | `row_number`, `rank`, and `dense_rank` are native-eligible for row-preserving mutate windows. |
| window whole-partition aggregates | native parity | `count`, `sum`, `mean`, `min`, and `max` are native-eligible for whole-partition row-preserving mutate windows. |
| window running aggregates | native parity | `count`, `sum`, `mean`, `min`, and `max` are native-eligible for `frame running` row-preserving mutate windows. |
| window offset functions | native parity | `lag` and `lead` are native-eligible with literal or expression offsets and omitted, null, or expression defaults. |
| window value functions | native parity | `first_value` and `last_value` are native-eligible for all named frames. |
| window distribution functions | native parity | `percent_rank` and `cume_dist` are native-eligible. |
| window multi-key ordering | native parity | A mutate stage or single assignment may contain multiple distinct composite order groups and remains native-eligible. |
| window whole-partition frame | native parity | `frame whole_partition` desugars to the whole-partition bound pair. |
| window running frame | native parity | `frame running` desugars to the unbounded-preceding-to-current-row bound pair. |
| window bounded frames | native parity | `frame remaining`, `frame trailing N`, `frame leading N`, and `frame centered N` are native-eligible with row-identical edge truncation, tie handling, all-null order-key behavior, and `N = 0` handling. |

## Source Coverage

| Item | Status | Notes |
| --- | --- | --- |
| path-backed CSV | native parity | Polars lazy CSV scan is eligible. |
| path-backed Parquet | native parity | Polars lazy Parquet scan is eligible. |
| path-backed Arrow IPC file | native parity | Polars lazy IPC scan is eligible. |
| path-backed Arrow IPC stream | native parity | Stream file is read into a native dataframe, then the lazy pipeline continues; output bytes match the row engine. |
| JSON Lines | native parity | Path, stdin, and host-byte JSON Lines inputs are native-eligible using row-identical schema and text semantics. |
| stdin | native parity | CSV, JSON Lines, Parquet, and Arrow IPC file/stream stdin bytes scan through native orchestration. |
| byte-backed host files | native parity | CSV, JSON Lines, Parquet, and Arrow IPC file/stream host bytes scan through native orchestration when no real filesystem path is available. |
| named bindings | native parity | Binding-backed inputs are native-eligible for join/union right sides and binding-start pipelines when referenced bindings are valid. |

## Pipeline Shape Coverage

| Item | Status | Notes |
| --- | --- | --- |
| binding-start pipelines | native parity | Binding-start pipelines are native-eligible when the referenced binding is valid and acyclic. |
| named-output programs | native parity | Named-output programs are native-eligible when every output pipeline and referenced binding is valid. |
| non-terminal save | native parity | Uses native frame cache/fan-out and writes in stage order before later stages continue. |

## Sink Coverage

| Item | Status | Notes |
| --- | --- | --- |
| path | native parity | Every format uses the native direct writer or row-compatible text encoder. |
| stdout | native parity | Every format is byte-clean through the native writer path. |
| bytes | native parity | Every format uses the native writer path. |
| CSV | native parity | Native direct writer streams rows through the row writer's cell encoder; bytes match the row writer. |
| JSON Lines | native parity | Native direct writer streams rows through the row writer's record encoder; bytes match the row writer. |
| Parquet | native parity | Native direct writer. |
| Arrow IPC file | native parity | Native direct writer. |
| Arrow IPC stream | native parity | Native direct writer. |

## Host Boundary Coverage

| Item | Status | Notes |
| --- | --- | --- |
| WASM | row-only by design | `wasm-target-graph`; Polars, Parquet, object-store, and native filesystem assumptions are excluded from the wasm target graph. |
| LSP/editor | row-only by design | `editor-service`; language services expose no native dataframe internals. |
| PDL-to-Algraf Arrow IPC | native parity | PDL emits Arrow IPC stream and Algraf consumes it across the process boundary. |
