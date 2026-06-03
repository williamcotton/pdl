use pdl_core::{codes, Diagnostic, Span};
use pdl_data::{
    compare_values, read_csv, NullsOrder as DataNullsOrder, Row,
    SortDirection as DataSortDirection, SortSpec, Table, Value,
};
use pdl_driver::{PreparedProgram, SinkDescriptor, SourceDescriptor};
use pdl_semantics::{
    AggItemIr, BinaryOpIr, ExprIr, NullsOrderIr, PipelineIr, PipelineStartIr, SortDirectionIr,
    StageIr, UnaryOpIr,
};
use std::cmp::Ordering;
use std::collections::BTreeMap;

use crate::output::{emit_csv_stdout, write_csv_output};
use crate::planning::{plan_prepared, PlanningOptions};

#[derive(Clone, Debug, Default)]
pub struct RunOptions {
    pub stdout_format: Option<String>,
    pub dry_run: bool,
}

#[derive(Clone, Debug, Default)]
pub struct RunResult {
    pub stdout: Option<Vec<u8>>,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn run_prepared(prepared: &PreparedProgram, options: RunOptions) -> RunResult {
    let plan = match plan_prepared(
        prepared,
        PlanningOptions {
            stdout_format: options.stdout_format.clone(),
            dry_run: options.dry_run,
        },
    ) {
        Ok(plan) => plan,
        Err(diagnostics) => {
            return RunResult {
                stdout: None,
                diagnostics,
            };
        }
    };

    let mut runtime = Runtime {
        prepared,
        diagnostics: prepared.diagnostics(),
        cache: BTreeMap::new(),
        dry_run: plan.dry_run,
        stdout: None,
    };

    let Some(ir) = prepared.analysis.ir.as_ref() else {
        runtime.diagnostics.push(Diagnostic::error(
            codes::E1505,
            "semantic IR is unavailable for execution",
            Span::zero(),
        ));
        return RunResult {
            stdout: None,
            diagnostics: runtime.diagnostics,
        };
    };
    let Some(main) = &ir.main else {
        runtime.diagnostics.push(Diagnostic::error(
            codes::E1502,
            "no runnable main pipeline",
            Span::zero(),
        ));
        return RunResult {
            stdout: None,
            diagnostics: runtime.diagnostics,
        };
    };

    let table = match runtime.execute_pipeline(main) {
        Ok(table) => table,
        Err(diagnostic) => {
            runtime.diagnostics.push(diagnostic);
            return RunResult {
                stdout: None,
                diagnostics: runtime.diagnostics,
            };
        }
    };

    let stdout = if plan.stdout_format.as_deref() == Some("csv") {
        match emit_csv_stdout(&table) {
            Ok(bytes) => Some(bytes),
            Err(diagnostic) => {
                runtime.diagnostics.push(diagnostic);
                None
            }
        }
    } else {
        runtime.stdout.take()
    };

    RunResult {
        stdout,
        diagnostics: runtime.diagnostics,
    }
}

struct Runtime<'a> {
    prepared: &'a PreparedProgram,
    diagnostics: Vec<Diagnostic>,
    cache: BTreeMap<String, Table>,
    dry_run: bool,
    stdout: Option<Vec<u8>>,
}

