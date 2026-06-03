use pdl_core::Diagnostic;
use pdl_data::TableSchema;
use std::path::PathBuf;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SchemaCacheKey {
    pub identity: SourceIdentity,
    pub fingerprint: String,
}

impl SchemaCacheKey {
    pub fn new(identity: SourceIdentity, fingerprint: impl Into<String>) -> Self {
        Self {
            identity,
            fingerprint: fingerprint.into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SourceIdentity {
    Path { resolved_path: PathBuf },
    Stdin { host_stream_id: String },
    InMemory { host_file_id: String },
}

#[derive(Clone, Debug, PartialEq)]
pub enum SchemaCacheEntry {
    Schema(TableSchema),
    LoadError(Vec<Diagnostic>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreviewRequest {
    pub key: SchemaCacheKey,
    pub max_rows: usize,
    pub max_bytes: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_cache_key_uses_identity_and_fingerprint() {
        let left = SchemaCacheKey::new(
            SourceIdentity::Path {
                resolved_path: PathBuf::from("/data/sales.csv"),
            },
            "len:10",
        );
        let right = SchemaCacheKey::new(
            SourceIdentity::Path {
                resolved_path: PathBuf::from("/data/sales.csv"),
            },
            "len:11",
        );

        assert_ne!(left, right);
    }
}
