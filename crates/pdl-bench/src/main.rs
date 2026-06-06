use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Instant;

use arrow_array::{ArrayRef, Float64Array, Int64Array, RecordBatch, StringArray};
use arrow_ipc::writer::StreamWriter;
use arrow_schema::{DataType as ArrowDataType, Field, Schema};
use chrono::Utc;
use clap::{Parser, Subcommand, ValueEnum};
use parquet::arrow::ArrowWriter;

const TLC_URL: &str =
    "https://d37ci6vzurychx.cloudfront.net/trip-data/yellow_tripdata_2024-01.parquet";
const TLC_ZONES_URL: &str = "https://d37ci6vzurychx.cloudfront.net/misc/taxi_zone_lookup.csv";
const SFO_URL: &str = "https://static.sfomuseum.org/parquet/sfomuseum-data-flights-2026-03.parquet";

const REPORT_HEADER: [&str; 22] = [
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
    },
    /// Compare a run report with a baseline report.
    Compare {
        #[arg(long)]
        baseline: String,
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
        } => {
            run_suite(&root, suite, tier, run_label, profile, engine, no_generate)?;
        }
        Commands::Compare {
            baseline,
            run_label,
        } => {
            compare_run(&root, &baseline, &run_label)?;
        }
        Commands::Snapshot {
            run_label,
            baseline,
        } => {
            snapshot_run(&root, &run_label, &baseline)?;
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

fn run_suite(
    root: &Path,
    suite: Suite,
    tier: Tier,
    run_label: Option<String>,
    profile: BuildProfile,
    engine: BenchmarkEngine,
    no_generate: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if !matches!(suite, Suite::Large) {
        unreachable!("clap only exposes known suite values");
    }

    let required = [
        root.join("bench/data/generated/million-row.csv"),
        root.join("bench/data/generated/million-row.parquet"),
        root.join("bench/data/generated/million-row.arrow"),
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
    };

    let workloads = large_workloads();
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
    for row in run_rows {
        let Some(baseline) = baseline_by_key.get(&row.key_without_engine()) else {
            println!(
                "{:<43} {:<18} {:<7} {:<11} {:>10} {:>10} {:>11}",
                row.workload,
                row.format_label(),
                row.engine,
                row.status,
                "-",
                row.elapsed_ms.to_string(),
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
                baseline.elapsed_ms,
                row.elapsed_ms,
                "-"
            );
            continue;
        }
        let improvement = if baseline.elapsed_ms > 0 {
            (baseline.elapsed_ms as f64 - row.elapsed_ms as f64) * 100.0
                / baseline.elapsed_ms as f64
        } else {
            0.0
        };
        println!(
            "{:<43} {:<18} {:<7} {:<11} {:>10} {:>10} {:+10.1}%",
            row.workload,
            row.format_label(),
            row.engine,
            row.status,
            baseline.elapsed_ms,
            row.elapsed_ms,
            improvement
        );
    }
    Ok(())
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
        });
    }
    Ok(rows)
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
        },
        Workload {
            name: "million_row_segment_summary_parquet",
            program: "bench/workloads/large/million_row_segment_summary_parquet.pdl",
            dataset: "million-row",
            input_format: "parquet",
            output_format: "csv",
            required_path: "bench/data/generated/million-row.parquet",
        },
        Workload {
            name: "million_row_segment_summary_arrow_stream",
            program: "bench/workloads/large/million_row_segment_summary_arrow_stream.pdl",
            dataset: "million-row",
            input_format: "arrow-stream",
            output_format: "csv",
            required_path: "bench/data/generated/million-row.arrow",
        },
        Workload {
            name: "million_row_segment_summary",
            program: "bench/workloads/large/million_row_segment_summary.pdl",
            dataset: "million-row",
            input_format: "csv",
            output_format: "arrow-stream",
            required_path: "bench/data/generated/million-row.csv",
        },
        Workload {
            name: "million_row_top_scores",
            program: "bench/workloads/large/million_row_top_scores.pdl",
            dataset: "million-row",
            input_format: "csv",
            output_format: "csv",
            required_path: "bench/data/generated/million-row.csv",
        },
        Workload {
            name: "million_row_projection_smoke",
            program: "bench/workloads/large/million_row_projection_smoke.pdl",
            dataset: "million-row",
            input_format: "csv",
            output_format: "csv",
            required_path: "bench/data/generated/million-row.csv",
        },
        Workload {
            name: "million_row_distinct_segments",
            program: "bench/workloads/large/million_row_distinct_segments.pdl",
            dataset: "million-row",
            input_format: "csv",
            output_format: "csv",
            required_path: "bench/data/generated/million-row.csv",
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
}

