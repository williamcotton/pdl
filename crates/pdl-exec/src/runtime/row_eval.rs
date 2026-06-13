// Row-runtime cell and window evaluation extracted from `runtime.rs` as part of
// the v0.42 split. Public surface stays internal to `pdl-exec`; see
// `runtime.rs` for the cross-module layout overview.

use pdl_core::{codes, Diagnostic, Span};
use pdl_data::NullsOrder as DataNullsOrder;
use pdl_data::{compare_values, Row, Table, Value};
use pdl_semantics::{
    AggItemIr, BinaryOpIr, ExprIr, FrameBoundIr, NullsOrderIr, SortDirectionIr, UnaryOpIr,
    WindowFrameIr, WindowSpecIr,
};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy)]
pub(crate) enum ExprRole {
    PredicateRoot,
    Default,
    ComparisonLeft,
    ComparisonRight,
}

#[derive(Clone, Copy)]
pub(super) struct EvalScope<'a> {
    pub(super) window_row_index: Option<usize>,
    pub(super) runtime_context: &'a BTreeMap<String, Value>,
}

pub(crate) fn eval_row_expr(
    expr: &ExprIr,
    table: &Table,
    row: &Row,
    role: ExprRole,
    window_row_index: Option<usize>,
    runtime_context: &BTreeMap<String, Value>,
) -> Result<Value, Diagnostic> {
    let _ = role;
    match expr {
        ExprIr::Quoted { value, .. } => Ok(Value::String(value.clone())),
        ExprIr::Number { value, .. } => Ok(Value::Number(*value)),
        ExprIr::Bool { value, .. } => Ok(Value::Bool(*value)),
        ExprIr::Null { .. } => Ok(Value::Null),
        ExprIr::Ident { value, span } => column_value(table, row, value, *span),
        ExprIr::Context { name, span, .. } => runtime_context.get(name).cloned().ok_or_else(|| {
            Diagnostic::error(
                codes::E2002,
                format!("unknown context value `{name}`"),
                *span,
            )
        }),
        ExprIr::Call { name, args, span } => eval_call(
            name,
            args,
            table,
            row,
            *span,
            window_row_index,
            runtime_context,
        ),
        ExprIr::Window {
            function,
            args,
            spec,
            span,
        } => match window_row_index {
            Some(row_index) => eval_window_expr(
                function,
                args,
                spec,
                table,
                row_index,
                *span,
                runtime_context,
            ),
            None => Err(Diagnostic::error(
                codes::E1226,
                "window expressions are supported only in `mutate` assignments",
                *span,
            )),
        },
        ExprIr::Unary { op, expr, span } => {
            let value = eval_row_expr(
                expr,
                table,
                row,
                ExprRole::Default,
                window_row_index,
                runtime_context,
            )?;
            match op {
                UnaryOpIr::Not => match value {
                    Value::Bool(value) => Ok(Value::Bool(!value)),
                    Value::Null => Ok(Value::Null),
                    _ => Err(Diagnostic::error(
                        codes::E1302,
                        "`not` requires a boolean",
                        *span,
                    )),
                },
                UnaryOpIr::Neg => match value {
                    Value::Number(value) => Ok(Value::Number(-value)),
                    _ => Err(Diagnostic::error(
                        codes::E1302,
                        "`-` requires a number",
                        *span,
                    )),
                },
            }
        }
        ExprIr::Binary {
            left,
            op,
            right,
            span,
        } => eval_binary(
            *op,
            left,
            right,
            table,
            row,
            *span,
            EvalScope {
                window_row_index,
                runtime_context,
            },
        ),
    }
}

