use pdl_core::Diagnostic;

use crate::services::{range_for_span, EditorDiagnostic};

pub fn diagnostics_for_editor(source: &str, diagnostics: &[Diagnostic]) -> Vec<EditorDiagnostic> {
    diagnostics
        .iter()
        .map(|diagnostic| EditorDiagnostic {
            range: range_for_span(source, diagnostic.span),
            severity: diagnostic.severity,
            code: diagnostic.code.to_string(),
            message: diagnostic.message.clone(),
        })
        .collect()
}