struct WorkloadRun {
    record: Vec<String>,
    failed: bool,
}

fn run_workload(
    context: &RunContext<'_>,
    workload: &Workload,
) -> Result<WorkloadRun, Box<dyn std::error::Error>> {
    let root = context.root;
    let run_dir = context.run_dir;
    let required_path = root.join(workload.required_path);
    if !required_path.exists() {
        return Err(format!("missing {}", relative(root, &required_path)).into());
    }
    let ext = extension_for_format(workload.output_format);
    let output_name = format!(
        "{}-{}-{}.{}",
        workload.name,
        context.engine.as_str(),
        workload.output_format,
        ext
    );
    let log_name = format!(
        "{}-{}-{}.log",
        workload.name,
        context.engine.as_str(),
        workload.output_format
    );
    let output_path = run_dir.join(output_name);
    let log_path = run_dir.join(log_name);
    let bin = root.join("target").join(context.profile.dir()).join("pdl");
    let command_text = format!(
        "{} run {} --stdout-format {} --engine {}",
        relative(root, &bin),
        workload.program,
        workload.output_format,
        context.engine.as_str()
    );

    let stdout = File::create(&output_path)?;
    let stderr = File::create(&log_path)?;
    let start = Instant::now();
    let status = Command::new(&bin)
        .current_dir(root)
        .arg("run")
        .arg(workload.program)
        .arg("--stdout-format")
        .arg(workload.output_format)
        .arg("--engine")
        .arg(context.engine.as_str())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .status()?;
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
    let unsupported_native =
        matches!(context.engine, BenchmarkEngine::Native) && log_text.contains("E1211");
    let status_text = if status.success() {
        "ok"
    } else if unsupported_native {
        "unsupported"
    } else {
        "failed"
    };
    let notes = if unsupported_native {
        "native unsupported"
    } else {
        context.engine.as_str()
    };
    let failed = !status.success() && !unsupported_native;

    Ok(WorkloadRun {
        record: vec![
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
            status_text.to_string(),
            command_text,
            relative(root, &output_path),
            relative(root, &log_path),
            context.input_rows.to_string(),
            output_rows,
            String::new(),
            output_bytes.to_string(),
            elapsed_ms.to_string(),
            context.run_timestamp.to_string(),
            context.git_ref.to_string(),
            notes.to_string(),
        ],
        failed,
    })
}

fn failure_record(context: &RunContext<'_>, workload: &Workload, notes: &str) -> Vec<String> {
    let log_path = context.run_dir.join(format!(
        "{}-{}-{}.log",
        workload.name,
        context.engine.as_str(),
        workload.output_format
    ));
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
        "failed".to_string(),
        String::new(),
        String::new(),
        relative(context.root, &log_path),
        String::new(),
        String::new(),
        String::new(),
        "0".to_string(),
        "0".to_string(),
        context.run_timestamp.to_string(),
        context.git_ref.to_string(),
        notes.to_string(),
    ]
}

fn generate_all(root: &Path, rows: usize) -> Result<(), Box<dyn std::error::Error>> {
    prepare_sources(root, rows)?;
    Ok(())
}

fn prepare_sources(root: &Path, rows: usize) -> Result<(), Box<dyn std::error::Error>> {
    generate_million_row_csv(root, rows)?;
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
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = out.with_extension("csv.tmp");
    let mut writer = BufWriter::new(File::create(&tmp)?);
    writeln!(writer, "row,segment,x,score,latency_ms")?;
    for row in 0..rows {
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
    fs::rename(&tmp, &out)?;
    println!("generated {} rows at {}", rows, relative(root, &out));
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
