# PDL

PDL is a Unix-pipeline-style tabular data transformation DSL.

The current `0.4.0` implementation supports a CSV-backed first slice with
registered lettered diagnostics and the v0.4 architecture split:

```bash
cargo run -p pdl-cli -- run examples/top_regions.pdl
```

That command loads `examples/sales.csv` and writes
`examples/top_regions.csv`.

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
