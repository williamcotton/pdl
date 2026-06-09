// Shared harness helpers for the parity integration tests. Each integration
// test binary includes this module, so not every item is used from every
// binary.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use pdl_data::{DataFormat, Table};

/// Root of the PDL workspace (the directory holding `Cargo.toml` and
/// `examples/`).
pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub fn examples_dir() -> PathBuf {
    workspace_root().join("examples")
}

pub fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

/// Every `examples/*.pdl` source, sorted for deterministic test order.
pub fn example_sources() -> Vec<PathBuf> {
    let mut sources = std::fs::read_dir(examples_dir())
        .expect("read examples directory")
        .map(|entry| entry.expect("read examples entry").path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "pdl"))
        .collect::<Vec<_>>();
    sources.sort();
    assert!(!sources.is_empty(), "no examples found in examples/");
    sources
}

pub fn example_name(source: &Path) -> String {
    source
        .file_stem()
        .expect("example file stem")
        .to_str()
        .expect("utf-8 example name")
        .to_string()
}

/// Per-example run configuration: the stdin fixture format (if the example
/// loads from stdin) and the `--stdout-format` to request (if the example
/// streams its result to stdout instead of saving files).
pub struct ExampleConfig {
    pub stdin_format: Option<&'static str>,
    pub stdout_format: Option<&'static str>,
}

pub fn example_config(name: &str) -> ExampleConfig {
    match name {
        "stdin_orders_csv" => ExampleConfig {
            stdin_format: Some("csv"),
            stdout_format: Some("csv"),
        },
        // The program itself declares `save stdout format "arrow-stream"`.
        "arrow_stream_passthrough" => ExampleConfig {
            stdin_format: Some("arrow-stream"),
            stdout_format: None,
        },
        "stdout_jsonl" => ExampleConfig {
            stdin_format: None,
            stdout_format: Some("jsonl"),
        },
        "stdout_arrow_file" => ExampleConfig {
            stdin_format: None,
            stdout_format: Some("arrow-file"),
        },
        "stdout_arrow_stream" => ExampleConfig {
            stdin_format: None,
            stdout_format: Some("arrow-stream"),
        },
        "stdout_parquet" => ExampleConfig {
            stdin_format: None,
            stdout_format: Some("parquet"),
        },
        // Named outputs save their own files; a shared stdout stream is a
        // planning error for multi-output programs.
        "reactive_trip_dashboard" => ExampleConfig {
            stdin_format: None,
            stdout_format: None,
        },
        _ => ExampleConfig {
            stdin_format: None,
            stdout_format: Some("csv"),
        },
    }
}

/// Stdin payload for an example, derived from the committed CSV fixture in
/// `fixtures/stdin/<example>.csv`. Arrow-stream payloads are encoded from the
/// CSV fixture at run time so the repository only carries text fixtures.
pub fn stdin_bytes(name: &str, format: &str) -> Vec<u8> {
    let fixture = fixtures_dir().join("stdin").join(format!("{name}.csv"));
    let csv_bytes = std::fs::read(&fixture).unwrap_or_else(|error| {
        panic!(
            "missing stdin fixture {} for example {name}: {error}",
            fixture.display()
        )
    });
    match format {
        "csv" => csv_bytes,
        "arrow-stream" => {
            let table = pdl_data::read_table_from_bytes(&fixture, DataFormat::Csv, &csv_bytes)
                .expect("decode stdin CSV fixture");
            pdl_data::write_table_to_bytes(DataFormat::ArrowStream, &table)
                .expect("encode arrow-stream stdin fixture")
        }
        other => panic!("unsupported stdin fixture format `{other}` for example {name}"),
    }
}

