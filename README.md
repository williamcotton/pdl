# PDL

PDL is a Unix-pipeline-style tabular data transformation DSL.

The current `0.11.0` implementation supports a CSV-backed first slice with
registered lettered diagnostics, load-free driver data plans, phase-tagged
preparation reports, semantic-IR execution planning, schema-aware editor/LSP/WASM
diagnostics, recoverable syntax diagnostics for malformed filter/sort/aggregate
stages and missing stage pipes, a minimal React/Vite/Monaco browser demo, and
WASM in-memory CSV execution. It includes row-preserving data manipulation with
`mutate`, `distinct`, and scalar cleanup functions:

```bash
cargo run -p pdl-cli -- run examples/top_regions.pdl --stdout-format csv
```

That command loads `examples/sales.csv` and streams the result as CSV.

Data cleaning examples are also available:

```bash
cargo run -p pdl-cli -- run examples/orders_cleaned.pdl --stdout-format csv
cargo run -p pdl-cli -- run examples/order_region_summary.pdl --stdout-format csv
```

Use `check` while editing:

```bash
cargo run -p pdl-cli -- check examples/top_regions.pdl
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
