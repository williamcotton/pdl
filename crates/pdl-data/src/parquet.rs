use bytes::Bytes;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::arrow::ArrowWriter;
use pdl_core::{codes, Diagnostic, Span};
use std::path::Path;

use crate::arrow::{rows_from_batch, table_to_batch};
use crate::Table;

pub fn read_parquet_schema_from_bytes(
    path: &Path,
    bytes: &[u8],
) -> Result<Vec<String>, Diagnostic> {
    let builder = parquet_reader_builder(path, bytes)?;
    Ok(builder
        .schema()
        .fields()
        .iter()
        .map(|field| field.name().clone())
        .collect())
}

pub fn read_parquet_from_bytes(path: &Path, bytes: &[u8]) -> Result<Table, Diagnostic> {
    let builder = parquet_reader_builder(path, bytes)?;
    let schema = builder.schema().clone();
    let columns: Vec<String> = schema
        .fields()
        .iter()
        .map(|field| field.name().clone())
        .collect();
    let mut reader = builder
        .build()
        .map_err(|error| parquet_read_error(path, error))?;
    let mut rows = Vec::new();

    for batch in &mut reader {
        let batch = batch.map_err(|error| parquet_read_error(path, error.into()))?;
        rows.extend(rows_from_batch(path, "Parquet", &batch)?);
    }

    Ok(Table { columns, rows })
}

pub fn write_parquet_to_vec(table: &Table) -> Result<Vec<u8>, Diagnostic> {
    let batch = table_to_batch(table)?;
    let mut bytes = Vec::new();
    {
        let mut writer =
            ArrowWriter::try_new(&mut bytes, batch.schema(), None).map_err(parquet_write_error)?;
        writer.write(&batch).map_err(parquet_write_error)?;
        writer.close().map_err(parquet_write_error)?;
    }
    Ok(bytes)
}

fn parquet_reader_builder(
    path: &Path,
    bytes: &[u8],
) -> Result<ParquetRecordBatchReaderBuilder<Bytes>, Diagnostic> {
    ParquetRecordBatchReaderBuilder::try_new(Bytes::copy_from_slice(bytes))
        .map_err(|error| parquet_read_error(path, error))
}

fn parquet_read_error(path: &Path, error: parquet::errors::ParquetError) -> Diagnostic {
    Diagnostic::error(
        codes::E1804,
        format!("Parquet parse failed for `{}`: {error}", path.display()),
        Span::zero(),
    )
}

fn parquet_write_error(error: parquet::errors::ParquetError) -> Diagnostic {
    Diagnostic::error(
        codes::E1704,
        format!("Parquet write failed: {error}"),
        Span::zero(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Row, Value};

    #[test]
    fn parquet_round_trips_supported_values() {
        let table = Table::new(
            vec!["region".to_string(), "amount".to_string()],
            vec![Row {
                values: vec![Value::String("West".to_string()), Value::Number(350.0)],
            }],
        );

        let bytes = write_parquet_to_vec(&table).expect("write parquet");
        assert!(bytes.starts_with(b"PAR1"));
        assert!(bytes.ends_with(b"PAR1"));
        assert_eq!(
            read_parquet_schema_from_bytes(Path::new("memory.parquet"), &bytes).expect("schema"),
            table.columns
        );
        assert_eq!(
            read_parquet_from_bytes(Path::new("memory.parquet"), &bytes).expect("read table"),
            table
        );
    }
}
