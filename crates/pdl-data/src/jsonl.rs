use indexmap::IndexSet;
use pdl_core::{codes, Diagnostic, Span};
use serde_json::{Map, Number};
use std::io::Write;
use std::path::Path;

use crate::{format_number, Row, Table, Value};

pub fn read_json_lines_schema_from_bytes(
    path: &Path,
    bytes: &[u8],
) -> Result<Vec<String>, Diagnostic> {
    let rows = parse_json_lines(path, bytes)?;
    let mut columns = IndexSet::new();
    for object in rows {
        for key in object.keys() {
            columns.insert(key.clone());
        }
    }
    Ok(columns.into_iter().collect())
}

pub fn read_json_lines_from_bytes(path: &Path, bytes: &[u8]) -> Result<Table, Diagnostic> {
    let objects = parse_json_lines(path, bytes)?;
    let mut columns = IndexSet::new();
    for object in &objects {
        for key in object.keys() {
            columns.insert(key.clone());
        }
    }
    let columns: Vec<String> = columns.into_iter().collect();
    let rows = objects
        .into_iter()
        .map(|object| Row {
            values: columns
                .iter()
                .map(|column| {
                    object
                        .get(column)
                        .map(json_value_to_table_value)
                        .unwrap_or(Value::Null)
                })
                .collect(),
        })
        .collect();

    Ok(Table { columns, rows })
}

pub fn write_json_lines_to_vec(table: &Table) -> Result<Vec<u8>, Diagnostic> {
    let mut output = Vec::new();
    for row in &table.rows {
        write_json_lines_record(&mut output, &table.columns, &row.values)?;
    }
    Ok(output)
}

/// Emits one JSON Lines record with the row writer's exact encoding (stable
/// column-order fields, integral-number narrowing, trailing `\n`). The native
/// engine writes through this so NDJSON bytes stay byte-identical to the row
/// writer without materializing a row table.
pub(crate) fn write_json_lines_record(
    writer: &mut dyn Write,
    columns: &[String],
    values: &[Value],
) -> Result<(), Diagnostic> {
    let mut object = Map::new();
    for (index, column) in columns.iter().enumerate() {
        let value = values.get(index).unwrap_or(&Value::Null);
        object.insert(column.clone(), table_value_to_json(value)?);
    }
    serde_json::to_writer(&mut *writer, &object).map_err(json_write_error)?;
    writer.write_all(b"\n").map_err(|error| {
        Diagnostic::error(
            codes::E1704,
            format!("JSON Lines write failed: {error}"),
            Span::zero(),
        )
    })
}

fn parse_json_lines(
    path: &Path,
    bytes: &[u8],
) -> Result<Vec<Map<String, serde_json::Value>>, Diagnostic> {
    let text = std::str::from_utf8(bytes).map_err(|error| {
        Diagnostic::error(
            codes::E1804,
            format!(
                "JSON Lines input for `{}` is not UTF-8: {error}",
                path.display()
            ),
            Span::zero(),
        )
    })?;
    let mut rows = Vec::new();
    for (line_index, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let value = serde_json::from_str::<serde_json::Value>(trimmed).map_err(|error| {
            Diagnostic::error(
                codes::E1804,
                format!(
                    "JSON Lines parse failed for `{}` at line {}: {error}",
                    path.display(),
                    line_index + 1
                ),
                Span::zero(),
            )
        })?;
        match value {
            serde_json::Value::Object(object) => rows.push(object),
            _ => {
                return Err(Diagnostic::error(
                    codes::E1804,
                    format!(
                        "JSON Lines row in `{}` at line {} is not an object",
                        path.display(),
                        line_index + 1
                    ),
                    Span::zero(),
                ));
            }
        }
    }
    Ok(rows)
}

fn json_value_to_table_value(value: &serde_json::Value) -> Value {
    match value {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(value) => Value::Bool(*value),
        serde_json::Value::Number(value) => value
            .as_f64()
            .map(Value::Number)
            .unwrap_or_else(|| Value::String(value.to_string())),
        serde_json::Value::String(value) => Value::String(value.clone()),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
            Value::String(value.to_string())
        }
    }
}

fn table_value_to_json(value: &Value) -> Result<serde_json::Value, Diagnostic> {
    match value {
        Value::Null => Ok(serde_json::Value::Null),
        Value::Bool(value) => Ok(serde_json::Value::Bool(*value)),
        Value::Number(value) => {
            if value.is_finite()
                && value.fract() == 0.0
                && *value >= i64::MIN as f64
                && *value <= i64::MAX as f64
            {
                return Ok(serde_json::Value::Number(Number::from(*value as i64)));
            }
            Number::from_f64(*value)
                .map(serde_json::Value::Number)
                .ok_or_else(|| {
                    Diagnostic::error(
                        codes::E1704,
                        format!(
                            "JSON Lines output cannot encode non-finite number `{}`",
                            format_number(*value)
                        ),
                        Span::zero(),
                    )
                })
        }
        Value::String(value) => Ok(serde_json::Value::String(value.clone())),
        // Geometry has no JSON Lines scalar encoding; geometry-bearing tables
        // are rejected before this point (PDL_SPEC §10.13).
        Value::Geometry(_) => Err(Diagnostic::error(
            codes::E1711,
            "JSON Lines output cannot encode geometry; save as `geojson` or drop the geometry first",
            Span::zero(),
        )),
    }
}

fn json_write_error(error: serde_json::Error) -> Diagnostic {
    Diagnostic::error(
        codes::E1704,
        format!("JSON Lines write failed: {error}"),
        Span::zero(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_lines_reads_deterministic_schema_and_rows() {
        let bytes = br#"{"region":"West","amount":10,"active":true}
{"amount":12.5,"extra":{"source":"manual"}}
"#;

        let table =
            read_json_lines_from_bytes(Path::new("memory.jsonl"), bytes).expect("read json lines");

        assert_eq!(
            table,
            Table::new(
                vec![
                    "region".to_string(),
                    "amount".to_string(),
                    "active".to_string(),
                    "extra".to_string(),
                ],
                vec![
                    Row {
                        values: vec![
                            Value::String("West".to_string()),
                            Value::Number(10.0),
                            Value::Bool(true),
                            Value::Null,
                        ],
                    },
                    Row {
                        values: vec![
                            Value::Null,
                            Value::Number(12.5),
                            Value::Null,
                            Value::String(r#"{"source":"manual"}"#.to_string()),
                        ],
                    },
                ],
            )
        );
    }

    #[test]
    fn json_lines_output_is_stable() {
        let table = Table::new(
            vec!["region".to_string(), "amount".to_string()],
            vec![Row {
                values: vec![Value::String("West".to_string()), Value::Number(350.0)],
            }],
        );

        let first = write_json_lines_to_vec(&table).expect("first write");
        let second = write_json_lines_to_vec(&table).expect("second write");

        assert_eq!(first, second);
        assert_eq!(
            String::from_utf8(first).expect("jsonl utf-8"),
            "{\"region\":\"West\",\"amount\":350}\n"
        );
    }
}
