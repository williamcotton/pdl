use std::fs;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root")
}

fn read_workspace_file(path: &str) -> String {
    fs::read_to_string(workspace_root().join(path)).expect(path)
}

#[test]
fn syntax_and_semantics_keep_allowed_dependency_direction() {
    let syntax = read_workspace_file("crates/pdl-syntax/Cargo.toml");
    assert!(syntax.contains("pdl-core"));
    for forbidden in [
        "pdl-data",
        "pdl-driver",
        "pdl-semantics",
        "pdl-exec",
        "pdl-editor-services",
        "pdl-lsp",
        "pdl-cli",
        "pdl-wasm",
    ] {
        assert!(
            !syntax.contains(forbidden),
            "pdl-syntax must not depend on {forbidden}"
        );
    }

    let semantics = read_workspace_file("crates/pdl-semantics/Cargo.toml");
    for forbidden in [
        "pdl-driver",
        "pdl-exec",
        "pdl-editor-services",
        "pdl-lsp",
        "pdl-cli",
        "pdl-wasm",
    ] {
        assert!(
            !semantics.contains(forbidden),
            "pdl-semantics must not depend on {forbidden}"
        );
    }
}

#[test]
fn public_crates_above_data_do_not_leak_concrete_engines() {
    let root = workspace_root();
    for crate_name in [
        "pdl-syntax",
        "pdl-semantics",
        "pdl-driver",
        "pdl-exec",
        "pdl-editor-services",
        "pdl-lsp",
        "pdl-cli",
        "pdl-wasm",
    ] {
        let src = root.join("crates").join(crate_name).join("src");
        for file in rust_files(&src) {
            let text = fs::read_to_string(&file).expect("rust source");
            for forbidden in [
                "polars::",
                "DataFrame",
                "LazyFrame",
                "arrow_array",
                "arrow_ipc",
                "arrow_schema",
                "parquet::",
            ] {
                assert!(
                    !text.contains(forbidden),
                    "{} must not expose or mention concrete engine symbol {forbidden}",
                    file.display()
                );
            }
        }
    }
}

#[test]
fn exec_uses_ir_not_syntax_stage_inspection() {
    let manifest = read_workspace_file("crates/pdl-exec/Cargo.toml");
    let planning = read_workspace_file("crates/pdl-exec/src/planning.rs");
    let runtime = read_workspace_file("crates/pdl-exec/src/runtime.rs");

    assert!(!manifest.contains("pdl-syntax"));
    assert!(!planning.contains("pdl_syntax"));
    assert!(!runtime.contains("pdl_syntax"));
    assert!(planning.contains("StageIr"));
    assert!(runtime.contains("StageIr"));
    assert!(planning.contains("driver_plan"));
    assert!(runtime.contains("driver_plan"));
}

#[test]
fn wasm_manifest_does_not_enable_native_format_features() {
    let wasm_manifest = read_workspace_file("crates/pdl-wasm/Cargo.toml");

    assert!(!wasm_manifest.contains("native-formats"));
    assert!(!wasm_manifest.contains("polars-engine"));
}

fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_rust_files(dir, &mut files);
    files
}

fn collect_rust_files(dir: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("source directory") {
        let entry = entry.expect("directory entry");
        let path = entry.path();
        if path.is_dir() {
            collect_rust_files(&path, files);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
}