pub(crate) fn eval_call(
    name: &str,
    args: &[ExprIr],
    table: &Table,
    row: &Row,
    span: Span,
    window_row_index: Option<usize>,
    runtime_context: &BTreeMap<String, Value>,
) -> Result<Value, Diagnostic> {
    match name {
        "col" => match args {
            [expr] => {
                let value = eval_row_expr(
                    expr,
                    table,
                    row,
                    ExprRole::Default,
                    window_row_index,
                    runtime_context,
                )?;
                let Value::String(column) = value else {
                    return Err(Diagnostic::error(
                        codes::E2004,
                        "col() requires a string value",
                        span,
                    ));
                };
                column_value(table, row, &column, span)
            }
            _ => Err(Diagnostic::error(
                codes::E1402,
                "col() expects one argument",
                span,
            )),
        },
        "is_null" => match args {
            [expr] => Ok(Value::Bool(matches!(
                eval_row_expr(
                    expr,
                    table,
                    row,
                    ExprRole::Default,
                    window_row_index,
                    runtime_context,
                )?,
                Value::Null
            ))),
            _ => Err(Diagnostic::error(
                codes::E1402,
                "is_null() expects one argument",
                span,
            )),
        },
        "not_null" => match args {
            [expr] => Ok(Value::Bool(!matches!(
                eval_row_expr(
                    expr,
                    table,
                    row,
                    ExprRole::Default,
                    window_row_index,
                    runtime_context,
                )?,
                Value::Null
            ))),
            _ => Err(Diagnostic::error(
                codes::E1402,
                "not_null() expects one argument",
                span,
            )),
        },
        "coalesce" => {
            for arg in args {
                let value = eval_row_expr(
                    arg,
                    table,
                    row,
                    ExprRole::Default,
                    window_row_index,
                    runtime_context,
                )?;
                if !matches!(value, Value::Null) {
                    return Ok(value);
                }
            }
            Ok(Value::Null)
        }
        "concat" => {
            let mut text = String::new();
            for arg in args {
                let value = eval_row_expr(
                    arg,
                    table,
                    row,
                    ExprRole::Default,
                    window_row_index,
                    runtime_context,
                )?;
                if !matches!(value, Value::Null) {
                    text.push_str(&value.to_csv_cell());
                }
            }
            Ok(Value::String(text))
        }
        "lower" => eval_single_arg(
            args,
            table,
            row,
            span,
            window_row_index,
            runtime_context,
            |value| Ok(map_text(value, |text| text.to_lowercase())),
        ),
        "upper" => eval_single_arg(
            args,
            table,
            row,
            span,
            window_row_index,
            runtime_context,
            |value| Ok(map_text(value, |text| text.to_uppercase())),
        ),
        "trim" => eval_single_arg(
            args,
            table,
            row,
            span,
            window_row_index,
            runtime_context,
            |value| Ok(map_text(value, |text| text.trim().to_string())),
        ),
        "contains" | "starts_with" => {
            let [_, _] = args else {
                return Err(Diagnostic::error(
                    codes::E1402,
                    format!("{name}() expects two arguments"),
                    span,
                ));
            };
            let values =
                eval_args_as_optional_text(args, table, row, window_row_index, runtime_context)?;
            let [value, pattern] = values.as_slice() else {
                unreachable!("checked text predicate arity")
            };
            Ok(match (value, pattern) {
                (Some(value), Some(pattern)) => Value::Bool(match name {
                    "contains" => value.contains(pattern),
                    "starts_with" => value.starts_with(pattern),
                    _ => unreachable!(),
                }),
                _ => Value::Null,
            })
        }
        "replace" => {
            let [_, _, _] = args else {
                return Err(Diagnostic::error(
                    codes::E1402,
                    "replace() expects three arguments",
                    span,
                ));
            };
            let values =
                eval_args_as_optional_text(args, table, row, window_row_index, runtime_context)?;
            let [value, pattern, replacement] = values.as_slice() else {
                unreachable!("checked replace arity")
            };
            Ok(match (value, pattern, replacement) {
                (Some(value), Some(pattern), Some(replacement)) => {
                    Value::String(value.replace(pattern, replacement))
                }
                _ => Value::Null,
            })
        }
        "to_string" => eval_single_arg(
            args,
            table,
            row,
            span,
            window_row_index,
            runtime_context,
            |value| {
                Ok(match value {
                    Value::Null => Value::Null,
                    _ => Value::String(value.to_csv_cell()),
                })
            },
        ),
        "to_number" => eval_single_arg(
            args,
            table,
            row,
            span,
            window_row_index,
            runtime_context,
            |value| {
                Ok(match value {
                    Value::Null => Value::Null,
                    Value::Number(_) => value,
                    _ => value
                        .to_csv_cell()
                        .trim()
                        .parse::<f64>()
                        .map(Value::Number)
                        .unwrap_or(Value::Null),
                })
            },
        ),
        "to_boolean" => eval_single_arg(
            args,
            table,
            row,
            span,
            window_row_index,
            runtime_context,
            |value| {
                Ok(match value {
                    Value::Null => Value::Null,
                    Value::Bool(_) => value,
                    _ => match value.to_csv_cell().trim() {
                        "true" => Value::Bool(true),
                        "false" => Value::Bool(false),
                        _ => Value::Null,
                    },
                })
            },
        ),
        "abs" => eval_single_arg(
            args,
            table,
            row,
            span,
            window_row_index,
            runtime_context,
            |value| match value {
                Value::Null => Ok(Value::Null),
                Value::Number(value) => Ok(Value::Number(value.abs())),
                _ => Err(Diagnostic::error(
                    codes::E1302,
                    "abs() requires a number",
                    span,
                )),
            },
        ),
        "round" => match args {
            [value_expr] => {
                let value = eval_row_expr(
                    value_expr,
                    table,
                    row,
                    ExprRole::Default,
                    window_row_index,
                    runtime_context,
                )?;
                round_value(value, 0, span)
            }
            [value_expr, digits_expr] => {
                let digits = round_digits(digits_expr, span)?;
                let value = eval_row_expr(
                    value_expr,
                    table,
                    row,
                    ExprRole::Default,
                    window_row_index,
                    runtime_context,
                )?;
                round_value(value, digits, span)
            }
            _ => Err(Diagnostic::error(
                codes::E1402,
                "round() expects one or two arguments",
                span,
            )),
        },
        "if_else" => match args {
            [condition, when_true, when_false] => {
                let condition = eval_row_expr(
                    condition,
                    table,
                    row,
                    ExprRole::Default,
                    window_row_index,
                    runtime_context,
                )?;
                match condition {
                    Value::Bool(true) => eval_row_expr(
                        when_true,
                        table,
                        row,
                        ExprRole::Default,
                        window_row_index,
                        runtime_context,
                    ),
                    Value::Bool(false) => eval_row_expr(
                        when_false,
                        table,
                        row,
                        ExprRole::Default,
                        window_row_index,
                        runtime_context,
                    ),
                    Value::Null => Ok(Value::Null),
                    _ => Err(Diagnostic::error(
                        codes::E1302,
                        "if_else() condition requires a boolean",
                        span,
                    )),
                }
            }
            _ => Err(Diagnostic::error(
                codes::E1402,
                "if_else() expects three arguments",
                span,
            )),
        },
        "date" => eval_single_arg(
            args,
            table,
            row,
            span,
            window_row_index,
            runtime_context,
            |value| {
                Ok(parse_temporal_value(value)
                    .map(|parsed| Value::String(pdl_data::normalize_date(&parsed)))
                    .unwrap_or(Value::Null))
            },
        ),
        "datetime" => eval_single_arg(
            args,
            table,
            row,
            span,
            window_row_index,
            runtime_context,
            |value| {
                Ok(parse_temporal_value(value)
                    .and_then(|parsed| pdl_data::normalize_datetime(&parsed))
                    .map(Value::String)
                    .unwrap_or(Value::Null))
            },
        ),
        "year" | "month" | "day" => eval_single_arg(
            args,
            table,
            row,
            span,
            window_row_index,
            runtime_context,
            |value| {
                Ok(parse_temporal_value(value)
                    .map(|parsed| {
                        Value::Number(match name {
                            "year" => f64::from(pdl_data::temporal_year(&parsed)),
                            "month" => f64::from(pdl_data::temporal_month(&parsed)),
                            "day" => f64::from(pdl_data::temporal_day(&parsed)),
                            _ => unreachable!("matched temporal field function"),
                        })
                    })
                    .unwrap_or(Value::Null))
            },
        ),
        "date_floor" => match args {
            [value_expr, unit_expr] => {
                let unit_value = eval_row_expr(
                    unit_expr,
                    table,
                    row,
                    ExprRole::Default,
                    window_row_index,
                    runtime_context,
                )?;
                let unit = temporal_floor_unit(&unit_value, span)?;
                let value = eval_row_expr(
                    value_expr,
                    table,
                    row,
                    ExprRole::Default,
                    window_row_index,
                    runtime_context,
                )?;
                Ok(parse_temporal_value(value)
                    .map(|parsed| {
                        let floored = pdl_data::floor_temporal(&parsed, unit);
                        Value::String(
                            pdl_data::normalize_datetime(&floored)
                                .unwrap_or_else(|| pdl_data::normalize_date(&floored)),
                        )
                    })
                    .unwrap_or(Value::Null))
            }
            _ => Err(Diagnostic::error(
                codes::E1402,
                "date_floor() expects two arguments",
                span,
            )),
        },
        "date_format" => match args {
            [value_expr, pattern_expr] => {
                let pattern_value = eval_row_expr(
                    pattern_expr,
                    table,
                    row,
                    ExprRole::Default,
                    window_row_index,
                    runtime_context,
                )?;
                let pattern = temporal_format_pattern(&pattern_value, span)?;
                let value = eval_row_expr(
                    value_expr,
                    table,
                    row,
                    ExprRole::Default,
                    window_row_index,
                    runtime_context,
                )?;
                Ok(parse_temporal_value(value)
                    .and_then(|parsed| pdl_data::format_temporal(&parsed, &pattern))
                    .map(Value::String)
                    .unwrap_or(Value::Null))
            }
            _ => Err(Diagnostic::error(
                codes::E1402,
                "date_format() expects two arguments",
                span,
            )),
        },
        _ => Err(Diagnostic::error(
            codes::E1401,
            format!("unknown function `{name}`"),
            span,
        )),
    }
}

