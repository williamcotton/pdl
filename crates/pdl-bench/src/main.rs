use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use arrow_array::{ArrayRef, Float64Array, Int64Array, RecordBatch, StringArray};
use arrow_ipc::writer::StreamWriter;
use arrow_schema::{DataType as ArrowDataType, Field, Schema};
use chrono::Utc;
use clap::{Parser, Subcommand, ValueEnum};
use parquet::arrow::ArrowWriter;
use serde_json::Value as JsonValue;

const TLC_URL: &str =
    "https://d37ci6vzurychx.cloudfront.net/trip-data/yellow_tripdata_2024-01.parquet";
const TLC_ZONES_URL: &str = "https://d37ci6vzurychx.cloudfront.net/misc/taxi_zone_lookup.csv";
const SFO_URL: &str = "https://static.sfomuseum.org/parquet/sfomuseum-data-flights-2026-03.parquet";

const REPORT_HEADER: [&str; 50] = [
    "repo",
    "tool",
    "run_label",
    "suite",
    "workload",
    "dataset",
    "tier",
    "input_format",
    "output_format",
    "engine",
    "status",
    "command",
    "output_path",
    "log_path",
    "input_rows",
    "output_rows",
    "marks",
    "output_bytes",
    "elapsed_ms",
    "run_timestamp_utc",
    "git_ref",
    "notes",
    "sample_count",
    "warmup_count",
    "min_ms",
    "median_ms",
    "p90_ms",
    "max_ms",
    "stddev_ms",
    "failed_samples",
    "unsupported_samples",
    "peak_rss_bytes",
    "row_materialization",
    "selected_engine",
    "eligible_engine",
    "fallback_reason",
    "sink_strategy",
    "source_boundary",
    "required_source_columns",
    "phase_scan_ms",
    "phase_transform_ms",
    "phase_collect_ms",
    "phase_write_ms",
    "os",
    "cpu_model",
    "logical_cores",
    "rustc",
    "build_profile",
    "git_dirty",
    "feature_flags",
];

fn main() {
    if let Err(err) = run() {
        eprintln!("pdl-bench: {err}");
        std::process::exit(1);
    }
}

