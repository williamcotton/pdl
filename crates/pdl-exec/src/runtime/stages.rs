// Stage-specific row transformations and schema-compatibility checks extracted
// from `runtime.rs` as part of the v0.42 split. See `runtime.rs` for the
// cross-module layout overview.

use pdl_core::{codes, Diagnostic, Span};
use pdl_data::{Row, Table, Value};
use pdl_semantics::{CompleteFillItemIr, JoinKindIr};
use std::collections::{BTreeMap, BTreeSet};

use crate::runtime::row_eval::{eval_row_expr, ExprRole};

pub(crate) fn pivot_longer(
    table: Table,
    columns: &[String],
    names_to: &str,
    values_to: &str,
    span: Span,
) -> Result<Table, Diagnostic> {
    if columns.is_empty() {
        return Err(Diagnostic::error(
            codes::E1203,
            "pivot_longer requires at least one source column",
            span,
        ));
    }
    let mut selected_indices = Vec::new();
    for column in columns {
        let index = table.column_index(column).ok_or_else(|| {
            Diagnostic::error(codes::E1005, format!("unknown column `{column}`"), span)
        })?;
        selected_indices.push((column.clone(), index));
    }
    let selected_names: BTreeSet<&String> = columns.iter().collect();
    let copied = table
        .columns
        .iter()
        .enumerate()
        .filter(|(_, column)| !selected_names.contains(*column))
        .map(|(index, column)| (index, column.clone()))
        .collect::<Vec<_>>();
    if copied.iter().any(|(_, column)| column == names_to) {
        return Err(Diagnostic::error(
            codes::E1207,
            format!("pivot_longer names_to `{names_to}` already exists"),
            span,
        ));
    }
    if copied.iter().any(|(_, column)| column == values_to) {
        return Err(Diagnostic::error(
            codes::E1207,
            format!("pivot_longer values_to `{values_to}` already exists"),
            span,
        ));
    }
    if names_to == values_to {
        return Err(Diagnostic::error(
            codes::E1207,
            "pivot_longer names_to and values_to must be different columns",
            span,
        ));
    }

    let mut output_columns = copied
        .iter()
        .map(|(_, column)| column.clone())
        .collect::<Vec<_>>();
    output_columns.push(names_to.to_string());
    output_columns.push(values_to.to_string());

    let mut rows = Vec::new();
    for row in &table.rows {
        for (column, source_index) in &selected_indices {
            let mut values = copied
                .iter()
                .map(|(index, _)| row.values.get(*index).cloned().unwrap_or(Value::Null))
                .collect::<Vec<_>>();
            values.push(Value::String(column.clone()));
            values.push(
                row.values
                    .get(*source_index)
                    .cloned()
                    .unwrap_or(Value::Null),
            );
            rows.push(Row { values });
        }
    }

    Ok(Table {
        columns: output_columns,
        rows,
    })
}

