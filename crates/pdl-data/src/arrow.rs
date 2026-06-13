use arrow_array::cast::AsArray;
use arrow_array::types::{
    Float32Type, Float64Type, Int16Type, Int32Type, Int64Type, Int8Type, UInt16Type, UInt32Type,
    UInt64Type, UInt8Type,
};
use arrow_array::{Array, ArrayRef, BooleanArray, Float64Array, RecordBatch, StringArray};
use arrow_ipc::reader::{FileReader, StreamReader};
use arrow_ipc::writer::{FileWriter, StreamWriter};
use arrow_schema::{DataType, Field, Schema};
use pdl_core::{codes, Diagnostic, Span};
use std::io::Cursor;
use std::path::Path;
use std::sync::Arc;

use crate::{format_number, Row, Table, Value};

pub fn read_arrow_stream_schema_from_bytes(
    path: &Path,
    bytes: &[u8],
) -> Result<Vec<String>, Diagnostic> {
    let reader = stream_reader(path, bytes)?;
    Ok(reader
        .schema()
        .fields()
        .iter()
        .map(|field| field.name().clone())
        .collect())
}

pub fn read_arrow_stream_from_bytes(path: &Path, bytes: &[u8]) -> Result<Table, Diagnostic> {
    let mut reader = stream_reader(path, bytes)?;
    let schema = reader.schema();
    let columns: Vec<String> = schema
        .fields()
        .iter()
        .map(|field| field.name().clone())
        .collect();
    let mut rows = Vec::new();

    for batch in &mut reader {
        let batch = batch.map_err(|error| arrow_read_error(path, error))?;
        rows.extend(rows_from_batch(path, "Arrow IPC stream", &batch)?);
    }

    Ok(Table { columns, rows })
}

pub fn read_arrow_file_schema_from_bytes(
    path: &Path,
    bytes: &[u8],
) -> Result<Vec<String>, Diagnostic> {
    let reader = file_reader(path, bytes)?;
    Ok(reader
        .schema()
        .fields()
        .iter()
        .map(|field| field.name().clone())
        .collect())
}

pub fn read_arrow_file_from_bytes(path: &Path, bytes: &[u8]) -> Result<Table, Diagnostic> {
    let mut reader = file_reader(path, bytes)?;
    let schema = reader.schema();
    let columns: Vec<String> = schema
        .fields()
        .iter()
        .map(|field| field.name().clone())
        .collect();
    let mut rows = Vec::new();

    for batch in &mut reader {
        let batch = batch.map_err(|error| arrow_file_read_error(path, error))?;
        rows.extend(rows_from_batch(path, "Arrow IPC file", &batch)?);
    }

    Ok(Table { columns, rows })
}

pub fn write_arrow_stream_to_vec(table: &Table) -> Result<Vec<u8>, Diagnostic> {
    let batch = table_to_batch(table)?;
    let mut bytes = Vec::new();
    {
        let mut writer =
            StreamWriter::try_new(&mut bytes, batch.schema_ref()).map_err(|error| {
                Diagnostic::error(
                    codes::E1704,
                    format!("Arrow IPC stream header write failed: {error}"),
                    Span::zero(),
                )
            })?;
        writer.write(&batch).map_err(|error| {
            Diagnostic::error(
                codes::E1704,
                format!("Arrow IPC stream batch write failed: {error}"),
                Span::zero(),
            )
        })?;
        writer.finish().map_err(|error| {
            Diagnostic::error(
                codes::E1704,
                format!("Arrow IPC stream finish failed: {error}"),
                Span::zero(),
            )
        })?;
    }
    Ok(bytes)
}

