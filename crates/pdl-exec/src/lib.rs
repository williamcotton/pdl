use pdl_core::{has_errors, Diagnostic, Span};
use pdl_data::{
    compare_values, read_csv, write_csv, write_csv_to_vec, NullsOrder, Row, SortDirection,
    SortSpec, Table, Value,
};
use pdl_driver::{program, resolve_input_path, resolve_output_path, PreparedProgram};
use pdl_syntax::{
    AggItem, BinaryOp, Expr, Pipeline, PipelineStart, SaveStage, SinkRef, SourceRef, Stage, UnaryOp,
};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

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
    let diagnostics = prepared.diagnostics();
    if has_errors(&diagnostics) {
        return RunResult {
            stdout: None,
            diagnostics,
        };
    }

    if let Some(format) = &options.stdout_format {
        if format != "csv" {
            let mut diagnostics = diagnostics;
            diagnostics.push(Diagnostic::error(
                "P1705",
                format!("stdout format `{format}` is not supported in 0.1.0-alpha.1"),
                Span::zero(),
            ));
            return RunResult {
                stdout: None,
                diagnostics,
            };
        }
    }

    let mut runtime = Runtime {
        prepared,
        diagnostics,
        cache: BTreeMap::new(),
        dry_run: options.dry_run,
        stdout: None,
    };
    let Some(main) = &program(prepared).main else {
        runtime.diagnostics.push(Diagnostic::error(
            "P1502",
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

    let stdout = if options.stdout_format.as_deref() == Some("csv") {
        match write_csv_to_vec(&table) {
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
    fn execute_pipeline(&mut self, pipeline: &Pipeline) -> Result<Table, Diagnostic> {
        let mut table = match &pipeline.start {
            PipelineStart::Load(load) => self.execute_load(load)?,
            PipelineStart::Binding(name) => self.execute_binding(&name.value)?,
        };
        let mut grouping: Option<Vec<String>> = None;

        for stage in &pipeline.stages {
            match stage {
                Stage::Filter { expr, .. } => {
                    table = self.filter(table, expr)?;
                    grouping = None;
                }
                Stage::Select { items, .. } => {
                    let selection: Vec<(String, String)> = items
                        .iter()
                        .map(|item| {
                            (
                                item.column.value.clone(),
                                item.alias.as_ref().unwrap_or(&item.column).value.clone(),
                            )
                        })
                        .collect();
                    table = table.select(&selection);
                    grouping = None;
                }
                Stage::Drop { columns, .. } => {
                    let columns: Vec<String> =
                        columns.iter().map(|column| column.value.clone()).collect();
                    table = table.drop_columns(&columns);
                    grouping = None;
                }
                Stage::Rename { items, .. } => {
                    let renames: Vec<(String, String)> = items
                        .iter()
                        .map(|item| (item.old.value.clone(), item.new.value.clone()))
                        .collect();
                    table = table.rename_columns(&renames);
                    grouping = None;
                }
                Stage::GroupBy { columns, .. } => {
                    grouping = Some(columns.iter().map(|column| column.value.clone()).collect());
                }
                Stage::Agg { items, .. } => {
                    table = self.aggregate(&table, grouping.take().unwrap_or_default(), items)?;
                }
                Stage::Sort { items, .. } => {
                    let specs = items
                        .iter()
                        .map(|item| {
                            let direction = match item.direction {
                                pdl_syntax::SortDirection::Asc => SortDirection::Asc,
                                pdl_syntax::SortDirection::Desc => SortDirection::Desc,
                            };
                            let nulls = item
                                .nulls
                                .map(|nulls| match nulls {
                                    pdl_syntax::NullsOrder::First => NullsOrder::First,
                                    pdl_syntax::NullsOrder::Last => NullsOrder::Last,
                                })
                                .unwrap_or(match direction {
                                    SortDirection::Asc => NullsOrder::Last,
                                    SortDirection::Desc => NullsOrder::First,
                                });
                            SortSpec {
                                column: item.column.value.clone(),
                                direction,
                                nulls,
                            }
                        })
                        .collect::<Vec<_>>();
                    table.stable_sort(&specs);
                }
                Stage::Limit { n, .. } => {
                    table = table.limit(*n);
                }
                Stage::Save(save) => {
                    self.execute_save(save, &table)?;
                }
                Stage::Unsupported { name, .. } => {
                    return Err(Diagnostic::error(
                        "P1211",
                        format!("stage `{}` is deferred in 0.1.0-alpha.1", name.value),
                        name.span,
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
        let binding = program(self.prepared)
            .bindings
            .iter()
            .find(|binding| binding.name.value == name)
            .ok_or_else(|| {
                Diagnostic::error("P1007", format!("unknown binding `{name}`"), Span::zero())
            })?;
        let table = self.execute_pipeline(&binding.pipeline)?;
        self.cache.insert(name.to_string(), table.clone());
        Ok(table)
    }

    fn execute_load(&self, load: &pdl_syntax::LoadStage) -> Result<Table, Diagnostic> {
        match &load.source {
            SourceRef::Path(path) => {
                if let Some(format) = &load.format {
                    if format.value != "csv" {
                        return Err(Diagnostic::error(
                            "P1215",
                            format!(
                                "format `{}` is not supported in 0.1.0-alpha.1",
                                format.value
                            ),
                            format.span,
                        ));
                    }
                }
                read_csv(&resolve_input_path(&self.prepared.path, &path.value))
            }
            SourceRef::Stdin(span) => Err(Diagnostic::error(
                "P1211",
                "stdin loading is deferred in 0.1.0-alpha.1",
                *span,
            )),
        }
    }

    fn execute_save(&mut self, save: &SaveStage, table: &Table) -> Result<(), Diagnostic> {
        if self.dry_run {
            return Ok(());
        }
        if let Some(format) = &save.format {
            if format.value != "csv" {
                return Err(Diagnostic::error(
                    "P1705",
                    format!(
                        "output format `{}` is not supported in 0.1.0-alpha.1",
                        format.value
                    ),
                    format.span,
                ));
            }
        }
        match &save.sink {
            SinkRef::Path(path) => write_csv(&resolve_output_path(&path.value), table),
            SinkRef::Stdout(_) => {
                let bytes = write_csv_to_vec(table)?;
                self.stdout = Some(bytes);
                Ok(())
            }
        }
    }

    fn filter(&self, table: Table, expr: &Expr) -> Result<Table, Diagnostic> {
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
        items: &[AggItem],
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
        columns.extend(items.iter().map(|item| item.alias.value.clone()));
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
    expr: &Expr,
    table: &Table,
    row: &Row,
    role: ExprRole,
) -> Result<Value, Diagnostic> {
    match expr {
        Expr::Quoted(value) => match role {
            ExprRole::ComparisonLeft => column_value(table, row, &value.value, value.span),
            ExprRole::PredicateRoot | ExprRole::Default => {
                if table.column_index(&value.value).is_some() {
                    column_value(table, row, &value.value, value.span)
                } else {
                    Ok(Value::String(value.value.clone()))
                }
            }
            ExprRole::ComparisonRight => Ok(Value::String(value.value.clone())),
        },
        Expr::Number(value) => Ok(Value::Number(value.value)),
        Expr::Bool(value) => Ok(Value::Bool(value.value)),
        Expr::Null(_) => Ok(Value::Null),
        Expr::Ident(value) => Err(Diagnostic::error(
            "P0008",
            format!("unexpected bare identifier `{}` in expression", value.value),
            value.span,
        )),
        Expr::Call { name, args, span } => eval_call(name.value.as_str(), args, table, row, *span),
        Expr::Unary { op, expr, span } => {
            let value = eval_row_expr(expr, table, row, ExprRole::Default)?;
            match op {
                UnaryOp::Not => match value {
                    Value::Bool(value) => Ok(Value::Bool(!value)),
                    Value::Null => Ok(Value::Null),
                    _ => Err(Diagnostic::error(
                        "P1302",
                        "`not` requires a boolean",
                        *span,
                    )),
                },
                UnaryOp::Neg => match value {
                    Value::Number(value) => Ok(Value::Number(-value)),
                    _ => Err(Diagnostic::error("P1302", "`-` requires a number", *span)),
                },
            }
        }
        Expr::Binary {
            left,
            op,
            right,
            span,
        } => eval_binary(*op, left, right, table, row, *span),
    }
}

fn eval_call(
    name: &str,
    args: &[Expr],
    table: &Table,
    row: &Row,
    span: Span,
) -> Result<Value, Diagnostic> {
    match name {
        "col" => match args {
            [Expr::Quoted(column)] => column_value(table, row, &column.value, column.span),
            _ => Err(Diagnostic::error(
                "P1402",
                "col() expects one quoted column name",
                span,
            )),
        },
        "lit" => match args {
            [Expr::Quoted(value)] => Ok(Value::String(value.value.clone())),
            [expr] => eval_row_expr(expr, table, row, ExprRole::Default),
            _ => Err(Diagnostic::error(
                "P1402",
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
                "P1402",
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
                "P1402",
                "not_null() expects one argument",
                span,
            )),
        },
        _ => Err(Diagnostic::error(
            "P1401",
            format!("unknown function `{name}`"),
            span,
        )),
    }
}

fn eval_binary(
    op: BinaryOp,
    left: &Expr,
    right: &Expr,
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
        BinaryOp::And => {
            let left = eval_row_expr(left, table, row, ExprRole::Default)?;
            let right = eval_row_expr(right, table, row, ExprRole::Default)?;
            Ok(nullable_and(left, right))
        }
        BinaryOp::Or => {
            let left = eval_row_expr(left, table, row, ExprRole::Default)?;
            let right = eval_row_expr(right, table, row, ExprRole::Default)?;
            Ok(nullable_or(left, right))
        }
        BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Rem => {
            let left = eval_row_expr(left, table, row, ExprRole::Default)?;
            let right = eval_row_expr(right, table, row, ExprRole::Default)?;
            let (Some(left), Some(right)) = (left.as_number(), right.as_number()) else {
                return Err(Diagnostic::error(
                    "P1302",
                    "arithmetic requires numeric operands",
                    span,
                ));
            };
            match op {
                BinaryOp::Add => Ok(Value::Number(left + right)),
                BinaryOp::Sub => Ok(Value::Number(left - right)),
                BinaryOp::Mul => Ok(Value::Number(left * right)),
                BinaryOp::Div if right == 0.0 => {
                    Err(Diagnostic::error("P1407", "division by zero", span))
                }
                BinaryOp::Div => Ok(Value::Number(left / right)),
                BinaryOp::Rem => Ok(Value::Number(left % right)),
                _ => unreachable!(),
            }
        }
        _ => unreachable!("comparison operators returned earlier"),
    }
}

fn compare_for_op(left: &Value, op: BinaryOp, right: &Value) -> Value {
    let Some(ordering) = compare_values(left, right) else {
        return Value::Null;
    };
    let result = match op {
        BinaryOp::Eq => ordering == Ordering::Equal,
        BinaryOp::Ne => ordering != Ordering::Equal,
        BinaryOp::Lt => ordering == Ordering::Less,
        BinaryOp::Lte => matches!(ordering, Ordering::Less | Ordering::Equal),
        BinaryOp::Gt => ordering == Ordering::Greater,
        BinaryOp::Gte => matches!(ordering, Ordering::Greater | Ordering::Equal),
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
        .ok_or_else(|| Diagnostic::error("P1005", format!("unknown column `{column}`"), span))
}

fn eval_aggregate(item: &AggItem, table: &Table, rows: &[&Row]) -> Result<Value, Diagnostic> {
    match item.function.value.as_str() {
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
            "P1401",
            format!("unknown aggregate function `{function}`"),
            item.function.span,
        )),
    }
}

fn aggregate_arg_values(
    expr: &Expr,
    table: &Table,
    rows: &[&Row],
) -> Result<Vec<Value>, Diagnostic> {
    rows.iter()
        .map(|row| eval_aggregate_expr(expr, table, row))
        .collect()
}

fn eval_aggregate_expr(expr: &Expr, table: &Table, row: &Row) -> Result<Value, Diagnostic> {
    match expr {
        Expr::Quoted(value) => column_value(table, row, &value.value, value.span),
        Expr::Call { name, args, span } if name.value == "lit" => match args.as_slice() {
            [Expr::Quoted(value)] => Ok(Value::String(value.value.clone())),
            _ => Err(Diagnostic::error(
                "P1402",
                "lit() expects one quoted value",
                *span,
            )),
        },
        Expr::Call { name, args, .. } if name.value == "col" => match args.as_slice() {
            [Expr::Quoted(value)] => column_value(table, row, &value.value, value.span),
            _ => Err(Diagnostic::error(
                "P1402",
                "col() expects one quoted column name",
                name.span,
            )),
        },
        _ => eval_row_expr(expr, table, row, ExprRole::Default),
    }
}

fn is_comparison_op(op: BinaryOp) -> bool {
    matches!(
        op,
        BinaryOp::Eq | BinaryOp::Ne | BinaryOp::Lt | BinaryOp::Lte | BinaryOp::Gt | BinaryOp::Gte
    )
}

#[allow(dead_code)]
fn ensure_unique(values: impl IntoIterator<Item = String>) -> bool {
    let mut seen = BTreeSet::new();
    values.into_iter().all(|value| seen.insert(value))
}
