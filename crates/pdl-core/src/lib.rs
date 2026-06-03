pub mod diagnostic;
pub mod error;
pub mod severity;
pub mod source;
pub mod span;

pub use diagnostic::{all_codes, codes, Diagnostic, DiagnosticCode, RelatedSpan};
pub use error::PdlError;
pub use severity::Severity;
pub use source::line_col;
pub use span::{ByteOffset, Span};

pub fn has_errors(diagnostics: &[Diagnostic]) -> bool {
    diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == Severity::Error)
}
