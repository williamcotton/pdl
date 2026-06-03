use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

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

fn command_output(args: &[&str]) -> std::process::Output {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    Command::new(env!("CARGO_BIN_EXE_pdl"))
        .current_dir(repo_root)
        .args(args)
        .output()
        .expect("run pdl example")
}

fn command_with_stdin(args: &[&str], stdin: &[u8]) -> std::process::Output {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut child = Command::new(env!("CARGO_BIN_EXE_pdl"))
        .current_dir(repo_root)
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
