use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use pdl_data::{DataFormat, Row, Table, Value};

#[test]
fn top_regions_example_runs_to_csv_stdout() {
    assert_example_stdout(
        "examples/top_regions.pdl",
        "region,total_revenue,avg_age,orders\nWest,350,34,2\nNorth,200,31.5,2\nEast,90,28,1\n",
    );
}

#[test]
fn orders_cleaned_example_runs_to_csv_stdout() {
    assert_example_stdout(
        "examples/orders_cleaned.pdl",
        "order_id,region_channel,net_amount,priority\nA100,NORTH:web,100,standard\nA102,WEST:web,150,high\nA103,EAST:partner,90,standard\n",
    );
}

#[test]
fn order_region_summary_example_runs_to_csv_stdout() {
    assert_example_stdout(
        "examples/order_region_summary.pdl",
        "region_channel,orders,revenue\nWEST:web,1,150\nNORTH:web,1,100\nEAST:partner,1,90\n",
    );
}

#[test]
fn segment_revenue_example_runs_to_csv_stdout() {
    assert_example_stdout(
        "examples/segment_revenue.pdl",
        "segment,revenue,orders\nEnterprise,550,4\nSMB,90,1\nConsumer,50,1\n",
    );
}

#[test]
fn daily_orders_union_example_runs_to_csv_stdout() {
    assert_example_stdout(
        "examples/daily_orders_union.pdl",
        "order_id,region,amount\nA1,North,10\nA2,South,20\nA3,West,30\n",
    );
}

#[test]
fn customer_window_metrics_example_runs_to_csv_stdout() {
    assert_example_stdout(
        "examples/customer_window_metrics.pdl",
        "region,customer_id,amount,customer_sale_number,customer_revenue,region_revenue_rank\nWest,C003,200,1,350,1\nWest,C003,150,2,350,1\nNorth,C001,120,1,200,2\nNorth,C001,80,2,200,2\nEast,C005,90,1,90,3\nSouth,C004,50,1,50,4\n",
    );
}

#[test]
fn jsonl_orders_example_runs_to_csv_stdout() {
    assert_example_stdout(
        "examples/jsonl_orders.pdl",
        "order_id,region,amount\nJ100,North,40\nJ102,West,60\n",
    );
}

#[test]
fn csv_stdin_example_runs_to_csv_stdout() {
    let output = command_with_stdin(
        &[
            "run",
            "examples/stdin_orders_csv.pdl",
            "--stdin-format",
            "csv",
            "--stdout-format",
            "csv",
        ],
        b"order_id,region,amount,status\nA2,South,20,pending\nA1,North,10,completed\nA3,West,30,completed\n",
    );

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "order_id,region,amount\nA1,North,10\nA3,West,30\n"
    );
    assert!(
        output.stderr.is_empty(),
        "diagnostics should stay off stdout and be absent for the valid example: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn jsonl_stdout_example_emits_clean_json_lines() {
    let output = command_output(&[
        "run",
        "examples/stdout_jsonl.pdl",
        "--stdout-format",
        "jsonl",
    ]);

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "{\"region\":\"West\",\"amount\":200}\n{\"region\":\"West\",\"amount\":150}\n{\"region\":\"North\",\"amount\":120}\n{\"region\":\"East\",\"amount\":90}\n{\"region\":\"North\",\"amount\":80}\n{\"region\":\"South\",\"amount\":50}\n"
    );
}

#[test]
fn arrow_stream_stdout_example_emits_clean_arrow_bytes() {
    let output = command_output(&[
        "run",
        "examples/stdout_arrow_stream.pdl",
        "--stdout-format",
        "arrow-stream",
    ]);

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert!(output.stdout.starts_with(&[0xff, 0xff, 0xff, 0xff]));
    let table = pdl_data::read_table_from_bytes(
        Path::new("stdout.arrow"),
        DataFormat::ArrowStream,
        &output.stdout,
    )
    .expect("read arrow stdout");
    assert_eq!(
        table,
        Table::new(
            vec!["region".to_string(), "amount".to_string()],
            vec![
                Row {
                    values: vec![Value::String("West".to_string()), Value::Number(200.0)],
                },
                Row {
                    values: vec![Value::String("West".to_string()), Value::Number(150.0)],
                },
                Row {
                    values: vec![Value::String("North".to_string()), Value::Number(120.0)],
                },
                Row {
                    values: vec![Value::String("East".to_string()), Value::Number(90.0)],
                },
                Row {
                    values: vec![Value::String("North".to_string()), Value::Number(80.0)],
                },
                Row {
                    values: vec![Value::String("South".to_string()), Value::Number(50.0)],
                },
            ],
        )
    );
}

