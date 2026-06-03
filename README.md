# PDL

PDL is a Unix-pipeline-style tabular data transformation DSL.

The current `0.5.0` implementation supports a CSV-backed first slice with
registered lettered diagnostics, load-free driver data plans, phase-tagged
preparation reports, semantic-IR execution planning, and the v0.5 architecture
boundary checks:

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
