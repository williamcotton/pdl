use pdl_core::{line_col, Diagnostic};

pub fn print_diagnostics(source_name: &str, source: &str, diagnostics: &[Diagnostic]) {
    let rendered = render_diagnostics(source_name, source, diagnostics);
    if !rendered.is_empty() {
        eprintln!("{rendered}");
    }
}

fn render_diagnostics(source_name: &str, source: &str, diagnostics: &[Diagnostic]) -> String {
    diagnostics
        .iter()
        .map(|diagnostic| render_diagnostic(source_name, source, diagnostic))
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn render_diagnostic(source_name: &str, source: &str, diagnostic: &Diagnostic) -> String {
    let (line, col) = line_col(source, diagnostic.span.start);
    let (line_start, line_end) = line_bounds(source, diagnostic.span.start);
    let source_line = &source[line_start..line_end];
    let underline_start = diagnostic.span.start.clamp(line_start, line_end);
    let underline_end = diagnostic.span.end.clamp(underline_start, line_end);
    let underline_padding = source[line_start..underline_start].chars().count();
    let underline_width = source[underline_start..underline_end]
        .chars()
        .count()
        .max(1);
    let gutter_width = line.to_string().len();
    let gutter_pad = " ".repeat(gutter_width);

    format!(
        "{}:{}:{}: {}[{}]: {}\n{} | {}\n{} | {}{}",
        source_name,
        line,
        col,
        diagnostic.severity,
        diagnostic.code,
        diagnostic.message,
        line,
        source_line,
        gutter_pad,
        " ".repeat(underline_padding),
        "^".repeat(underline_width),
    )
}

fn line_bounds(source: &str, byte_offset: usize) -> (usize, usize) {
    let offset = byte_offset.min(source.len());
    let line_start = source[..offset]
        .rfind('\n')
        .map_or(0, |newline| newline + 1);
    let line_end = source[offset..]
        .find('\n')
        .map_or(source.len(), |newline| offset + newline);
    (line_start, line_end)
}

#[cfg(test)]
mod tests {
    use pdl_core::{codes, Span};

    use super::*;

    #[test]
    fn grouped_diagnostics_have_blank_line_between_blocks() {
        let source = "load \"sales.csv\"\n  filter \"staus\" == \"completed\"";
        let diagnostics = [
            Diagnostic::error(codes::E0001, "expected `|` before stage", Span::new(19, 25)),
            Diagnostic::error(codes::E1005, "unknown column `staus`", Span::new(26, 33)),
        ];

        let rendered = render_diagnostics("main.pdl", source, &diagnostics);

        assert!(rendered.contains("\n\nmain.pdl:2:10: error[E1005]"));
    }

    #[test]
    fn diagnostic_rendering_uses_source_snippet_and_caret() {
        let diagnostic = Diagnostic::error(codes::E1005, "unknown column", Span::new(4, 6));

        assert_eq!(
            render_diagnostic("test.pdl", "ab\ncde", &diagnostic),
            "test.pdl:2:2: error[E1005]: unknown column\n2 | cde\n  |  ^^"
        );
    }

    #[test]
    fn diagnostic_rendering_counts_non_ascii_columns_as_chars() {
        let source = "load \"é.csv\"\n  | filter \"staus\" == \"completed\"";
        let start = source.find("\"staus\"").expect("span start");
        let diagnostic = Diagnostic::error(
            codes::E1005,
            "unknown column `staus`",
            Span::new(start, start + "\"staus\"".len()),
        );

        assert_eq!(
            render_diagnostic("main.pdl", source, &diagnostic),
            "main.pdl:2:12: error[E1005]: unknown column `staus`\n2 |   | filter \"staus\" == \"completed\"\n  |            ^^^^^^^"
        );
    }
}