#[test]
fn arrow_file_stdout_example_emits_clean_arrow_file_bytes() {
    let output = command_output(&[
        "run",
        "examples/stdout_arrow_file.pdl",
        "--stdout-format",
        "arrow-file",
    ]);

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert!(output.stdout.starts_with(b"ARROW1"));
    assert!(output.stdout.ends_with(b"ARROW1"));
    assert_completed_sales_table(DataFormat::ArrowFile, &output.stdout);
}

#[test]
fn parquet_stdout_example_emits_clean_parquet_bytes() {
    let output = command_output(&[
        "run",
        "examples/stdout_parquet.pdl",
        "--stdout-format",
        "parquet",
    ]);

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert!(output.stdout.starts_with(b"PAR1"));
    assert!(output.stdout.ends_with(b"PAR1"));
    assert_completed_sales_table(DataFormat::Parquet, &output.stdout);
}

#[test]
fn arrow_stream_stdin_to_stdout_example_round_trips_bytes() {
    let input = Table::new(
        vec!["region".to_string(), "amount".to_string()],
        vec![
            Row {
                values: vec![Value::String("East".to_string()), Value::Number(10.0)],
            },
            Row {
                values: vec![Value::String("West".to_string()), Value::Number(30.0)],
            },
        ],
    );
    let stdin =
        pdl_data::write_table_to_bytes(DataFormat::ArrowStream, &input).expect("write arrow stdin");
    let output = command_with_stdin(
        &[
            "run",
            "examples/arrow_stream_passthrough.pdl",
            "--stdin-format",
            "arrow-stream",
        ],
        &stdin,
    );

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let table = pdl_data::read_table_from_bytes(
        Path::new("stdout.arrow"),
        DataFormat::ArrowStream,
        &output.stdout,
    )
    .expect("read arrow stdout");
    assert_eq!(
        table,
        Table::new(
            vec!["region".to_string(), "amount".to_string()],
            vec![
                Row {
                    values: vec![Value::String("West".to_string()), Value::Number(30.0)],
                },
                Row {
                    values: vec![Value::String("East".to_string()), Value::Number(10.0)],
                },
            ],
        )
    );
}

#[test]
fn native_file_formats_save_and_load_by_path_inference() {
    for (name, extension, format) in [
        ("parquet", "parquet", DataFormat::Parquet),
        ("arrow-file", "arrow", DataFormat::ArrowFile),
        ("jsonl", "jsonl", DataFormat::JsonLines),
    ] {
        let save_program = temp_path(&format!("save-{name}"), "pdl");
        let load_program = temp_path(&format!("load-{name}"), "pdl");
        let output_path = temp_path(&format!("format-{name}"), extension);
        let sales_path = repo_root().join("examples/sales.csv");

        std::fs::write(
            &save_program,
            format!(
                "load \"{}\"\n  | filter status == \"completed\"\n  | select region, amount\n  | sort amount desc\n  | save \"{}\"\n",
                sales_path.display(),
                output_path.display()
            ),
        )
        .expect("write save program");

        let output = command_output_owned(&["run", save_program.to_str().expect("utf-8 path")]);
        assert!(
            output.status.success(),
            "{name} save stderr:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stdout.is_empty());
        assert!(output.stderr.is_empty());
        let bytes = std::fs::read(&output_path).expect("read saved format");
        assert_completed_sales_table(format, &bytes);

        std::fs::write(
            &load_program,
            format!("load \"{}\"\n  | sort amount desc\n", output_path.display()),
        )
        .expect("write load program");
        let output = command_output_owned(&[
            "run",
            load_program.to_str().expect("utf-8 path"),
            "--stdout-format",
            "csv",
        ]);
        assert!(
            output.status.success(),
            "{name} load stderr:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8(output.stdout).expect("stdout is UTF-8"),
            "region,amount\nWest,200\nWest,150\nNorth,120\nEast,90\nNorth,80\nSouth,50\n"
        );
        assert!(output.stderr.is_empty());

        let _ = std::fs::remove_file(save_program);
        let _ = std::fs::remove_file(load_program);
        let _ = std::fs::remove_file(output_path);
    }
}

#[test]
fn stdin_format_conflict_reports_e1217_on_stderr_with_empty_stdout() {
    let output = command_output(&[
        "run",
        "examples/stdin_orders_csv.pdl",
        "--stdin-format",
        "arrow-stream",
        "--stdout-format",
        "csv",
    ]);

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(stderr.contains("error[E1217]"), "{stderr}");
    assert!(!stderr.contains("E1806"), "{stderr}");
}

#[test]
fn fmt_check_rejects_unformatted_source_and_fmt_rewrites() {
    let path = temp_pdl_path("fmt-unformatted");
    std::fs::write(&path, r#"load "sales.csv"|select region"#).expect("write temp pdl");

    let output = command_output_owned(&["fmt", "--check", path.to_str().expect("utf-8 path")]);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(stderr.contains("is not formatted"), "{stderr}");

    let output = command_output_owned(&["fmt", path.to_str().expect("utf-8 path")]);
    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(&path).expect("read formatted pdl"),
        "load \"sales.csv\"\n  | select region\n"
    );

    let _ = std::fs::remove_file(path);
}

#[test]
fn schema_command_prints_main_schema() {
    let output = command_output(&["schema", "examples/top_regions.pdl"]);

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    assert!(stdout.contains("total_revenue"), "{stdout}");
    assert!(stdout.contains("avg_age"), "{stdout}");
}

#[test]
fn schema_binding_json_inspects_lazy_binding() {
    let output = command_output(&[
        "schema",
        "examples/segment_revenue.pdl",
        "--binding",
        "customers",
        "--json",
    ]);

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    assert!(stdout.contains("\"binding\": \"customers\""), "{stdout}");
    assert!(stdout.contains("\"name\": \"segment\""), "{stdout}");
}

#[test]
fn plan_command_prints_dry_run_execution_plan() {
    let output = command_output(&["plan", "examples/top_regions.pdl", "--stdout-format", "csv"]);

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    assert!(stdout.contains("execution:"), "{stdout}");
    assert!(stdout.contains("stdout format csv"), "{stdout}");
}

#[test]
fn ast_ir_and_manifest_commands_emit_json() {
    let ast = command_output(&["ast", "examples/top_regions.pdl"]);
    assert!(
        ast.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&ast.stderr)
    );
    let ast_stdout = String::from_utf8(ast.stdout).expect("ast stdout is UTF-8");
    assert!(ast_stdout.contains("\"program\""), "{ast_stdout}");
    assert!(ast_stdout.contains("\"filter\""), "{ast_stdout}");

    let ir = command_output(&["ir", "examples/top_regions.pdl"]);
    assert!(
        ir.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&ir.stderr)
    );
    let ir_stdout = String::from_utf8(ir.stdout).expect("ir stdout is UTF-8");
    assert!(ir_stdout.contains("\"ir\""), "{ir_stdout}");
    assert!(ir_stdout.contains("\"group_by\""), "{ir_stdout}");

    let manifest = command_output(&[
        "manifest",
        "examples/stdout_arrow_stream.pdl",
        "--stdout-format",
        "arrow-stream",
    ]);
    assert!(
        manifest.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&manifest.stderr)
    );
    let manifest_stdout = String::from_utf8(manifest.stdout).expect("manifest stdout is UTF-8");
    assert!(manifest_stdout.contains("\"manifest_version\": \"0.37.0\""));
    assert!(manifest_stdout.contains("\"observability\""));
    assert!(manifest_stdout.contains("\"stream_interop\""));
    assert!(manifest_stdout.contains("\"arrow-stream\""));
}

