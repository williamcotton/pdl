use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
struct HostSchema {
    path: String,
    columns: Vec<String>,
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
}
