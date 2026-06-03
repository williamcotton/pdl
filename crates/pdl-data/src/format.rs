use pdl_core::{codes, Diagnostic, Span};
use std::fs;
use std::path::Path;

#[cfg(feature = "arrow-ipc")]
use crate::arrow::{
    read_arrow_file_from_bytes, read_arrow_file_schema_from_bytes, read_arrow_stream_from_bytes,
    read_arrow_stream_schema_from_bytes, write_arrow_file_to_vec, write_arrow_stream_to_vec,
};
use crate::csv::{read_csv_from_bytes, read_csv_schema_from_bytes};
use crate::frame::Table;
use crate::jsonl::{
    read_json_lines_from_bytes, read_json_lines_schema_from_bytes, write_json_lines_to_vec,
};
#[cfg(feature = "parquet")]
use crate::parquet::{
    read_parquet_from_bytes, read_parquet_schema_from_bytes, write_parquet_to_vec,
};

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

    pub fn is_supported_input(self) -> bool {
        match self {
            DataFormat::Csv => true,
            DataFormat::ArrowFile | DataFormat::ArrowStream => cfg!(feature = "arrow-ipc"),
            DataFormat::Parquet => cfg!(feature = "parquet"),
            DataFormat::JsonLines => true,
        }
    }

    pub fn is_supported_output(self) -> bool {
        match self {
            DataFormat::Csv => true,
            DataFormat::ArrowFile | DataFormat::ArrowStream => cfg!(feature = "arrow-ipc"),
            DataFormat::Parquet => cfg!(feature = "parquet"),
            DataFormat::JsonLines => true,
        }
    }

    pub fn is_binary(self) -> bool {
        matches!(
            self,
            DataFormat::Parquet | DataFormat::ArrowFile | DataFormat::ArrowStream
        )
    }
}

pub fn sniff_format_from_bytes(bytes: &[u8]) -> Result<DataFormat, Diagnostic> {
    if bytes.starts_with(b"PAR1") {
        return Ok(DataFormat::Parquet);
    }
    if bytes.starts_with(b"ARROW1") {
        return Ok(DataFormat::ArrowFile);
    }
    if bytes.starts_with(&[0xff, 0xff, 0xff, 0xff]) {
        return Ok(DataFormat::ArrowStream);
    }
    if let Ok(text) = std::str::from_utf8(bytes) {
        let trimmed = text.trim_start_matches(|character: char| character.is_whitespace());
        if trimmed.starts_with('{') || trimmed.starts_with('[') {
            return Ok(DataFormat::JsonLines);
        }
        return Ok(DataFormat::Csv);
    }

    Err(Diagnostic::error(
        codes::E1216,
        "could not infer supported format from stream bytes",
        Span::zero(),
    ))
}

pub fn read_schema_from_bytes(
    path: &Path,
    format: DataFormat,
    bytes: &[u8],
) -> Result<Vec<String>, Diagnostic> {
    match format {
        DataFormat::Csv => read_csv_schema_from_bytes(path, bytes),
        DataFormat::Parquet => read_parquet_schema(path, format, bytes),
        DataFormat::ArrowFile => read_arrow_file_schema(path, format, bytes),
        DataFormat::ArrowStream => read_arrow_stream_schema(path, format, bytes),
        DataFormat::JsonLines => read_json_lines_schema_from_bytes(path, bytes),
    }
}

pub fn read_table_from_bytes(
    path: &Path,
    format: DataFormat,
    bytes: &[u8],
) -> Result<Table, Diagnostic> {
    match format {
        DataFormat::Csv => read_csv_from_bytes(path, bytes),
        DataFormat::Parquet => read_parquet_table(path, format, bytes),
        DataFormat::ArrowFile => read_arrow_file_table(path, format, bytes),
        DataFormat::ArrowStream => read_arrow_stream_table(path, format, bytes),
        DataFormat::JsonLines => read_json_lines_from_bytes(path, bytes),
    }
}

pub fn write_table_to_bytes(format: DataFormat, table: &Table) -> Result<Vec<u8>, Diagnostic> {
    match format {
        DataFormat::Csv => crate::csv::write_csv_to_vec(table),
        DataFormat::Parquet => write_parquet_bytes(format, table),
        DataFormat::ArrowFile => write_arrow_file_bytes(format, table),
        DataFormat::ArrowStream => write_arrow_stream_bytes(format, table),
        DataFormat::JsonLines => write_json_lines_to_vec(table),
    }
}

#[cfg(feature = "parquet")]
fn read_parquet_schema(
    path: &Path,
    _format: DataFormat,
    bytes: &[u8],
) -> Result<Vec<String>, Diagnostic> {
    read_parquet_schema_from_bytes(path, bytes)
}

#[cfg(not(feature = "parquet"))]
fn read_parquet_schema(
    _path: &Path,
    format: DataFormat,
    _bytes: &[u8],
) -> Result<Vec<String>, Diagnostic> {
    Err(unsupported_input_format(format))
}

