use indexmap::IndexMap;
use pdl_core::{Diagnostic, Span};
use std::cmp::Ordering;
use std::fs::File;
use std::io::{BufReader, BufWriter, Write};
use std::path::Path;

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
}

impl Value {
    pub fn parse_csv_cell(cell: &str) -> Self {
        if cell.is_empty() {
            Self::Null
        } else if cell == "true" {
            Self::Bool(true)
        } else if cell == "false" {
            Self::Bool(false)
        } else if let Ok(number) = cell.parse::<f64>() {
            Self::Number(number)
        } else {
            Self::String(cell.to_string())
        }
    }

    pub fn to_csv_cell(&self) -> String {
        match self {
            Value::Null => String::new(),
            Value::Bool(value) => value.to_string(),
            Value::Number(value) => format_number(*value),
            Value::String(value) => value.clone(),
        }
    }

    pub fn is_truthy_true(&self) -> bool {
        matches!(self, Value::Bool(true))
    }

    pub fn as_number(&self) -> Option<f64> {
        match self {
            Value::Number(value) => Some(*value),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub values: Vec<Value>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Table {
    pub columns: Vec<String>,
    pub rows: Vec<Row>,
}

impl Table {
    pub fn new(columns: Vec<String>, rows: Vec<Row>) -> Self {
        Self { columns, rows }
    }

    pub fn column_index(&self, name: &str) -> Option<usize> {
        self.column_index_map().get(name).copied()
    }

    pub fn column_index_map(&self) -> IndexMap<String, usize> {
        self.columns
            .iter()
            .enumerate()
            .map(|(index, column)| (column.clone(), index))
            .collect()
    }

    pub fn value<'a>(&'a self, row: &'a Row, column: &str) -> Option<&'a Value> {
        self.column_index(column)
            .and_then(|index| row.values.get(index))
    }

    pub fn select(&self, items: &[(String, String)]) -> Self {
        let indices: Vec<usize> = items
            .iter()
            .filter_map(|(source, _)| self.column_index(source))
            .collect();
        let columns = items.iter().map(|(_, output)| output.clone()).collect();
        let rows = self
            .rows
            .iter()
            .map(|row| Row {
                values: indices
                    .iter()
                    .map(|index| row.values[*index].clone())
                    .collect(),
            })
            .collect();
        Table { columns, rows }
    }

    pub fn drop_columns(&self, columns_to_drop: &[String]) -> Self {
        let keep: Vec<(usize, String)> = self
            .columns
            .iter()
            .enumerate()
            .filter(|(_, column)| !columns_to_drop.iter().any(|drop| drop == *column))
            .map(|(index, column)| (index, column.clone()))
            .collect();
        let columns = keep.iter().map(|(_, column)| column.clone()).collect();
        let rows = self
            .rows
            .iter()
            .map(|row| Row {
                values: keep
                    .iter()
                    .map(|(index, _)| row.values[*index].clone())
                    .collect(),
            })
            .collect();
        Table { columns, rows }
    }

    pub fn rename_columns(&self, renames: &[(String, String)]) -> Self {
        let columns = self
            .columns
            .iter()
            .map(|column| {
                renames
                    .iter()
                    .find_map(|(old, new)| (old == column).then(|| new.clone()))
                    .unwrap_or_else(|| column.clone())
            })
            .collect();
        Table {
            columns,
            rows: self.rows.clone(),
        }
    }

    pub fn limit(&self, n: usize) -> Self {
        Table {
            columns: self.columns.clone(),
            rows: self.rows.iter().take(n).cloned().collect(),
        }
    }

    pub fn stable_sort(&mut self, items: &[SortSpec]) {
        let columns = self.columns.clone();
        self.rows.sort_by(|left, right| {
            for item in items {
                let Some(index) = columns.iter().position(|column| column == &item.column) else {
                    continue;
                };
                let ordering = compare_values_for_sort(
                    left.values.get(index).unwrap_or(&Value::Null),
                    right.values.get(index).unwrap_or(&Value::Null),
                    item.nulls,
                );
                let ordering = match item.direction {
                    SortDirection::Asc => ordering,
                    SortDirection::Desc => ordering.reverse(),
                };
                if ordering != Ordering::Equal {
                    return ordering;
                }
            }
            Ordering::Equal
        });
    }
}

#[cfg(feature = "polars-engine")]
pub fn native_engine_name() -> &'static str {
    let _ = std::any::type_name::<polars::prelude::DataFrame>();
    "polars"
}

#[cfg(not(feature = "polars-engine"))]
pub fn native_engine_name() -> &'static str {
    "in-memory"
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SortDirection {
    Asc,
    Desc,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NullsOrder {
    First,
    Last,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SortSpec {
    pub column: String,
    pub direction: SortDirection,
    pub nulls: NullsOrder,
}

pub fn read_csv_schema(path: &Path) -> Result<Vec<String>, Diagnostic> {
    let file = File::open(path).map_err(|error| {
        Diagnostic::error(
            "P1801",
            format!(
                "source file `{}` could not be opened: {error}",
                path.display()
            ),
            Span::zero(),
        )
    })?;
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .from_reader(BufReader::new(file));
    let headers = reader.headers().map_err(|error| {
        Diagnostic::error(
            "P1804",
            format!("CSV header parse failed for `{}`: {error}", path.display()),
            Span::zero(),
        )
    })?;
    Ok(headers.iter().map(str::to_string).collect())
}

pub fn read_csv(path: &Path) -> Result<Table, Diagnostic> {
    let file = File::open(path).map_err(|error| {
        Diagnostic::error(
            "P1801",
            format!(
                "source file `{}` could not be opened: {error}",
                path.display()
            ),
            Span::zero(),
        )
    })?;
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .from_reader(BufReader::new(file));
    let headers = reader.headers().map_err(|error| {
        Diagnostic::error(
            "P1804",
            format!("CSV header parse failed for `{}`: {error}", path.display()),
            Span::zero(),
        )
    })?;
    let columns: Vec<String> = headers.iter().map(str::to_string).collect();
    let mut rows = Vec::new();

    for record in reader.records() {
        let record = record.map_err(|error| {
            Diagnostic::error(
                "P1804",
                format!("CSV row parse failed for `{}`: {error}", path.display()),
                Span::zero(),
            )
        })?;
        rows.push(Row {
            values: record.iter().map(Value::parse_csv_cell).collect(),
        });
    }

    Ok(Table { columns, rows })
}

pub fn write_csv(path: &Path, table: &Table) -> Result<(), Diagnostic> {
    let file = File::create(path).map_err(|error| {
        Diagnostic::error(
            "P1704",
            format!(
                "output file `{}` could not be created: {error}",
                path.display()
            ),
            Span::zero(),
        )
    })?;
    write_csv_to_writer(BufWriter::new(file), table)
}

pub fn write_csv_to_vec(table: &Table) -> Result<Vec<u8>, Diagnostic> {
    let mut bytes = Vec::new();
    write_csv_to_writer(&mut bytes, table)?;
    Ok(bytes)
}

fn write_csv_to_writer<W: Write>(writer: W, table: &Table) -> Result<(), Diagnostic> {
    let mut writer = csv::WriterBuilder::new()
        .has_headers(true)
        .terminator(csv::Terminator::Any(b'\n'))
        .from_writer(writer);
    writer.write_record(&table.columns).map_err(|error| {
        Diagnostic::error(
            "P1704",
            format!("CSV header write failed: {error}"),
            Span::zero(),
        )
    })?;
    for row in &table.rows {
        let record: Vec<String> = row.values.iter().map(Value::to_csv_cell).collect();
        writer.write_record(record).map_err(|error| {
            Diagnostic::error(
                "P1704",
                format!("CSV row write failed: {error}"),
                Span::zero(),
            )
        })?;
    }
    writer.flush().map_err(|error| {
        Diagnostic::error("P1704", format!("CSV flush failed: {error}"), Span::zero())
    })
}

pub fn compare_values(left: &Value, right: &Value) -> Option<Ordering> {
    match (left, right) {
        (Value::Null, Value::Null) => Some(Ordering::Equal),
        (Value::Null, _) | (_, Value::Null) => None,
        (Value::Bool(left), Value::Bool(right)) => Some(left.cmp(right)),
        (Value::Number(left), Value::Number(right)) => left.partial_cmp(right),
        (Value::String(left), Value::String(right)) => Some(left.cmp(right)),
        (Value::Number(left), Value::String(right)) => right
            .parse::<f64>()
            .ok()
            .and_then(|right| left.partial_cmp(&right)),
        (Value::String(left), Value::Number(right)) => left
            .parse::<f64>()
            .ok()
            .and_then(|left| left.partial_cmp(right)),
        _ => Some(left.to_csv_cell().cmp(&right.to_csv_cell())),
    }
}

fn compare_values_for_sort(left: &Value, right: &Value, nulls: NullsOrder) -> Ordering {
    match (left, right) {
        (Value::Null, Value::Null) => Ordering::Equal,
        (Value::Null, _) => match nulls {
            NullsOrder::First => Ordering::Less,
            NullsOrder::Last => Ordering::Greater,
        },
        (_, Value::Null) => match nulls {
            NullsOrder::First => Ordering::Greater,
            NullsOrder::Last => Ordering::Less,
        },
        _ => compare_values(left, right).unwrap_or(Ordering::Equal),
    }
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
