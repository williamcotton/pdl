use thiserror::Error;

#[derive(Debug, Error)]
pub enum PdlError {
    #[error("{0}")]
    Diagnostic(String),
    #[error("I/O failed: {0}")]
    Io(String),
}
