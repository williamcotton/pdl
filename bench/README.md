# PDL Benchmarks

`pdl-bench` owns the benchmark lifecycle for this repo.

```bash
cargo run -p pdl-bench -- generate --tier smoke
cargo run -p pdl-bench -- download --dataset all
cargo run -p pdl-bench -- prepare --tier smoke
cargo run -p pdl-bench -- run --suite large --tier smoke --run-label before-v0.32
cargo run -p pdl-bench -- snapshot --run-label before-v0.32 --baseline v0.31.0-before-v0.32
```

## Layout

- `bench/workloads/` is tracked source: `.pdl` programs grouped by suite.
- `bench/data/generated/` is ignored deterministic synthetic data.
- `bench/data/raw/` is ignored downloaded source data.
- `bench/data/prepared/` is ignored derived benchmark-ready data.
- `bench/runs/<run-label>/` is ignored benchmark output.
- `bench/baselines/<baseline>/` is tracked curated benchmark history.

Every run writes `bench/runs/<run-label>/report.csv`. Reports use the same
column contract as `algraf-bench` and include `git describe --tags --always
--dirty` so before/after runs can be compared by tag or commit.

Use `snapshot` to promote an ignored run report into `bench/baselines/`.
Snapshots copy `report.csv` and write `environment.txt` with the git ref,
system, Rust/Cargo versions, source report, and snapshot timestamp.

The current generated source family is `million-row` in CSV, Parquet, and Arrow
IPC stream form.