/// Path to the `pdl` binary, building `pdl-cli` on first use. The nested
/// `cargo build` is a freshness no-op when the harness runs inside
/// `cargo test --workspace`.
pub fn pdl_binary() -> &'static Path {
    static BINARY: OnceLock<PathBuf> = OnceLock::new();
    BINARY.get_or_init(|| {
        let output = Command::new(env!("CARGO"))
            .args([
                "build",
                "-p",
                "pdl-cli",
                "--message-format=json-render-diagnostics",
            ])
            .current_dir(workspace_root())
            .stderr(Stdio::inherit())
            .output()
            .expect("run cargo build -p pdl-cli");
        assert!(output.status.success(), "cargo build -p pdl-cli failed");
        let stdout = String::from_utf8(output.stdout).expect("cargo build json output");
        for line in stdout.lines() {
            let Ok(message) = serde_json::from_str::<serde_json::Value>(line) else {
                continue;
            };
            if message["reason"] == "compiler-artifact" && message["target"]["name"] == "pdl" {
                if let Some(path) = message["executable"].as_str() {
                    return PathBuf::from(path);
                }
            }
        }
        panic!("could not locate the pdl binary in cargo build output");
    })
}

/// Captured outputs of one `pdl run` invocation: stdout bytes plus every file
/// the run created (or rewrote) in its sandbox, keyed by file name. Saved
/// files and named-output files land here.
pub struct ExampleRun {
    pub stdout: Vec<u8>,
    pub saved: BTreeMap<String, Vec<u8>>,
}

/// Runs one example through `pdl run --engine <engine>` in an isolated copy
/// of `examples/` and captures stdout plus saved files.
pub fn run_example(source: &Path, engine: &str) -> ExampleRun {
    let name = example_name(source);
    let config = example_config(&name);
    let sandbox = create_sandbox(&name, engine);

    let before = snapshot_files(&sandbox);

    let mut command = Command::new(pdl_binary());
    command
        .current_dir(&sandbox)
        .arg("run")
        .arg(format!("{name}.pdl"))
        .args(["--engine", engine]);
    if let Some(format) = config.stdin_format {
        command.args(["--stdin-format", format]);
    }
    if let Some(format) = config.stdout_format {
        command.args(["--stdout-format", format]);
    }
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|error| panic!("spawn pdl run for {name} ({engine}): {error}"));
    if let Some(format) = config.stdin_format {
        child
            .stdin
            .as_mut()
            .expect("stdin pipe")
            .write_all(&stdin_bytes(&name, format))
            .expect("write stdin fixture");
    }
    let output = child.wait_with_output().expect("wait for pdl run");

    assert!(
        output.status.success(),
        "{name} ({engine}) failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "{name} ({engine}) wrote to stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let after = snapshot_files(&sandbox);
    let saved = after
        .into_iter()
        .filter(|(file, bytes)| before.get(file) != Some(bytes))
        .collect();

    std::fs::remove_dir_all(&sandbox).expect("clean sandbox");
    ExampleRun {
        stdout: output.stdout,
        saved,
    }
}

fn create_sandbox(name: &str, engine: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock after epoch")
        .as_nanos();
    let sandbox = std::env::temp_dir().join(format!(
        "pdl-parity-{name}-{engine}-{}-{nonce}",
        std::process::id()
    ));
    std::fs::create_dir_all(&sandbox).expect("create sandbox");
    for entry in std::fs::read_dir(examples_dir()).expect("read examples directory") {
        let path = entry.expect("read examples entry").path();
        if path.is_file() {
            let file_name = path.file_name().expect("example file name");
            std::fs::copy(&path, sandbox.join(file_name)).expect("copy example fixture");
        }
    }
    sandbox
}

fn snapshot_files(directory: &Path) -> BTreeMap<String, Vec<u8>> {
    std::fs::read_dir(directory)
        .expect("read sandbox")
        .map(|entry| entry.expect("read sandbox entry").path())
        .filter(|path| path.is_file())
        .map(|path| {
            let name = path
                .file_name()
                .expect("sandbox file name")
                .to_str()
                .expect("utf-8 sandbox file name")
                .to_string();
            let bytes = std::fs::read(&path).expect("read sandbox file");
            (name, bytes)
        })
        .collect()
}

/// Formats whose output bytes are produced by engine-specific direct writers
/// in v0.42/v0.43. The encodings are semantically equal but not yet
/// byte-identical between engines; byte unification is the v0.44
/// `native-sink-writer` work. These payloads are compared as decoded tables.
/// CSV and JSON Lines always go through the row writers and are compared as
/// bytes; the row engine is the byte spec.
pub fn binary_table_format(format: DataFormat) -> bool {
    matches!(
        format,
        DataFormat::ArrowFile | DataFormat::ArrowStream | DataFormat::Parquet
    )
}