#[derive(Parser)]
#[command(name = "pdl-bench")]
#[command(about = "PDL benchmark data generation and before/after run reporting")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Generate local benchmark datasets under bench/data/generated.
    Generate {
        #[arg(long, value_enum, default_value_t = Tier::Stress)]
        tier: Tier,
        /// Override the tier's default row count.
        #[arg(long)]
        rows: Option<usize>,
    },
    /// Download external raw benchmark sources under bench/data/raw.
    Download {
        #[arg(long, value_enum, default_value_t = ExternalDataset::All)]
        dataset: ExternalDataset,
        #[arg(long)]
        force: bool,
    },
    /// Prepare generated/downloaded sources into benchmark-ready files.
    Prepare {
        #[arg(long, value_enum, default_value_t = Tier::Stress)]
        tier: Tier,
        /// Override the tier's default row count.
        #[arg(long)]
        rows: Option<usize>,
    },
    /// Run benchmark workloads and write bench/runs/<run-label>/report.csv.
    Run {
        #[arg(long, value_enum, default_value_t = Suite::Large)]
        suite: Suite,
        #[arg(long, value_enum, default_value_t = Tier::Stress)]
        tier: Tier,
        #[arg(long)]
        run_label: Option<String>,
        #[arg(long, value_enum, default_value_t = BuildProfile::Debug)]
        profile: BuildProfile,
        #[arg(long, value_enum, default_value_t = BenchmarkEngine::Auto)]
        engine: BenchmarkEngine,
        /// Do not generate missing benchmark data before running.
        #[arg(long)]
        no_generate: bool,
        /// Measured samples per workload after warmups.
        #[arg(long, default_value_t = 1)]
        samples: usize,
        /// Unreported warmup executions per workload.
        #[arg(long, default_value_t = 0)]
        warmups: usize,
        /// Shuffle workload order for this run.
        #[arg(long)]
        randomize: bool,
        /// Milliseconds to sleep between measured samples.
        #[arg(long, default_value_t = 0)]
        cooldown_ms: u64,
    },
    /// Compare a run report with a baseline report.
    Compare {
        #[arg(long)]
        baseline: String,
        #[arg(long)]
        run_label: String,
        /// Allowed relative regression before compare exits non-zero.
        #[arg(long, default_value_t = 0.05)]
        max_relative_regression: f64,
        /// Allowed absolute regression in milliseconds before compare exits non-zero.
        #[arg(long, default_value_t = 50)]
        max_absolute_regression_ms: u128,
    },
    /// Generate a compact Markdown table from a run report.
    Markdown {
        #[arg(long)]
        run_label: String,
    },
    /// Copy an ignored run report into a tracked baseline directory.
    Snapshot {
        #[arg(long)]
        run_label: String,
        #[arg(long)]
        baseline: String,
    },
    /// Remove ignored benchmark run directories.
    Clean {
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum Suite {
    Large,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum ExternalDataset {
    All,
    Tlc,
    Sfo,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum Tier {
    Smoke,
    Local,
    Stress,
}

impl Tier {
    fn as_str(self) -> &'static str {
        match self {
            Tier::Smoke => "smoke",
            Tier::Local => "local",
            Tier::Stress => "stress",
        }
    }

    fn rows(self) -> usize {
        match self {
            Tier::Smoke => 1_000,
            Tier::Local => 100_000,
            Tier::Stress => 1_000_000,
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum BuildProfile {
    Debug,
    Release,
}

impl BuildProfile {
    fn dir(self) -> &'static str {
        match self {
            BuildProfile::Debug => "debug",
            BuildProfile::Release => "release",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum BenchmarkEngine {
    Auto,
    Row,
    Native,
}

impl BenchmarkEngine {
    fn as_str(self) -> &'static str {
        match self {
            BenchmarkEngine::Auto => "auto",
            BenchmarkEngine::Row => "row",
            BenchmarkEngine::Native => "native",
        }
    }
}

struct Workload {
    name: &'static str,
    program: &'static str,
    dataset: &'static str,
    input_format: &'static str,
    output_format: &'static str,
    required_path: &'static str,
    /// When set, this file is fed to the pdl process on stdin so the
    /// workload exercises the byte-backed stdin scan path (v0.46).
    stdin_path: Option<&'static str>,
}

impl Workload {
    fn stdout_format(&self) -> Option<&'static str> {
        match self.output_format {
            "multi-csv" => None,
            format => Some(format),
        }
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let root = repo_root();
    match cli.command {
        Commands::Generate { tier, rows } => {
            generate_all(&root, rows.unwrap_or_else(|| tier.rows()))?;
        }
        Commands::Download { dataset, force } => {
            download_external(&root, dataset, force)?;
        }
        Commands::Prepare { tier, rows } => {
            prepare_sources(&root, rows.unwrap_or_else(|| tier.rows()))?;
        }
        Commands::Run {
            suite,
            tier,
            run_label,
            profile,
            engine,
            no_generate,
            samples,
            warmups,
            randomize,
            cooldown_ms,
        } => {
            run_suite(
                &root,
                RunSuiteOptions {
                    suite,
                    tier,
                    run_label,
                    profile,
                    engine,
                    no_generate,
                    samples,
                    warmups,
                    randomize,
                    cooldown_ms,
                },
            )?;
        }
        Commands::Compare {
            baseline,
            run_label,
            max_relative_regression,
            max_absolute_regression_ms,
        } => {
            compare_run(
                &root,
                &baseline,
                &run_label,
                CompareThresholds {
                    max_relative_regression,
                    max_absolute_regression_ms,
                },
            )?;
        }
        Commands::Markdown { run_label } => {
            markdown_run(&root, &run_label)?;
        }
        Commands::Snapshot {
            run_label,
            baseline,
        } => {
            snapshot_run(&root, &run_label, &baseline)?;
        }
        Commands::Clean { dry_run } => {
            clean_runs(&root, dry_run)?;
        }
    }
    Ok(())
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("pdl-bench crate should live under crates/")
        .to_path_buf()
}

struct RunSuiteOptions {
    suite: Suite,
    tier: Tier,
    run_label: Option<String>,
    profile: BuildProfile,
    engine: BenchmarkEngine,
    no_generate: bool,
    samples: usize,
    warmups: usize,
    randomize: bool,
    cooldown_ms: u64,
}

fn run_suite(root: &Path, options: RunSuiteOptions) -> Result<(), Box<dyn std::error::Error>> {
    let RunSuiteOptions {
        suite,
        tier,
        run_label,
        profile,
        engine,
        no_generate,
        samples,
        warmups,
        randomize,
        cooldown_ms,
    } = options;
    if !matches!(suite, Suite::Large) {
        unreachable!("clap only exposes known suite values");
    }
    if samples == 0 {
        return Err("--samples must be greater than zero".into());
    }

    let required = [
        root.join("bench/data/generated/million-row.csv"),
        root.join("bench/data/generated/million-row.parquet"),
        root.join("bench/data/generated/million-row.arrow"),
        root.join("bench/data/generated/segment-dimension.csv"),
        root.join("bench/data/generated/million-row-part-a.csv"),
        root.join("bench/data/generated/million-row-part-b.csv"),
        root.join("bench/data/generated/million-row-narrow.csv"),
        root.join("bench/data/generated/million-row-events.csv"),
        root.join("bench/data/generated/million-row-events.jsonl"),
    ];
    if required.iter().any(|path| !path.exists()) {
        if no_generate {
            return Err("missing generated benchmark datasets under bench/data/generated".into());
        }
        generate_all(root, tier.rows())?;
    }

    build_cli(root, profile)?;

    let run_timestamp = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let run_label =
        run_label.unwrap_or_else(|| format!("run-{}", Utc::now().format("%Y%m%dT%H%M%SZ")));
    let run_label = sanitize_label(&run_label);
    let run_dir = root.join("bench/runs").join(&run_label);
    fs::create_dir_all(&run_dir)?;
    let report_path = run_dir.join("report.csv");
    let mut report = csv::Writer::from_path(&report_path)?;
    report.write_record(REPORT_HEADER)?;
    let git_ref = git_ref(root);
    let input_rows =
        csv_data_rows(&root.join("bench/data/generated/million-row.csv")).unwrap_or(tier.rows());
    let metadata = SystemMetadata::capture(root, profile);
    let context = RunContext {
        root,
        run_dir: &run_dir,
        tier,
        profile,
        input_rows,
        run_timestamp: &run_timestamp,
        git_ref: &git_ref,
        run_label: &run_label,
        engine,
        samples,
        warmups,
        cooldown_ms,
        metadata,
    };

    let mut workloads = large_workloads().iter().collect::<Vec<_>>();
    if randomize {
        shuffle_workloads(&mut workloads, run_timestamp.as_bytes());
    }
    let mut failures = 0usize;
    for workload in workloads {
        let outcome = run_workload(&context, workload);
        match outcome {
            Ok(outcome) => {
                if outcome.failed {
                    failures += 1;
                }
                report.write_record(outcome.record)?;
            }
            Err(err) => {
                failures += 1;
                report.write_record(failure_record(&context, workload, &err.to_string()))?;
            }
        }
    }
    report.flush()?;
    println!("wrote {}", relative(root, &report_path));

    if failures > 0 {
        return Err(format!("{failures} workload(s) failed").into());
    }
    Ok(())
}

fn compare_run(
    root: &Path,
    baseline: &str,
    run_label: &str,
    thresholds: CompareThresholds,
) -> Result<(), Box<dyn std::error::Error>> {
    let baseline_path = resolve_report_path(root, "bench/baselines", baseline);
    let run_path = resolve_report_path(root, "bench/runs", run_label);
    if !baseline_path.exists() {
        return Err(format!(
            "missing baseline report: {}",
            relative(root, &baseline_path)
        )
        .into());
    }
    if !run_path.exists() {
        return Err(format!("missing run report: {}", relative(root, &run_path)).into());
    }
    let baseline_rows = read_report(&baseline_path)?;
    let run_rows = read_report(&run_path)?;
    let mut baseline_by_key = BTreeMap::new();
    for row in baseline_rows {
        baseline_by_key.insert(row.key_without_engine(), row);
    }

    println!(
        "{:<43} {:<18} {:<7} {:<11} {:>10} {:>10} {:>11}",
        "workload", "format", "engine", "status", "baseline", "current", "improvement"
    );
    let mut regressions = 0usize;
    for row in run_rows {
        let Some(baseline) = baseline_by_key.get(&row.key_without_engine()) else {
            println!(
                "{:<43} {:<18} {:<7} {:<11} {:>10} {:>10} {:>11}",
                row.workload,
                row.format_label(),
                row.engine,
                row.status,
                "-",
                row.compare_ms().to_string(),
                "no baseline"
            );
            continue;
        };
        if row.status != "ok" || baseline.status != "ok" {
            println!(
                "{:<43} {:<18} {:<7} {:<11} {:>10} {:>10} {:>11}",
                row.workload,
                row.format_label(),
                row.engine,
                row.status,
                baseline.compare_ms(),
                row.compare_ms(),
                "-"
            );
            continue;
        }
        let baseline_ms = baseline.compare_ms();
        let current_ms = row.compare_ms();
        let improvement = if baseline_ms > 0 {
            (baseline_ms as f64 - current_ms as f64) * 100.0 / baseline_ms as f64
        } else {
            0.0
        };
        let regression_ms = current_ms.saturating_sub(baseline_ms);
        let regression_ratio = if baseline_ms > 0 {
            regression_ms as f64 / baseline_ms as f64
        } else {
            0.0
        };
        let regressed = regression_ms > thresholds.max_absolute_regression_ms
            && regression_ratio > thresholds.max_relative_regression;
        if regressed {
            regressions += 1;
        }
        println!(
            "{:<43} {:<18} {:<7} {:<11} {:>10} {:>10} {:+10.1}%{}",
            row.workload,
            row.format_label(),
            row.engine,
            row.status,
            baseline_ms,
            current_ms,
            improvement,
            if regressed { " regression" } else { "" }
        );
    }
    if regressions > 0 {
        return Err(format!("{regressions} regression(s) exceeded configured thresholds").into());
    }
    Ok(())
}

struct CompareThresholds {
    max_relative_regression: f64,
    max_absolute_regression_ms: u128,
}

fn resolve_report_path(root: &Path, parent: &str, label_or_path: &str) -> PathBuf {
    let candidate = PathBuf::from(label_or_path);
    if candidate.exists() {
        return candidate;
    }
    root.join(parent)
        .join(sanitize_label(label_or_path))
        .join("report.csv")
}

#[derive(Clone, Debug)]
struct ReportRow {
    workload: String,
    input_format: String,
    output_format: String,
    engine: String,
    status: String,
    elapsed_ms: u128,
    median_ms: Option<u128>,
    output_bytes: Option<u64>,
    peak_rss_bytes: Option<u64>,
    stddev_ms: Option<f64>,
}

impl ReportRow {
    fn key_without_engine(&self) -> (String, String, String) {
        (
            self.workload.clone(),
            self.input_format.clone(),
            self.output_format.clone(),
        )
    }

    fn format_label(&self) -> String {
        format!("{}->{}", self.input_format, self.output_format)
    }

    fn compare_ms(&self) -> u128 {
        self.median_ms.unwrap_or(self.elapsed_ms)
    }
}

fn read_report(path: &Path) -> Result<Vec<ReportRow>, Box<dyn std::error::Error>> {
    let mut reader = csv::Reader::from_path(path)?;
    let headers = reader.headers()?.clone();
    let index = |name: &str| -> Result<usize, Box<dyn std::error::Error>> {
        headers
            .iter()
            .position(|header| header == name)
            .ok_or_else(|| format!("report {} is missing `{name}`", path.display()).into())
    };
    let workload_index = index("workload")?;
    let input_format_index = index("input_format")?;
    let output_format_index = index("output_format")?;
    let engine_index = headers.iter().position(|header| header == "engine");
    let status_index = index("status")?;
    let elapsed_ms_index = index("elapsed_ms")?;
    let median_ms_index = headers.iter().position(|header| header == "median_ms");
    let output_bytes_index = headers.iter().position(|header| header == "output_bytes");
    let peak_rss_bytes_index = headers.iter().position(|header| header == "peak_rss_bytes");
    let stddev_ms_index = headers.iter().position(|header| header == "stddev_ms");
    let mut rows = Vec::new();
    for record in reader.records() {
        let record = record?;
        rows.push(ReportRow {
            workload: record.get(workload_index).unwrap_or_default().to_string(),
            input_format: record
                .get(input_format_index)
                .unwrap_or_default()
                .to_string(),
            output_format: record
                .get(output_format_index)
                .unwrap_or_default()
                .to_string(),
            engine: engine_index
                .and_then(|index| record.get(index))
                .filter(|value| !value.is_empty())
                .unwrap_or("unspecified")
                .to_string(),
            status: record.get(status_index).unwrap_or_default().to_string(),
            elapsed_ms: record
                .get(elapsed_ms_index)
                .unwrap_or_default()
                .parse()
                .unwrap_or(0),
            median_ms: parse_optional_u128(&record, median_ms_index),
            output_bytes: parse_optional_u64(&record, output_bytes_index),
            peak_rss_bytes: parse_optional_u64(&record, peak_rss_bytes_index),
            stddev_ms: parse_optional_f64(&record, stddev_ms_index),
        });
    }
    Ok(rows)
}

fn parse_optional_u128(record: &csv::StringRecord, index: Option<usize>) -> Option<u128> {
    index
        .and_then(|index| record.get(index))
        .filter(|value| !value.is_empty())
        .and_then(|value| value.parse().ok())
}

fn parse_optional_u64(record: &csv::StringRecord, index: Option<usize>) -> Option<u64> {
    index
        .and_then(|index| record.get(index))
        .filter(|value| !value.is_empty())
        .and_then(|value| value.parse().ok())
}

fn parse_optional_f64(record: &csv::StringRecord, index: Option<usize>) -> Option<f64> {
    index
        .and_then(|index| record.get(index))
        .filter(|value| !value.is_empty())
        .and_then(|value| value.parse().ok())
}

fn markdown_run(root: &Path, run_label: &str) -> Result<(), Box<dyn std::error::Error>> {
    let run_path = resolve_report_path(root, "bench/runs", run_label);
    if !run_path.exists() {
        return Err(format!("missing run report: {}", relative(root, &run_path)).into());
    }
    let rows = read_report(&run_path)?;
    println!("| workload | format | engine | status | median ms | stddev ms | output bytes | peak RSS bytes |");
    println!("| --- | --- | --- | --- | ---: | ---: | ---: | ---: |");
    for row in rows {
        println!(
            "| {} | {} | {} | {} | {} | {} | {} | {} |",
            row.workload,
            row.format_label(),
            row.engine,
            row.status,
            row.compare_ms(),
            row.stddev_ms
                .map(|value| format!("{value:.3}"))
                .unwrap_or_default(),
            row.output_bytes
                .map(|value| value.to_string())
                .unwrap_or_default(),
            row.peak_rss_bytes
                .map(|value| value.to_string())
                .unwrap_or_default()
        );
    }
    Ok(())
}

fn clean_runs(root: &Path, dry_run: bool) -> Result<(), Box<dyn std::error::Error>> {
    let runs_dir = root.join("bench/runs");
    if !runs_dir.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(&runs_dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let path = entry.path();
        if dry_run {
            println!("would remove {}", relative(root, &path));
        } else {
            fs::remove_dir_all(&path)?;
            println!("removed {}", relative(root, &path));
        }
    }
    Ok(())
}

fn snapshot_run(
    root: &Path,
    run_label: &str,
    baseline: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let run_label = sanitize_label(run_label);
    let baseline = sanitize_label(baseline);
    let source_report = root.join("bench/runs").join(&run_label).join("report.csv");
    if !source_report.exists() {
        return Err(format!("missing run report: {}", relative(root, &source_report)).into());
    }
    let git_status = command_output(root, "git", &["status", "--short"]);

    let baseline_dir = root.join("bench/baselines").join(&baseline);
    fs::create_dir_all(&baseline_dir)?;
    let baseline_report = baseline_dir.join("report.csv");
    fs::copy(&source_report, &baseline_report)?;

    let environment = baseline_dir.join("environment.txt");
    write_environment(
        root,
        "pdl",
        &run_label,
        &baseline,
        &source_report,
        &environment,
        &git_status,
    )?;

    println!("wrote {}", relative(root, &baseline_report));
    println!("wrote {}", relative(root, &environment));
    Ok(())
}

fn write_environment(
    root: &Path,
    repo: &str,
    run_label: &str,
    baseline: &str,
    source_report: &Path,
    path: &Path,
    git_status: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut file = BufWriter::new(File::create(path)?);
    writeln!(file, "repo: {repo}")?;
    writeln!(file, "baseline: {baseline}")?;
    writeln!(file, "run_label: {run_label}")?;
    writeln!(
        file,
        "snapshot_timestamp_utc: {}",
        Utc::now().format("%Y-%m-%dT%H:%M:%SZ")
    )?;
    writeln!(file, "source_report: {}", relative(root, source_report))?;
    writeln!(file, "git_ref: {}", git_ref(root))?;
    writeln!(file, "git_status_short:")?;
    if git_status.trim().is_empty() {
        writeln!(file, "  clean")?;
    } else {
        for line in git_status.lines() {
            writeln!(file, "  {line}")?;
        }
    }
    writeln!(
        file,
        "system: {}",
        command_output(root, "uname", &["-a"]).trim()
    )?;
    writeln!(
        file,
        "rustc: {}",
        command_output(root, "rustc", &["-V"]).trim()
    )?;
    writeln!(
        file,
        "cargo: {}",
        command_output(root, "cargo", &["-V"]).trim()
    )?;
    writeln!(
        file,
        "run_command: cargo run -p pdl-bench -- run --suite large --run-label {run_label}"
    )?;
    Ok(())
}

fn large_workloads() -> &'static [Workload] {
    &[
        Workload {
            name: "million_row_segment_summary",
            program: "bench/workloads/large/million_row_segment_summary.pdl",
            dataset: "million-row",
            input_format: "csv",
            output_format: "csv",
            required_path: "bench/data/generated/million-row.csv",
            stdin_path: None,
        },
        Workload {
            name: "million_row_segment_summary_parquet",
            program: "bench/workloads/large/million_row_segment_summary_parquet.pdl",
            dataset: "million-row",
            input_format: "parquet",
            output_format: "csv",
            required_path: "bench/data/generated/million-row.parquet",
            stdin_path: None,
        },
        Workload {
            name: "million_row_segment_summary_arrow_stream",
            program: "bench/workloads/large/million_row_segment_summary_arrow_stream.pdl",
            dataset: "million-row",
            input_format: "arrow-stream",
            output_format: "csv",
            required_path: "bench/data/generated/million-row.arrow",
            stdin_path: None,
        },
        Workload {
            name: "million_row_segment_summary",
            program: "bench/workloads/large/million_row_segment_summary.pdl",
            dataset: "million-row",
            input_format: "csv",
            output_format: "arrow-stream",
            required_path: "bench/data/generated/million-row.csv",
            stdin_path: None,
        },
        Workload {
            name: "million_row_mutate_csv",
            program: "bench/workloads/large/million_row_mutate_csv.pdl",
            dataset: "million-row",
            input_format: "csv",
            output_format: "csv",
            required_path: "bench/data/generated/million-row.csv",
            stdin_path: None,
        },
        Workload {
            name: "million_row_mutate_parquet",
            program: "bench/workloads/large/million_row_mutate_parquet.pdl",
            dataset: "million-row",
            input_format: "parquet",
            output_format: "csv",
            required_path: "bench/data/generated/million-row.parquet",
            stdin_path: None,
        },
        Workload {
            name: "million_row_mutate_arrow_stream",
            program: "bench/workloads/large/million_row_mutate_arrow_stream.pdl",
            dataset: "million-row",
            input_format: "arrow-stream",
            output_format: "csv",
            required_path: "bench/data/generated/million-row.arrow",
            stdin_path: None,
        },
        Workload {
            name: "million_row_mutate_csv",
            program: "bench/workloads/large/million_row_mutate_csv.pdl",
            dataset: "million-row",
            input_format: "csv",
            output_format: "arrow-stream",
            required_path: "bench/data/generated/million-row.csv",
            stdin_path: None,
        },
        Workload {
            name: "million_row_mutate_csv_stdin",
            program: "bench/workloads/large/million_row_mutate_csv_stdin.pdl",
            dataset: "million-row",
            input_format: "csv",
            output_format: "csv",
            required_path: "bench/data/generated/million-row.csv",
            stdin_path: Some("bench/data/generated/million-row.csv"),
        },
        Workload {
            name: "million_row_top_scores",
            program: "bench/workloads/large/million_row_top_scores.pdl",
            dataset: "million-row",
            input_format: "csv",
            output_format: "csv",
            required_path: "bench/data/generated/million-row.csv",
            stdin_path: None,
        },
        Workload {
            name: "million_row_projection_smoke",
            program: "bench/workloads/large/million_row_projection_smoke.pdl",
            dataset: "million-row",
            input_format: "csv",
            output_format: "csv",
            required_path: "bench/data/generated/million-row.csv",
            stdin_path: None,
        },
        Workload {
            name: "million_row_distinct_segments",
            program: "bench/workloads/large/million_row_distinct_segments.pdl",
            dataset: "million-row",
            input_format: "csv",
            output_format: "csv",
            required_path: "bench/data/generated/million-row.csv",
            stdin_path: None,
        },
        Workload {
            name: "million_row_join_dimension",
            program: "bench/workloads/large/million_row_join_dimension.pdl",
            dataset: "million-row",
            input_format: "csv",
            output_format: "csv",
            required_path: "bench/data/generated/segment-dimension.csv",
            stdin_path: None,
        },
        Workload {
            name: "million_row_composite_join_rollup",
            program: "bench/workloads/large/million_row_composite_join_rollup.pdl",
            dataset: "million-row",
            input_format: "csv",
            output_format: "csv",
            required_path: "bench/data/generated/million-row.csv",
            stdin_path: None,
        },
        Workload {
            name: "million_row_composite_join_lookup",
            program: "bench/workloads/large/million_row_composite_join_lookup.pdl",
            dataset: "million-row",
            input_format: "csv",
            output_format: "csv",
            required_path: "bench/data/generated/million-row.csv",
            stdin_path: None,
        },
        Workload {
            name: "million_row_union_partitions",
            program: "bench/workloads/large/million_row_union_partitions.pdl",
            dataset: "million-row",
            input_format: "csv",
            output_format: "csv",
            required_path: "bench/data/generated/million-row-part-a.csv",
            stdin_path: None,
        },
        // v0.49: heterogeneous-schema union coverage exercises null padding
        // and downstream fill semantics after all language rows reached native parity.
        Workload {
            name: "million_row_union_null_padding",
            program: "bench/workloads/large/million_row_union_null_padding.pdl",
            dataset: "million-row",
            input_format: "csv",
            output_format: "csv",
            required_path: "bench/data/generated/million-row-narrow.csv",
            stdin_path: None,
        },
        Workload {
            name: "windowed_sales_rank",
            program: "bench/workloads/large/windowed_sales_rank.pdl",
            dataset: "million-row",
            input_format: "csv",
            output_format: "csv",
            required_path: "bench/data/generated/million-row.csv",
            stdin_path: None,
        },
        Workload {
            name: "million_row_window_running",
            program: "bench/workloads/large/million_row_window_running.pdl",
            dataset: "million-row",
            input_format: "csv",
            output_format: "csv",
            required_path: "bench/data/generated/million-row.csv",
            stdin_path: None,
        },
        Workload {
            name: "million_row_window_offsets_values",
            program: "bench/workloads/large/million_row_window_offsets_values.pdl",
            dataset: "million-row",
            input_format: "csv",
            output_format: "csv",
            required_path: "bench/data/generated/million-row.csv",
            stdin_path: None,
        },
        // v0.49: dynamic offset windows now remain native-eligible.
        Workload {
            name: "million_row_dynamic_window_offsets",
            program: "bench/workloads/large/million_row_dynamic_window_offsets.pdl",
            dataset: "million-row-events",
            input_format: "csv",
            output_format: "csv",
            required_path: "bench/data/generated/million-row-events.csv",
            stdin_path: None,
        },
        Workload {
            name: "pdl_to_algraf_arrow_handoff",
            program: "bench/workloads/large/pdl_to_algraf_arrow_handoff.pdl",
            dataset: "million-row",
            input_format: "csv",
            output_format: "arrow-stream",
            required_path: "bench/data/generated/million-row.csv",
            stdin_path: None,
        },
        // v0.49: temporal functions are native-eligible across mutate,
        // grouping keys, and aggregate inputs.
        Workload {
            name: "million_row_temporal_buckets",
            program: "bench/workloads/large/million_row_temporal_buckets.pdl",
            dataset: "million-row-events",
            input_format: "csv",
            output_format: "csv",
            required_path: "bench/data/generated/million-row-events.csv",
            stdin_path: None,
        },
        // v0.49: JSON Lines scans use the same native orchestration coverage
        // as other shipped input formats.
        Workload {
            name: "million_row_jsonl_temporal_buckets",
            program: "bench/workloads/large/million_row_jsonl_temporal_buckets.pdl",
            dataset: "million-row-events",
            input_format: "jsonl",
            output_format: "csv",
            required_path: "bench/data/generated/million-row-events.jsonl",
            stdin_path: None,
        },
        // v0.49: dynamic column indirection, dynamic replace arguments, and
        // mixed-class `if_else` are native-eligible.
        Workload {
            name: "million_row_dynamic_text_and_col",
            program: "bench/workloads/large/million_row_dynamic_text_and_col.pdl",
            dataset: "million-row-events",
            input_format: "csv",
            output_format: "csv",
            required_path: "bench/data/generated/million-row-events.csv",
            stdin_path: None,
        },
        // v0.44: writer-dominated workload measuring the native CSV and
        // NDJSON direct writers against the row-format writers.
        Workload {
            name: "million_row_text_emission",
            program: "bench/workloads/large/million_row_text_emission.pdl",
            dataset: "million-row",
            input_format: "csv",
            output_format: "csv",
            required_path: "bench/data/generated/million-row.csv",
            stdin_path: None,
        },
        Workload {
            name: "million_row_text_emission",
            program: "bench/workloads/large/million_row_text_emission.pdl",
            dataset: "million-row",
            input_format: "csv",
            output_format: "jsonl",
            required_path: "bench/data/generated/million-row.csv",
            stdin_path: None,
        },
        // v0.45: reshape-dominated workload measuring the native
        // `pivot_longer` lowering (unpivot plus order-restoring sort)
        // against the row-runtime reshape.
        Workload {
            name: "million_row_pivot_longer",
            program: "bench/workloads/large/million_row_pivot_longer.pdl",
            dataset: "million-row",
            input_format: "csv",
            output_format: "csv",
            required_path: "bench/data/generated/million-row.csv",
            stdin_path: None,
        },
        // v0.45: join-dominated workload measuring the native `complete`
        // lowering (key-domain cross join plus fill projection) against
        // the row-runtime key expansion.
        Workload {
            name: "million_row_complete_buckets",
            program: "bench/workloads/large/million_row_complete_buckets.pdl",
            dataset: "million-row",
            input_format: "csv",
            output_format: "csv",
            required_path: "bench/data/generated/million-row.csv",
            stdin_path: None,
        },
        // v0.48: representative pipeline-shape workload. It uses a binding
        // start, two named outputs, and a non-terminal save fan-out.
        Workload {
            name: "million_row_multi_output_fanout",
            program: "bench/workloads/large/million_row_multi_output_fanout.pdl",
            dataset: "million-row",
            input_format: "csv",
            output_format: "multi-csv",
            required_path: "bench/data/generated/million-row.csv",
            stdin_path: None,
        },
    ]
}

struct RunContext<'a> {
    root: &'a Path,
    run_dir: &'a Path,
    tier: Tier,
    profile: BuildProfile,
    input_rows: usize,
    run_timestamp: &'a str,
    git_ref: &'a str,
    run_label: &'a str,
    engine: BenchmarkEngine,
    samples: usize,
    warmups: usize,
    cooldown_ms: u64,
    metadata: SystemMetadata,
}

#[derive(Clone)]
struct SystemMetadata {
    os: String,
    cpu_model: String,
    logical_cores: String,
    rustc: String,
    build_profile: String,
    git_dirty: String,
    feature_flags: String,
}

impl SystemMetadata {
    fn capture(root: &Path, profile: BuildProfile) -> Self {
        Self {
            os: command_output(root, "uname", &["-a"]).trim().to_string(),
            cpu_model: cpu_model(root),
            logical_cores: std::thread::available_parallelism()
                .map(|cores| cores.get().to_string())
                .unwrap_or_else(|_| "unknown".to_string()),
            rustc: command_output(root, "rustc", &["-V"]).trim().to_string(),
            build_profile: profile.dir().to_string(),
            git_dirty: (!command_output(root, "git", &["status", "--short"])
                .trim()
                .is_empty())
            .to_string(),
            feature_flags: "default".to_string(),
        }
    }
}

struct WorkloadRun {
    record: Vec<String>,
    failed: bool,
}

#[derive(Default)]
struct PlanFacts {
    selected_engine: String,
    eligible_engine: String,
    fallback_reason: String,
    sink_strategy: String,
    source_boundary: String,
    row_materialization: String,
    required_source_columns: String,
}

struct SampleOutcome {
    elapsed_ms: u128,
    output_bytes: u64,
    output_rows: String,
    peak_rss_bytes: Option<u64>,
    unsupported: bool,
    failed: bool,
    output_path: PathBuf,
    log_path: PathBuf,
}

fn run_workload(
    context: &RunContext<'_>,
    workload: &Workload,
) -> Result<WorkloadRun, Box<dyn std::error::Error>> {
    let root = context.root;
    let required_path = root.join(workload.required_path);
    if !required_path.exists() {
        return Err(format!("missing {}", relative(root, &required_path)).into());
    }
    let plan_facts = plan_facts(context, workload).unwrap_or_default();
    for warmup in 0..context.warmups {
        let _ = run_workload_once(context, workload, warmup, true)?;
    }

    let mut samples = Vec::new();
    for sample in 0..context.samples {
        if sample > 0 && context.cooldown_ms > 0 {
            thread::sleep(Duration::from_millis(context.cooldown_ms));
        }
        samples.push(run_workload_once(context, workload, sample, false)?);
    }

    let failed_samples = samples.iter().filter(|sample| sample.failed).count();
    let unsupported_samples = samples.iter().filter(|sample| sample.unsupported).count();
    let failed = failed_samples > 0;
    let status_text = if failed {
        "failed"
    } else if unsupported_samples == samples.len() {
        "unsupported"
    } else {
        "ok"
    };
    let measured = samples
        .iter()
        .filter(|sample| !sample.failed && !sample.unsupported)
        .map(|sample| sample.elapsed_ms)
        .collect::<Vec<_>>();
    let stats = sample_stats(&measured);
    let representative = samples
        .iter()
        .find(|sample| !sample.failed && !sample.unsupported)
        .or_else(|| samples.first())
        .expect("at least one sample");
    let peak_rss = samples
        .iter()
        .filter_map(|sample| sample.peak_rss_bytes)
        .max();
    let output_bytes = representative.output_bytes;
    let output_rows = representative.output_rows.clone();
    let notes = if unsupported_samples > 0 {
        "native unsupported"
    } else {
        context.engine.as_str()
    };
    let stdin_redirect = workload
        .stdin_path
        .map(|stdin_path| format!(" < {stdin_path}"))
        .unwrap_or_default();
    let stdout_format_arg = workload
        .stdout_format()
        .map(|format| format!(" --stdout-format {format}"))
        .unwrap_or_default();
    let command_text = format!(
        "{} run {}{} --engine {}{}",
        relative(root, &pdl_bin(context)),
        workload.program,
        stdout_format_arg,
        context.engine.as_str(),
        stdin_redirect
    );

    let mut record = base_record(
        context,
        workload,
        BaseRecordFields {
            status: status_text,
            command: command_text,
            output_path: relative(root, &representative.output_path),
            log_path: relative(root, &representative.log_path),
            output_rows,
            output_bytes,
            elapsed_ms: stats.median_ms,
            notes,
        },
    );
    append_v0_36_fields(
        &mut record,
        context,
        &stats,
        failed_samples,
        unsupported_samples,
        peak_rss,
        &plan_facts,
    );
    Ok(WorkloadRun { record, failed })
}

fn failure_record(context: &RunContext<'_>, workload: &Workload, notes: &str) -> Vec<String> {
    let log_path = context.run_dir.join(format!(
        "{}-{}-{}.log",
        workload.name,
        context.engine.as_str(),
        workload.output_format
    ));
    let stats = SampleStats::default();
    let plan_facts = PlanFacts::default();
    let mut record = base_record(
        context,
        workload,
        BaseRecordFields {
            status: "failed",
            command: String::new(),
            output_path: String::new(),
            log_path: relative(context.root, &log_path),
            output_rows: String::new(),
            output_bytes: 0,
            elapsed_ms: 0,
            notes,
        },
    );
    append_v0_36_fields(&mut record, context, &stats, 1, 0, None, &plan_facts);
    record
}

fn run_workload_once(
    context: &RunContext<'_>,
    workload: &Workload,
    index: usize,
    warmup: bool,
) -> Result<SampleOutcome, Box<dyn std::error::Error>> {
    let root = context.root;
    let ext = extension_for_format(workload.output_format);
    let suffix = if context.samples + context.warmups > 1 {
        if warmup {
            format!("-warmup{:02}", index + 1)
        } else {
            format!("-sample{:02}", index + 1)
        }
    } else {
        String::new()
    };
    let output_path = context.run_dir.join(format!(
        "{}-{}-{}{}.{}",
        workload.name,
        context.engine.as_str(),
        workload.output_format,
        suffix,
        ext
    ));
    let log_path = context.run_dir.join(format!(
        "{}-{}-{}{}.log",
        workload.name,
        context.engine.as_str(),
        workload.output_format,
        suffix
    ));
    let stdout = File::create(&output_path)?;
    let stderr = File::create(&log_path)?;
    let start = Instant::now();
    let status = run_pdl_command(context, workload, stdout, stderr)?;
    let elapsed_ms = start.elapsed().as_millis();
    let output_bytes = byte_count(&output_path);
    let output_rows = if workload.output_format == "csv" && status.success() {
        csv_data_rows(&output_path)
            .map(|rows| rows.to_string())
            .unwrap_or_default()
    } else {
        String::new()
    };
    let log_text = fs::read_to_string(&log_path).unwrap_or_default();
    let unsupported =
        matches!(context.engine, BenchmarkEngine::Native) && log_text.contains("E1211");
    let failed = !status.success() && !unsupported;
    Ok(SampleOutcome {
        elapsed_ms,
        output_bytes,
        output_rows,
        peak_rss_bytes: parse_peak_rss_bytes(&log_text),
        unsupported,
        failed,
        output_path: output_path
            .strip_prefix(root)
            .unwrap_or(&output_path)
            .to_path_buf(),
        log_path: log_path
            .strip_prefix(root)
            .unwrap_or(&log_path)
            .to_path_buf(),
    })
}

fn run_pdl_command(
    context: &RunContext<'_>,
    workload: &Workload,
    stdout: File,
    stderr: File,
) -> Result<ExitStatus, Box<dyn std::error::Error>> {
    let bin = pdl_bin(context);
    let mut command = if let Some(time_args) = time_args() {
        let mut command = Command::new("/usr/bin/time");
        command.args(time_args).arg(&bin);
        command
    } else {
        Command::new(&bin)
    };
    command
        .current_dir(context.root)
        .arg("run")
        .arg(workload.program);
    if let Some(stdout_format) = workload.stdout_format() {
        command.arg("--stdout-format").arg(stdout_format);
    }
    command
        .arg("--engine")
        .arg(context.engine.as_str())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));
    if let Some(stdin_path) = workload.stdin_path {
        command.stdin(Stdio::from(File::open(context.root.join(stdin_path))?));
    }
    Ok(command.status()?)
}

fn time_args() -> Option<&'static [&'static str]> {
    let time = Path::new("/usr/bin/time");
    if !time.exists() {
        return None;
    }
    if cfg!(target_os = "macos") || cfg!(target_os = "freebsd") {
        Some(&["-l"])
    } else if cfg!(target_os = "linux") {
        Some(&["-v"])
    } else {
        None
    }
}

fn parse_peak_rss_bytes(log_text: &str) -> Option<u64> {
    for line in log_text.lines() {
        let trimmed = line.trim();
        if trimmed.contains("maximum resident set size") {
            let value = trimmed
                .split_whitespace()
                .find_map(|part| part.parse::<u64>().ok())?;
            return Some(
                if cfg!(target_os = "macos") || cfg!(target_os = "freebsd") {
                    value
                } else {
                    value.saturating_mul(1024)
                },
            );
        }
        if let Some(value) = trimmed.strip_prefix("Maximum resident set size (kbytes):") {
            return value
                .trim()
                .parse::<u64>()
                .ok()
                .map(|kb| kb.saturating_mul(1024));
        }
    }
    None
}

fn pdl_bin(context: &RunContext<'_>) -> PathBuf {
    context
        .root
        .join("target")
        .join(context.profile.dir())
        .join("pdl")
}

fn plan_facts(
    context: &RunContext<'_>,
    workload: &Workload,
) -> Result<PlanFacts, Box<dyn std::error::Error>> {
    let mut command = Command::new(pdl_bin(context));
    command
        .current_dir(context.root)
        .arg("plan")
        .arg(workload.program);
    if let Some(stdout_format) = workload.stdout_format() {
        command.arg("--stdout-format").arg(stdout_format);
    }
    command
        .arg("--engine")
        .arg(context.engine.as_str())
        .arg("--json");
    if let Some(stdin_path) = workload.stdin_path {
        command.stdin(Stdio::from(File::open(context.root.join(stdin_path))?));
    } else {
        command.stdin(Stdio::null());
    }
    let output = command.output()?;
    if !output.status.success() {
        return Ok(PlanFacts::default());
    }
    let value: JsonValue = serde_json::from_slice(&output.stdout)?;
    let observability = &value["execution"]["observability"];
    Ok(PlanFacts {
        selected_engine: json_string(observability, "selected_engine"),
        eligible_engine: json_string(observability, "eligible_engine"),
        fallback_reason: json_string(observability, "fallback_reason"),
        sink_strategy: json_string(observability, "sink_strategy"),
        source_boundary: json_string(observability, "source_boundary"),
        row_materialization: observability["row_materialization"]
            .as_bool()
            .map(|value| value.to_string())
            .unwrap_or_default(),
        required_source_columns: observability["required_source_columns"]
            .as_array()
            .map(|columns| {
                columns
                    .iter()
                    .filter_map(|column| column.as_str())
                    .collect::<Vec<_>>()
                    .join("|")
            })
            .unwrap_or_default(),
    })
}

fn json_string(value: &JsonValue, key: &str) -> String {
    value[key].as_str().unwrap_or_default().to_string()
}

#[derive(Default)]
struct SampleStats {
    min_ms: u128,
    median_ms: u128,
    p90_ms: u128,
    max_ms: u128,
    stddev_ms: f64,
    count: usize,
}

fn sample_stats(values: &[u128]) -> SampleStats {
    if values.is_empty() {
        return SampleStats::default();
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let count = sorted.len();
    let mean = sorted.iter().map(|value| *value as f64).sum::<f64>() / count as f64;
    let variance = sorted
        .iter()
        .map(|value| {
            let delta = *value as f64 - mean;
            delta * delta
        })
        .sum::<f64>()
        / count as f64;
    SampleStats {
        min_ms: sorted[0],
        median_ms: sorted[count / 2],
        p90_ms: sorted[((count - 1) * 90).div_ceil(100)],
        max_ms: sorted[count - 1],
        stddev_ms: variance.sqrt(),
        count,
    }
}

struct BaseRecordFields<'a> {
    status: &'a str,
    command: String,
    output_path: String,
    log_path: String,
    output_rows: String,
    output_bytes: u64,
    elapsed_ms: u128,
    notes: &'a str,
}

fn base_record(
    context: &RunContext<'_>,
    workload: &Workload,
    fields: BaseRecordFields<'_>,
) -> Vec<String> {
    vec![
        "pdl".to_string(),
        "pdl-bench".to_string(),
        context.run_label.to_string(),
        "large".to_string(),
        workload.name.to_string(),
        workload.dataset.to_string(),
        context.tier.as_str().to_string(),
        workload.input_format.to_string(),
        workload.output_format.to_string(),
        context.engine.as_str().to_string(),
        fields.status.to_string(),
        fields.command,
        fields.output_path,
        fields.log_path,
        context.input_rows.to_string(),
        fields.output_rows,
        String::new(),
        fields.output_bytes.to_string(),
        fields.elapsed_ms.to_string(),
        context.run_timestamp.to_string(),
        context.git_ref.to_string(),
        fields.notes.to_string(),
    ]
}

fn append_v0_36_fields(
    record: &mut Vec<String>,
    context: &RunContext<'_>,
    stats: &SampleStats,
    failed_samples: usize,
    unsupported_samples: usize,
    peak_rss_bytes: Option<u64>,
    plan_facts: &PlanFacts,
) {
    record.extend([
        stats.count.to_string(),
        context.warmups.to_string(),
        stats.min_ms.to_string(),
        stats.median_ms.to_string(),
        stats.p90_ms.to_string(),
        stats.max_ms.to_string(),
        format!("{:.3}", stats.stddev_ms),
        failed_samples.to_string(),
        unsupported_samples.to_string(),
        peak_rss_bytes
            .map(|value| value.to_string())
            .unwrap_or_default(),
        plan_facts.row_materialization.clone(),
        plan_facts.selected_engine.clone(),
        plan_facts.eligible_engine.clone(),
        plan_facts.fallback_reason.clone(),
        plan_facts.sink_strategy.clone(),
        plan_facts.source_boundary.clone(),
        plan_facts.required_source_columns.clone(),
        String::new(),
        String::new(),
        String::new(),
        String::new(),
        context.metadata.os.clone(),
        context.metadata.cpu_model.clone(),
        context.metadata.logical_cores.clone(),
        context.metadata.rustc.clone(),
        context.metadata.build_profile.clone(),
        context.metadata.git_dirty.clone(),
        context.metadata.feature_flags.clone(),
    ]);
}

fn generate_all(root: &Path, rows: usize) -> Result<(), Box<dyn std::error::Error>> {
    prepare_sources(root, rows)?;
    Ok(())
}

fn prepare_sources(root: &Path, rows: usize) -> Result<(), Box<dyn std::error::Error>> {
    generate_million_row_csv(root, rows)?;
    generate_partitioned_million_row_csv(root, rows)?;
    generate_million_row_narrow_csv(root, rows)?;
    generate_million_row_events(root, rows)?;
    generate_segment_dimension(root)?;
    let batch = million_row_batch(rows)?;
    write_parquet(
        root,
        &batch,
        &root.join("bench/data/generated/million-row.parquet"),
    )?;
    write_arrow_stream(
        root,
        &batch,
        &root.join("bench/data/generated/million-row.arrow"),
    )?;
    Ok(())
}

fn generate_million_row_csv(root: &Path, rows: usize) -> Result<(), Box<dyn std::error::Error>> {
    if rows == 0 {
        return Err("--rows must be greater than zero".into());
    }
    let out = root.join("bench/data/generated/million-row.csv");
    write_million_row_csv(root, &out, 0, rows)
}

fn generate_partitioned_million_row_csv(
    root: &Path,
    rows: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    if rows == 0 {
        return Err("--rows must be greater than zero".into());
    }
    let first = rows / 2;
    let second = rows - first;
    write_million_row_csv(
        root,
        &root.join("bench/data/generated/million-row-part-a.csv"),
        0,
        first,
    )?;
    write_million_row_csv(
        root,
        &root.join("bench/data/generated/million-row-part-b.csv"),
        first,
        second,
    )
}

fn generate_million_row_narrow_csv(
    root: &Path,
    rows: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    if rows == 0 {
        return Err("--rows must be greater than zero".into());
    }
    let out = root.join("bench/data/generated/million-row-narrow.csv");
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = out.with_extension("csv.tmp");
    let mut writer = BufWriter::new(File::create(&tmp)?);
    writeln!(writer, "row,segment,x,score")?;
    for row in 0..rows {
        let segment_index = row % 4;
        let segment = ["A", "B", "C", "D"][segment_index];
        let x = (row % 10_000) as f64 / 100.0;
        let cycle = (row * 37) % 1_000;
        let drift = row / 100_000;
        let score =
            20.0 + (segment_index as f64 * 5.0) + (x * 0.3) + (cycle as f64 / 25.0) + drift as f64;
        writeln!(writer, "{row},{segment},{x:.2},{score:.3}")?;
    }
    writer.flush()?;
    fs::rename(&tmp, out.as_path())?;
    println!("generated {} rows at {}", rows, relative(root, &out));
    Ok(())
}

fn generate_million_row_events(root: &Path, rows: usize) -> Result<(), Box<dyn std::error::Error>> {
    if rows == 0 {
        return Err("--rows must be greater than zero".into());
    }
    let csv_out = root.join("bench/data/generated/million-row-events.csv");
    let jsonl_out = root.join("bench/data/generated/million-row-events.jsonl");
    if let Some(parent) = csv_out.parent() {
        fs::create_dir_all(parent)?;
    }
    let csv_tmp = csv_out.with_extension("csv.tmp");
    let jsonl_tmp = jsonl_out.with_extension("jsonl.tmp");
    let mut csv_writer = BufWriter::new(File::create(&csv_tmp)?);
    let mut jsonl_writer = BufWriter::new(File::create(&jsonl_tmp)?);
    writeln!(
        csv_writer,
        "row,segment,ordered_at,amount,metric_column,gross_amount,discount,label,pattern,replacement,flag,offset"
    )?;
    for row in 0..rows {
        let segment_index = row % 4;
        let segment = ["A", "B", "C", "D"][segment_index];
        let month = (row % 12) + 1;
        let day = (row % 28) + 1;
        let ordered_at = format!("2026-{month:02}-{day:02}T12:34:56Z");
        let amount = 50.0 + ((row * 29) % 5_000) as f64 / 10.0;
        let gross_amount = amount + 10.0;
        let discount = if row % 5 == 0 {
            0.0
        } else {
            ((row * 7) % 300) as f64 / 10.0
        };
        let metric_column = if row % 2 == 0 {
            "gross_amount"
        } else {
            "discount"
        };
        let label = format!("{segment}-channel-{}", row % 10);
        let pattern = if row % 2 == 0 { "-" } else { "channel" };
        let replacement = if row % 2 == 0 { ":" } else { "route" };
        let flag = row % 3 == 0;
        let offset = (row % 3) + 1;
        writeln!(
            csv_writer,
            "{row},{segment},{ordered_at},{amount:.2},{metric_column},{gross_amount:.2},{discount:.2},{label},{pattern},{replacement},{flag},{offset}"
        )?;
        writeln!(
            jsonl_writer,
            "{{\"row\":{row},\"segment\":\"{segment}\",\"ordered_at\":\"{ordered_at}\",\"amount\":{amount:.2},\"metric_column\":\"{metric_column}\",\"gross_amount\":{gross_amount:.2},\"discount\":{discount:.2},\"label\":\"{label}\",\"pattern\":\"{pattern}\",\"replacement\":\"{replacement}\",\"flag\":{flag},\"offset\":{offset}}}"
        )?;
    }
    csv_writer.flush()?;
    jsonl_writer.flush()?;
    fs::rename(&csv_tmp, csv_out.as_path())?;
    fs::rename(&jsonl_tmp, jsonl_out.as_path())?;
    println!("generated {} rows at {}", rows, relative(root, &csv_out));
    println!("generated {} rows at {}", rows, relative(root, &jsonl_out));
    Ok(())
}

fn generate_segment_dimension(root: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let out = root.join("bench/data/generated/segment-dimension.csv");
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = out.with_extension("csv.tmp");
    let mut writer = BufWriter::new(File::create(&tmp)?);
    writeln!(writer, "segment,tier")?;
    writeln!(writer, "A,core")?;
    writeln!(writer, "B,core")?;
    writeln!(writer, "C,growth")?;
    writeln!(writer, "D,growth")?;
    writer.flush()?;
    fs::rename(&tmp, out.as_path())?;
    println!("generated {}", relative(root, &out));
    Ok(())
}

fn write_million_row_csv(
    root: &Path,
    out: &Path,
    start_row: usize,
    rows: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = out.with_extension("csv.tmp");
    let mut writer = BufWriter::new(File::create(&tmp)?);
    writeln!(writer, "row,segment,x,score,latency_ms")?;
    for row in start_row..start_row + rows {
        let segment_index = row % 4;
        let segment = ["A", "B", "C", "D"][segment_index];
        let x = (row % 10_000) as f64 / 100.0;
        let cycle = (row * 37) % 1_000;
        let drift = row / 100_000;
        let score =
            20.0 + (segment_index as f64 * 5.0) + (x * 0.3) + (cycle as f64 / 25.0) + drift as f64;
        let latency = 40.0 + (segment_index as f64 * 12.0) + (((row * 17) % 900) as f64 / 3.0);
        writeln!(writer, "{row},{segment},{x:.2},{score:.3},{latency:.3}")?;
    }
    writer.flush()?;
    fs::rename(&tmp, out)?;
    println!("generated {} rows at {}", rows, relative(root, out));
    Ok(())
}

fn million_row_batch(rows: usize) -> Result<RecordBatch, Box<dyn std::error::Error>> {
    if rows == 0 {
        return Err("--rows must be greater than zero".into());
    }
    let mut row_values = Vec::with_capacity(rows);
    let mut segments = Vec::with_capacity(rows);
    let mut xs = Vec::with_capacity(rows);
    let mut scores = Vec::with_capacity(rows);
    let mut latencies = Vec::with_capacity(rows);
    for row in 0..rows {
        let segment_index = row % 4;
        let segment = ["A", "B", "C", "D"][segment_index];
        let x = (row % 10_000) as f64 / 100.0;
        let cycle = (row * 37) % 1_000;
        let drift = row / 100_000;
        let score =
            20.0 + (segment_index as f64 * 5.0) + (x * 0.3) + (cycle as f64 / 25.0) + drift as f64;
        let latency = 40.0 + (segment_index as f64 * 12.0) + (((row * 17) % 900) as f64 / 3.0);
        row_values.push(row as i64);
        segments.push(Some(segment));
        xs.push(x);
        scores.push(score);
        latencies.push(latency);
    }
    let schema = Arc::new(Schema::new(vec![
        Field::new("row", ArrowDataType::Int64, false),
        Field::new("segment", ArrowDataType::Utf8, false),
        Field::new("x", ArrowDataType::Float64, false),
        Field::new("score", ArrowDataType::Float64, false),
        Field::new("latency_ms", ArrowDataType::Float64, false),
    ]));
    Ok(RecordBatch::try_new(
        schema,
        vec![
            Arc::new(Int64Array::from(row_values)) as ArrayRef,
            Arc::new(StringArray::from_iter(segments)) as ArrayRef,
            Arc::new(Float64Array::from(xs)) as ArrayRef,
            Arc::new(Float64Array::from(scores)) as ArrayRef,
            Arc::new(Float64Array::from(latencies)) as ArrayRef,
        ],
    )?)
}

fn write_parquet(
    root: &Path,
    batch: &RecordBatch,
    path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = File::create(path)?;
    let mut writer = ArrowWriter::try_new(file, batch.schema(), None)?;
    writer.write(batch)?;
    writer.close()?;
    println!("generated {}", relative(root, path));
    Ok(())
}

fn write_arrow_stream(
    root: &Path,
    batch: &RecordBatch,
    path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = File::create(path)?;
    let mut writer = StreamWriter::try_new(&mut file, batch.schema_ref())?;
    writer.write(batch)?;
    writer.finish()?;
    println!("generated {}", relative(root, path));
    Ok(())
}

fn download_external(
    root: &Path,
    dataset: ExternalDataset,
    force: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if matches!(dataset, ExternalDataset::All | ExternalDataset::Tlc) {
        download_one(
            root,
            "NYC TLC January 2024 trips",
            &std::env::var("PDL_TLC_URL").unwrap_or_else(|_| TLC_URL.to_string()),
            &root.join("bench/data/raw/tlc/yellow_tripdata_2024-01.parquet"),
            force,
        )?;
        download_one(
            root,
            "NYC TLC taxi zones",
            &std::env::var("PDL_TLC_ZONES_URL").unwrap_or_else(|_| TLC_ZONES_URL.to_string()),
            &root.join("bench/data/raw/tlc/taxi_zone_lookup.csv"),
            force,
        )?;
    }
    if matches!(dataset, ExternalDataset::All | ExternalDataset::Sfo) {
        download_one(
            root,
            "SFO Museum March 2026 flights",
            &std::env::var("PDL_SFO_URL").unwrap_or_else(|_| SFO_URL.to_string()),
            &root.join("bench/data/raw/sfo/sfomuseum-data-flights-2026-03.parquet"),
            force,
        )?;
    }
    Ok(())
}

fn download_one(
    root: &Path,
    label: &str,
    url: &str,
    out: &Path,
    force: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if out.exists() && !force {
        println!("kept existing {} at {}", label, relative(root, out));
        return Ok(());
    }
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = out.with_extension("download");
    println!("downloading {} -> {}", label, relative(root, out));
    let status = Command::new("curl")
        .args(["-L", "--fail", "--show-error", "--output"])
        .arg(&tmp)
        .arg(url)
        .status()?;
    if !status.success() {
        let _ = fs::remove_file(&tmp);
        return Err(format!("download failed for {label}").into());
    }
    fs::rename(tmp, out)?;
    Ok(())
}

fn build_cli(root: &Path, profile: BuildProfile) -> Result<(), Box<dyn std::error::Error>> {
    let mut command = Command::new("cargo");
    command
        .current_dir(root)
        .arg("build")
        .arg("-p")
        .arg("pdl-cli");
    if matches!(profile, BuildProfile::Release) {
        command.arg("--release");
    }
    let status = command.status()?;
    if !status.success() {
        return Err("cargo build -p pdl-cli failed".into());
    }
    Ok(())
}

fn csv_data_rows(path: &Path) -> Option<usize> {
    let file = File::open(path).ok()?;
    let reader = BufReader::new(file);
    let lines = reader.lines().map_while(Result::ok).count();
    Some(lines.saturating_sub(1))
}

fn byte_count(path: &Path) -> u64 {
    fs::metadata(path)
        .map(|metadata| metadata.len())
        .unwrap_or(0)
}

fn extension_for_format(format: &str) -> &'static str {
    match format {
        "csv" => "csv",
        "jsonl" => "jsonl",
        "arrow-stream" | "arrow-file" => "arrow",
        "parquet" => "parquet",
        _ => "out",
    }
}