#[cfg(feature = "parquet")]
fn read_parquet_table(path: &Path, _format: DataFormat, bytes: &[u8]) -> Result<Table, Diagnostic> {
    read_parquet_from_bytes(path, bytes)
}

#[cfg(not(feature = "parquet"))]
fn read_parquet_table(
    _path: &Path,
    format: DataFormat,
    _bytes: &[u8],
) -> Result<Table, Diagnostic> {
    Err(unsupported_input_format(format))
}

#[cfg(feature = "parquet")]
fn write_parquet_bytes(format: DataFormat, table: &Table) -> Result<Vec<u8>, Diagnostic> {
    let _ = format;
    write_parquet_to_vec(table)
}

#[cfg(not(feature = "parquet"))]
fn write_parquet_bytes(format: DataFormat, _table: &Table) -> Result<Vec<u8>, Diagnostic> {
    Err(unsupported_output_format(format))
}

#[cfg(feature = "arrow-ipc")]
fn read_arrow_file_schema(
    path: &Path,
    _format: DataFormat,
    bytes: &[u8],
) -> Result<Vec<String>, Diagnostic> {
    read_arrow_file_schema_from_bytes(path, bytes)
}

#[cfg(not(feature = "arrow-ipc"))]
fn read_arrow_file_schema(
    _path: &Path,
    format: DataFormat,
    _bytes: &[u8],
) -> Result<Vec<String>, Diagnostic> {
    Err(unsupported_input_format(format))
}

#[cfg(feature = "arrow-ipc")]
fn read_arrow_file_table(
    path: &Path,
    _format: DataFormat,
    bytes: &[u8],
) -> Result<Table, Diagnostic> {
    read_arrow_file_from_bytes(path, bytes)
}

#[cfg(not(feature = "arrow-ipc"))]
fn read_arrow_file_table(
    _path: &Path,
    format: DataFormat,
    _bytes: &[u8],
) -> Result<Table, Diagnostic> {
    Err(unsupported_input_format(format))
}

#[cfg(feature = "arrow-ipc")]
fn write_arrow_file_bytes(format: DataFormat, table: &Table) -> Result<Vec<u8>, Diagnostic> {
    let _ = format;
    write_arrow_file_to_vec(table)
}

#[cfg(not(feature = "arrow-ipc"))]
fn write_arrow_file_bytes(format: DataFormat, _table: &Table) -> Result<Vec<u8>, Diagnostic> {
    Err(unsupported_output_format(format))
}

#[cfg(feature = "arrow-ipc")]
fn read_arrow_stream_schema(
    path: &Path,
    _format: DataFormat,
    bytes: &[u8],
) -> Result<Vec<String>, Diagnostic> {
    read_arrow_stream_schema_from_bytes(path, bytes)
}

#[cfg(not(feature = "arrow-ipc"))]
fn read_arrow_stream_schema(
    _path: &Path,
    format: DataFormat,
    _bytes: &[u8],
) -> Result<Vec<String>, Diagnostic> {
    Err(unsupported_input_format(format))
}

#[cfg(feature = "arrow-ipc")]
fn read_arrow_stream_table(
    path: &Path,
    _format: DataFormat,
    bytes: &[u8],
) -> Result<Table, Diagnostic> {
    read_arrow_stream_from_bytes(path, bytes)
}

#[cfg(not(feature = "arrow-ipc"))]
fn read_arrow_stream_table(
    _path: &Path,
    format: DataFormat,
    _bytes: &[u8],
) -> Result<Table, Diagnostic> {
    Err(unsupported_input_format(format))
}

#[cfg(feature = "arrow-ipc")]
fn write_arrow_stream_bytes(format: DataFormat, table: &Table) -> Result<Vec<u8>, Diagnostic> {
    let _ = format;
    write_arrow_stream_to_vec(table)
}

#[cfg(not(feature = "arrow-ipc"))]
fn write_arrow_stream_bytes(format: DataFormat, _table: &Table) -> Result<Vec<u8>, Diagnostic> {
    Err(unsupported_output_format(format))
}

pub fn write_table_to_path(
    path: &Path,
    format: DataFormat,
    table: &Table,
) -> Result<(), Diagnostic> {
    let bytes = write_table_to_bytes(format, table)?;
    fs::write(path, bytes).map_err(|error| {
        Diagnostic::error(
            codes::E1704,
            format!(
                "output file `{}` could not be written: {error}",
                path.display()
            ),
            Span::zero(),
        )
    })
}

#[cfg(any(not(feature = "arrow-ipc"), not(feature = "parquet")))]
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

#[cfg(any(not(feature = "arrow-ipc"), not(feature = "parquet")))]
fn unsupported_output_format(format: DataFormat) -> Diagnostic {
    Diagnostic::error(
        codes::E1705,
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
