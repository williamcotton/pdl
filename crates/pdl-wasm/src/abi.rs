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
    #[serde(default = "default_stdout_format")]
    stdout_format: String,
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

fn default_stdout_format() -> String {
    "csv".to_string()
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
    let result = pdl_exec::run_prepared_with_io(
        &prepared,
        pdl_exec::RunOptions {
            stdout_format: Some(request.stdout_format),
            dry_run: false,
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
        "diagnostics": result.diagnostics,
        "error": null,
    })
    .to_string()
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
        request => match editor_service_result(&request_payload.source, &program_path, request) {
            Ok(result) => result,
            Err(error) => return editor_service_error_json(error),
        },
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
            pdl_editor_services::hover(source, Some(program_path), position),
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
  | filter "sttus" == "completed"
  | group_by "region"
  | agg sum("amount") as "total_revenue", mean("customer_age") as "avg_age", count() as "orders"
  | sort "total_revenue" desc
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

        let corrected = source.replace("\"sttus\"", "\"status\"");
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
            serde_json::from_str(&format_json(r#"load "sales.csv"|select "region""#))
                .expect("json");

        assert_eq!(
            payload["formatted"],
            "load \"sales.csv\"\n  | select \"region\""
        );
    }

    #[test]
    fn run_json_executes_against_in_memory_csv_bytes() {
        let request = serde_json::json!({
            "source": r#"load "sales.csv"
  | filter "status" == "completed"
  | group_by "region"
  | agg sum("amount") as "total_revenue"
  | sort "total_revenue" desc
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
    fn editor_service_json_uses_in_memory_csv_schema_for_diagnostics() {
        let request = serde_json::json!({
            "source": r#"load "sales.csv" | filter "sttus" == "completed""#,
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
}
