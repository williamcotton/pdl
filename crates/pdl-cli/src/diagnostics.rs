use pdl_core::{line_col, Diagnostic, Severity};
use std::{env, io::IsTerminal};

pub fn print_diagnostics(source_name: &str, source: &str, diagnostics: &[Diagnostic]) {
    let rendered =
        render_diagnostics_with_style(source_name, source, diagnostics, DiagnosticStyle::stderr());
    if !rendered.is_empty() {
        eprintln!("{rendered}");
    }
}

#[cfg(test)]
fn render_diagnostics(source_name: &str, source: &str, diagnostics: &[Diagnostic]) -> String {
    render_diagnostics_with_style(source_name, source, diagnostics, DiagnosticStyle::Plain)
}

fn render_diagnostics_with_style(
    source_name: &str,
    source: &str,
    diagnostics: &[Diagnostic],
    style: DiagnosticStyle,
) -> String {
    diagnostics
        .iter()
        .map(|diagnostic| render_diagnostic_with_style(source_name, source, diagnostic, style))
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[cfg(test)]
fn render_diagnostic(source_name: &str, source: &str, diagnostic: &Diagnostic) -> String {
    render_diagnostic_with_style(source_name, source, diagnostic, DiagnosticStyle::Plain)
}

fn render_diagnostic_with_style(
    source_name: &str,
    source: &str,
    diagnostic: &Diagnostic,
    style: DiagnosticStyle,
) -> String {
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
    let underline = style.underline(&"^".repeat(underline_width), diagnostic.severity);
    let gutter_width = line.to_string().len();
    let gutter_pad = " ".repeat(gutter_width);
    let detail_pad = " ".repeat(gutter_width + 1);
    let label = style.severity_label(diagnostic.severity, diagnostic.code);

    let mut rendered = format!(
        "{}: {}\n{}--> {}:{}:{}\n{} |\n{} | {}\n{} | {}{}",
        label,
        diagnostic.message,
        gutter_pad,
        source_name,
        line,
        col,
        gutter_pad,
        line,
        source_line,
        gutter_pad,
        " ".repeat(underline_padding),
        underline,
    );

    if diagnostic.help.is_some() || !diagnostic.related.is_empty() {
        rendered.push_str(&format!("\n{} |", gutter_pad));
    }
    if let Some(help) = &diagnostic.help {
        rendered.push_str(&format!("\n{}= help: {help}", detail_pad));
    }
    for related in &diagnostic.related {
        let (related_line, related_col) = line_col(source, related.span.start);
        rendered.push_str(&format!(
            "\n{}= note: {} at {}:{}:{}",
            detail_pad, related.message, source_name, related_line, related_col
        ));
    }

    rendered
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DiagnosticStyle {
    Plain,
    Ansi,
}

impl DiagnosticStyle {
    fn stderr() -> Self {
        if env::var_os("NO_COLOR").is_none() && std::io::stderr().is_terminal() {
            Self::Ansi
        } else {
            Self::Plain
        }
    }

    fn severity_label(self, severity: Severity, code: &str) -> String {
        let label = format!("{severity}[{code}]");
        match self {
            Self::Plain => label,
            Self::Ansi => paint(&label, severity_color(severity), true),
        }
    }

    fn underline(self, underline: &str, severity: Severity) -> String {
        match self {
            Self::Plain => underline.to_string(),
            Self::Ansi => paint(underline, severity_color(severity), true),
        }
    }
}

fn severity_color(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "31",
        Severity::Warning => "33",
        Severity::Info => "36",
        Severity::Hint => "2",
    }
}

fn paint(text: &str, color: &str, bold: bool) -> String {
    let weight = if bold { "1;" } else { "" };
    format!("\x1b[{weight}{color}m{text}\x1b[0m")
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

        assert!(rendered.contains("\n\nerror[E1005]: unknown column `staus`"));
        assert!(rendered.contains(" --> main.pdl:2:10"));
    }

    #[test]
    fn diagnostic_rendering_uses_source_snippet_and_caret() {
        let diagnostic = Diagnostic::error(codes::E1005, "unknown column", Span::new(4, 6));

        assert_eq!(
            render_diagnostic("test.pdl", "ab\ncde", &diagnostic),
            "error[E1005]: unknown column\n --> test.pdl:2:2\n  |\n2 | cde\n  |  ^^"
        );
    }

    #[test]
    fn diagnostic_rendering_uses_help_and_related_notes() {
        let diagnostic = Diagnostic::error(codes::E1005, "unknown column", Span::new(4, 6))
            .with_help("check the loaded CSV header")
            .with_related(Span::new(0, 2), "loaded here");

        assert_eq!(
            render_diagnostic("test.pdl", "ab\ncde", &diagnostic),
            "error[E1005]: unknown column\n --> test.pdl:2:2\n  |\n2 | cde\n  |  ^^\n  |\n  = help: check the loaded CSV header\n  = note: loaded here at test.pdl:1:1"
        );
    }

    #[test]
    fn diagnostic_rendering_can_color_severity_and_underline() {
        let diagnostic = Diagnostic::error(codes::E1005, "unknown column", Span::new(4, 6));

        let rendered =
            render_diagnostic_with_style("test.pdl", "ab\ncde", &diagnostic, DiagnosticStyle::Ansi);

        assert!(rendered.contains("\x1b[1;31merror[E1005]\x1b[0m"));
        assert!(rendered.contains("\x1b[1;31m^^\x1b[0m"));
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
            "error[E1005]: unknown column `staus`\n --> main.pdl:2:12\n  |\n2 |   | filter \"staus\" == \"completed\"\n  |            ^^^^^^^"
        );
    }
}