fn assert_example_stdout(example: &str, expected_stdout: &str) {
    let output = command_output(&["run", example, "--dry-run", "--stdout-format", "csv"]);

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        expected_stdout
    );
    assert!(
        output.stderr.is_empty(),
        "diagnostics should stay off stdout and be absent for the valid example: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_completed_sales_table(format: DataFormat, bytes: &[u8]) {
    let table = pdl_data::read_table_from_bytes(Path::new("stdout.data"), format, bytes)
        .expect("read table bytes");
    assert_eq!(
        table,
        Table::new(
            vec!["region".to_string(), "amount".to_string()],
            vec![
                Row {
                    values: vec![Value::String("West".to_string()), Value::Number(200.0)],
                },
                Row {
                    values: vec![Value::String("West".to_string()), Value::Number(150.0)],
                },
                Row {
                    values: vec![Value::String("North".to_string()), Value::Number(120.0)],
                },
                Row {
                    values: vec![Value::String("East".to_string()), Value::Number(90.0)],
                },
                Row {
                    values: vec![Value::String("North".to_string()), Value::Number(80.0)],
                },
                Row {
                    values: vec![Value::String("South".to_string()), Value::Number(50.0)],
                },
            ],
        )
    );
}

fn command_output(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_pdl"))
        .current_dir(repo_root())
        .args(args)
        .output()
        .expect("run pdl example")
}

fn command_output_owned(args: &[&str]) -> std::process::Output {
    command_output(args)
}

fn command_with_stdin(args: &[&str], stdin: &[u8]) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_pdl"))
        .current_dir(repo_root())
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn pdl example");
    child
        .stdin
        .as_mut()
        .expect("stdin pipe")
        .write_all(stdin)
        .expect("write stdin");
    child.wait_with_output().expect("wait for pdl example")
}

fn temp_pdl_path(name: &str) -> std::path::PathBuf {
    temp_path(name, "pdl")
}

fn temp_path(name: &str, extension: &str) -> std::path::PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock after epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "pdl-{name}-{}-{nonce}.{extension}",
        std::process::id()
    ))
}

fn repo_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}