pub(crate) fn complete(
    table: Table,
    keys: &[String],
    fills: &[CompleteFillItemIr],
    span: Span,
    runtime_context: &BTreeMap<String, Value>,
) -> Result<Table, Diagnostic> {
    if keys.is_empty() {
        return Err(Diagnostic::error(
            codes::E1203,
            "complete requires at least one key column",
            span,
        ));
    }
    let key_indices = keys
        .iter()
        .map(|key| {
            table.column_index(key).ok_or_else(|| {
                Diagnostic::error(codes::E1005, format!("unknown column `{key}`"), span)
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let fill_indices = fills
        .iter()
        .map(|fill| {
            table.column_index(&fill.column).ok_or_else(|| {
                Diagnostic::error(
                    codes::E1005,
                    format!("unknown column `{}`", fill.column),
                    fill.span,
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    let mut observed_by_key = vec![Vec::<Value>::new(); keys.len()];
    let mut observed_seen = vec![BTreeSet::<String>::new(); keys.len()];
    let mut existing = BTreeMap::<Vec<String>, Row>::new();
    for row in &table.rows {
        let mut tuple_key = Vec::new();
        for (position, index) in key_indices.iter().enumerate() {
            let value = row.values.get(*index).cloned().unwrap_or(Value::Null);
            let key = value.to_csv_cell();
            if observed_seen[position].insert(key.clone()) {
                observed_by_key[position].push(value.clone());
            }
            tuple_key.push(key);
        }
        if existing.insert(tuple_key.clone(), row.clone()).is_some() {
            return Err(Diagnostic::error(
                codes::E1208,
                "complete found duplicate input rows for the same key tuple",
                span,
            ));
        }
    }

    let mut rows = Vec::new();
    let mut tuple_values = Vec::new();
    let context = CompleteContext {
        table: &table,
        observed_by_key: &observed_by_key,
        key_indices: &key_indices,
        fills,
        fill_indices: &fill_indices,
        existing: &existing,
        runtime_context,
    };
    complete_rows(&context, &mut tuple_values, &mut rows)?;

    Ok(Table {
        columns: table.columns,
        rows,
    })
}

pub(super) struct CompleteContext<'a> {
    pub(super) table: &'a Table,
    pub(super) observed_by_key: &'a [Vec<Value>],
    pub(super) key_indices: &'a [usize],
    pub(super) fills: &'a [CompleteFillItemIr],
    pub(super) fill_indices: &'a [usize],
    pub(super) existing: &'a BTreeMap<Vec<String>, Row>,
    pub(super) runtime_context: &'a BTreeMap<String, Value>,
}

fn complete_rows(
    context: &CompleteContext<'_>,
    tuple_values: &mut Vec<Value>,
    rows: &mut Vec<Row>,
) -> Result<(), Diagnostic> {
    if tuple_values.len() == context.observed_by_key.len() {
        let tuple_key = tuple_values
            .iter()
            .map(Value::to_csv_cell)
            .collect::<Vec<_>>();
        if let Some(row) = context.existing.get(&tuple_key) {
            rows.push(row.clone());
            return Ok(());
        }

        let mut values = vec![Value::Null; context.table.columns.len()];
        for (key_position, column_index) in context.key_indices.iter().enumerate() {
            values[*column_index] = tuple_values[key_position].clone();
        }
        let base_row = Row {
            values: values.clone(),
        };
        for (fill, column_index) in context.fills.iter().zip(context.fill_indices) {
            values[*column_index] = eval_row_expr(
                &fill.expr,
                context.table,
                &base_row,
                ExprRole::Default,
                None,
                context.runtime_context,
            )?;
        }
        rows.push(Row { values });
        return Ok(());
    }

    let position = tuple_values.len();
    for value in &context.observed_by_key[position] {
        tuple_values.push(value.clone());
        complete_rows(context, tuple_values, rows)?;
        tuple_values.pop();
    }
    Ok(())
}

pub(crate) fn join_columns(
    left_columns: &[String],
    right_columns: &[String],
    keys: &[(String, String)],
    kind: JoinKindIr,
    span: Span,
) -> Result<Vec<String>, Diagnostic> {
    if matches!(kind, JoinKindIr::Semi | JoinKindIr::Anti) {
        return Ok(left_columns.to_vec());
    }

    let right_keys = keys
        .iter()
        .map(|(_, right_key)| right_key)
        .collect::<BTreeSet<_>>();
    let mut columns = left_columns.to_vec();
    for column in right_columns {
        if right_keys.contains(column) {
            continue;
        }
        let mut output = column.clone();
        if columns.iter().any(|existing| existing == &output) {
            output.push_str("_right");
            if columns.iter().any(|existing| existing == &output) {
                return Err(Diagnostic::error(
                    codes::E1207,
                    format!("output column collision `{output}`"),
                    span,
                ));
            }
        }
        columns.push(output);
    }
    Ok(columns)
}

pub(crate) fn right_non_key_indices(columns: &[String], keys: &[(String, String)]) -> Vec<usize> {
    let right_keys = keys
        .iter()
        .map(|(_, right_key)| right_key)
        .collect::<BTreeSet<_>>();
    columns
        .iter()
        .enumerate()
        .filter_map(|(index, column)| (!right_keys.contains(column)).then_some(index))
        .collect()
}

pub(crate) fn join_index(
    table: &Table,
    key_indices: &[usize],
) -> BTreeMap<Vec<String>, Vec<usize>> {
    let mut matches = BTreeMap::new();
    for (row_index, row) in table.rows.iter().enumerate() {
        if let Some(key) = row_join_key(row, key_indices) {
            matches.entry(key).or_insert_with(Vec::new).push(row_index);
        }
    }
    matches
}

pub(crate) fn row_join_key(row: &Row, indices: &[usize]) -> Option<Vec<String>> {
    indices
        .iter()
        .map(|index| row_key(row, *index))
        .collect::<Option<Vec<_>>>()
}

pub(crate) fn row_key(row: &Row, index: usize) -> Option<String> {
    match row.values.get(index).unwrap_or(&Value::Null) {
        Value::Null => None,
        value => Some(value.to_csv_cell()),
    }
}

pub(crate) fn combine_rows(
    left_row: &Row,
    right_row: Option<&Row>,
    right_value_indices: &[usize],
    left_width: usize,
) -> Row {
    let mut values = (0..left_width)
        .map(|index| left_row.values.get(index).cloned().unwrap_or(Value::Null))
        .collect::<Vec<_>>();
    match right_row {
        Some(right_row) => {
            values.extend(
                right_value_indices
                    .iter()
                    .map(|index| right_row.values.get(*index).cloned().unwrap_or(Value::Null)),
            );
        }
        None => values.extend((0..right_value_indices.len()).map(|_| Value::Null)),
    }
    Row { values }
}

pub(crate) fn right_only_row(
    right_row: &Row,
    right_key_indices: &[usize],
    left_key_indices: &[usize],
    left_width: usize,
    right_value_indices: &[usize],
) -> Row {
    let mut values = vec![Value::Null; left_width];
    for (right_key_index, left_key_index) in right_key_indices.iter().zip(left_key_indices) {
        if let Some(value) = right_row.values.get(*right_key_index) {
            if let Some(left_key) = values.get_mut(*left_key_index) {
                *left_key = value.clone();
            }
        }
    }
    values.extend(
        right_value_indices
            .iter()
            .map(|index| right_row.values.get(*index).cloned().unwrap_or(Value::Null)),
    );
    Row { values }
}

pub(crate) fn join_semi_anti(
    left: Table,
    right: &Table,
    keys: &[(String, String)],
    kind: JoinKindIr,
) -> Table {
    let left_indices = keys
        .iter()
        .filter_map(|(left_key, _)| left.column_index(left_key))
        .collect::<Vec<_>>();
    if left_indices.len() != keys.len() {
        return left;
    }
    let right_indices = keys
        .iter()
        .filter_map(|(_, right_key)| right.column_index(right_key))
        .collect::<Vec<_>>();
    if right_indices.len() != keys.len() {
        return left;
    }
    let right_matches = join_index(right, &right_indices);
    let rows = left
        .rows
        .iter()
        .filter(|row| {
            let matched = row_join_key(row, &left_indices)
                .as_ref()
                .is_some_and(|key| right_matches.contains_key(key));
            match kind {
                JoinKindIr::Semi => matched,
                JoinKindIr::Anti => !matched,
                _ => unreachable!("semi/anti helper called for non-semi join"),
            }
        })
        .cloned()
        .collect();
    Table {
        columns: left.columns,
        rows,
    }
}

pub(crate) fn ensure_key_types_compatible(
    left: &Table,
    left_key: &str,
    right: &Table,
    right_key: &str,
    span: Span,
) -> Result<(), Diagnostic> {
    let left_classes = column_value_classes(left, left_key);
    let right_classes = column_value_classes(right, right_key);
    if left_classes.is_empty() || right_classes.is_empty() || left_classes == right_classes {
        return Ok(());
    }

    Err(Diagnostic::error(
        codes::E1208,
        format!("join keys `{left_key}` and `{right_key}` have incompatible observed types"),
        span,
    ))
}

pub(crate) fn ensure_union_compatible(
    left: &Table,
    right: &Table,
    by_name: bool,
    span: Span,
) -> Result<(), Diagnostic> {
    if by_name {
        let left_names: BTreeSet<&String> = left.columns.iter().collect();
        let right_names: BTreeSet<&String> = right.columns.iter().collect();
        for column in left_names.intersection(&right_names) {
            ensure_union_column_compatible(left, column, right, column, span)?;
        }
    } else {
        for (left_column, right_column) in left.columns.iter().zip(&right.columns) {
            ensure_union_column_compatible(left, left_column, right, right_column, span)?;
        }
    }
    Ok(())
}

pub(crate) fn ensure_union_column_compatible(
    left: &Table,
    left_column: &str,
    right: &Table,
    right_column: &str,
    span: Span,
) -> Result<(), Diagnostic> {
    let left_classes = column_value_classes(left, left_column);
    let right_classes = column_value_classes(right, right_column);
    if left_classes.is_empty() || right_classes.is_empty() || left_classes == right_classes {
        return Ok(());
    }

    Err(Diagnostic::error(
        codes::E1209,
        format!(
            "union columns `{left_column}` and `{right_column}` have incompatible observed types"
        ),
        span,
    ))
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum ValueClass {
    Bool,
    Number,
    String,
}

fn column_value_classes(table: &Table, column: &str) -> BTreeSet<ValueClass> {
    let Some(index) = table.column_index(column) else {
        return BTreeSet::new();
    };
    table
        .rows
        .iter()
        .filter_map(|row| match row.values.get(index).unwrap_or(&Value::Null) {
            Value::Null => None,
            Value::Bool(_) => Some(ValueClass::Bool),
            Value::Number(_) => Some(ValueClass::Number),
            Value::String(_) => Some(ValueClass::String),
        })
        .collect()
}
