use std::path::PathBuf;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SourceInput {
    Stdin,
    Inline { label: String },
    Path(PathBuf),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceOrigin {
    pub input: SourceInput,
}

impl SourceOrigin {
    pub fn path(path: impl Into<PathBuf>) -> Self {
        Self {
            input: SourceInput::Path(path.into()),
        }
    }

    pub fn inline(label: impl Into<String>) -> Self {
        Self {
            input: SourceInput::Inline {
                label: label.into(),
            },
        }
    }

    pub fn stdin() -> Self {
        Self {
            input: SourceInput::Stdin,
        }
    }
}
