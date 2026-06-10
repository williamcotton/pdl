# PDL Benchmarks

`pdl-bench` owns the benchmark lifecycle for this repo.

```bash
cargo run -p pdl-bench -- generate --tier smoke
cargo run -p pdl-bench -- download --dataset all
cargo run -p pdl-bench -- prepare --tier smoke
cargo run -p pdl-bench -- run --suite large --tier smoke --run-label before-v0.32
cargo run -p pdl-bench -- run --suite large --tier smoke \
  --run-label sampled-smoke --samples 3 --warmups 1 --randomize
cargo run -p pdl-bench -- markdown --run-label sampled-smoke
cargo run -p pdl-bench -- snapshot --run-label before-v0.32 --baseline v0.31.0-before-v0.32
```

## Layout

- `bench/workloads/` is tracked source: `.pdl` programs grouped by suite.
- `bench/data/generated/` is ignored deterministic synthetic data.
- `bench/data/raw/` is ignored downloaded source data.
- `bench/data/prepared/` is ignored derived benchmark-ready data.
- `bench/runs/<run-label>/` is ignored benchmark output.
- `bench/baselines/<baseline>/` is tracked curated benchmark history.

Every run writes `bench/runs/<run-label>/report.csv`. Reports keep the original
timing columns and add v0.36 columns for repeated sample count, warmup count,
min/median/p90/max/stddev, failed and unsupported sample counts, peak RSS where
`/usr/bin/time` supports it, plan observability, selected/eligible engine, sink
strategy, row-materialization status, system metadata, build profile, dirty
flag, and feature flags.

Use `compare` to gate median regressions against configurable absolute and
relative thresholds:

```bash
cargo run -p pdl-bench -- compare \
  --baseline full-baseline-20260606 \
  --run-label sampled-smoke \
  --max-relative-regression 0.05 \
  --max-absolute-regression-ms 50
```

Use `markdown` to generate a compact release table from a report. Use
`clean --dry-run` to inspect ignored run directories that `clean` would remove.

Use `snapshot` to promote an ignored run report into `bench/baselines/`.
Snapshots copy `report.csv` and write `environment.txt` with the git ref,
system, Rust/Cargo versions, source report, and snapshot timestamp.

The current generated source family is `million-row` in CSV, Parquet, and Arrow
IPC stream form. The large suite also includes CSV partition files, a segment
dimension table, composite-key join workloads, window-heavy rank, running,
offset, value, and distribution workloads, and a writer-dominated
`million_row_text_emission` workload (v0.44) that measures CSV and NDJSON
emission row-vs-native.

## PDL-to-Algraf Arrow Smoke

The tracked smoke command keeps the process boundary as Arrow IPC. It runs a
PDL workload that filters, mutates, ranks rows with a window expression,
projects, sorts, and limits a large table, then renders a bounded Algraf
histogram from Arrow-stream stdin:

```bash
cargo build -p pdl-cli --release
(cd ../algraf && cargo build -p algraf-cli --release)
scripts/pdl-algraf-arrow-smoke.sh v0_39_pdl_algraf_smoke
```

Set `PDL_BIN` or `ALGRAF_BIN` to override the binaries. The smoke writes
`bench/runs/<run-label>/pdl_algraf_smoke.csv` with PDL engine, Arrow bytes, PDL
elapsed time, Algraf elapsed time, SVG bytes, and status.