fn parse_temporal_value(value: Value) -> Option<pdl_data::TemporalValue> {
    value_to_optional_text(value).and_then(|text| pdl_data::parse_temporal(&text))
}

pub(crate) fn temporal_floor_unit(
    unit: &Value,
    span: Span,
) -> Result<pdl_data::TemporalUnit, Diagnostic> {
    let Value::String(unit) = unit else {
        return Err(Diagnostic::error(
            codes::E1403,
            "date_floor() unit must be a string",
            span,
        ));
    };
    pdl_data::parse_temporal_unit(unit).ok_or_else(|| {
        Diagnostic::error(
            codes::E1406,
            format!(
                "date_floor() unit `{unit}` is not supported; use \"day\", \"week\", \"month\", or \"year\""
            ),
            span,
        )
    })
}

pub(crate) fn temporal_format_pattern(pattern: &Value, span: Span) -> Result<String, Diagnostic> {
    let Value::String(pattern) = pattern else {
        return Err(Diagnostic::error(
            codes::E1403,
            "date_format() pattern must be a string",
            span,
        ));
    };
    pdl_data::validate_format_pattern(pattern).map_err(|token| {
        Diagnostic::error(
            codes::E1406,
            format!("date_format() pattern token `{token}` is not supported"),
            span,
        )
    })?;
    Ok(pattern.clone())
}

