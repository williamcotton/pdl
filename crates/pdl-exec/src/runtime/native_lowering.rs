// Expression, aggregate, and window translation to the `pdl-data` facade.
// Extracted from `runtime.rs` as part of the v0.42 split. See `runtime.rs`
// for the cross-module layout overview.

use pdl_core::{codes, Diagnostic, Span};
use pdl_data::{
    DataAggItem, DataBinaryOp, DataExpr, DataLiteral, DataScalarFunction, DataUnaryOp,
    DataWindowFrame, DataWindowFunction, DataWindowSpec, NullsOrder as DataNullsOrder,
    SortDirection as DataSortDirection, SortSpec, Value,
};
use pdl_semantics::{
    AggItemIr, BinaryOpIr, ExprIr, FrameBoundIr, MutateItemIr, NullsOrderIr, SortDirectionIr,
    UnaryOpIr, WindowFrameIr, WindowSpecIr,
};
use std::collections::BTreeMap;

use crate::planning::NativeUnsupportedReason;
use crate::runtime::native_planning::{
    resolve_native_column_name, resolve_native_column_names, unsupported_native_pipeline,
};
use crate::runtime::row_eval::round_digits;

pub(crate) fn lower_data_expr(
    expr: &ExprIr,
    context: &BTreeMap<String, Value>,
) -> Result<DataExpr, Diagnostic> {
    match expr {
        ExprIr::Quoted { value, .. } => Ok(DataExpr::Literal(DataLiteral::String(value.clone()))),
        ExprIr::Number { value, .. } => Ok(DataExpr::Literal(DataLiteral::Number(*value))),
        ExprIr::Bool { value, .. } => Ok(DataExpr::Literal(DataLiteral::Bool(*value))),
        ExprIr::Null { .. } => Ok(DataExpr::Literal(DataLiteral::Null)),
        ExprIr::Ident { value, .. } => Ok(DataExpr::Column(value.clone())),
        ExprIr::Context { name, span, .. } => {
            context.get(name).map(value_to_data_literal).ok_or_else(|| {
                Diagnostic::error(
                    codes::E2002,
                    format!("unknown context value `{name}`"),
                    *span,
                )
            })
        }
        ExprIr::Unary { op, expr, .. } => Ok(DataExpr::Unary {
            op: match op {
                UnaryOpIr::Not => DataUnaryOp::Not,
                UnaryOpIr::Neg => DataUnaryOp::Neg,
            },
            expr: Box::new(lower_data_expr(expr, context)?),
        }),
        ExprIr::Binary {
            left, op, right, ..
        } => Ok(DataExpr::Binary {
            left: Box::new(lower_data_expr(left, context)?),
            op: match op {
                BinaryOpIr::Or => DataBinaryOp::Or,
                BinaryOpIr::And => DataBinaryOp::And,
                BinaryOpIr::Eq => DataBinaryOp::Eq,
                BinaryOpIr::Ne => DataBinaryOp::Ne,
                BinaryOpIr::Lt => DataBinaryOp::Lt,
                BinaryOpIr::Lte => DataBinaryOp::Lte,
                BinaryOpIr::Gt => DataBinaryOp::Gt,
                BinaryOpIr::Gte => DataBinaryOp::Gte,
                BinaryOpIr::Add => DataBinaryOp::Add,
                BinaryOpIr::Sub => DataBinaryOp::Sub,
                BinaryOpIr::Mul => DataBinaryOp::Mul,
                BinaryOpIr::Div => DataBinaryOp::Div,
                BinaryOpIr::Rem => DataBinaryOp::Rem,
            },
            right: Box::new(lower_data_expr(right, context)?),
        }),
        ExprIr::Call { name, args, span } => lower_data_call(name, args, *span, context),
        ExprIr::Window {
            function,
            args,
            spec,
            span,
        } => lower_data_window(function, args, spec, *span, context),
    }
}