pub fn write_arrow_file_to_vec(table: &Table) -> Result<Vec<u8>, Diagnostic> {
    let batch = table_to_batch(table)?;
    let mut bytes = Vec::new();
    {
        let mut writer = FileWriter::try_new(&mut bytes, batch.schema_ref()).map_err(|error| {
            Diagnostic::error(
                codes::E1704,
                format!("Arrow IPC file header write failed: {error}"),
                Span::zero(),
            )
        })?;
        writer.write(&batch).map_err(|error| {
            Diagnostic::error(
                codes::E1704,
                format!("Arrow IPC file batch write failed: {error}"),
                Span::zero(),
            )
        })?;
        writer.finish().map_err(|error| {
            Diagnostic::error(
                codes::E1704,
                format!("Arrow IPC file finish failed: {error}"),
                Span::zero(),
            )
        })?;
    }
    Ok(bytes)
}

fn stream_reader<'a>(
    path: &Path,
    bytes: &'a [u8],
) -> Result<StreamReader<Cursor<&'a [u8]>>, Diagnostic> {
    StreamReader::try_new(Cursor::new(bytes), None).map_err(|error| arrow_read_error(path, error))
}

fn file_reader<'a>(
    path: &Path,
    bytes: &'a [u8],
) -> Result<FileReader<Cursor<&'a [u8]>>, Diagnostic> {
    FileReader::try_new(Cursor::new(bytes), None)
        .map_err(|error| arrow_file_read_error(path, error))
}

pub(crate) fn rows_from_batch(
    path: &Path,
    format_label: &str,
    batch: &RecordBatch,
) -> Result<Vec<Row>, Diagnostic> {
    let mut rows = Vec::with_capacity(batch.num_rows());
    for row_index in 0..batch.num_rows() {
        let mut values = Vec::with_capacity(batch.num_columns());
        for column_index in 0..batch.num_columns() {
            let field = batch.schema().field(column_index).clone();
            values.push(value_from_array(
                path,
                format_label,
                field.name(),
                batch.column(column_index).as_ref(),
                row_index,
            )?);
        }
        rows.push(Row { values });
    }
    Ok(rows)
}

fn value_from_array(
    path: &Path,
    format_label: &str,
    column: &str,
    array: &dyn Array,
    row_index: usize,
) -> Result<Value, Diagnostic> {
    if array.is_null(row_index) {
        return Ok(Value::Null);
    }

    match array.data_type() {
        DataType::Boolean => Ok(Value::Bool(array.as_boolean().value(row_index))),
        DataType::Float64 => Ok(Value::Number(
            array.as_primitive::<Float64Type>().value(row_index),
        )),
        DataType::Float32 => Ok(Value::Number(f64::from(
            array.as_primitive::<Float32Type>().value(row_index),
        ))),
        DataType::Int8 => Ok(Value::Number(
            array.as_primitive::<Int8Type>().value(row_index) as f64,
        )),
        DataType::Int16 => Ok(Value::Number(
            array.as_primitive::<Int16Type>().value(row_index) as f64,
        )),
        DataType::Int32 => Ok(Value::Number(
            array.as_primitive::<Int32Type>().value(row_index) as f64,
        )),
        DataType::Int64 => Ok(Value::Number(
            array.as_primitive::<Int64Type>().value(row_index) as f64,
        )),
        DataType::UInt8 => Ok(Value::Number(
            array.as_primitive::<UInt8Type>().value(row_index) as f64,
        )),
        DataType::UInt16 => Ok(Value::Number(
            array.as_primitive::<UInt16Type>().value(row_index) as f64,
        )),
        DataType::UInt32 => Ok(Value::Number(
            array.as_primitive::<UInt32Type>().value(row_index) as f64,
        )),
        DataType::UInt64 => Ok(Value::Number(
            array.as_primitive::<UInt64Type>().value(row_index) as f64,
        )),
        DataType::Utf8 => Ok(Value::String(
            array.as_string::<i32>().value(row_index).to_string(),
        )),
        DataType::LargeUtf8 => Ok(Value::String(
            array.as_string::<i64>().value(row_index).to_string(),
        )),
        DataType::Utf8View => Ok(Value::String(
            array.as_string_view().value(row_index).to_string(),
        )),
        DataType::Null => Ok(Value::Null),
        data_type => Err(Diagnostic::error(
            if format_label == "Parquet" {
                codes::E1808
            } else {
                codes::E1215
            },
            format!(
                "{format_label} column `{column}` in `{}` has unsupported data type `{data_type}`",
                path.display()
            ),
            Span::zero(),
        )),
    }
}