pub(crate) fn round_digits(expr: &ExprIr, span: Span) -> Result<i32, Diagnostic> {
    let ExprIr::Number { value, .. } = expr else {
        return Err(Diagnostic::error(
            codes::E1206,
            "round() digits must be an integer literal from 0 through 12",
            span,
        ));
    };
    if value.fract() != 0.0 || !(0.0..=12.0).contains(value) {
        return Err(Diagnostic::error(
            codes::E1206,
            "round() digits must be an integer literal from 0 through 12",
            span,
        ));
    }
    Ok(*value as i32)
}

pub(crate) fn round_value(value: Value, digits: i32, span: Span) -> Result<Value, Diagnostic> {
    match value {
        Value::Null => Ok(Value::Null),
        Value::Number(value) => {
            let scale = 10_f64.powi(digits);
            let rounded = (value * scale).round() / scale;
            let normalized = if rounded == 0.0 { 0.0 } else { rounded };
            Ok(Value::Number(normalized))
        }
        _ => Err(Diagnostic::error(
            codes::E1302,
            "round() requires a number",
            span,
        )),
    }
}

pub(crate) fn eval_window_expr(
    function: &str,
    args: &[ExprIr],
    spec: &WindowSpecIr,
    table: &Table,
    current_index: usize,
    span: Span,
    runtime_context: &BTreeMap<String, Value>,
) -> Result<Value, Diagnostic> {
    let partition = ordered_partition_indices(table, spec, current_index);
    let Some(position) = partition.iter().position(|index| *index == current_index) else {
        return Ok(Value::Null);
    };

    match function {
        "row_number" => Ok(Value::Number((position + 1) as f64)),
        "rank" => Ok(Value::Number(
            rank_value(table, spec, &partition, position) as f64
        )),
        "dense_rank" => Ok(Value::Number(
            dense_rank_value(table, spec, &partition, position) as f64,
        )),
        "percent_rank" => {
            if partition.len() <= 1 {
                Ok(Value::Number(0.0))
            } else {
                let rank = rank_value(table, spec, &partition, position);
                Ok(Value::Number(
                    (rank.saturating_sub(1)) as f64 / (partition.len() - 1) as f64,
                ))
            }
        }
        "cume_dist" => {
            if partition.is_empty() {
                Ok(Value::Null)
            } else {
                let last_peer = last_peer_position(table, spec, &partition, position);
                Ok(Value::Number(
                    (last_peer + 1) as f64 / partition.len() as f64,
                ))
            }
        }
        "lag" => eval_offset_window(args, table, &partition, position, -1, span, runtime_context),
        "lead" => eval_offset_window(args, table, &partition, position, 1, span, runtime_context),
        "first_value" => {
            let Some(arg) = args.first() else {
                return Err(Diagnostic::error(
                    codes::E1402,
                    "first_value() expects one argument",
                    span,
                ));
            };
            let frame = frame_indices(spec.frame.as_ref(), &partition, position);
            let Some(row_index) = frame.first() else {
                return Ok(Value::Null);
            };
            eval_row_expr(
                arg,
                table,
                &table.rows[*row_index],
                ExprRole::Default,
                None,
                runtime_context,
            )
        }
        "last_value" => {
            let Some(arg) = args.first() else {
                return Err(Diagnostic::error(
                    codes::E1402,
                    "last_value() expects one argument",
                    span,
                ));
            };
            let frame = frame_indices(spec.frame.as_ref(), &partition, position);
            let Some(row_index) = frame.last() else {
                return Ok(Value::Null);
            };
            eval_row_expr(
                arg,
                table,
                &table.rows[*row_index],
                ExprRole::Default,
                None,
                runtime_context,
            )
        }
        "count" | "sum" | "mean" | "min" | "max" => {
            let frame = frame_indices(spec.frame.as_ref(), &partition, position);
            let rows = frame
                .iter()
                .map(|index| &table.rows[*index])
                .collect::<Vec<_>>();
            eval_window_aggregate(function, args, table, &rows, span, runtime_context)
        }
        _ => Err(Diagnostic::error(
            codes::E1401,
            format!("unknown window function `{function}`"),
            span,
        )),
    }
}

pub(crate) fn ordered_partition_indices(
    table: &Table,
    spec: &WindowSpecIr,
    current_index: usize,
) -> Vec<usize> {
    let current_key = partition_key(table, spec, current_index);
    let mut indices = table
        .rows
        .iter()
        .enumerate()
        .filter_map(|(index, _)| {
            (partition_key(table, spec, index) == current_key).then_some(index)
        })
        .collect::<Vec<_>>();
    if !spec.order_by.is_empty() {
        indices.sort_by(|left, right| compare_rows_for_window_order(table, spec, *left, *right));
    }
    indices
}