impl Runtime<'_> {
    fn execute_pipeline(&mut self, pipeline: &PipelineIr) -> Result<Table, Diagnostic> {
        let mut table = match &pipeline.start {
            PipelineStartIr::Load { format, span, .. } => {
                self.execute_load(*span, format.as_deref())?
            }
            PipelineStartIr::Binding { name, .. } => self.execute_binding(name)?,
        };
        let mut grouping: Option<Vec<String>> = None;

        for stage in &pipeline.stages {
            match stage {
                StageIr::Filter { expr, .. } => {
                    table = self.filter(table, expr)?;
                    grouping = None;
                }
                StageIr::Select { items, .. } => {
                    let selection: Vec<(String, String)> = items
                        .iter()
                        .map(|item| (item.source.clone(), item.output.clone()))
                        .collect();
                    table = table.select(&selection);
                    grouping = None;
                }
                StageIr::Drop { columns, .. } => {
                    table = table.drop_columns(columns);
                    grouping = None;
                }
                StageIr::Rename { items, .. } => {
                    let renames: Vec<(String, String)> = items
                        .iter()
                        .map(|item| (item.old.clone(), item.new.clone()))
                        .collect();
                    table = table.rename_columns(&renames);
                    grouping = None;
                }
                StageIr::GroupBy { columns, .. } => {
                    grouping = Some(columns.clone());
                }
                StageIr::Agg { items, .. } => {
                    table = self.aggregate(&table, grouping.take().unwrap_or_default(), items)?;
                }
                StageIr::Sort { items, .. } => {
                    let specs = items
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
                            SortSpec {
                                column: item.column.clone(),
                                direction,
                                nulls,
                            }
                        })
                        .collect::<Vec<_>>();
                    table.stable_sort(&specs);
                }
                StageIr::Limit { n, .. } => {
                    table = table.limit(*n);
                }
                StageIr::Save { format, span, .. } => {
                    self.execute_save(*span, format.as_deref(), &table)?;
                }
                StageIr::Unsupported { name, span } => {
                    return Err(Diagnostic::error(
                        codes::E1211,
                        format!("stage `{name}` is deferred in 0.5.0"),
                        *span,
                    ));
                }
            }
        }

        Ok(table)
    }

    fn execute_binding(&mut self, name: &str) -> Result<Table, Diagnostic> {
        if let Some(table) = self.cache.get(name) {
            return Ok(table.clone());
        }
        let binding = self
            .prepared
            .analysis
            .ir
            .as_ref()
            .and_then(|ir| ir.bindings.iter().find(|binding| binding.name == name))
            .ok_or_else(|| {
                Diagnostic::error(
                    codes::E1007,
                    format!("unknown binding `{name}`"),
                    Span::zero(),
                )
            })?;
        let table = self.execute_pipeline(&binding.pipeline)?;
        self.cache.insert(name.to_string(), table.clone());
        Ok(table)
    }

    fn execute_load(
        &self,
        stage_span: Span,
        explicit_format: Option<&str>,
    ) -> Result<Table, Diagnostic> {
        if let Some(format) = explicit_format {
            if format != "csv" {
                return Err(Diagnostic::error(
                    codes::E1215,
                    format!("format `{format}` is not supported in 0.5.0"),
                    stage_span,
                ));
            }
        }
        let Some(input) = self.prepared.driver_plan.input_for_stage_span(stage_span) else {
            return Err(Diagnostic::error(
                codes::E1505,
                "driver source facts are unavailable for execution",
                stage_span,
            ));
        };
        match &input.source {
            SourceDescriptor::Path { resolved_path, .. } => read_csv(resolved_path),
            SourceDescriptor::Stdin => Err(Diagnostic::error(
                codes::E1211,
                "stdin loading is deferred in 0.5.0",
                input.span,
            )),
        }
    }

    fn execute_save(
        &mut self,
        stage_span: Span,
        explicit_format: Option<&str>,
        table: &Table,
    ) -> Result<(), Diagnostic> {
        if self.dry_run {
            return Ok(());
        }
        if let Some(format) = explicit_format {
            if format != "csv" {
                return Err(Diagnostic::error(
                    codes::E1705,
                    format!("output format `{format}` is not supported in 0.5.0"),
                    stage_span,
                ));
            }
        }
        let Some(sink) = self.prepared.driver_plan.sink_for_stage_span(stage_span) else {
            return Err(Diagnostic::error(
                codes::E1505,
                "driver sink facts are unavailable for execution",
                stage_span,
            ));
        };
        match &sink.sink {
            SinkDescriptor::Path { resolved_path, .. } => write_csv_output(resolved_path, table),
            SinkDescriptor::Stdout => {
                let bytes = emit_csv_stdout(table)?;
                self.stdout = Some(bytes);
                Ok(())
            }
        }
    }

    fn filter(&self, table: Table, expr: &ExprIr) -> Result<Table, Diagnostic> {
        let rows = table
            .rows
            .iter()
            .filter_map(
                |row| match eval_row_expr(expr, &table, row, ExprRole::PredicateRoot) {
                    Ok(value) if value.is_truthy_true() => Some(Ok(row.clone())),
                    Ok(_) => None,
                    Err(diagnostic) => Some(Err(diagnostic)),
                },
            )
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Table {
            columns: table.columns,
            rows,
        })
    }

    fn aggregate(
        &self,
        table: &Table,
        group_keys: Vec<String>,
        items: &[AggItemIr],
    ) -> Result<Table, Diagnostic> {
        let mut grouped: BTreeMap<Vec<String>, Vec<&Row>> = BTreeMap::new();
        if group_keys.is_empty() {
            grouped.insert(Vec::new(), table.rows.iter().collect());
        } else {
            for row in &table.rows {
                let key = group_keys
                    .iter()
                    .map(|column| {
                        table
                            .value(row, column)
                            .unwrap_or(&Value::Null)
                            .to_csv_cell()
                    })
                    .collect::<Vec<_>>();
                grouped.entry(key).or_default().push(row);
            }
        }

        let mut columns = group_keys.clone();
        columns.extend(items.iter().map(|item| item.alias.clone()));
        let mut rows = Vec::new();

        for (key, group_rows) in grouped {
            let mut values = key.into_iter().map(Value::String).collect::<Vec<_>>();
            for item in items {
                values.push(eval_aggregate(item, table, &group_rows)?);
            }
            rows.push(Row { values });
        }

        Ok(Table { columns, rows })
    }
}

