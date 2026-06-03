use pdl_core::{render_diagnostic, Diagnostic};

pub fn print_diagnostics(source_name: &str, source: &str, diagnostics: &[Diagnostic]) {
    for diagnostic in diagnostics {
        eprintln!("{}", render_diagnostic(source_name, source, diagnostic));
    }
}
