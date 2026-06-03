use std::path::Path;
use std::process::Command;

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

fn assert_example_stdout(example: &str, expected_stdout: &str) {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = Command::new(env!("CARGO_BIN_EXE_pdl"))
        .current_dir(repo_root)
        .args(["run", example, "--dry-run", "--stdout-format", "csv"])
        .output()
        .expect("run pdl example");

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