#[derive(Clone, Copy)]
enum ExprRole {
    PredicateRoot,
    Default,
    ComparisonLeft,
    ComparisonRight,
}

fn eval_row_expr(
    expr: &ExprIr,
    table: &Table,
    row: &Row,
    role: ExprRole,
) -> Result<Value, Diagnostic> {
    match expr {
        ExprIr::Quoted { value, span } => match role {
            ExprRole::ComparisonLeft => column_value(table, row, value, *span),
            ExprRole::PredicateRoot | ExprRole::Default => {
                if table.column_index(value).is_some() {
                    column_value(table, row, value, *span)
                } else {
                    Ok(Value::String(value.clone()))
                }
            }
            ExprRole::ComparisonRight => Ok(Value::String(value.clone())),
        },
        ExprIr::Number { value, .. } => Ok(Value::Number(*value)),
        ExprIr::Bool { value, .. } => Ok(Value::Bool(*value)),
        ExprIr::Null { .. } => Ok(Value::Null),
        ExprIr::Ident { value, span } => Err(Diagnostic::error(
            codes::E0008,
            format!("unexpected bare identifier `{value}` in expression"),
            *span,
        )),
        ExprIr::Call { name, args, span } => eval_call(name, args, table, row, *span),
        ExprIr::Unary { op, expr, span } => {
            let value = eval_row_expr(expr, table, row, ExprRole::Default)?;
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
        } => eval_binary(*op, left, right, table, row, *span),
    }
}