pub(crate) fn partition_key(table: &Table, spec: &WindowSpecIr, row_index: usize) -> Vec<Value> {
    let row = &table.rows[row_index];
    spec.partition_by
        .iter()
        .map(|column| table.value(row, column).cloned().unwrap_or(Value::Null))
        .collect()
}

pub(crate) fn compare_rows_for_window_order(
    table: &Table,
    spec: &WindowSpecIr,
    left_index: usize,
    right_index: usize,
) -> Ordering {
    let left = &table.rows[left_index];
    let right = &table.rows[right_index];
    for item in &spec.order_by {
        let Some(column_index) = table.column_index(&item.column) else {
            continue;
        };
        let nulls = item
            .nulls
            .map(|nulls| match nulls {
                NullsOrderIr::First => DataNullsOrder::First,
                NullsOrderIr::Last => DataNullsOrder::Last,
            })
            .unwrap_or(match item.direction {
                SortDirectionIr::Asc => DataNullsOrder::Last,
                SortDirectionIr::Desc => DataNullsOrder::First,
            });
        let ordering = compare_values_for_window_sort(
            left.values.get(column_index).unwrap_or(&Value::Null),
            right.values.get(column_index).unwrap_or(&Value::Null),
            nulls,
        );
        let ordering = match item.direction {
            SortDirectionIr::Asc => ordering,
            SortDirectionIr::Desc => ordering.reverse(),
        };
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    Ordering::Equal
}

pub(crate) fn compare_values_for_window_sort(
    left: &Value,
    right: &Value,
    nulls: DataNullsOrder,
) -> Ordering {
    match (left, right) {
        (Value::Null, Value::Null) => Ordering::Equal,
        (Value::Null, _) => match nulls {
            DataNullsOrder::First => Ordering::Less,
            DataNullsOrder::Last => Ordering::Greater,
        },
        (_, Value::Null) => match nulls {
            DataNullsOrder::First => Ordering::Greater,
            DataNullsOrder::Last => Ordering::Less,
        },
        _ => compare_values(left, right).unwrap_or(Ordering::Equal),
    }
}

pub(crate) fn rank_value(
    table: &Table,
    spec: &WindowSpecIr,
    partition: &[usize],
    position: usize,
) -> usize {
    let current_key = order_key(table, spec, partition[position]);
    partition
        .iter()
        .position(|index| order_key(table, spec, *index) == current_key)
        .map_or(position + 1, |index| index + 1)
}

pub(crate) fn dense_rank_value(
    table: &Table,
    spec: &WindowSpecIr,
    partition: &[usize],
    position: usize,
) -> usize {
    let current_key = order_key(table, spec, partition[position]);
    let mut previous = None;
    let mut rank = 0usize;
    for index in partition.iter().take(position + 1) {
        let key = order_key(table, spec, *index);
        if previous.as_ref() != Some(&key) {
            rank += 1;
            previous = Some(key.clone());
        }
        if key == current_key {
            return rank;
        }
    }
    rank
}

pub(crate) fn last_peer_position(
    table: &Table,
    spec: &WindowSpecIr,
    partition: &[usize],
    position: usize,
) -> usize {
    let current_key = order_key(table, spec, partition[position]);
    partition
        .iter()
        .rposition(|index| order_key(table, spec, *index) == current_key)
        .unwrap_or(position)
}

pub(crate) fn order_key(table: &Table, spec: &WindowSpecIr, row_index: usize) -> Vec<Value> {
    let row = &table.rows[row_index];
    spec.order_by
        .iter()
        .map(|item| {
            table
                .value(row, &item.column)
                .cloned()
                .unwrap_or(Value::Null)
        })
        .collect()
}

pub(crate) fn eval_offset_window(
    args: &[ExprIr],
    table: &Table,
    partition: &[usize],
    position: usize,
    direction: isize,
    span: Span,
    runtime_context: &BTreeMap<String, Value>,
) -> Result<Value, Diagnostic> {
    let Some(value_expr) = args.first() else {
        return Err(Diagnostic::error(
            codes::E1402,
            "lag/lead expects at least one argument",
            span,
        ));
    };
    let offset = window_offset(
        args.get(1),
        table,
        partition[position],
        span,
        runtime_context,
    )? as isize;
    let target = position as isize + direction * offset;
    if target < 0 || target >= partition.len() as isize {
        return match args.get(2) {
            Some(default) => eval_row_expr(
                default,
                table,
                &table.rows[partition[position]],
                ExprRole::Default,
                None,
                runtime_context,
            ),
            None => Ok(Value::Null),
        };
    }
    let row_index = partition[target as usize];
    eval_row_expr(
        value_expr,
        table,
        &table.rows[row_index],
        ExprRole::Default,
        None,
        runtime_context,
    )
}

pub(crate) fn window_offset(
    offset: Option<&ExprIr>,
    table: &Table,
    current_index: usize,
    span: Span,
    runtime_context: &BTreeMap<String, Value>,
) -> Result<usize, Diagnostic> {
    let Some(offset) = offset else {
        return Ok(1);
    };
    let value = eval_row_expr(
        offset,
        table,
        &table.rows[current_index],
        ExprRole::Default,
        None,
        runtime_context,
    )?;
    match value {
        Value::Number(value) if value >= 0.0 && value.fract() == 0.0 => Ok(value as usize),
        _ => Err(Diagnostic::error(
            codes::E1206,
            "lag/lead offset must be a non-negative integer",
            offset.span().join(span),
        )),
    }
}

pub(crate) fn frame_indices(
    frame: Option<&WindowFrameIr>,
    partition: &[usize],
    position: usize,
) -> Vec<usize> {
    let Some(frame) = frame else {
        return partition.to_vec();
    };
    if partition.is_empty() {
        return Vec::new();
    }
    let last = partition.len() as isize - 1;
    let start = frame_bound_position(&frame.start, position as isize, last);
    let end = frame_bound_position(&frame.end, position as isize, last);
    if start > end {
        return Vec::new();
    }
    let start = start.clamp(0, last) as usize;
    let end = end.clamp(0, last) as usize;
    if start > end {
        return Vec::new();
    }
    partition[start..=end].to_vec()
}

fn frame_bound_position(bound: &FrameBoundIr, position: isize, last: isize) -> isize {
    match bound {
        FrameBoundIr::UnboundedPreceding { .. } => 0,
        FrameBoundIr::Preceding { rows, .. } => position - *rows as isize,
        FrameBoundIr::CurrentRow { .. } => position,
        FrameBoundIr::Following { rows, .. } => position + *rows as isize,
        FrameBoundIr::UnboundedFollowing { .. } => last,
    }
}

fn eval_window_aggregate(
    function: &str,
    args: &[ExprIr],
    table: &Table,
    rows: &[&Row],
    span: Span,
    runtime_context: &BTreeMap<String, Value>,
) -> Result<Value, Diagnostic> {
    match function {
        "count" if args.is_empty() => Ok(Value::Number(rows.len() as f64)),
        "count" => {
            let Some(arg) = args.first() else {
                return Err(Diagnostic::error(
                    codes::E1402,
                    "count() expects zero or one argument",
                    span,
                ));
            };
            let values = aggregate_arg_values(arg, table, rows, runtime_context)?;
            Ok(Value::Number(
                values
                    .iter()
                    .filter(|value| !matches!(value, Value::Null))
                    .count() as f64,
            ))
        }
        "sum" => {
            let Some(arg) = args.first() else {
                return Err(Diagnostic::error(
                    codes::E1402,
                    "sum() expects one argument",
                    span,
                ));
            };
            let values = aggregate_arg_values(arg, table, rows, runtime_context)?;
            let mut found = false;
            let mut sum = 0.0;
            for value in values {
                if let Value::Number(number) = value {
                    found = true;
                    sum += number;
                }
            }
            Ok(if found {
                Value::Number(sum)
            } else {
                Value::Null
            })
        }
        "mean" => {
            let Some(arg) = args.first() else {
                return Err(Diagnostic::error(
                    codes::E1402,
                    "mean() expects one argument",
                    span,
                ));
            };
            let values = aggregate_arg_values(arg, table, rows, runtime_context)?;
            let numbers = values
                .into_iter()
                .filter_map(|value| value.as_number())
                .collect::<Vec<_>>();
            if numbers.is_empty() {
                Ok(Value::Null)
            } else {
                Ok(Value::Number(
                    numbers.iter().sum::<f64>() / numbers.len() as f64,
                ))
            }
        }
        "min" => {
            let Some(arg) = args.first() else {
                return Err(Diagnostic::error(
                    codes::E1402,
                    "min() expects one argument",
                    span,
                ));
            };
            aggregate_arg_values(arg, table, rows, runtime_context).map(|values| {
                values
                    .into_iter()
                    .filter(|value| !matches!(value, Value::Null))
                    .min_by(|left, right| compare_values(left, right).unwrap_or(Ordering::Equal))
                    .unwrap_or(Value::Null)
            })
        }
        "max" => {
            let Some(arg) = args.first() else {
                return Err(Diagnostic::error(
                    codes::E1402,
                    "max() expects one argument",
                    span,
                ));
            };
            aggregate_arg_values(arg, table, rows, runtime_context).map(|values| {
                values
                    .into_iter()
                    .filter(|value| !matches!(value, Value::Null))
                    .max_by(|left, right| compare_values(left, right).unwrap_or(Ordering::Equal))
                    .unwrap_or(Value::Null)
            })
        }
        _ => Err(Diagnostic::error(
            codes::E1401,
            format!("unknown window aggregate `{function}`"),
            span,
        )),
    }
}

fn eval_single_arg(
    args: &[ExprIr],
    table: &Table,
    row: &Row,
    span: Span,
    window_row_index: Option<usize>,
    runtime_context: &BTreeMap<String, Value>,
    apply: impl FnOnce(Value) -> Result<Value, Diagnostic>,
) -> Result<Value, Diagnostic> {
    match args {
        [expr] => {
            let value = eval_row_expr(
                expr,
                table,
                row,
                ExprRole::Default,
                window_row_index,
                runtime_context,
            )?;
            apply(value)
        }
        _ => Err(Diagnostic::error(
            codes::E1402,
            "function expects one argument",
            span,
        )),
    }
}

fn eval_args_as_optional_text(
    args: &[ExprIr],
    table: &Table,
    row: &Row,
    window_row_index: Option<usize>,
    runtime_context: &BTreeMap<String, Value>,
) -> Result<Vec<Option<String>>, Diagnostic> {
    args.iter()
        .map(|arg| {
            let value = eval_row_expr(
                arg,
                table,
                row,
                ExprRole::Default,
                window_row_index,
                runtime_context,
            )?;
            Ok(value_to_optional_text(value))
        })
        .collect()
}

fn value_to_optional_text(value: Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(value) => Some(value),
        value => Some(value.to_csv_cell()),
    }
}

