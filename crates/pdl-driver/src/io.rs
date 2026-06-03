use pdl_core::{codes, Diagnostic, Span};
use pdl_data::read_csv_schema;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub trait DriverIo {
    fn read_source(&self, path: &Path) -> Result<String, Diagnostic>;
    fn read_csv_schema(&self, path: &Path) -> Result<Vec<String>, Diagnostic>;
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
}

#[derive(Clone, Debug, Default)]
pub struct InMemoryDriverIo {
    sources: BTreeMap<PathBuf, String>,
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
        self.schemas.get(path).cloned().ok_or_else(|| {
            Diagnostic::error(
                codes::E1818,
                format!("in-memory schema for `{}` was not provided", path.display()),
                Span::zero(),
            )
        })
    }
}
