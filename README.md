# PDL

PDL is a Unix-pipeline-style tabular data transformation DSL.

The current `0.1.0-alpha.1` implementation supports a CSV-backed first slice:

```bash
cargo run -p pdl-cli -- run examples/top_regions.pdl
```

That command loads `examples/sales.csv` and writes
`examples/top_regions.csv`.

Use `check` while editing:

```bash
cargo run -p pdl-cli -- check examples/top_regions.pdl
```