fn map_text(value: Value, apply: impl FnOnce(String) -> String) -> Value {
    match value {
        Value::Null => Value::Null,
        Value::String(value) => Value::String(apply(value)),
        _ => Value::String(apply(value.to_csv_cell())),
    }
}

fn eval_binary(
    op: BinaryOpIr,
    left: &ExprIr,
    right: &ExprIr,
    table: &Table,
    row: &Row,
    span: Span,
    scope: EvalScope<'_>,
) -> Result<Value, Diagnostic> {
    if is_comparison_op(op) {
        let left = eval_row_expr(
            left,
            table,
            row,
            ExprRole::ComparisonLeft,
            scope.window_row_index,
            scope.runtime_context,
        )?;
        let right = eval_row_expr(
            right,
            table,
            row,
            ExprRole::ComparisonRight,
            scope.window_row_index,
            scope.runtime_context,
        )?;
        return Ok(compare_for_op(&left, op, &right));
    }

    match op {
        BinaryOpIr::And => {
            let left = eval_row_expr(
                left,
                table,
                row,
                ExprRole::Default,
                scope.window_row_index,
                scope.runtime_context,
            )?;
            let right = eval_row_expr(
                right,
                table,
                row,
                ExprRole::Default,
                scope.window_row_index,
                scope.runtime_context,
            )?;
            Ok(nullable_and(left, right))
        }
        BinaryOpIr::Or => {
            let left = eval_row_expr(
                left,
                table,
                row,
                ExprRole::Default,
                scope.window_row_index,
                scope.runtime_context,
            )?;
            let right = eval_row_expr(
                right,
                table,
                row,
                ExprRole::Default,
                scope.window_row_index,
                scope.runtime_context,
            )?;
            Ok(nullable_or(left, right))
        }
        BinaryOpIr::Add | BinaryOpIr::Sub | BinaryOpIr::Mul | BinaryOpIr::Div | BinaryOpIr::Rem => {
            let left = eval_row_expr(
                left,
                table,
                row,
                ExprRole::Default,
                scope.window_row_index,
                scope.runtime_context,
            )?;
            let right = eval_row_expr(
                right,
                table,
                row,
                ExprRole::Default,
                scope.window_row_index,
                scope.runtime_context,
            )?;
            let (Some(left), Some(right)) = (left.as_number(), right.as_number()) else {
                return Err(Diagnostic::error(
                    codes::E1302,
                    "arithmetic requires numeric operands",
                    span,
                ));
            };
            match op {
                BinaryOpIr::Add => Ok(Value::Number(left + right)),
                BinaryOpIr::Sub => Ok(Value::Number(left - right)),
                BinaryOpIr::Mul => Ok(Value::Number(left * right)),
                BinaryOpIr::Div if right == 0.0 => {
                    Err(Diagnostic::error(codes::E1407, "division by zero", span))
                }
                BinaryOpIr::Div => Ok(Value::Number(left / right)),
                BinaryOpIr::Rem => Ok(Value::Number(left % right)),
                _ => unreachable!(),
            }
        }
        _ => unreachable!("comparison operators returned earlier"),
    }
}

