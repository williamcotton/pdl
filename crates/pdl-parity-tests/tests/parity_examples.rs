// Row-vs-native parity harness (v0.43). Every example in `examples/` runs
// through `pdl run` on both engines with stdin fixtures supplied per example.
// The row engine is the parity spec: stdout payloads and saved/named-output
// files from the other engine legs must match it. CSV and JSON Lines are
// compared byte-for-byte — since v0.44 the native direct writers emit those
// bytes through the row writers' cell encoders. Arrow IPC file/stream and
// Parquet keep engine-specific encodings by design and are compared as
// decoded tables (see `common::binary_table_format`).

mod common;

use std::collections::BTreeSet;

use common::{
    assert_run_parity, example_config, example_name, example_sources, expected_selected_engine,
    run_example,
};

#[test]
fn parity_examples() {
    let mut exercised_formats = BTreeSet::new();

    for source in example_sources() {
        let name = example_name(&source);
        let config = example_config(&name);

        let reference = run_example(&source, "row");
        for engine in candidate_engines(&name) {
            let candidate = run_example(&source, engine);
            assert_run_parity(&name, engine, &reference, &candidate);
        }

        if let Some(format) = config.stdout_format {
            exercised_formats.insert(format.to_string());
        }
        if config.stdin_format == Some("arrow-stream") {
            // The passthrough example saves to stdout in arrow-stream form.
            exercised_formats.insert("arrow-stream".to_string());
        }
        for file in reference.saved.keys() {
            if let Some(extension) = std::path::Path::new(file)
                .extension()
                .and_then(|extension| extension.to_str())
            {
                exercised_formats.insert(extension.to_string());
            }
        }
    }

    // The harness must cover every supported interchange format.
    for format in ["csv", "jsonl", "arrow-file", "arrow-stream", "parquet"] {
        assert!(
            exercised_formats.contains(format),
            "parity harness no longer exercises the `{format}` format; \
             exercised: {exercised_formats:?}"
        );
    }
}

/// Engine legs compared against the row reference. `row-strict` must always
/// succeed and match (it proves the row engine still handles the pipeline
/// end-to-end); `auto` must match whichever engine it selects; a forced
/// `native` leg runs only for examples the fixtures pin as natively
/// executed, because forcing native on a row-only pipeline is an error by
/// contract.
fn candidate_engines(name: &str) -> Vec<&'static str> {
    let mut engines = vec!["row-strict", "auto"];
    if expected_selected_engine(name) == "native" {
        engines.push("native");
    }
    engines
}
