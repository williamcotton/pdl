use pdl_core::{codes, Diagnostic, Span};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
#[cfg(feature = "polars-engine")]
use std::ops::Neg;
use std::path::Path;

use crate::format::{read_table_from_bytes, write_table_to_bytes, write_table_to_path, DataFormat};
use crate::frame::{compare_values, Row, SortSpec, Table};
#[cfg(feature = "polars-engine")]
use crate::frame::{NullsOrder, SortDirection};
use crate::value::Value;

#[cfg(feature = "polars-engine")]
use native::{IntoLazy, LazyFileListReader, SerReader, SerWriter};
#[cfg(feature = "polars-engine")]
use polars::prelude as native;

#[cfg(feature = "polars-engine")]
pub fn native_engine_name() -> &'static str {
    let _ = std::any::type_name::<native::DataFrame>();
    "polars"
}

#[cfg(not(feature = "polars-engine"))]
pub fn native_engine_name() -> &'static str {
    "in-memory"
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DataBackend {
    #[default]
    PortableRows,
    NativePolars,
}

pub struct DataPlan {
    inner: DataPlanInner,
}

enum DataPlanInner {
    Rows(Table),
    #[cfg(feature = "polars-engine")]
    Native(NativePlan),
}

#[cfg(feature = "polars-engine")]
struct NativePlan {
    format: DataFormat,
    plan: native::LazyFrame,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DataSource<'a> {
    Path {
        path: &'a Path,
        format: DataFormat,
    },
    Bytes {
        logical_path: &'a Path,
        format: DataFormat,
        bytes: &'a [u8],
    },
}

