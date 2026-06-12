<p>
  <img src="demo/public/site-brand-mark.svg" alt="PDL site brand mark" width="76" height="76">
</p>

# PDL

[![Test Suite](https://github.com/williamcotton/pdl/workflows/CI/badge.svg)](https://github.com/williamcotton/pdl/actions/workflows/ci.yml)

[Download VS Code VSIX](https://github.com/williamcotton/pdl/releases/latest/download/pdl-vscode-latest.vsix) | [Download browser WASM](https://github.com/williamcotton/pdl/releases/latest/download/pdl-wasm-latest.wasm)

PDL is a Unix-pipeline-style tabular data transformation DSL. You describe how
tables are loaded, cleaned, reshaped, joined, aggregated, ranked, and saved in a
`.pdl` file. The `pdl` binary parses the source, validates it against the data,
and emits deterministic files or stdout streams for downstream tools such as
Algraf.

Runnable examples live in [`examples/`](examples/).

Live site: [`https://williamcotton.github.io/pdl/`](https://williamcotton.github.io/pdl/)
Full demos: [`https://williamcotton.github.io/pdl/demos`](https://williamcotton.github.io/pdl/demos)

## A tour in eight pipelines

Each example below is a runnable file under [`examples/`](examples/). The tour
starts with one table and one aggregation, then adds cleanup, temporal
bucketing, joins, unions, window analytics, and typed stream handoff.

## 1. Top regions: filter, aggregate, sort

`load` creates the table source, `filter` keeps completed rows, `group_by` plus
`agg` collapses each region, and `sort`/`limit` make the output deterministic.

```pdl
load "sales.csv"
  | filter status == "completed"
  | group_by region
  | agg
      total_revenue = sum(amount),
      avg_age = mean(customer_age),
      orders = count()
  | sort total_revenue desc
  | limit 3
```

```bash
pdl run examples/top_regions.pdl --stdout-format csv
```

## 2. Clean orders: mutate and normalize fields

Expressions run inside stages. This pipeline trims text, normalizes case,
computes net revenue, tags priority orders, removes duplicate order IDs, and
selects the final shape.

```pdl
load "orders_raw.csv"
  | filter lower(trim(status)) == "completed"
  | mutate
      net_amount = gross_amount - coalesce(discount, 0),
      region_channel = concat(upper(trim(region)), ":", lower(trim(channel))),
      priority = if_else(gross_amount >= 150, "high", "standard")
  | distinct order_id
  | select order_id, region_channel, net_amount, priority
  | sort order_id
```

## 3. Monthly order totals: temporal bucketing

Temporal scalar functions parse ISO dates and RFC3339 timestamps (`Z` and
fixed offsets) deterministically. `date_format` builds stable bucket keys —
monthly `%Y-%m`, ISO-weekly `%G-W%V`, hourly `%H` — `date`/`year`/`month`/`day`
extract calendar fields, and `date_floor` snaps a value to the start of a day,
month, or year. Unparseable values become null.

```pdl
load "order_events.csv"
  | mutate
      order_month = date_format(ordered_at, "%Y-%m"),
      order_year = year(ordered_at),
      order_day = date(ordered_at)
  | group_by region, order_month
  | agg orders = count(), revenue = sum(amount)
  | sort order_month, region
```

```bash
pdl run examples/monthly_order_totals.pdl --stdout-format csv
```

## 4. Segment revenue: join a lookup table

Named `let` bindings keep lookup tables explicit. `join customers on
customer_id kind left` adds customer segments before the revenue summary.

```pdl
let customers =
  load "customers.csv"
  | select customer_id, segment

load "sales.csv"
  | filter status == "completed"
  | join customers on customer_id kind left
  | group_by segment
  | agg revenue = sum(amount), orders = count()
  | sort revenue desc
```

## 5. Daily orders: union files by name

`union ... by_name true distinct true` combines same-shaped daily extracts while
deduplicating rows and preserving deterministic output order.

```pdl
let day2 =
  load "daily_orders_2026_02_02.csv"

load "daily_orders_2026_02_01.csv"
  | union day2 by_name true distinct true
  | sort order_id
```

## 6. Customer windows: row-preserving analytics

Window expressions add ranks, row numbers, and totals without collapsing the
table. Use explicit `partition_by` and `order_by` clauses when the analytic
depends on row groups or ordering.

```pdl
load "sales.csv"
  | filter status == "completed"
  | mutate
      customer_sale_number =
        row_number() over (
          partition_by customer_id
          order_by amount desc
        ),
      customer_revenue =
        sum(amount) over (
          partition_by customer_id
        )
```

Aggregate windows take an optional named frame: `frame whole_partition`,
`frame running`, `frame remaining`, `frame trailing N`, `frame leading N`, or
`frame centered N`. See
[`examples/window_frame_named.pdl`](examples/window_frame_named.pdl) for the
native-parity frames and
[`examples/window_frame_bounded.pdl`](examples/window_frame_bounded.pdl) for
bounded-frame analytics.

```pdl
load "sales.csv"
  | filter status == "completed"
  | mutate
      running_revenue =
        sum(amount) over (
          partition_by region
          order_by amount, customer_id
          frame running
        )
```

## 7. Reshape: pivot_longer and complete

`pivot_longer` reshapes wide value columns into name/value rows, and
`complete` inserts missing key combinations with explicit fills. See
[`examples/pivot_longer_basics.pdl`](examples/pivot_longer_basics.pdl) and
[`examples/complete_keys.pdl`](examples/complete_keys.pdl).

```pdl
load "monthly_sales.csv"
  | pivot_longer jan, feb, mar names_to month values_to amount
```

```pdl
load "daily_visits.csv"
  | complete region, day fill visits = 0
```

## 8. Arrow streams: hand off typed tables

PDL can read and write Arrow IPC streams on stdin/stdout. That makes it useful
as a preparation step before a renderer or another tabular tool.

```bash
pdl run examples/stdout_arrow_stream.pdl --stdout-format arrow-stream > /tmp/sales.arrow
pdl run examples/arrow_stream_passthrough.pdl --stdin-format arrow-stream < /tmp/sales.arrow > /tmp/sales.sorted.arrow
```

The typed-stream handoff composes with [Algraf](https://github.com/williamcotton/algraf)
without writing an intermediate file. The renderer accepts `--data -` and the
matching `--data-format arrow-stream`:

```bash
pdl run examples/stdout_arrow_stream.pdl --stdout-format arrow-stream \
  | algraf render chart.ag --data - --data-format arrow-stream --output chart.svg
```

## Install and run

Install the packaged binary with Homebrew:

```bash
brew tap williamcotton/pdl
brew install williamcotton/pdl/pdl
brew update && brew upgrade williamcotton/pdl/pdl
```

Then use `pdl` directly:

```bash
pdl run examples/top_regions.pdl --stdout-format csv
pdl check examples/top_regions.pdl
pdl fmt --check examples/top_regions.pdl
pdl schema examples/top_regions.pdl
pdl plan examples/top_regions.pdl --stdout-format csv
pdl manifest examples/stdout_arrow_stream.pdl --stdout-format arrow-stream
pdl controls examples/reactive_controls_dashboard.pdl --json
pdl serve examples/reactive_controls_dashboard.pdl
```

From a checkout, build the native binary:

```bash
cargo build -p pdl-cli
target/debug/pdl run examples/top_regions.pdl --stdout-format csv
```

Reactive host-driven workflows can declare parameters and state defaults, then
let a browser host override them through the runtime context map:

```bash
target/debug/pdl run examples/reactive_trip_dashboard.pdl
```

The v0.52 controls example exposes renderable parameter metadata and can be
served locally without a custom Studio page:

```bash
target/debug/pdl controls examples/reactive_controls_dashboard.pdl --json
target/debug/pdl serve examples/reactive_controls_dashboard.pdl
```

The v0.48 pipeline-shape examples exercise native binding starts, named
outputs, and non-terminal save fan-out:

```bash
target/debug/pdl run examples/binding_start.pdl --stdout-format csv
target/debug/pdl run examples/named_outputs.pdl
target/debug/pdl run examples/non_terminal_save.pdl --stdout-format csv
```

The v0.49 coverage example exercises dynamic column indirection, dynamic text
replacement, and mixed-class conditionals on the native engine:

```bash
target/debug/pdl run examples/dynamic_text_and_col.pdl --stdout-format csv
```

The v0.50 performance release keeps that native parity contract and exposes
bridge/materialization facts through plan JSON:

```bash
target/debug/pdl plan examples/dynamic_text_and_col.pdl --stdout-format csv --json
```

## File formats and output

PDL supports CSV, JSON Lines, Parquet, Arrow IPC file, and Arrow IPC stream
loading/saving. Native execution covers path-backed, stdin, and host-byte
CSV, JSON Lines, Parquet, Arrow IPC file, and Arrow IPC stream inputs, plus
every sink format. Stdout can emit CSV, JSON Lines, Parquet, Arrow IPC file, or
Arrow IPC stream when requested.

```bash
pdl run examples/stdout_jsonl.pdl --stdout-format jsonl
pdl run examples/stdout_arrow_file.pdl --stdout-format arrow-file > /tmp/sales.arrow
pdl run examples/stdout_parquet.pdl --stdout-format parquet > /tmp/sales.parquet
```

Human diagnostics and logs go to stderr so stdout stays a clean data stream.

## Native execution engine

`pdl run` defaults to `--engine auto`: it classifies the pipeline ahead of data
load and picks a Polars 0.53–backed native engine for every shipped language
feature on native hosts. The portable row runtime remains the parity reference,
the browser/WASM execution path, and the `--engine row` opt-in. Force a
specific engine with `--engine native`, `--engine native-strict`, `--engine row`,
or `--engine row-strict` when you want to pin behavior or make fallback a CI
failure.

```bash
pdl plan examples/top_regions.pdl --stdout-format csv
```

`pdl plan` reports the selected engine and any fallback reasons stage by stage,
so the cost of running natively (or not) is visible before you execute. Native
coverage includes every shipped source, stage, expression family, sink, and
pipeline shape. The only row-only rows in the coverage matrix are non-execution
host boundaries: WASM execution and LSP/editor services.

## Editor and browser support

The CLI includes an LSP server:

```bash
pdl lsp
```

The VS Code client is packaged as a GitHub Release asset. From a checkout:

```bash
cd editors/vscode
npm install
npm run package
```

The root-level [`demo/`](demo) app builds `crates/pdl-wasm` for the browser,
loads the generated `wasm/pdl.wasm` asset through the host's configured public
base path, and calls the shared parser, analyzer, formatter, editor services,
and executor over host-supplied in-memory files.

```bash
cd demo
npm install
npm run dev
```

The browser runtime returns `{ stdout, files, outputs, diagnostics, error }`.
Host apps own networking and file selection; the WASM runtime only sees the
in-memory file map.

PDL v0.27 also provides local package-shaped browser integrations:
`packages/wasm` (`pdl-wasm`) for runtime loading and ABI types, and
`editors/monaco` (`pdl-editor`) for Monaco/React editor wiring. During
development, hosts can install them with filesystem `file:` paths and pass a
local `wasmUrl` for a copied `public/wasm/pdl.wasm` artifact. See
[`docs/NPM_PACKAGES.md`](docs/NPM_PACKAGES.md).

## Workspace layout

Cargo workspace with ten crates under [`crates/`](crates/):

| Crate | Responsibility |
| --- | --- |
| `pdl-core` | Shared primitives: `Span`, `Diagnostic`, `Severity`, source IDs |
| `pdl-syntax` | Lexer, parser, AST/CST (rowan), parse diagnostics, formatter |
| `pdl-data` | Dataframe abstraction, logical schemas, CSV/Arrow/Parquet adapters |
| `pdl-semantics` | Name resolution, stage validation, type checking, IR |
| `pdl-driver` | Source/path resolution, format sniffing, schema loading, IO boundary |
| `pdl-exec` | Planning, streaming/blocking execution, writes, manifests, previews |
| `pdl-editor-services` | Shared editor features: completion, hover, tokens, navigation |
| `pdl-lsp` | tower-lsp backend, document cache, LSP transport |
| `pdl-cli` | The `pdl` binary: arg parsing, command dispatch, I/O |
| `pdl-wasm` | Browser/WASM runtime over in-memory files and editor-service ABI |
