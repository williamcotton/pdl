use std::path::{Path, PathBuf};

pub fn resolve_input_path(program_path: &Path, source: &str) -> PathBuf {
    let path = PathBuf::from(source);
    if path.is_absolute() {
        path
    } else {
        program_path
            .parent()
            .map_or_else(|| PathBuf::from(source), |parent| parent.join(source))
    }
}

pub fn resolve_output_path(source: &str) -> PathBuf {
    PathBuf::from(source)
}