fn decode_table(label: &str, format: DataFormat, bytes: &[u8]) -> Table {
    pdl_data::read_table_from_bytes(Path::new(label), format, bytes)
        .unwrap_or_else(|error| panic!("decode {label}: {error:?}"))
}

/// Asserts payload parity between the row-engine reference and another
/// engine's payload, honoring the byte-vs-decoded-table policy above.
pub fn assert_payload_parity(
    context: &str,
    format: Option<DataFormat>,
    reference: &[u8],
    candidate: &[u8],
) {
    let format = format.or_else(|| {
        (!reference.is_empty())
            .then(|| pdl_data::sniff_format_from_bytes(reference).ok())
            .flatten()
    });
    match format {
        Some(format) if binary_table_format(format) => {
            assert_eq!(
                decode_table(context, format, reference),
                decode_table(context, format, candidate),
                "{context}: decoded {} tables differ from the row engine",
                format.canonical_name()
            );
        }
        _ => {
            assert_eq!(
                reference,
                candidate,
                "{context}: output bytes differ from the row engine\nrow:\n{}\ncandidate:\n{}",
                String::from_utf8_lossy(reference),
                String::from_utf8_lossy(candidate)
            );
        }
    }
}

/// Compares a full run (stdout plus saved files) against the row-engine
/// reference run.
pub fn assert_run_parity(name: &str, engine: &str, reference: &ExampleRun, candidate: &ExampleRun) {
    let config = example_config(name);
    let stdout_format = config.stdout_format.and_then(DataFormat::from_name);
    assert_payload_parity(
        &format!("{name} ({engine}) stdout"),
        stdout_format,
        &reference.stdout,
        &candidate.stdout,
    );
    assert_eq!(
        reference.saved.keys().collect::<Vec<_>>(),
        candidate.saved.keys().collect::<Vec<_>>(),
        "{name} ({engine}): saved file set differs from the row engine"
    );
    for (file, reference_bytes) in &reference.saved {
        let format = Path::new(file)
            .extension()
            .and_then(|extension| extension.to_str())
            .and_then(saved_extension_format);
        assert_payload_parity(
            &format!("{name} ({engine}) saved file {file}"),
            format,
            reference_bytes,
            &candidate.saved[file],
        );
    }
}

fn saved_extension_format(extension: &str) -> Option<DataFormat> {
    match extension {
        "csv" => Some(DataFormat::Csv),
        "jsonl" | "ndjson" => Some(DataFormat::JsonLines),
        "parquet" => Some(DataFormat::Parquet),
        "arrow" => Some(DataFormat::ArrowFile),
        "arrows" => Some(DataFormat::ArrowStream),
        _ => None,
    }
}

/// Reads the committed `selected_engine` fixture for an example.
pub fn expected_selected_engine(name: &str) -> String {
    let fixture = fixtures_dir()
        .join("selected_engine")
        .join(format!("{name}.txt"));
    std::fs::read_to_string(&fixture)
        .unwrap_or_else(|error| {
            panic!(
                "missing selected_engine fixture {} for example {name}: {error}\n\
                 every examples/*.pdl needs a fixture recording its expected \
                 PlanObservability.selected_engine under --engine auto",
                fixture.display()
            )
        })
        .trim()
        .to_string()
}

/// Runs `pdl plan --json --engine auto` for an example and returns
/// `execution.observability`.
pub fn plan_observability(source: &Path) -> serde_json::Value {
    let name = example_name(source);
    let config = example_config(&name);
    let mut command = Command::new(pdl_binary());
    command
        .current_dir(workspace_root())
        .arg("plan")
        .arg(format!("examples/{name}.pdl"))
        .args(["--json", "--engine", "auto"]);
    if let Some(format) = config.stdin_format {
        command.args(["--stdin-format", format]);
    }
    let output = command.output().expect("run pdl plan");
    assert!(
        output.status.success(),
        "pdl plan {name} failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let plan: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("parse pdl plan json");
    plan["execution"]["observability"].clone()
}
