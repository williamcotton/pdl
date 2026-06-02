use pdl_core::{Diagnostic, Severity};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TextPosition {
    pub line: u32,
    pub character: u32,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TextRange {
    pub start: TextPosition,
    pub end: TextPosition,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EditorDiagnostic {
    pub range: TextRange,
    pub severity: Severity,
    pub code: String,
    pub message: String,
}

pub fn diagnostics_for_editor(source: &str, diagnostics: &[Diagnostic]) -> Vec<EditorDiagnostic> {
    diagnostics
        .iter()
        .map(|diagnostic| EditorDiagnostic {
            range: range_for_span(source, diagnostic.span),
            severity: diagnostic.severity.clone(),
            code: diagnostic.code.clone(),
            message: diagnostic.message.clone(),
        })
        .collect()
}

pub fn range_for_span(source: &str, span: pdl_core::Span) -> TextRange {
    TextRange {
        start: position_for_byte_offset(source, span.start),
        end: position_for_byte_offset(source, span.end),
    }
}

pub fn position_for_byte_offset(source: &str, byte_offset: usize) -> TextPosition {
    let mut line = 0u32;
    let mut character = 0u32;

    for (index, ch) in source.char_indices() {
        if index >= byte_offset {
            break;
        }

        if ch == '\n' {
            line += 1;
            character = 0;
        } else {
            character += ch.len_utf16() as u32;
        }
    }

    TextPosition { line, character }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_use_utf16_columns() {
        let source = "a\n😀x";

        assert_eq!(
            position_for_byte_offset(source, source.find('x').expect("x offset")),
            TextPosition {
                line: 1,
                character: 2
            }
        );
    }
}
