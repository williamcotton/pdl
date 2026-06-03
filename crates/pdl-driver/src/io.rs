use pdl_core::{codes, Diagnostic, Span};
use pdl_data::{read_csv_schema, read_schema_from_bytes, DataFormat};
use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DriverMetadata {
    pub len: Option<u64>,
    pub fingerprint: Option<String>,
}

pub trait DriverIo {
    fn read_source(&self, path: &Path) -> Result<String, Diagnostic>;
    fn read_csv_schema(&self, path: &Path) -> Result<Vec<String>, Diagnostic>;
    fn read_path_bytes(&self, path: &Path) -> Result<Vec<u8>, Diagnostic>;
    fn read_stdin_bytes(&self) -> Result<Vec<u8>, Diagnostic>;
    fn path_metadata(&self, path: &Path) -> Result<DriverMetadata, Diagnostic>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct OsDriverIo;

impl DriverIo for OsDriverIo {
    fn read_source(&self, path: &Path) -> Result<String, Diagnostic> {
        fs::read_to_string(path).map_err(|error| {
            Diagnostic::error(
                codes::E1802,
                format!("could not read PDL file `{}`: {error}", path.display()),
                Span::zero(),
            )
        })
    }

    fn read_csv_schema(&self, path: &Path) -> Result<Vec<String>, Diagnostic> {
        read_csv_schema(path)
    }

    fn read_path_bytes(&self, path: &Path) -> Result<Vec<u8>, Diagnostic> {
        fs::read(path).map_err(|error| {
            Diagnostic::error(
                codes::E1802,
                format!("could not read data file `{}`: {error}", path.display()),
                Span::zero(),
            )
        })
    }

    fn read_stdin_bytes(&self) -> Result<Vec<u8>, Diagnostic> {
        let mut bytes = Vec::new();
        std::io::stdin().read_to_end(&mut bytes).map_err(|error| {
            Diagnostic::error(
                codes::E1806,
                format!("stdin read failed: {error}"),
                Span::zero(),
            )
        })?;
        Ok(bytes)
    }

    fn path_metadata(&self, path: &Path) -> Result<DriverMetadata, Diagnostic> {
        fs::metadata(path)
            .map(|metadata| DriverMetadata {
                len: Some(metadata.len()),
                fingerprint: Some(format!("len:{}", metadata.len())),
            })
            .map_err(|error| {
                Diagnostic::error(
                    codes::E1802,
                    format!("could not stat data file `{}`: {error}", path.display()),
                    Span::zero(),
                )
            })
    }
}

#[derive(Clone, Debug, Default)]
pub struct InMemoryDriverIo {
    sources: BTreeMap<PathBuf, String>,
    files: BTreeMap<PathBuf, Vec<u8>>,
    stdin: Option<Vec<u8>>,
    schemas: BTreeMap<PathBuf, Vec<String>>,
}

impl InMemoryDriverIo {
    pub fn with_source(mut self, path: impl Into<PathBuf>, source: impl Into<String>) -> Self {
        self.sources.insert(path.into(), source.into());
        self
    }

    pub fn with_schema(
        mut self,
        path: impl Into<PathBuf>,
        columns: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.schemas
            .insert(path.into(), columns.into_iter().map(Into::into).collect());
        self
    }

    pub fn with_file_bytes(mut self, path: impl Into<PathBuf>, bytes: impl Into<Vec<u8>>) -> Self {
        self.files.insert(path.into(), bytes.into());
        self
    }

    pub fn with_stdin_bytes(mut self, bytes: impl Into<Vec<u8>>) -> Self {
        self.stdin = Some(bytes.into());
        self
    }
}

impl DriverIo for InMemoryDriverIo {
    fn read_source(&self, path: &Path) -> Result<String, Diagnostic> {
        self.sources.get(path).cloned().ok_or_else(|| {
            Diagnostic::error(
                codes::E1802,
                format!("in-memory PDL source `{}` was not provided", path.display()),
                Span::zero(),
            )
        })
    }

    fn read_csv_schema(&self, path: &Path) -> Result<Vec<String>, Diagnostic> {
        if let Some(schema) = self.schemas.get(path) {
            return Ok(schema.clone());
        }
        if let Some(bytes) = self.files.get(path) {
            return read_schema_from_bytes(path, DataFormat::Csv, bytes);
        }
        Err(Diagnostic::error(
            codes::E1818,
            format!("in-memory schema for `{}` was not provided", path.display()),
            Span::zero(),
        ))
    }

    fn read_path_bytes(&self, path: &Path) -> Result<Vec<u8>, Diagnostic> {
        self.files.get(path).cloned().ok_or_else(|| {
            Diagnostic::error(
                codes::E1802,
                format!("in-memory bytes for `{}` were not provided", path.display()),
                Span::zero(),
            )
        })
    }

    fn read_stdin_bytes(&self) -> Result<Vec<u8>, Diagnostic> {
        self.stdin.clone().ok_or_else(|| {
            Diagnostic::error(
                codes::E1806,
                "in-memory stdin bytes were not provided",
                Span::zero(),
            )
        })
    }

    fn path_metadata(&self, path: &Path) -> Result<DriverMetadata, Diagnostic> {
        self.files
            .get(path)
            .map(|bytes| DriverMetadata {
                len: Some(bytes.len() as u64),
                fingerprint: Some(format!("memory-len:{}", bytes.len())),
            })
            .ok_or_else(|| {
                Diagnostic::error(
                    codes::E1802,
                    format!(
                        "in-memory metadata for `{}` was not provided",
                        path.display()
                    ),
                    Span::zero(),
                )
            })
    }
}