pub enum DataSink<'a> {
    Path {
        path: &'a Path,
        format: DataFormat,
    },
    Writer {
        format: DataFormat,
        writer: &'a mut dyn Write,
    },
    Bytes {
        format: DataFormat,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum DataExpr {
    Column(String),
    Literal(DataLiteral),
    Unary {
        op: DataUnaryOp,
        expr: Box<DataExpr>,
    },
    Binary {
        left: Box<DataExpr>,
        op: DataBinaryOp,
        right: Box<DataExpr>,
    },
    Call {
        function: DataScalarFunction,
        args: Vec<DataExpr>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum DataLiteral {
    String(String),
    Number(f64),
    Bool(bool),
    Null,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DataUnaryOp {
    Not,
    Neg,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DataBinaryOp {
    Or,
    And,
    Eq,
    Ne,
    Lt,
    Lte,
    Gt,
    Gte,
    Add,
    Sub,
    Mul,
    Div,
    Rem,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DataScalarFunction {
    IsNull,
    NotNull,
    Lower,
    Upper,
    Trim,
    Abs,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DataAggItem {
    pub function: String,
    pub args: Vec<DataExpr>,
    pub alias: String,
}

impl DataPlan {
    pub fn scan(source: DataSource<'_>) -> Result<Self, Diagnostic> {
        Self::scan_with_backend(source, DataBackend::PortableRows)
    }

    pub fn scan_with_backend(
        source: DataSource<'_>,
        backend: DataBackend,
    ) -> Result<Self, Diagnostic> {
        match backend {
            DataBackend::PortableRows => scan_rows(source),
            DataBackend::NativePolars => scan_native(source),
        }
    }

    pub fn from_table(table: Table) -> Self {
        Self {
            inner: DataPlanInner::Rows(table),
        }
    }

    pub fn backend(&self) -> DataBackend {
        match &self.inner {
            DataPlanInner::Rows(_) => DataBackend::PortableRows,
            #[cfg(feature = "polars-engine")]
            DataPlanInner::Native(_) => DataBackend::NativePolars,
        }
    }

    pub fn filter(self, expr: DataExpr) -> Result<Self, Diagnostic> {
        match self.inner {
            DataPlanInner::Rows(table) => Ok(Self::from_table(filter_rows(table, &expr)?)),
            #[cfg(feature = "polars-engine")]
            DataPlanInner::Native(plan) => {
                let expr = native_expr(&expr)?;
                Ok(Self {
                    inner: DataPlanInner::Native(NativePlan {
                        format: plan.format,
                        plan: plan.plan.filter(expr),
                    }),
                })
            }
        }
    }

    pub fn select(self, items: &[(String, String)]) -> Result<Self, Diagnostic> {
        match self.inner {
            DataPlanInner::Rows(table) => Ok(Self::from_table(table.select(items))),
            #[cfg(feature = "polars-engine")]
            DataPlanInner::Native(plan) => {
                let expressions = items
                    .iter()
                    .map(|(source, output)| native::col(source).alias(output))
                    .collect::<Vec<_>>();
                Ok(Self {
                    inner: DataPlanInner::Native(NativePlan {
                        format: plan.format,
                        plan: plan.plan.select(expressions),
                    }),
                })
            }
        }
    }

    pub fn drop_columns(self, columns: &[String]) -> Result<Self, Diagnostic> {
        match self.inner {
            DataPlanInner::Rows(table) => Ok(Self::from_table(table.drop_columns(columns))),
            #[cfg(feature = "polars-engine")]
            DataPlanInner::Native(plan) => Ok(Self {
                inner: DataPlanInner::Native(NativePlan {
                    format: plan.format,
                    plan: plan.plan.drop(native::cols(columns.iter().cloned())),
                }),
            }),
        }
    }

    pub fn rename_columns(self, renames: &[(String, String)]) -> Result<Self, Diagnostic> {
        match self.inner {
            DataPlanInner::Rows(table) => Ok(Self::from_table(table.rename_columns(renames))),
            #[cfg(feature = "polars-engine")]
            DataPlanInner::Native(plan) => {
                let (old, new): (Vec<_>, Vec<_>) = renames.iter().cloned().unzip();
                Ok(Self {
                    inner: DataPlanInner::Native(NativePlan {
                        format: plan.format,
                        plan: plan.plan.rename(old, new, true),
                    }),
                })
            }
        }
    }

    pub fn mutate(self, items: &[(String, DataExpr)]) -> Result<Self, Diagnostic> {
        match self.inner {
            DataPlanInner::Rows(table) => Ok(Self::from_table(mutate_rows(table, items)?)),
            #[cfg(feature = "polars-engine")]
            DataPlanInner::Native(_) => Err(unsupported_native_operation("mutate")),
        }
    }

    pub fn sort(self, specs: &[SortSpec]) -> Result<Self, Diagnostic> {
        match self.inner {
            DataPlanInner::Rows(mut table) => {
                table.stable_sort(specs);
                Ok(Self::from_table(table))
            }
            #[cfg(feature = "polars-engine")]
            DataPlanInner::Native(plan) => {
                let columns = specs
                    .iter()
                    .map(|spec| spec.column.clone())
                    .collect::<Vec<_>>();
                let descending = specs
                    .iter()
                    .map(|spec| spec.direction == SortDirection::Desc)
                    .collect::<Vec<_>>();
                let nulls_last = specs
                    .iter()
                    .map(|spec| spec.nulls == NullsOrder::Last)
                    .collect::<Vec<_>>();
                let options = native::SortMultipleOptions {
                    descending,
                    nulls_last,
                    maintain_order: true,
                    ..Default::default()
                };
                Ok(Self {
                    inner: DataPlanInner::Native(NativePlan {
                        format: plan.format,
                        plan: plan.plan.sort(columns, options),
                    }),
                })
            }
        }
    }

    pub fn limit(self, n: usize) -> Result<Self, Diagnostic> {
        match self.inner {
            DataPlanInner::Rows(table) => Ok(Self::from_table(table.limit(n))),
            #[cfg(feature = "polars-engine")]
            DataPlanInner::Native(plan) => Ok(Self {
                inner: DataPlanInner::Native(NativePlan {
                    format: plan.format,
                    plan: plan.plan.limit(n as native::IdxSize),
                }),
            }),
        }
    }

    pub fn distinct(self, columns: &[String]) -> Result<Self, Diagnostic> {
        match self.inner {
            DataPlanInner::Rows(table) => Ok(Self::from_table(table.distinct(columns))),
            #[cfg(feature = "polars-engine")]
            DataPlanInner::Native(plan) => {
                let subset = if columns.is_empty() {
                    None
                } else {
                    Some(columns.iter().map(native::col).collect::<Vec<_>>())
                };
                Ok(Self {
                    inner: DataPlanInner::Native(NativePlan {
                        format: plan.format,
                        plan: plan
                            .plan
                            .unique_stable_generic(subset, native::UniqueKeepStrategy::First),
                    }),
                })
            }
        }
    }

    pub fn aggregate(
        self,
        group_keys: &[String],
        items: &[DataAggItem],
    ) -> Result<Self, Diagnostic> {
        match self.inner {
            DataPlanInner::Rows(table) => {
                Ok(Self::from_table(aggregate_rows(&table, group_keys, items)?))
            }
            #[cfg(feature = "polars-engine")]
            DataPlanInner::Native(plan) => {
                let aggregations = items
                    .iter()
                    .map(native_agg_expr)
                    .collect::<Result<Vec<_>, _>>()?;
                let aggregated = if group_keys.is_empty() {
                    plan.plan.select(aggregations)
                } else {
                    let keys = group_keys
                        .iter()
                        .map(|key| native::col(key).cast(native::DataType::String).alias(key))
                        .collect::<Vec<_>>();
                    let options = native::SortMultipleOptions {
                        descending: vec![false; group_keys.len()],
                        nulls_last: vec![true; group_keys.len()],
                        maintain_order: true,
                        ..Default::default()
                    };
                    plan.plan
                        .group_by(keys)
                        .agg(aggregations)
                        .sort(group_keys.to_vec(), options)
                };
                Ok(Self {
                    inner: DataPlanInner::Native(NativePlan {
                        format: plan.format,
                        plan: aggregated,
                    }),
                })
            }
        }
    }

    pub fn collect(self) -> Result<Table, Diagnostic> {
        match self.inner {
            DataPlanInner::Rows(table) => Ok(table),
            #[cfg(feature = "polars-engine")]
            DataPlanInner::Native(plan) => native_collect_to_table(plan),
        }
    }

    pub fn write_to_sink(self, sink: DataSink<'_>) -> Result<Option<Vec<u8>>, Diagnostic> {
        match self.inner {
            DataPlanInner::Rows(table) => write_rows_to_sink(&table, sink),
            #[cfg(feature = "polars-engine")]
            DataPlanInner::Native(plan) => write_native_to_sink(plan, sink),
        }
    }
}

fn scan_rows(source: DataSource<'_>) -> Result<DataPlan, Diagnostic> {
    let table = match source {
        DataSource::Path { path, format } => {
            if format == DataFormat::Csv {
                crate::csv::read_csv(path)?
            } else {
                let bytes = std::fs::read(path).map_err(|error| {
                    Diagnostic::error(
                        codes::E1802,
                        format!("could not read data file `{}`: {error}", path.display()),
                        Span::zero(),
                    )
                })?;
                read_table_from_bytes(path, format, &bytes)?
            }
        }
        DataSource::Bytes {
            logical_path,
            format,
            bytes,
        } => read_table_from_bytes(logical_path, format, bytes)?,
    };
    Ok(DataPlan::from_table(table))
}

#[cfg(feature = "polars-engine")]
fn scan_native(source: DataSource<'_>) -> Result<DataPlan, Diagnostic> {
    let DataSource::Path { path, format } = source else {
        return Err(unsupported_native_operation("byte-backed input"));
    };
    let plan = match format {
        DataFormat::Csv => native::LazyCsvReader::new(native_path(path)?)
            .with_has_header(true)
            .finish()
            .map_err(native_read_error(path, format))?,
        DataFormat::Parquet => {
            native::LazyFrame::scan_parquet(native_path(path)?, Default::default())
                .map_err(native_read_error(path, format))?
        }
        DataFormat::ArrowStream => {
            let file = std::fs::File::open(path).map_err(|error| {
                Diagnostic::error(
                    codes::E1802,
                    format!("could not read data file `{}`: {error}", path.display()),
                    Span::zero(),
                )
            })?;
            native::IpcStreamReader::new(file)
                .finish()
                .map_err(native_read_error(path, format))?
                .lazy()
        }
        DataFormat::ArrowFile | DataFormat::JsonLines => {
            return Err(unsupported_native_format(format));
        }
    };
    Ok(DataPlan {
        inner: DataPlanInner::Native(NativePlan { format, plan }),
    })
}

#[cfg(not(feature = "polars-engine"))]
fn scan_native(_source: DataSource<'_>) -> Result<DataPlan, Diagnostic> {
    Err(Diagnostic::error(
        codes::E1215,
        "native data backend is not enabled",
        Span::zero(),
    ))
}

fn write_rows_to_sink(table: &Table, sink: DataSink<'_>) -> Result<Option<Vec<u8>>, Diagnostic> {
    match sink {
        DataSink::Path { path, format } => {
            write_table_to_path(path, format, table)?;
            Ok(None)
        }
        DataSink::Writer { format, writer } => {
            let bytes = write_table_to_bytes(format, table)?;
            writer.write_all(&bytes).map_err(output_write_error)?;
            Ok(None)
        }
        DataSink::Bytes { format } => write_table_to_bytes(format, table).map(Some),
    }
}

#[cfg(feature = "polars-engine")]
fn write_native_to_sink(
    plan: NativePlan,
    sink: DataSink<'_>,
) -> Result<Option<Vec<u8>>, Diagnostic> {
    match sink {
        DataSink::Bytes { format } => {
            let mut bytes = Vec::new();
            write_native_to_writer(plan, format, &mut bytes)?;
            Ok(Some(bytes))
        }
        DataSink::Writer { format, writer } => {
            write_native_to_writer(plan, format, writer)?;
            Ok(None)
        }
        DataSink::Path { path, format } => {
            if format == DataFormat::ArrowStream {
                let file = std::fs::File::create(path).map_err(|error| {
                    Diagnostic::error(
                        codes::E1704,
                        format!(
                            "output file `{}` could not be created: {error}",
                            path.display()
                        ),
                        Span::zero(),
                    )
                })?;
                let mut writer = std::io::BufWriter::new(file);
                write_native_to_writer(plan, format, &mut writer)?;
                Ok(None)
            } else {
                let table = native_collect_to_table(plan)?;
                write_table_to_path(path, format, &table)?;
                Ok(None)
            }
        }
    }
}

#[cfg(feature = "polars-engine")]
fn write_native_to_writer(
    plan: NativePlan,
    format: DataFormat,
    writer: &mut dyn Write,
) -> Result<(), Diagnostic> {
    if format != DataFormat::ArrowStream {
        let table = native_collect_to_table(plan)?;
        let bytes = write_table_to_bytes(format, &table)?;
        writer.write_all(&bytes).map_err(output_write_error)?;
        return Ok(());
    }

    let mut frame = plan
        .plan
        .collect()
        .map_err(native_collect_error(plan.format))?;
    native::IpcStreamWriter::new(writer)
        .finish(&mut frame)
        .map_err(|error| {
            Diagnostic::error(
                codes::E1704,
                format!("native Arrow IPC stream write failed: {error}"),
                Span::zero(),
            )
        })
}

fn output_write_error(error: std::io::Error) -> Diagnostic {
    Diagnostic::error(
        codes::E1704,
        format!("output write failed: {error}"),
        Span::zero(),
    )
}

fn filter_rows(table: Table, expr: &DataExpr) -> Result<Table, Diagnostic> {
    let rows = table
        .rows
        .iter()
        .filter_map(|row| match eval_row_expr(expr, &table, row) {
            Ok(value) if value.is_truthy_true() => Some(Ok(row.clone())),
            Ok(_) => None,
            Err(diagnostic) => Some(Err(diagnostic)),
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Table {
        columns: table.columns,
        rows,
    })
}

fn mutate_rows(table: Table, items: &[(String, DataExpr)]) -> Result<Table, Diagnostic> {
    let input_columns = table.columns.clone();
    let mut columns = input_columns.clone();
    for (column, _) in items {
        if !columns.iter().any(|existing| existing == column) {
            columns.push(column.clone());
        }
    }
    let rows = table
        .rows
        .iter()
        .map(|row| {
            let mut values = row.values.clone();
            for (column, expr) in items {
                let value = eval_row_expr(expr, &table, row)?;
                if let Some(index) = input_columns.iter().position(|existing| existing == column) {
                    values[index] = value;
                } else {
                    values.push(value);
                }
            }
            Ok(Row { values })
        })
        .collect::<Result<Vec<_>, Diagnostic>>()?;
    Ok(Table { columns, rows })
}

fn aggregate_rows(
    table: &Table,
    group_keys: &[String],
    items: &[DataAggItem],
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

    let mut columns = group_keys.to_vec();
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

fn eval_aggregate(item: &DataAggItem, table: &Table, rows: &[&Row]) -> Result<Value, Diagnostic> {
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
            let numbers = values
                .into_iter()
                .filter_map(|value| value.as_number())
                .collect::<Vec<_>>();
            if numbers.is_empty() {
                Ok(Value::Null)
            } else {
                Ok(Value::Number(numbers.iter().sum()))
            }
        }
        "mean" => {
            let values = aggregate_arg_values(&item.args[0], table, rows)?;
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
        "count_distinct" => {
            let values = aggregate_arg_values(&item.args[0], table, rows)?;
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
            Span::zero(),
        )),
    }
}

fn aggregate_arg_values(
    expr: &DataExpr,
    table: &Table,
    rows: &[&Row],
) -> Result<Vec<Value>, Diagnostic> {
    rows.iter()
        .map(|row| eval_row_expr(expr, table, row))
        .collect()
}

fn eval_row_expr(expr: &DataExpr, table: &Table, row: &Row) -> Result<Value, Diagnostic> {
    match expr {
        DataExpr::Column(column) => column_value(table, row, column),
        DataExpr::Literal(literal) => Ok(literal_value(literal)),
        DataExpr::Unary { op, expr } => {
            let value = eval_row_expr(expr, table, row)?;
            match op {
                DataUnaryOp::Not => match value {
                    Value::Bool(value) => Ok(Value::Bool(!value)),
                    Value::Null => Ok(Value::Null),
                    _ => Err(type_error("`not` requires a boolean")),
                },
                DataUnaryOp::Neg => match value {
                    Value::Number(value) => Ok(Value::Number(-value)),
                    _ => Err(type_error("`-` requires a number")),
                },
            }
        }
        DataExpr::Binary { left, op, right } => {
            let left_value = eval_row_expr(left, table, row)?;
            let right_value = eval_row_expr(right, table, row)?;
            eval_binary_value(left_value, *op, right_value)
        }
        DataExpr::Call { function, args } => eval_scalar_function(*function, args, table, row),
    }
}

fn literal_value(literal: &DataLiteral) -> Value {
    match literal {
        DataLiteral::String(value) => Value::String(value.clone()),
        DataLiteral::Number(value) => Value::Number(*value),
        DataLiteral::Bool(value) => Value::Bool(*value),
        DataLiteral::Null => Value::Null,
    }
}

fn column_value(table: &Table, row: &Row, column: &str) -> Result<Value, Diagnostic> {
    table.value(row, column).cloned().ok_or_else(|| {
        Diagnostic::error(
            codes::E1005,
            format!("unknown column `{column}`"),
            Span::zero(),
        )
    })
}

fn eval_binary_value(left: Value, op: DataBinaryOp, right: Value) -> Result<Value, Diagnostic> {
    match op {
        DataBinaryOp::Or => match (left, right) {
            (Value::Bool(true), _) | (_, Value::Bool(true)) => Ok(Value::Bool(true)),
            (Value::Bool(false), Value::Bool(false)) => Ok(Value::Bool(false)),
            (Value::Null, _) | (_, Value::Null) => Ok(Value::Null),
            _ => Err(type_error("`or` requires booleans")),
        },
        DataBinaryOp::And => match (left, right) {
            (Value::Bool(false), _) | (_, Value::Bool(false)) => Ok(Value::Bool(false)),
            (Value::Bool(true), Value::Bool(true)) => Ok(Value::Bool(true)),
            (Value::Null, _) | (_, Value::Null) => Ok(Value::Null),
            _ => Err(type_error("`and` requires booleans")),
        },
        DataBinaryOp::Eq => Ok(Value::Bool(values_equal(&left, &right))),
        DataBinaryOp::Ne => Ok(Value::Bool(!values_equal(&left, &right))),
        DataBinaryOp::Lt | DataBinaryOp::Lte | DataBinaryOp::Gt | DataBinaryOp::Gte => {
            if matches!(left, Value::Null) || matches!(right, Value::Null) {
                return Ok(Value::Null);
            }
            let Some(ordering) = compare_values(&left, &right) else {
                return Ok(Value::Null);
            };
            Ok(Value::Bool(match op {
                DataBinaryOp::Lt => ordering == Ordering::Less,
                DataBinaryOp::Lte => ordering != Ordering::Greater,
                DataBinaryOp::Gt => ordering == Ordering::Greater,
                DataBinaryOp::Gte => ordering != Ordering::Less,
                _ => unreachable!("handled comparison op"),
            }))
        }
        DataBinaryOp::Add
        | DataBinaryOp::Sub
        | DataBinaryOp::Mul
        | DataBinaryOp::Div
        | DataBinaryOp::Rem => match (left, right) {
            (Value::Number(left), Value::Number(right)) => Ok(Value::Number(match op {
                DataBinaryOp::Add => left + right,
                DataBinaryOp::Sub => left - right,
                DataBinaryOp::Mul => left * right,
                DataBinaryOp::Div => left / right,
                DataBinaryOp::Rem => left % right,
                _ => unreachable!("handled arithmetic op"),
            })),
            (Value::Null, _) | (_, Value::Null) => Ok(Value::Null),
            _ => Err(type_error("arithmetic requires numbers")),
        },
    }
}

fn values_equal(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Null, Value::Null) => true,
        (Value::Null, _) | (_, Value::Null) => false,
        _ => compare_values(left, right) == Some(Ordering::Equal),
    }
}

fn eval_scalar_function(
    function: DataScalarFunction,
    args: &[DataExpr],
    table: &Table,
    row: &Row,
) -> Result<Value, Diagnostic> {
    let [arg] = args else {
        return Err(Diagnostic::error(
            codes::E1402,
            "scalar function expects one argument",
            Span::zero(),
        ));
    };
    let value = eval_row_expr(arg, table, row)?;
    match function {
        DataScalarFunction::IsNull => Ok(Value::Bool(matches!(value, Value::Null))),
        DataScalarFunction::NotNull => Ok(Value::Bool(!matches!(value, Value::Null))),
        DataScalarFunction::Lower => Ok(map_text(value, |text| text.to_ascii_lowercase())),
        DataScalarFunction::Upper => Ok(map_text(value, |text| text.to_ascii_uppercase())),
        DataScalarFunction::Trim => Ok(map_text(value, |text| text.trim().to_string())),
        DataScalarFunction::Abs => match value {
            Value::Null => Ok(Value::Null),
            Value::Number(value) => Ok(Value::Number(value.abs())),
            _ => Err(type_error("abs() requires a number")),
        },
    }
}

fn map_text(value: Value, map: impl FnOnce(&str) -> String) -> Value {
    match value {
        Value::Null => Value::Null,
        Value::String(value) => Value::String(map(&value)),
        value => Value::String(map(&value.to_csv_cell())),
    }
}

fn type_error(message: &'static str) -> Diagnostic {
    Diagnostic::error(codes::E1302, message, Span::zero())
}

#[cfg(feature = "polars-engine")]
fn native_expr(expr: &DataExpr) -> Result<native::Expr, Diagnostic> {
    Ok(match expr {
        DataExpr::Column(column) => native::col(column),
        DataExpr::Literal(literal) => native_literal(literal),
        DataExpr::Unary {
            op: DataUnaryOp::Not,
            expr,
        } => native_expr(expr)?.not(),
        DataExpr::Unary {
            op: DataUnaryOp::Neg,
            expr,
        } => native_expr(expr)?.neg(),
        DataExpr::Binary { left, op, right } => {
            let left = native_expr(left)?;
            let right = native_expr(right)?;
            match op {
                DataBinaryOp::Or => left.or(right),
                DataBinaryOp::And => left.and(right),
                DataBinaryOp::Eq => left.eq(right),
                DataBinaryOp::Ne => left.neq(right),
                DataBinaryOp::Lt => left.lt(right),
                DataBinaryOp::Lte => left.lt_eq(right),
                DataBinaryOp::Gt => left.gt(right),
                DataBinaryOp::Gte => left.gt_eq(right),
                DataBinaryOp::Add => left + right,
                DataBinaryOp::Sub => left - right,
                DataBinaryOp::Mul => left * right,
                DataBinaryOp::Div => left / right,
                DataBinaryOp::Rem => left % right,
            }
        }
        DataExpr::Call { function, args } => {
            let [arg] = args.as_slice() else {
                return Err(unsupported_native_operation(
                    "multi-argument scalar function",
                ));
            };
            let arg = native_expr(arg)?;
            match function {
                DataScalarFunction::IsNull => arg.is_null(),
                DataScalarFunction::NotNull => arg.is_not_null(),
                DataScalarFunction::Abs => arg.abs(),
                DataScalarFunction::Lower
                | DataScalarFunction::Upper
                | DataScalarFunction::Trim => {
                    return Err(unsupported_native_operation("text scalar function"));
                }
            }
        }
    })
}

#[cfg(feature = "polars-engine")]
fn native_agg_expr(item: &DataAggItem) -> Result<native::Expr, Diagnostic> {
    let expr = match item.function.as_str() {
        "count" if item.args.is_empty() => native::len(),
        "count" => {
            let [arg] = item.args.as_slice() else {
                return Err(unsupported_native_operation("count aggregate arity"));
            };
            native_expr(arg)?.count()
        }
        "sum" => native_unary_agg(item, |expr| expr.sum())?,
        "mean" => native_unary_agg(item, |expr| expr.mean())?,
        "min" => native_unary_agg(item, |expr| expr.min())?,
        "max" => native_unary_agg(item, |expr| expr.max())?,
        _ => return Err(unsupported_native_operation("aggregate function")),
    };
    Ok(expr.alias(&item.alias))
}

#[cfg(feature = "polars-engine")]
fn native_unary_agg(
    item: &DataAggItem,
    aggregate: impl FnOnce(native::Expr) -> native::Expr,
) -> Result<native::Expr, Diagnostic> {
    let [arg] = item.args.as_slice() else {
        return Err(unsupported_native_operation("aggregate arity"));
    };
    Ok(aggregate(native_expr(arg)?))
}

#[cfg(feature = "polars-engine")]
fn native_literal(literal: &DataLiteral) -> native::Expr {
    match literal {
        DataLiteral::String(value) => native::lit(value.as_str()),
        DataLiteral::Number(value) => native::lit(*value),
        DataLiteral::Bool(value) => native::lit(*value),
        DataLiteral::Null => native::lit(native::NULL),
    }
}

#[cfg(feature = "polars-engine")]
fn native_path(path: &Path) -> Result<native::PlRefPath, Diagnostic> {
    native::PlRefPath::try_from_path(path).map_err(|error| {
        Diagnostic::error(
            codes::E1802,
            format!(
                "could not prepare native path `{}`: {error}",
                path.display()
            ),
            Span::zero(),
        )
    })
}

#[cfg(feature = "polars-engine")]
fn native_collect_to_table(plan: NativePlan) -> Result<Table, Diagnostic> {
    let frame = plan
        .plan
        .collect()
        .map_err(native_collect_error(plan.format))?;
    native_frame_to_table(&frame)
}

#[cfg(feature = "polars-engine")]
fn native_frame_to_table(frame: &native::DataFrame) -> Result<Table, Diagnostic> {
    let columns = frame
        .get_column_names()
        .iter()
        .map(|name| name.as_str().to_string())
        .collect::<Vec<_>>();
    let mut rows = Vec::with_capacity(frame.height());
    for row_index in 0..frame.height() {
        let values = frame
            .columns()
            .iter()
            .map(|column| {
                column
                    .get(row_index)
                    .map_err(native_value_error)
                    .and_then(native_value_to_pdl)
            })
            .collect::<Result<Vec<_>, _>>()?;
        rows.push(Row { values });
    }
    Ok(Table { columns, rows })
}

#[cfg(feature = "polars-engine")]
fn native_value_to_pdl(value: native::AnyValue<'_>) -> Result<Value, Diagnostic> {
    Ok(match value {
        native::AnyValue::Null => Value::Null,
        native::AnyValue::Boolean(value) => Value::Bool(value),
        native::AnyValue::String(value) => Value::String(value.to_string()),
        native::AnyValue::StringOwned(value) => Value::String(value.to_string()),
        native::AnyValue::Float32(value) => Value::Number(f64::from(value)),
        native::AnyValue::Float64(value) => Value::Number(value),
        native::AnyValue::Int8(value) => Value::Number(f64::from(value)),
        native::AnyValue::Int16(value) => Value::Number(f64::from(value)),
        native::AnyValue::Int32(value) => Value::Number(f64::from(value)),
        native::AnyValue::Int64(value) => Value::Number(value as f64),
        native::AnyValue::UInt8(value) => Value::Number(f64::from(value)),
        native::AnyValue::UInt16(value) => Value::Number(f64::from(value)),
        native::AnyValue::UInt32(value) => Value::Number(value as f64),
        native::AnyValue::UInt64(value) => Value::Number(value as f64),
        other => {
            return Err(Diagnostic::error(
                codes::E1215,
                format!(
                    "native column value has unsupported data type `{}`",
                    other.dtype()
                ),
                Span::zero(),
            ));
        }
    })
}

#[cfg(feature = "polars-engine")]
fn native_read_error(
    path: &Path,
    format: DataFormat,
) -> impl FnOnce(native::PolarsError) -> Diagnostic + '_ {
    move |error| {
        Diagnostic::error(
            codes::E1804,
            format!(
                "native {} scan failed for `{}`: {error}",
                format.canonical_name(),
                path.display()
            ),
            Span::zero(),
        )
    }
}

#[cfg(feature = "polars-engine")]
fn native_collect_error(format: DataFormat) -> impl FnOnce(native::PolarsError) -> Diagnostic {
    move |error| {
        Diagnostic::error(
            codes::E1804,
            format!(
                "native {} execution failed: {error}",
                format.canonical_name()
            ),
            Span::zero(),
        )
    }
}

#[cfg(feature = "polars-engine")]
fn native_value_error(error: native::PolarsError) -> Diagnostic {
    Diagnostic::error(
        codes::E1215,
        format!("native value conversion failed: {error}"),
        Span::zero(),
    )
}

#[cfg(feature = "polars-engine")]
fn unsupported_native_format(format: DataFormat) -> Diagnostic {
    Diagnostic::error(
        codes::E1215,
        format!(
            "format `{}` is not supported by the native data backend",
            format.canonical_name()
        ),
        Span::zero(),
    )
}

#[cfg(feature = "polars-engine")]
fn unsupported_native_operation(operation: &str) -> Diagnostic {
    Diagnostic::error(
        codes::E1211,
        format!("operation `{operation}` is not supported by the native data backend"),
        Span::zero(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_plan_reports_backend_and_writes_to_writer_sink() {
        let source = DataSource::Bytes {
            logical_path: Path::new("memory.csv"),
            format: DataFormat::Csv,
            bytes: b"region,amount\nWest,30\nEast,10\n",
        };
        let plan = DataPlan::scan(source).expect("row plan");
        assert_eq!(plan.backend(), DataBackend::PortableRows);

        let plan = plan
            .filter(DataExpr::Binary {
                left: Box::new(DataExpr::Column("amount".to_string())),
                op: DataBinaryOp::Gt,
                right: Box::new(DataExpr::Literal(DataLiteral::Number(20.0))),
            })
            .expect("filter");
        let mut bytes = Vec::new();
        let returned = plan
            .write_to_sink(DataSink::Writer {
                format: DataFormat::Csv,
                writer: &mut bytes,
            })
            .expect("write");
        assert_eq!(returned, None);
        assert_eq!(
            String::from_utf8(bytes).expect("utf8"),
            "region,amount\nWest,30\n"
        );
    }
}