fn eval_call(
    name: &str,
    args: &[ExprIr],
    table: &Table,
    row: &Row,
    span: Span,
) -> Result<Value, Diagnostic> {
    match name {
        "col" => match args {
            [ExprIr::Quoted {
                value: column,
                span,
            }] => column_value(table, row, column, *span),
            _ => Err(Diagnostic::error(
                codes::E1402,
                "col() expects one quoted column name",
                span,
            )),
        },
        "lit" => match args {
            [ExprIr::Quoted { value, .. }] => Ok(Value::String(value.clone())),
            [expr] => eval_row_expr(expr, table, row, ExprRole::Default),
            _ => Err(Diagnostic::error(
                codes::E1402,
                "lit() expects one argument",
                span,
            )),
        },
        "is_null" => match args {
            [expr] => Ok(Value::Bool(matches!(
                eval_row_expr(expr, table, row, ExprRole::Default)?,
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
                eval_row_expr(expr, table, row, ExprRole::Default)?,
                Value::Null
            ))),
            _ => Err(Diagnostic::error(
                codes::E1402,
                "not_null() expects one argument",
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

fn eval_binary(
    op: BinaryOpIr,
    left: &ExprIr,
    right: &ExprIr,
    table: &Table,
    row: &Row,
    span: Span,
) -> Result<Value, Diagnostic> {
    if is_comparison_op(op) {
        let left = eval_row_expr(left, table, row, ExprRole::ComparisonLeft)?;
        let right = eval_row_expr(right, table, row, ExprRole::ComparisonRight)?;
        return Ok(compare_for_op(&left, op, &right));
    }

    match op {
        BinaryOpIr::And => {
            let left = eval_row_expr(left, table, row, ExprRole::Default)?;
            let right = eval_row_expr(right, table, row, ExprRole::Default)?;
            Ok(nullable_and(left, right))
        }
        BinaryOpIr::Or => {
            let left = eval_row_expr(left, table, row, ExprRole::Default)?;
            let right = eval_row_expr(right, table, row, ExprRole::Default)?;
            Ok(nullable_or(left, right))
        }
        BinaryOpIr::Add | BinaryOpIr::Sub | BinaryOpIr::Mul | BinaryOpIr::Div | BinaryOpIr::Rem => {
            let left = eval_row_expr(left, table, row, ExprRole::Default)?;
            let right = eval_row_expr(right, table, row, ExprRole::Default)?;
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
    table
        .value(row, column)
        .cloned()
        .ok_or_else(|| Diagnostic::error(codes::E1005, format!("unknown column `{column}`"), span))
}

fn eval_aggregate(item: &AggItemIr, table: &Table, rows: &[&Row]) -> Result<Value, Diagnostic> {
    match item.function.as_str() {
        "count" if item.args.is_empty() => Ok(Value::Number(rows.len() as f64)),
        "count" => {
            let values = aggregate_arg_values(&item.args[0], table, rows)?;
            Ok(Value::Number(
                values
                    .iter()
                    .filter(|value| !matches!(value, Value::Null))
                    .count() as f64,
            ))
        }
        "sum" => {
            let values = aggregate_arg_values(&item.args[0], table, rows)?;
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
            let values = aggregate_arg_values(&item.args[0], table, rows)?;
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
        "min" => aggregate_arg_values(&item.args[0], table, rows).map(|values| {
            values
                .into_iter()
                .filter(|value| !matches!(value, Value::Null))
                .min_by(|left, right| compare_values(left, right).unwrap_or(Ordering::Equal))
                .unwrap_or(Value::Null)
        }),
        "max" => aggregate_arg_values(&item.args[0], table, rows).map(|values| {
            values
                .into_iter()
                .filter(|value| !matches!(value, Value::Null))
                .max_by(|left, right| compare_values(left, right).unwrap_or(Ordering::Equal))
                .unwrap_or(Value::Null)
        }),
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
) -> Result<Vec<Value>, Diagnostic> {
    rows.iter()
        .map(|row| eval_aggregate_expr(expr, table, row))
        .collect()
}

fn eval_aggregate_expr(expr: &ExprIr, table: &Table, row: &Row) -> Result<Value, Diagnostic> {
    match expr {
        ExprIr::Quoted { value, span } => column_value(table, row, value, *span),
        ExprIr::Call { name, args, span } if name == "lit" => match args.as_slice() {
            [ExprIr::Quoted { value, .. }] => Ok(Value::String(value.clone())),
            _ => Err(Diagnostic::error(
                codes::E1402,
                "lit() expects one quoted value",
                *span,
            )),
        },
        ExprIr::Call { name, args, span } if name == "col" => match args.as_slice() {
            [ExprIr::Quoted {
                value: column,
                span,
            }] => column_value(table, row, column, *span),
            _ => Err(Diagnostic::error(
                codes::E1402,
                "col() expects one quoted column name",
                *span,
            )),
        },
        _ => eval_row_expr(expr, table, row, ExprRole::Default),
    }
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
