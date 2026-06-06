# PDL v0.31 Plan

Status: Implemented
Target version: 0.31.0
Owner: PDL maintainers
Related spec: [`PDL_SPEC.md`](PDL_SPEC.md)
Predecessor plan: [`V0_30_PLAN.md`](V0_30_PLAN.md)
Related Algraf plan: [`V0_68_PLAN.md`](../../algraf/docs/V0_68_PLAN.md)
Follow-on performance plan: [`V0_32_PLAN.md`](V0_32_PLAN.md)
Roadmap theme: Benchmark infrastructure and cross-repo baseline alignment.
Cross-repo coordination: `../algraf/` for matching dataset lifecycle,
workload layout, CSV report schema, and baseline snapshot conventions.

## Purpose

PDL v0.31 establishes the benchmark process needed before the native
datapipeline performance work starts. The goal is not to optimize execution in
this release. The goal is to make before/after performance evidence repeatable
across PDL and Algraf by aligning datasets, workload locations, generated
outputs, report files, and baseline snapshots.

Before this release, benchmark assets were split across `bench/`,
`bench-output/`, and `benchdata/`, with a mix of shell scripts, ad hoc Rust
examples, generated files, and manually captured timing output. That made it
hard to know which inputs were authoritative, which results were comparable, or
which cleanup was safe.

This release creates a single benchmark lifecycle:

```text
download/generate source data
  -> run tracked workloads
  -> write ignored per-run CSV reports
  -> snapshot selected reports as tracked baselines
```

The follow-on [`V0_32_PLAN.md`](V0_32_PLAN.md) owns the native Polars/data-plan
performance changes. This plan owns the measurement harness those changes will
be evaluated against.

## Scope

### Benchmark Crate

Status: Implemented.

Acceptance criteria:

- Add a workspace `pdl-bench` crate.
- Provide Rust commands for `generate`, `download`, `prepare`, `run`, and
  `snapshot` so the primary benchmark lifecycle is not shell-script driven.
- Keep legacy scripts as thin compatibility wrappers around `cargo run -p
  pdl-bench`.
- Support repeatable run labels so PDL and Algraf can be benchmarked with the
  same label.
- Write report output as CSV, not TSV.

### Directory Layout

Status: Implemented.

Acceptance criteria:

- Tracked workloads live under `bench/workloads/`.
- Ignored source and generated data live under `bench/data/`.
- Ignored per-run outputs live under `bench/runs/<run-label>/`.
- Tracked curated baselines live under `bench/baselines/<baseline>/`.
- Old `benchdata/`, `bench-output/`, and `bench/examples/` paths are removed or
  migrated.
- `.gitignore` documents only the active ignored benchmark directories.

### Dataset Family

Status: Implemented.

Acceptance criteria:

- Generate a deterministic million-row dataset.
- Write the generated dataset in CSV, Parquet, and Arrow IPC stream formats.
- Download and preserve external source files under `bench/data/raw/` when
  requested.
- Keep large downloaded/generated inputs out of git.
- Make full-size and smaller smoke/local tiers available from the same command
  surface.

### Workloads And Reports

Status: Implemented.

Acceptance criteria:

- Run the large workload suite from `bench/workloads/large/`.
- Include comparable CSV, Parquet, and Arrow-stream workloads where the current
  runtime supports them.
- Emit `bench/runs/<run-label>/report.csv` with one row per benchmark case.
- Capture command, workload, input/output formats, status, elapsed time, byte
  counts, and output path in the report.
- Treat expected diagnostics as report rows rather than silent failures.

### Baseline Snapshots

Status: Implemented.

Acceptance criteria:

- Add `pdl-bench snapshot --run-label <label> --baseline <name>`.
- Copy a selected ignored run report into
  `bench/baselines/<baseline>/report.csv`.
- Record environment metadata in
  `bench/baselines/<baseline>/environment.txt`.
- Capture git ref, git status, toolchain versions, host information, source
  report path, and run command.
- Capture git status before writing the baseline files so snapshots do not
  describe themselves as new untracked files.

### Baseline Run

Status: Implemented.

Acceptance criteria:

- Run the full benchmark lifecycle from scratch using downloaded/generated data.
- Capture a shared baseline label that can be compared with Algraf.
- Store the tracked baseline report under
  `bench/baselines/full-baseline-20260606/report.csv`.

Observed baseline:

- Run label: `full-baseline-20260606`.
- Report path: `bench/runs/full-baseline-20260606/report.csv`.
- Snapshot path: `bench/baselines/full-baseline-20260606/report.csv`.
- Result: 7 benchmark rows, all `ok`.
- Dataset formats covered: CSV, Parquet, and Arrow IPC stream.

## Non-Goals

- Implementing the native Polars execution engine.
- Changing PDL source-language semantics.
- Changing the normative format or CLI behavior in `PDL_SPEC.md` beyond
  documenting benchmark process if desired.
- Treating local baseline timings as CI thresholds before a reference machine,
  variance policy, and release-mode benchmark process are defined.

## Validation

- `cargo fmt --all`
- `cargo check -p pdl-bench`
- `cargo run -p pdl-bench -- generate --rows 100`
- `cargo run -p pdl-bench -- run --suite large --tier smoke --run-label smoke-pdl-bench --no-generate`
- `cargo run -p pdl-bench -- download --dataset all --force`
- `cargo run -p pdl-bench -- generate --tier stress`
- `cargo run -p pdl-bench -- run --suite large --tier stress --run-label full-baseline-20260606 --no-generate`
- `cargo run -p pdl-bench -- snapshot --run-label full-baseline-20260606 --baseline full-baseline-20260606`
