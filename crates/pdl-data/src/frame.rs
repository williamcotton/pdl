use indexmap::IndexMap;
use std::cmp::Ordering;
use std::collections::BTreeSet;

use crate::value::Value;

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
        self.columns.iter().position(|column| column == name)
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

    /// Names of columns that carry geometry values, in table column order
    /// (PDL_SPEC §10.13). A column is geometry-bearing when any of its cells is
    /// a [`Value::Geometry`]; an all-null geometry column is indistinguishable
    /// from an all-null scalar column and is reported as scalar.
    pub fn geometry_columns(&self) -> Vec<String> {
        self.columns
            .iter()
            .enumerate()
            .filter(|(index, _)| {
                self.rows
                    .iter()
                    .any(|row| matches!(row.values.get(*index), Some(Value::Geometry(_))))
            })
            .map(|(_, column)| column.clone())
            .collect()
    }

    /// Whether the column at `index` carries any geometry value.
    pub fn column_is_geometry(&self, index: usize) -> bool {
        self.rows
            .iter()
            .any(|row| matches!(row.values.get(index), Some(Value::Geometry(_))))
    }

    pub fn select(self, items: &[(String, String)]) -> Self {
        let indices: Vec<usize> = items
            .iter()
            .filter_map(|(source, _)| self.column_index(source))
            .collect();
        let columns = items.iter().map(|(_, output)| output.clone()).collect();
        let rows = self
            .rows
            .into_iter()
            .map(|row| Row {
                values: indices
                    .iter()
                    .map(|index| row.values.get(*index).cloned().unwrap_or(Value::Null))
                    .collect(),
            })
            .collect();
        Table { columns, rows }
    }

    pub fn drop_columns(self, columns_to_drop: &[String]) -> Self {
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
            .into_iter()
            .map(|row| Row {
                values: keep
                    .iter()
                    .map(|(index, _)| row.values.get(*index).cloned().unwrap_or(Value::Null))
                    .collect(),
            })
            .collect();
        Table { columns, rows }
    }

    pub fn rename_columns(self, renames: &[(String, String)]) -> Self {
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
            rows: self.rows,
        }
    }

    pub fn limit(mut self, n: usize) -> Self {
        self.rows.truncate(n);
        self
    }

    pub fn distinct(self, key_columns: &[String]) -> Self {
        let keys: Vec<String> = if key_columns.is_empty() {
            self.columns.clone()
        } else {
            key_columns.to_vec()
        };
        let key_indices: Vec<usize> = keys
            .iter()
            .filter_map(|column| self.column_index(column))
            .collect();
        let mut seen = BTreeSet::new();
        let rows = self
            .rows
            .into_iter()
            .filter(|row| {
                let key = key_indices
                    .iter()
                    .map(|index| row.values.get(*index).unwrap_or(&Value::Null).to_csv_cell())
                    .collect::<Vec<_>>();
                seen.insert(key)
            })
            .collect();
        Table {
            columns: self.columns,
            rows,
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
