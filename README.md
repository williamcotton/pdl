# PDL

PDL is a Unix-pipeline-style tabular data transformation DSL.

The current `0.17.0` implementation supports a native tabular-format slice with
registered lettered diagnostics, load-free driver data plans, phase-tagged
preparation reports, semantic-IR execution planning, schema-aware editor/LSP/WASM
diagnostics, recoverable syntax diagnostics for malformed filter/sort/aggregate
stages and missing stage pipes, a minimal React/Vite/Monaco browser demo, and
WASM in-memory CSV execution. Native CLI execution supports CSV, JSON Lines,
Parquet, Arrow IPC file, and Arrow IPC stream loading/saving, stdin sniffing,
and deterministic stdout interop. It includes row-preserving data manipulation
with `mutate`, `distinct`, scalar cleanup functions, window expressions,
multi-input `join`/`union`, and native CLI inspection through `fmt`, `schema`,
`plan`, `ast`, `ir`, and `manifest`:

```bash
cargo run -p pdl-cli -- run examples/top_regions.pdl --stdout-format csv
```

That command loads `examples/sales.csv` and streams the result as CSV.

Data cleaning examples are also available:

```bash
cargo run -p pdl-cli -- run examples/orders_cleaned.pdl --stdout-format csv
cargo run -p pdl-cli -- run examples/order_region_summary.pdl --stdout-format csv
```

Multi-input examples use named bindings:

```bash
cargo run -p pdl-cli -- run examples/segment_revenue.pdl --stdout-format csv
cargo run -p pdl-cli -- run examples/daily_orders_union.pdl --stdout-format csv
```

Window examples compute row-preserving analytics:

```bash
cargo run -p pdl-cli -- run examples/customer_window_metrics.pdl --stdout-format csv
```

Stream interop examples cover stdin and Arrow IPC stream output:

```bash
printf 'order_id,region,amount,status\nA1,North,10,completed\n' \
  | cargo run -p pdl-cli -- run examples/stdin_orders_csv.pdl --stdin-format csv --stdout-format csv
cargo run -p pdl-cli -- run examples/stdout_arrow_stream.pdl --stdout-format arrow-stream > /tmp/sales.arrow
cargo run -p pdl-cli -- run examples/arrow_stream_passthrough.pdl --stdin-format arrow-stream < /tmp/sales.arrow > /tmp/sales.sorted.arrow
```

Native file-format examples cover JSON Lines text and binary Arrow/Parquet
stdout:

```bash
cargo run -p pdl-cli -- run examples/jsonl_orders.pdl --stdout-format csv
cargo run -p pdl-cli -- run examples/stdout_jsonl.pdl --stdout-format jsonl
cargo run -p pdl-cli -- run examples/stdout_arrow_file.pdl --stdout-format arrow-file > /tmp/sales.arrow
cargo run -p pdl-cli -- run examples/stdout_parquet.pdl --stdout-format parquet > /tmp/sales.parquet
```

Use `check` while editing:

```bash
cargo run -p pdl-cli -- check examples/top_regions.pdl
```

Inspect and format programs without executing output artifacts:

```bash
cargo run -p pdl-cli -- fmt --check examples/top_regions.pdl
cargo run -p pdl-cli -- schema examples/top_regions.pdl
cargo run -p pdl-cli -- plan examples/top_regions.pdl --stdout-format csv
cargo run -p pdl-cli -- manifest examples/stdout_arrow_stream.pdl --stdout-format arrow-stream
```

Editor support is available through the Rust language server and thin VS Code
client:

```bash
cargo run -p pdl-cli -- lsp
cd editors/vscode
npm install
npm run package
```

Try the browser demo from `demo/`:

```bash
npm install
npm run dev
```