fn compare_for_op(left: &Value, op: BinaryOpIr, right: &Value) -> Value {
    let Some(ordering) = compare_values(left, right) else {
        return Value::Null;
    };
    let result = match op {
        BinaryOpIr::Eq => ordering == Ordering::Equal,
        BinaryOpIr::Ne => ordering != Ordering::Equal,
        BinaryOpIr::Lt => ordering == Ordering::Less,
        BinaryOpIr::Lte => matches!(ordering, Ordering::Less | Ordering::Equal),
        BinaryOpIr::Gt => ordering == Ordering::Greater,
        BinaryOpIr::Gte => matches!(ordering, Ordering::Greater | Ordering::Equal),
        _ => unreachable!(),
    };
    Value::Bool(result)
}

fn nullable_and(left: Value, right: Value) -> Value {
    match (left, right) {
        (Value::Bool(false), _) | (_, Value::Bool(false)) => Value::Bool(false),
        (Value::Bool(true), Value::Bool(true)) => Value::Bool(true),
        (Value::Null, _) | (_, Value::Null) => Value::Null,
        _ => Value::Null,
    }
}

fn nullable_or(left: Value, right: Value) -> Value {
    match (left, right) {
        (Value::Bool(true), _) | (_, Value::Bool(true)) => Value::Bool(true),
        (Value::Bool(false), Value::Bool(false)) => Value::Bool(false),
        (Value::Null, _) | (_, Value::Null) => Value::Null,
        _ => Value::Null,
    }
}