pub(crate) fn table_to_batch(table: &Table) -> Result<RecordBatch, Diagnostic> {
    let column_types: Vec<ColumnArrowType> = (0..table.columns.len())
        .map(|column_index| infer_column_type(table, column_index))
        .collect();
    let fields: Vec<Field> = table
        .columns
        .iter()
        .enumerate()
        .map(|(index, column)| {
            Field::new(
                column,
                column_types[index].data_type(),
                column_is_nullable(table, index),
            )
        })
        .collect();
    let schema = Arc::new(Schema::new(fields));
    let arrays: Vec<ArrayRef> = column_types
        .iter()
        .enumerate()
        .map(|(index, column_type)| array_for_column(table, index, *column_type))
        .collect();

    RecordBatch::try_new(schema, arrays).map_err(|error| {
        Diagnostic::error(
            codes::E1704,
            format!("Arrow record batch build failed: {error}"),
            Span::zero(),
        )
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ColumnArrowType {
    Boolean,
    Float64,
    Utf8,
}

impl ColumnArrowType {
    fn data_type(self) -> DataType {
        match self {
            ColumnArrowType::Boolean => DataType::Boolean,
            ColumnArrowType::Float64 => DataType::Float64,
            ColumnArrowType::Utf8 => DataType::Utf8,
        }
    }
}

fn infer_column_type(table: &Table, column_index: usize) -> ColumnArrowType {
    let mut observed = None;
    for row in &table.rows {
        let value = row.values.get(column_index).unwrap_or(&Value::Null);
        let value_type = match value {
            Value::Null => continue,
            Value::Bool(_) => ColumnArrowType::Boolean,
            Value::Number(_) => ColumnArrowType::Float64,
            Value::String(_) => ColumnArrowType::Utf8,
            // Geometry is rejected before Arrow encoding (PDL_SPEC §10.13).
            Value::Geometry(_) => continue,
        };
        match observed {
            None => observed = Some(value_type),
            Some(existing) if existing == value_type => {}
            Some(_) => return ColumnArrowType::Utf8,
        }
    }
    observed.unwrap_or(ColumnArrowType::Utf8)
}

fn column_is_nullable(table: &Table, column_index: usize) -> bool {
    table
        .rows
        .iter()
        .any(|row| matches!(row.values.get(column_index), None | Some(Value::Null)))
}

fn array_for_column(table: &Table, column_index: usize, column_type: ColumnArrowType) -> ArrayRef {
    match column_type {
        ColumnArrowType::Boolean => {
            Arc::new(BooleanArray::from_iter(table.rows.iter().map(|row| {
                match row.values.get(column_index).unwrap_or(&Value::Null) {
                    Value::Bool(value) => Some(*value),
                    Value::Null => None,
                    value => Some(value.to_csv_cell() == "true"),
                }
            })))
        }
        ColumnArrowType::Float64 => {
            Arc::new(Float64Array::from_iter(table.rows.iter().map(|row| {
                match row.values.get(column_index).unwrap_or(&Value::Null) {
                    Value::Number(value) if value.is_finite() => Some(*value),
                    Value::Number(value) => Some(*value),
                    Value::Null => None,
                    value => value.to_csv_cell().parse::<f64>().ok(),
                }
            })))
        }
        ColumnArrowType::Utf8 => Arc::new(StringArray::from_iter(table.rows.iter().map(|row| {
            match row.values.get(column_index).unwrap_or(&Value::Null) {
                Value::Null => None,
                Value::Number(value) => Some(format_number(*value)),
                value => Some(value.to_csv_cell()),
            }
        }))),
    }
}

fn arrow_read_error(path: &Path, error: arrow_schema::ArrowError) -> Diagnostic {
    Diagnostic::error(
        codes::E1804,
        format!(
            "Arrow IPC stream parse failed for `{}`: {error}",
            path.display()
        ),
        Span::zero(),
    )
}

fn arrow_file_read_error(path: &Path, error: arrow_schema::ArrowError) -> Diagnostic {
    Diagnostic::error(
        codes::E1804,
        format!(
            "Arrow IPC file parse failed for `{}`: {error}",
            path.display()
        ),
        Span::zero(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrow_stream_round_trips_supported_values() {
        let table = Table::new(
            vec![
                "name".to_string(),
                "amount".to_string(),
                "active".to_string(),
                "mixed".to_string(),
            ],
            vec![
                Row {
                    values: vec![
                        Value::String("North".to_string()),
                        Value::Number(10.0),
                        Value::Bool(true),
                        Value::Number(7.0),
                    ],
                },
                Row {
                    values: vec![
                        Value::Null,
                        Value::Number(12.5),
                        Value::Bool(false),
                        Value::String("other".to_string()),
                    ],
                },
            ],
        );

        let bytes = write_arrow_stream_to_vec(&table).expect("write arrow stream");
        assert!(bytes.starts_with(&[0xff, 0xff, 0xff, 0xff]));
        assert_eq!(
            read_arrow_stream_schema_from_bytes(Path::new("memory.arrow"), &bytes).expect("schema"),
            table.columns
        );
        assert_eq!(
            read_arrow_stream_from_bytes(Path::new("memory.arrow"), &bytes).expect("read table"),
            Table::new(
                table.columns,
                vec![
                    Row {
                        values: vec![
                            Value::String("North".to_string()),
                            Value::Number(10.0),
                            Value::Bool(true),
                            Value::String("7".to_string()),
                        ],
                    },
                    Row {
                        values: vec![
                            Value::Null,
                            Value::Number(12.5),
                            Value::Bool(false),
                            Value::String("other".to_string()),
                        ],
                    },
                ],
            )
        );
    }

    #[test]
    fn arrow_stream_output_is_deterministic() {
        let table = Table::new(
            vec!["region".to_string(), "amount".to_string()],
            vec![Row {
                values: vec![Value::String("West".to_string()), Value::Number(350.0)],
            }],
        );

        let first = write_arrow_stream_to_vec(&table).expect("first write");
        let second = write_arrow_stream_to_vec(&table).expect("second write");

        assert_eq!(first, second);
    }

    #[test]
    fn arrow_file_round_trips_supported_values() {
        let table = Table::new(
            vec!["region".to_string(), "amount".to_string()],
            vec![Row {
                values: vec![Value::String("West".to_string()), Value::Number(350.0)],
            }],
        );

        let bytes = write_arrow_file_to_vec(&table).expect("write arrow file");
        assert!(bytes.starts_with(b"ARROW1"));
        assert!(bytes.ends_with(b"ARROW1"));
        assert_eq!(
            read_arrow_file_schema_from_bytes(Path::new("memory.arrow"), &bytes).expect("schema"),
            table.columns
        );
        assert_eq!(
            read_arrow_file_from_bytes(Path::new("memory.arrow"), &bytes).expect("read table"),
            table
        );
    }

    #[test]
    fn rows_from_batch_reads_utf8_view_strings() {
        let schema = Arc::new(Schema::new(vec![Field::new(
            "region",
            DataType::Utf8View,
            true,
        )]));
        let values = Arc::new(arrow_array::StringViewArray::from(vec![
            Some("East"),
            None,
            Some("West"),
        ])) as ArrayRef;
        let batch = RecordBatch::try_new(schema, vec![values]).expect("record batch");

        assert_eq!(
            rows_from_batch(Path::new("memory.arrow"), "Arrow IPC stream", &batch).expect("rows"),
            vec![
                Row {
                    values: vec![Value::String("East".to_string())],
                },
                Row {
                    values: vec![Value::Null],
                },
                Row {
                    values: vec![Value::String("West".to_string())],
                },
            ]
        );
    }
}
