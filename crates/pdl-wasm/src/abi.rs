pub fn check_json(source: &str) -> String {
    let document = pdl_editor_services::analyze_document(source, None);
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