fn column_value(table: &Table, row: &Row, column: &str, span: Span) -> Result<Value, Diagnostic> {
    let value = table.value(row, column).cloned().ok_or_else(|| {
        Diagnostic::error(codes::E1005, format!("unknown column `{column}`"), span)
    })?;
    // Geometry is opaque: it cannot be evaluated as a scalar in arithmetic,
    // comparisons, functions, or control values (PDL_SPEC §10.13). Structural
    // stages (`select`, `drop`, `rename`, `sort`, joins, unions) move geometry
    // by column position and never route it through expression evaluation, so
    // reaching this point means geometry was used where a scalar is required.
    if value.is_geometry() {
        return Err(Diagnostic::error(
            codes::E1234,
            format!("geometry column `{column}` cannot be used as a scalar value"),
            span,
        ));
    }
    Ok(value)
}

pub(crate) fn eval_aggregate(
    item: &AggItemIr,
    table: &Table,
    rows: &[&Row],
    runtime_context: &BTreeMap<String, Value>,
) -> Result<Value, Diagnostic> {
    match item.function.as_str() {
        "count" if item.args.is_empty() => Ok(Value::Number(rows.len() as f64)),
        "count" => {
            let values = aggregate_arg_values(&item.args[0], table, rows, runtime_context)?;
            Ok(Value::Number(
                values
                    .iter()
                    .filter(|value| !matches!(value, Value::Null))
                    .count() as f64,
            ))
        }
        "sum" => {
            let values = aggregate_arg_values(&item.args[0], table, rows, runtime_context)?;
            let mut found = false;
            let mut sum = 0.0;
            for value in values {
                if let Value::Number(number) = value {
                    found = true;
                    sum += number;
                }
            }
            Ok(if found {
                Value::Number(sum)
            } else {
                Value::Null
            })
        }
        "mean" => {
            let values = aggregate_arg_values(&item.args[0], table, rows, runtime_context)?;
            let numbers: Vec<f64> = values
                .into_iter()
                .filter_map(|value| value.as_number())
                .collect();
            if numbers.is_empty() {
                Ok(Value::Null)
            } else {
                Ok(Value::Number(
                    numbers.iter().sum::<f64>() / numbers.len() as f64,
                ))
            }
        }
        "min" => aggregate_arg_values(&item.args[0], table, rows, runtime_context).map(|values| {
            values
                .into_iter()
                .filter(|value| !matches!(value, Value::Null))
                .min_by(|left, right| compare_values(left, right).unwrap_or(Ordering::Equal))
                .unwrap_or(Value::Null)
        }),
        "max" => aggregate_arg_values(&item.args[0], table, rows, runtime_context).map(|values| {
            values
                .into_iter()
                .filter(|value| !matches!(value, Value::Null))
                .max_by(|left, right| compare_values(left, right).unwrap_or(Ordering::Equal))
                .unwrap_or(Value::Null)
        }),
        "count_distinct" => {
            let values = aggregate_arg_values(&item.args[0], table, rows, runtime_context)?;
            let distinct = values
                .into_iter()
                .filter(|value| !matches!(value, Value::Null))
                .map(|value| value.to_csv_cell())
                .collect::<BTreeSet<_>>();
            Ok(Value::Number(distinct.len() as f64))
        }
        function => Err(Diagnostic::error(
            codes::E1401,
            format!("unknown aggregate function `{function}`"),
            item.span,
        )),
    }
}

fn aggregate_arg_values(
    expr: &ExprIr,
    table: &Table,
    rows: &[&Row],
    runtime_context: &BTreeMap<String, Value>,
) -> Result<Vec<Value>, Diagnostic> {
    rows.iter()
        .map(|row| eval_aggregate_expr(expr, table, row, runtime_context))
        .collect()
}

fn eval_aggregate_expr(
    expr: &ExprIr,
    table: &Table,
    row: &Row,
    runtime_context: &BTreeMap<String, Value>,
) -> Result<Value, Diagnostic> {
    eval_row_expr(expr, table, row, ExprRole::Default, None, runtime_context)
}

fn is_comparison_op(op: BinaryOpIr) -> bool {
    matches!(
        op,
        BinaryOpIr::Eq
            | BinaryOpIr::Ne
            | BinaryOpIr::Lt
            | BinaryOpIr::Lte
            | BinaryOpIr::Gt
            | BinaryOpIr::Gte
    )
}
