use pdl_data::{write_table_to_bytes, DataFormat, Table};
use pdl_driver::{PipelineLabel, SinkDescriptor};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
struct HostSchema {
    path: String,
    columns: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct BrowserRunRequest {
    source: String,
    #[serde(default)]
    files: BTreeMap<String, String>,
    #[serde(default = "default_program_path")]
    program_path: String,
    #[serde(default)]
    stdout_format: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BrowserEditorRequest {
    source: String,
    #[serde(default)]
    files: BTreeMap<String, String>,
    #[serde(default = "default_program_path")]
    program_path: String,
    request: BrowserEditorFeatureRequest,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum BrowserEditorFeatureRequest {
    Diagnostics,
    Hover {
        position: pdl_editor_services::TextPosition,
    },
    Completion {
        position: pdl_editor_services::TextPosition,
    },
    Formatting,
    SemanticTokens,
    DocumentSymbols,
    Definition {
        position: pdl_editor_services::TextPosition,
    },
    References {
        position: pdl_editor_services::TextPosition,
    },
    Rename {
        position: pdl_editor_services::TextPosition,
        #[serde(alias = "newName")]
        new_name: String,
    },
}

fn default_program_path() -> String {
    "memory/main.pdl".to_string()
}

pub fn check_json(source: &str) -> String {
    let document = pdl_editor_services::analyze_document(source, None);
    check_document_json(document)
}

pub fn check_json_with_schemas(source: &str, program_path: &str, schemas_json: &str) -> String {
    let schemas = match serde_json::from_str::<Vec<HostSchema>>(schemas_json) {
        Ok(schemas) => schemas,
        Err(error) => {
            return serde_json::json!({
                "diagnostics": [],
                "error": format!("invalid schema payload: {error}"),
            })
            .to_string();
        }
    };
    let path = Path::new(program_path);
    let document = pdl_editor_services::analyze_document_with_schemas(
        source,
        path,
        schemas
            .into_iter()
            .map(|schema| (PathBuf::from(schema.path), schema.columns)),
    );
    check_document_json(document)
}

fn check_document_json(document: pdl_editor_services::EditorDocument) -> String {
    serde_json::json!({
        "diagnostics": document.diagnostics,
    })
    .to_string()
}

pub fn format_json(source: &str) -> String {
    serde_json::json!({
        "formatted": pdl_syntax::format_source(source),
    })
    .to_string()
}

pub fn run_json(input: &str) -> String {
    run_browser_json(input.as_bytes())
}

pub fn editor_service_json(input: &str) -> String {
    editor_service_browser_json(input.as_bytes())
}

fn run_browser_json(input: &[u8]) -> String {
    let request = match serde_json::from_slice::<BrowserRunRequest>(input) {
        Ok(request) => request,
        Err(error) => {
            return serde_json::json!({
                "stdout": null,
                "diagnostics": [],
                "error": format!("invalid run request JSON: {error}"),
            })
            .to_string();
        }
    };
    let program_path = PathBuf::from(&request.program_path);
    let io = in_memory_io(&program_path, request.files);
    let prepared = pdl_driver::prepare_source_with_io(&program_path, request.source, &io);
    let stdout_format = request.stdout_format.or_else(|| {
        prepared
            .analysis
            .ir
            .as_ref()
            .map_or(true, |ir| ir.outputs.is_empty())
            .then(|| "csv".to_string())
    });
    let result = pdl_exec::run_prepared_with_io(
        &prepared,
        pdl_exec::RunOptions {
            stdout_format,
            dry_run: true,
            allow_binary_stdout: false,
        },
        &io,
    );
    let stdout = match result.stdout {
        Some(bytes) => match String::from_utf8(bytes) {
            Ok(stdout) => Some(stdout),
            Err(error) => {
                return serde_json::json!({
                    "stdout": null,
                    "diagnostics": result.diagnostics,
                    "error": format!("stdout was not valid UTF-8: {error}"),
                })
                .to_string();
            }
        },
        None => None,
    };

    serde_json::json!({
        "stdout": stdout,
        "files": saved_files_json(&prepared, &result.named_outputs),
        "outputs": result
            .named_outputs
            .iter()
            .map(|output| {
                serde_json::json!({
                    "name": output.name,
                    "table": table_json(&output.table),
                })
            })
            .collect::<Vec<_>>(),
        "diagnostics": result.diagnostics,
        "error": null,
    })
    .to_string()
}

fn saved_files_json(
    prepared: &pdl_driver::PreparedProgram,
    outputs: &[pdl_exec::runtime::NamedOutput],
) -> BTreeMap<String, String> {
    outputs
        .iter()
        .filter_map(|output| saved_file_for_output(prepared, output))
        .collect()
}

fn saved_file_for_output(
    prepared: &pdl_driver::PreparedProgram,
    output: &pdl_exec::runtime::NamedOutput,
) -> Option<(String, String)> {
    let sink = prepared.driver_plan.sinks.iter().find(|sink| {
        matches!(&sink.pipeline, PipelineLabel::Output(name) if name == &output.name)
            && matches!(sink.sink, SinkDescriptor::Path { .. })
    })?;
    let SinkDescriptor::Path { logical_path, .. } = &sink.sink else {
        return None;
    };
    let format = sink
        .format
        .explicit
        .as_deref()
        .and_then(DataFormat::from_name)
        .or(sink.format.inferred_from_path)
        .unwrap_or(DataFormat::Csv);
    if format.is_binary() {
        return None;
    }
    let bytes = write_table_to_bytes(format, &output.table).ok()?;
    let text = String::from_utf8(bytes).ok()?;
    Some((logical_path.clone(), text))
}

fn table_json(table: &Table) -> serde_json::Value {
    serde_json::json!({
        "columns": table.columns,
        "rows": table
            .rows
            .iter()
            .map(|row| {
                row.values
                    .iter()
                    .map(|value| value.to_csv_cell())
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>(),
    })
}

fn editor_service_browser_json(input: &[u8]) -> String {
    let request_payload = match serde_json::from_slice::<BrowserEditorRequest>(input) {
        Ok(request) => request,
        Err(error) => {
            return editor_service_error_json(format!(
                "invalid editor-service request JSON: {error}"
            ));
        }
    };
    let program_path = PathBuf::from(&request_payload.program_path);
    let io = in_memory_io(&program_path, request_payload.files);
    let document = pdl_editor_services::analyze_document_with_driver_io(
        &request_payload.source,
        &program_path,
        &io,
    );
    let result = match request_payload.request {
        BrowserEditorFeatureRequest::Diagnostics => serde_json::json!(document),
        request => {
            match editor_service_result(&request_payload.source, &program_path, &io, request) {
                Ok(result) => result,
                Err(error) => return editor_service_error_json(error),
            }
        }
    };

    serde_json::json!({
        "diagnostics": document.diagnostics,
        "result": result,
        "error": null,
    })
    .to_string()
}

fn editor_service_result(
    source: &str,
    program_path: &Path,
    io: &dyn pdl_driver::DriverIo,
    request: BrowserEditorFeatureRequest,
) -> Result<Value, String> {
    let result = match request {
        BrowserEditorFeatureRequest::Diagnostics => {
            serde_json::json!(pdl_editor_services::analyze_document(
                source,
                Some(program_path)
            ))
        }
        BrowserEditorFeatureRequest::Hover { position } => serde_json::to_value(
            pdl_editor_services::hover_with_driver_io(source, program_path, io, position),
        )
        .map_err(|error| error.to_string())?,
        BrowserEditorFeatureRequest::Completion { position } => serde_json::to_value(
            pdl_editor_services::completions(source, Some(program_path), position),
        )
        .map_err(|error| error.to_string())?,
        BrowserEditorFeatureRequest::Formatting => {
            serde_json::to_value(pdl_editor_services::formatting_edit(source))
                .map_err(|error| error.to_string())?
        }
        BrowserEditorFeatureRequest::SemanticTokens => {
            serde_json::to_value(pdl_editor_services::semantic_tokens(source))
                .map_err(|error| error.to_string())?
        }
        BrowserEditorFeatureRequest::DocumentSymbols => {
            serde_json::to_value(pdl_editor_services::document_symbols(source))
                .map_err(|error| error.to_string())?
        }
        BrowserEditorFeatureRequest::Definition { position } => {
            serde_json::to_value(pdl_editor_services::binding_definition(source, position))
                .map_err(|error| error.to_string())?
        }
        BrowserEditorFeatureRequest::References { position } => {
            serde_json::to_value(pdl_editor_services::binding_references(source, position))
                .map_err(|error| error.to_string())?
        }
        BrowserEditorFeatureRequest::Rename { position, new_name } => serde_json::to_value(
            pdl_editor_services::rename_binding_edits(source, position, &new_name),
        )
        .map_err(|error| error.to_string())?,
    };
    Ok(result)
}

fn editor_service_error_json(error: String) -> String {
    serde_json::json!({
        "diagnostics": [],
        "result": null,
        "error": error,
    })
    .to_string()
}

fn in_memory_io(
    program_path: &Path,
    files: BTreeMap<String, String>,
) -> pdl_driver::InMemoryDriverIo {
    let base_dir = program_path
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    let mut io = pdl_driver::InMemoryDriverIo::default();
    for (path, text) in files {
        let path = PathBuf::from(path);
        let bytes = text.into_bytes();
        io = io.with_file_bytes(path.clone(), bytes.clone());
        if path.is_relative() {
            io = io.with_file_bytes(base_dir.join(path), bytes);
        }
    }
    io
}

#[cfg(target_arch = "wasm32")]
fn pack_ptr_len(ptr: *mut u8, len: usize) -> u64 {
    ((len as u64) << 32) | (ptr as u64)
}

#[cfg(target_arch = "wasm32")]
fn leak_bytes(bytes: Vec<u8>) -> u64 {
    let len = bytes.len();
    let mut bytes = bytes.into_boxed_slice();
    let ptr = bytes.as_mut_ptr();
    std::mem::forget(bytes);
    pack_ptr_len(ptr, len)
}

#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn pdl_alloc(len: usize) -> *mut u8 {
    let mut buffer = Vec::<u8>::with_capacity(len);
    let ptr = buffer.as_mut_ptr();
    std::mem::forget(buffer);
    ptr
}

#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub unsafe extern "C" fn pdl_dealloc(ptr: *mut u8, len: usize) {
    if ptr.is_null() {
        return;
    }
    drop(Vec::from_raw_parts(ptr, 0, len));
}

#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub unsafe extern "C" fn pdl_run_json(ptr: *const u8, len: usize) -> u64 {
    if ptr.is_null() {
        return leak_bytes(
            serde_json::json!({
                "stdout": null,
                "diagnostics": [],
                "error": "run request pointer was null",
            })
            .to_string()
            .into_bytes(),
        );
    }

    let input = std::slice::from_raw_parts(ptr, len);
    leak_bytes(run_browser_json(input).into_bytes())
}

#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub unsafe extern "C" fn pdl_editor_service_json(ptr: *const u8, len: usize) -> u64 {
    if ptr.is_null() {
        return leak_bytes(
            editor_service_error_json("editor-service request pointer was null".to_string())
                .into_bytes(),
        );
    }

    let input = std::slice::from_raw_parts(ptr, len);
    leak_bytes(editor_service_browser_json(input).into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_json_uses_shared_editor_diagnostics() {
        let payload: serde_json::Value =
            serde_json::from_str(&check_json(r#"load "sales.csv" |"#)).expect("json");

        assert_eq!(payload["diagnostics"][0]["code"], "E0006");
    }

    #[test]
    fn check_json_with_schemas_uses_shared_schema_aware_editor_diagnostics() {
        let source = r#"load "sales.csv"
  | filter sttus == "completed"
  | group_by region
  | agg total_revenue = sum(amount), avg_age = mean(customer_age), orders = count()
  | sort total_revenue desc
  | limit 3"#;
        let schemas = serde_json::json!([
            {
                "path": "memory/sales.csv",
                "columns": ["region", "status", "amount", "customer_age"]
            }
        ])
        .to_string();

        let payload: serde_json::Value = serde_json::from_str(&check_json_with_schemas(
            source,
            "memory/main.pdl",
            &schemas,
        ))
        .expect("json");

        assert_eq!(payload["diagnostics"][0]["code"], "E1005");
        assert_eq!(
            payload["diagnostics"][0]["message"],
            "unknown column `sttus`"
        );

        let corrected = source.replace("sttus", "status");
        let corrected_payload: serde_json::Value = serde_json::from_str(&check_json_with_schemas(
            &corrected,
            "memory/main.pdl",
            &schemas,
        ))
        .expect("json");
        let diagnostics = corrected_payload["diagnostics"]
            .as_array()
            .expect("diagnostics");
        assert!(!diagnostics
            .iter()
            .any(|diagnostic| diagnostic["code"] == "E1005"));
    }

    #[test]
    fn format_json_uses_shared_formatter() {
        let payload: serde_json::Value =
            serde_json::from_str(&format_json(r#"load "sales.csv"|select region"#)).expect("json");

        assert_eq!(
            payload["formatted"],
            "load \"sales.csv\"\n  | select region"
        );
    }

    #[test]
    fn run_json_executes_against_in_memory_csv_bytes() {
        let request = serde_json::json!({
            "source": r#"load "sales.csv"
  | filter status == "completed"
  | group_by region
  | agg total_revenue = sum(amount)
  | sort total_revenue desc
  | limit 2"#,
            "files": {
                "sales.csv": "region,status,amount\nNorth,completed,120\nSouth,pending,75\nWest,completed,200\n"
            },
            "stdout_format": "csv"
        });

        let payload: serde_json::Value =
            serde_json::from_str(&run_json(&request.to_string())).expect("json");

        assert!(payload["error"].is_null(), "{payload}");
        assert_eq!(
            payload["stdout"],
            "region,total_revenue\nWest,200\nNorth,120\n"
        );
        assert_eq!(payload["diagnostics"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn run_json_executes_against_in_memory_json_lines_bytes() {
        let request = serde_json::json!({
            "source": r#"load "sales.jsonl"
  | filter status == "completed"
  | select region, amount
  | sort amount desc"#,
            "files": {
                "sales.jsonl": "{\"region\":\"North\",\"status\":\"completed\",\"amount\":120}\n{\"region\":\"South\",\"status\":\"pending\",\"amount\":75}\n{\"region\":\"West\",\"status\":\"completed\",\"amount\":200}\n"
            },
            "stdout_format": "jsonl"
        });

        let payload: serde_json::Value =
            serde_json::from_str(&run_json(&request.to_string())).expect("json");

        assert!(payload["error"].is_null(), "{payload}");
        assert_eq!(
            payload["stdout"],
            "{\"region\":\"West\",\"amount\":200}\n{\"region\":\"North\",\"amount\":120}\n"
        );
        assert_eq!(payload["diagnostics"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn run_json_returns_named_output_tables() {
        let request = serde_json::json!({
            "source": r#"let sales =
  load "sales.csv"

output west =
  sales
  | filter region == "West"

output totals =
  sales
  | agg total = sum(amount)"#,
            "files": {
                "sales.csv": "region,amount\nWest,30\nEast,10\n"
            },
            "stdout_format": "csv"
        });

        let payload: serde_json::Value =
            serde_json::from_str(&run_json(&request.to_string())).expect("json");

        assert!(payload["error"].is_null(), "{payload}");
        assert_eq!(payload["diagnostics"].as_array().unwrap().len(), 1);
        assert_eq!(payload["diagnostics"][0]["code"], "E1607");

        let request = serde_json::json!({
            "source": r#"let sales =
  load "sales.csv"

output west =
  sales
  | filter region == "West"
  | save "west.csv"

output totals =
  sales
  | agg total = sum(amount)
  | save "totals.csv""#,
            "files": {
                "sales.csv": "region,amount\nWest,30\nEast,10\n"
            }
        });

        let payload: serde_json::Value =
            serde_json::from_str(&run_json(&request.to_string())).expect("json");

        assert!(payload["error"].is_null(), "{payload}");
        assert_eq!(payload["diagnostics"].as_array().unwrap().len(), 0);
        assert_eq!(payload["outputs"][0]["name"], "west");
        assert_eq!(payload["outputs"][0]["table"]["columns"][0], "region");
        assert_eq!(payload["outputs"][0]["table"]["rows"][0][0], "West");
        assert_eq!(payload["outputs"][1]["name"], "totals");
        assert_eq!(payload["outputs"][1]["table"]["rows"][0][0], "40");
        assert_eq!(payload["files"]["west.csv"], "region,amount\nWest,30\n");
        assert_eq!(payload["files"]["totals.csv"], "total\n40\n");
        assert!(!Path::new("west.csv").exists());
        assert!(!Path::new("totals.csv").exists());
    }

    #[test]
    fn run_json_rejects_arrow_stdout_with_registered_diagnostic() {
        let request = serde_json::json!({
            "source": r#"load "sales.csv""#,
            "files": {
                "sales.csv": "region,status,amount\nNorth,completed,120\n"
            },
            "stdout_format": "arrow-stream"
        });

        let payload: serde_json::Value =
            serde_json::from_str(&run_json(&request.to_string())).expect("json");

        assert!(payload["error"].is_null(), "{payload}");
        assert!(payload["stdout"].is_null(), "{payload}");
        assert_eq!(payload["diagnostics"][0]["code"], "E1705");
        assert!(
            payload["diagnostics"][0]["message"]
                .as_str()
                .is_some_and(|message| message.contains("arrow-stream")),
            "{payload}"
        );
    }

    #[test]
    fn editor_service_json_uses_in_memory_csv_schema_for_diagnostics() {
        let request = serde_json::json!({
            "source": r#"load "sales.csv" | filter sttus == "completed""#,
            "files": {
                "sales.csv": "region,status,amount\nNorth,completed,120\n"
            },
            "request": { "kind": "diagnostics" }
        });

        let payload: serde_json::Value =
            serde_json::from_str(&editor_service_json(&request.to_string())).expect("json");

        assert!(payload["error"].is_null(), "{payload}");
        assert_eq!(payload["diagnostics"][0]["code"], "E1005");
        assert_eq!(
            payload["diagnostics"][0]["message"],
            "unknown column `sttus`"
        );
    }

    #[test]
    fn editor_service_json_serializes_expanded_semantic_token_names() {
        let request = serde_json::json!({
            "source": r#"let cleaned =
  load "orders.csv"
  | mutate net_amount = gross_amount

cleaned
  | group_by net_amount"#,
            "files": {
                "orders.csv": "gross_amount\n10\n"
            },
            "request": { "kind": "semanticTokens" }
        });

        let payload: serde_json::Value =
            serde_json::from_str(&editor_service_json(&request.to_string())).expect("json");

        assert!(payload["error"].is_null(), "{payload}");
        let token_types = payload["result"]
            .as_array()
            .expect("semantic tokens")
            .iter()
            .filter_map(|token| token["token_type"].as_str())
            .collect::<Vec<_>>();
        for expected in [
            "BindingDeclaration",
            "BindingReference",
            "ColumnDefinition",
            "ColumnReference",
        ] {
            assert!(
                token_types.contains(&expected),
                "missing {expected} in {token_types:?}"
            );
        }
    }

    #[test]
    fn editor_service_json_uses_in_memory_csv_bytes_for_hover_preview() {
        let request = serde_json::json!({
            "source": "load \"sales.csv\"\n  | group_by region",
            "files": {
                "sales.csv": "region,status,amount\nNorth,completed,120\nSouth,pending,75\nWest,completed,200\n"
            },
            "request": {
                "kind": "hover",
                "position": { "line": 1, "character": 15 }
            }
        });

        let payload: serde_json::Value =
            serde_json::from_str(&editor_service_json(&request.to_string())).expect("json");

        assert!(payload["error"].is_null(), "{payload}");
        let markdown = payload["result"]["markdown"]
            .as_str()
            .expect("hover markdown");
        assert!(markdown.contains("**column `region`**"));
        assert!(markdown.contains("Type: `string`"));
        assert!(markdown.contains("Samples: North, South, West"));
    }
}