pub(crate) fn lower_data_agg_items(
    items: &[AggItemIr],
    context: &BTreeMap<String, Value>,
) -> Result<Vec<DataAggItem>, Diagnostic> {
    items
        .iter()
        .map(|item| {
            let args = match item.function.as_str() {
                "count" if item.args.is_empty() => Vec::new(),
                "count" | "sum" | "mean" | "min" | "max" | "count_distinct" => {
                    let [arg] = item.args.as_slice() else {
                        return Err(unsupported_native_pipeline(
                            NativeUnsupportedReason::AggregateArity,
                            "aggregate arity is not supported by native execution",
                        ));
                    };
                    vec![lower_data_agg_arg(arg, context)?]
                }
                _ => {
                    return Err(unsupported_native_pipeline(
                        NativeUnsupportedReason::AggregateFunction,
                        "aggregate function is not supported by native execution",
                    ));
                }
            };
            Ok(DataAggItem {
                function: item.function.clone(),
                args,
                alias: item.alias.clone(),
            })
        })
        .collect()
}

pub(crate) fn lower_data_mutate_items(
    items: &[MutateItemIr],
    context: &BTreeMap<String, Value>,
) -> Result<Vec<(String, DataExpr)>, Diagnostic> {
    check_native_mutate_multi_key_window_order_groups(items)?;
    items
        .iter()
        .map(|item| Ok((item.column.clone(), lower_data_expr(&item.expr, context)?)))
        .collect()
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct NativeWindowSortGroupIr {
    partition_by: Vec<String>,
    order_by: Vec<(String, SortDirectionIr, Option<NullsOrderIr>)>,
}

pub(crate) fn check_native_mutate_multi_key_window_order_groups(
    items: &[MutateItemIr],
) -> Result<(), Diagnostic> {
    let mut group = None;
    for item in items {
        if expr_multi_key_window_order_incompatible(&item.expr, &mut group) {
            return Err(unsupported_native_pipeline(
                NativeUnsupportedReason::WindowExpression,
                "multiple multi-key window order groups",
            ));
        }
    }
    Ok(())
}

pub(super) fn expr_multi_key_window_order_incompatible(
    expr: &ExprIr,
    group: &mut Option<NativeWindowSortGroupIr>,
) -> bool {
    match expr {
        ExprIr::Window { args, spec, .. } => {
            if spec.order_by.len() > 1 {
                let next = NativeWindowSortGroupIr {
                    partition_by: spec.partition_by.clone(),
                    order_by: spec
                        .order_by
                        .iter()
                        .map(|item| (item.column.clone(), item.direction, item.nulls))
                        .collect(),
                };
                match group {
                    Some(current) if current != &next => return true,
                    Some(_) => {}
                    None => *group = Some(next),
                }
            }
            args.iter()
                .any(|arg| expr_multi_key_window_order_incompatible(arg, group))
        }
        ExprIr::Call { args, .. } => args
            .iter()
            .any(|arg| expr_multi_key_window_order_incompatible(arg, group)),
        ExprIr::Unary { expr, .. } => expr_multi_key_window_order_incompatible(expr, group),
        ExprIr::Binary { left, right, .. } => {
            expr_multi_key_window_order_incompatible(left, group)
                || expr_multi_key_window_order_incompatible(right, group)
        }
        ExprIr::Quoted { .. }
        | ExprIr::Number { .. }
        | ExprIr::Bool { .. }
        | ExprIr::Null { .. }
        | ExprIr::Ident { .. }
        | ExprIr::Context { .. } => false,
    }
}

pub(crate) fn lower_data_agg_arg(
    expr: &ExprIr,
    context: &BTreeMap<String, Value>,
) -> Result<DataExpr, Diagnostic> {
    lower_data_expr(expr, context)
}

pub(crate) fn lower_data_call(
    name: &str,
    args: &[ExprIr],
    span: Span,
    context: &BTreeMap<String, Value>,
) -> Result<DataExpr, Diagnostic> {
    if name == "col" {
        let [arg] = args else {
            return Err(Diagnostic::error(
                codes::E1402,
                "col() expects one argument",
                span,
            ));
        };
        return match lower_data_expr(arg, context)? {
            DataExpr::Literal(DataLiteral::String(column)) => Ok(DataExpr::Column(column)),
            _ => Err(unsupported_native_pipeline(
                NativeUnsupportedReason::DataDependentColIndirection,
                "native col() requires a string literal or context string",
            )),
        };
    }

    let function = match name {
        "is_null" => DataScalarFunction::IsNull,
        "not_null" => DataScalarFunction::NotNull,
        "coalesce" => DataScalarFunction::Coalesce,
        "concat" => DataScalarFunction::Concat,
        "if_else" => DataScalarFunction::IfElse,
        "lower" => DataScalarFunction::Lower,
        "upper" => DataScalarFunction::Upper,
        "trim" => DataScalarFunction::Trim,
        "contains" => DataScalarFunction::Contains,
        "starts_with" => DataScalarFunction::StartsWith,
        "replace" => {
            let [_, pattern, replacement] = args else {
                return Err(Diagnostic::error(
                    codes::E1402,
                    "replace() expects three arguments",
                    span,
                ));
            };
            if !native_static_text_arg(pattern) || !native_static_text_arg(replacement) {
                return Err(unsupported_native_pipeline(
                    NativeUnsupportedReason::DataDependentReplacePattern,
                    "native replace() requires literal pattern and replacement",
                ));
            }
            DataScalarFunction::Replace
        }
        "to_string" => DataScalarFunction::ToString,
        "to_number" => DataScalarFunction::ToNumber,
        "to_boolean" => DataScalarFunction::ToBoolean,
        "abs" => DataScalarFunction::Abs,
        "round" => {
            let digits = match args {
                [_] => 0,
                [_, digits] => round_digits(digits, span)? as u32,
                _ => {
                    return Err(Diagnostic::error(
                        codes::E1402,
                        "round() expects one or two arguments",
                        span,
                    ));
                }
            };
            DataScalarFunction::Round { digits }
        }
        _ => {
            return Err(unsupported_native_pipeline(
                NativeUnsupportedReason::ScalarFunction,
                "scalar function is not supported by native execution",
            ));
        }
    };
    Ok(DataExpr::Call {
        function,
        args: args
            .iter()
            .take(match function {
                DataScalarFunction::Round { .. } => 1,
                _ => args.len(),
            })
            .map(|arg| lower_data_expr(arg, context))
            .collect::<Result<Vec<_>, _>>()?,
    })
}

pub(crate) fn native_static_text_arg(arg: &ExprIr) -> bool {
    matches!(
        arg,
        ExprIr::Quoted { .. }
            | ExprIr::Number { .. }
            | ExprIr::Bool { .. }
            | ExprIr::Context { .. }
    )
}

pub(crate) fn lower_data_window(
    function: &str,
    args: &[ExprIr],
    spec: &WindowSpecIr,
    span: Span,
    context: &BTreeMap<String, Value>,
) -> Result<DataExpr, Diagnostic> {
    let function = match function {
        "row_number" => {
            if !args.is_empty() {
                return Err(unsupported_native_pipeline(
                    NativeUnsupportedReason::WindowExpression,
                    "window function arity is not supported by native execution",
                ));
            }
            DataWindowFunction::RowNumber
        }
        "rank" | "dense_rank" => {
            if !args.is_empty() {
                return Err(unsupported_native_pipeline(
                    NativeUnsupportedReason::WindowExpression,
                    "window function arity is not supported by native execution",
                ));
            }
            if spec.order_by.is_empty() {
                return Err(unsupported_native_pipeline(
                    NativeUnsupportedReason::WindowExpression,
                    "native rank windows require at least one order key",
                ));
            }
            if function == "rank" {
                DataWindowFunction::Rank
            } else {
                DataWindowFunction::DenseRank
            }
        }
        "percent_rank" | "cume_dist" => {
            if !args.is_empty() {
                return Err(unsupported_native_pipeline(
                    NativeUnsupportedReason::WindowExpression,
                    "window function arity is not supported by native execution",
                ));
            }
            if spec.order_by.is_empty() {
                return Err(unsupported_native_pipeline(
                    NativeUnsupportedReason::WindowExpression,
                    "native distribution windows require at least one order key",
                ));
            }
            if function == "percent_rank" {
                DataWindowFunction::PercentRank
            } else {
                DataWindowFunction::CumeDist
            }
        }
        "lag" | "lead" => {
            if args.is_empty() || args.len() > 3 {
                return Err(unsupported_native_pipeline(
                    NativeUnsupportedReason::WindowExpression,
                    "window function arity is not supported by native execution",
                ));
            }
            if spec.order_by.is_empty() {
                return Err(unsupported_native_pipeline(
                    NativeUnsupportedReason::WindowExpression,
                    "native offset windows require at least one order key",
                ));
            }
            match args.get(1) {
                None => {}
                Some(ExprIr::Number { value, .. }) if *value >= 0.0 && value.fract() == 0.0 => {}
                Some(_) => {
                    return Err(unsupported_native_pipeline(
                        NativeUnsupportedReason::WindowExpression,
                        "native offset windows require a non-negative integer literal offset",
                    ));
                }
            }
            if function == "lag" {
                DataWindowFunction::Lag
            } else {
                DataWindowFunction::Lead
            }
        }
        "first_value" | "last_value" => {
            let [_] = args else {
                return Err(unsupported_native_pipeline(
                    NativeUnsupportedReason::WindowExpression,
                    "window function arity is not supported by native execution",
                ));
            };
            if function == "first_value" {
                DataWindowFunction::FirstValue
            } else {
                DataWindowFunction::LastValue
            }
        }
        "count" => {
            if args.len() > 1 {
                return Err(unsupported_native_pipeline(
                    NativeUnsupportedReason::WindowExpression,
                    "window function arity is not supported by native execution",
                ));
            }
            DataWindowFunction::Count
        }
        "sum" | "mean" | "min" | "max" => {
            let [_] = args else {
                return Err(unsupported_native_pipeline(
                    NativeUnsupportedReason::WindowExpression,
                    "window function arity is not supported by native execution",
                ));
            };
            match function {
                "sum" => DataWindowFunction::Sum,
                "mean" => DataWindowFunction::Mean,
                "min" => DataWindowFunction::Min,
                "max" => DataWindowFunction::Max,
                _ => unreachable!("matched aggregate window"),
            }
        }
        _ => {
            return Err(unsupported_native_pipeline(
                NativeUnsupportedReason::WindowExpression,
                "window function is not supported by native execution",
            ));
        }
    };
    Ok(DataExpr::Window {
        function,
        args: args
            .iter()
            .map(|arg| lower_data_expr(arg, context))
            .collect::<Result<Vec<_>, _>>()?,
        spec: lower_data_window_spec(spec, span, context)?,
    })
}

pub(crate) fn lower_data_window_frame(spec: &WindowSpecIr) -> Result<DataWindowFrame, Diagnostic> {
    match spec.frame.as_ref() {
        None => Ok(DataWindowFrame::WholePartition),
        Some(WindowFrameIr {
            start: FrameBoundIr::UnboundedPreceding { .. },
            end: FrameBoundIr::UnboundedFollowing { .. },
            ..
        }) => Ok(DataWindowFrame::WholePartition),
        Some(WindowFrameIr {
            start: FrameBoundIr::UnboundedPreceding { .. },
            end: FrameBoundIr::CurrentRow { .. },
            ..
        }) => Ok(DataWindowFrame::UnboundedPrecedingToCurrentRow),
        Some(_) => Err(unsupported_native_pipeline(
            NativeUnsupportedReason::WindowExpression,
            "bounded window frames are not supported by native execution",
        )),
    }
}

pub(crate) fn lower_data_window_spec(
    spec: &WindowSpecIr,
    span: Span,
    context: &BTreeMap<String, Value>,
) -> Result<DataWindowSpec, Diagnostic> {
    Ok(DataWindowSpec {
        partition_by: resolve_native_column_names(&spec.partition_by, span, context)?,
        order_by: spec
            .order_by
            .iter()
            .map(|item| {
                let direction = match item.direction {
                    SortDirectionIr::Asc => DataSortDirection::Asc,
                    SortDirectionIr::Desc => DataSortDirection::Desc,
                };
                let nulls = item
                    .nulls
                    .map(|nulls| match nulls {
                        NullsOrderIr::First => DataNullsOrder::First,
                        NullsOrderIr::Last => DataNullsOrder::Last,
                    })
                    .unwrap_or(match direction {
                        DataSortDirection::Asc => DataNullsOrder::Last,
                        DataSortDirection::Desc => DataNullsOrder::First,
                    });
                Ok(SortSpec {
                    column: resolve_native_column_name(&item.column, item.span, context)?,
                    direction,
                    nulls,
                })
            })
            .collect::<Result<Vec<_>, Diagnostic>>()?,
        frame: lower_data_window_frame(spec)?,
        row_index: None,
        presorted: false,
    })
}

pub(crate) fn value_to_data_literal(value: &Value) -> DataExpr {
    DataExpr::Literal(match value {
        Value::Null => DataLiteral::Null,
        Value::Bool(value) => DataLiteral::Bool(*value),
        Value::Number(value) => DataLiteral::Number(*value),
        Value::String(value) => DataLiteral::String(value.clone()),
    })
}