fn git_ref(root: &Path) -> String {
    Command::new("git")
        .current_dir(root)
        .args(["describe", "--tags", "--always", "--dirty"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

fn command_output(root: &Path, program: &str, args: &[&str]) -> String {
    Command::new(program)
        .current_dir(root)
        .args(args)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .unwrap_or_else(|| "unknown".to_string())
}

fn cpu_model(root: &Path) -> String {
    if cfg!(target_os = "macos") {
        let model = command_output(root, "sysctl", &["-n", "machdep.cpu.brand_string"]);
        let model = model.trim();
        if !model.is_empty() && model != "unknown" {
            return model.to_string();
        }
    }
    if cfg!(target_os = "linux") {
        let cpuinfo = fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
        if let Some(model) = cpuinfo.lines().find_map(|line| {
            line.strip_prefix("model name")
                .and_then(|value| value.split_once(':').map(|(_, model)| model.trim()))
        }) {
            return model.to_string();
        }
    }
    "unknown".to_string()
}

fn shuffle_workloads(workloads: &mut [&Workload], seed_bytes: &[u8]) {
    let mut seed = seed_bytes.iter().fold(0xcbf29ce484222325_u64, |acc, byte| {
        acc.wrapping_mul(0x100000001b3)
            .wrapping_add(u64::from(*byte))
    });
    for index in (1..workloads.len()).rev() {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let swap_with = (seed as usize) % (index + 1);
        workloads.swap(index, swap_with);
    }
}

fn sanitize_label(label: &str) -> String {
    let sanitized: String = label
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '-'
            }
        })
        .collect();
    if sanitized.is_empty() {
        "run".to_string()
    } else {
        sanitized
    }
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}
