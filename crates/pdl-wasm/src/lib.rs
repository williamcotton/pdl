pub fn check_json(source: &str) -> String {
    let parse = pdl_syntax::parse(source);
    serde_json::json!({
        "diagnostics": parse.diagnostics,
    })
    .to_string()
}
