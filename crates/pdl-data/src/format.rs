use pdl_core::{codes, Diagnostic, Span};
use std::path::Path;

use crate::csv::{read_csv_from_bytes, read_csv_schema_from_bytes};
use crate::frame::Table;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DataFormat {
    Csv,
    Parquet,
    ArrowFile,
    ArrowStream,
    JsonLines,
}

impl DataFormat {
    pub fn canonical_name(self) -> &'static str {
        match self {
            DataFormat::Csv => "csv",
            DataFormat::Parquet => "parquet",
            DataFormat::ArrowFile => "arrow-file",
            DataFormat::ArrowStream => "arrow-stream",
            DataFormat::JsonLines => "jsonl",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "csv" => Some(DataFormat::Csv),
            "parquet" => Some(DataFormat::Parquet),
            "arrow-file" | "ipc" => Some(DataFormat::ArrowFile),
            "arrow-stream" | "arrow" => Some(DataFormat::ArrowStream),
            "jsonl" | "ndjson" => Some(DataFormat::JsonLines),
            _ => None,
        }
    }

    pub fn infer_from_path(path: impl AsRef<Path>) -> Option<Self> {
        match path
            .as_ref()
            .extension()
            .and_then(|extension| extension.to_str())
        {
            Some("csv") => Some(DataFormat::Csv),
            Some("parquet") | Some("pq") => Some(DataFormat::Parquet),
            Some("arrow") | Some("feather") => Some(DataFormat::ArrowFile),
            Some("jsonl") | Some("ndjson") => Some(DataFormat::JsonLines),
            _ => None,
        }
    }
}

pub fn read_schema_from_bytes(
    path: &Path,
    format: DataFormat,
    bytes: &[u8],
) -> Result<Vec<String>, Diagnostic> {
    match format {
        DataFormat::Csv => read_csv_schema_from_bytes(path, bytes),
        _ => Err(unsupported_input_format(format)),
    }
}

pub fn read_table_from_bytes(
    path: &Path,
    format: DataFormat,
    bytes: &[u8],
) -> Result<Table, Diagnostic> {
    match format {
        DataFormat::Csv => read_csv_from_bytes(path, bytes),
        _ => Err(unsupported_input_format(format)),
    }
}

fn unsupported_input_format(format: DataFormat) -> Diagnostic {
    Diagnostic::error(
        codes::E1215,
        format!(
            "format `{}` is not supported by the current data engine",
            format.canonical_name()
        ),
        Span::zero(),
    )
}

pub fn format_number(value: f64) -> String {
    if value.is_finite() && value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        let mut rendered = value.to_string();
        if rendered.contains('.') {
            while rendered.ends_with('0') {
                rendered.pop();
            }
            if rendered.ends_with('.') {
                rendered.push('0');
            }
        }
        rendered
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_format_is_stable_for_integer_values() {
        assert_eq!(format_number(10.0), "10");
        assert_eq!(format_number(10.5), "10.5");
    }
}
