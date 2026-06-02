use std::path::Path;
use std::process::Command;

#[test]
fn top_regions_example_runs_to_csv_stdout() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = Command::new(env!("CARGO_BIN_EXE_pdl"))
        .current_dir(repo_root)
        .args([
            "run",
            "examples/top_regions.pdl",
            "--dry-run",
            "--stdout-format",
            "csv",
        ])
        .output()
        .expect("run pdl example");

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        "region,total_revenue,avg_age,orders\nWest,350,34,2\nNorth,200,31.5,2\nEast,90,28,1\n"
    );
    assert!(
        output.stderr.is_empty(),
        "diagnostics should stay off stdout and be absent for the valid example: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
