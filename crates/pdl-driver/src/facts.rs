use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub trait ExternalFacts {
    fn schema_for_path(&self, path: &Path) -> Option<Vec<String>>;
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InMemoryFacts {
    schemas: BTreeMap<PathBuf, Vec<String>>,
}

impl InMemoryFacts {
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

impl ExternalFacts for InMemoryFacts {
    fn schema_for_path(&self, path: &Path) -> Option<Vec<String>> {
        self.schemas.get(path).cloned()
    }
}
