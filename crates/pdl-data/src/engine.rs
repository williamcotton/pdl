use pdl_core::{codes, Diagnostic, Span};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
#[cfg(feature = "polars-engine")]
use std::io::Cursor;
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

#[derive(Clone)]
pub struct DataPlan {
    inner: DataPlanInner,
}

// `NativePlan` embeds a Polars `LazyFrame`, which grew past the clippy
// variant-size threshold when the v0.45 `pivot`/`cross_join` features were
// enabled. `DataPlan` values move through builder-style calls and are never
// stored in collections, so boxing would add indirection without a
// measurable win.
#[allow(clippy::large_enum_variant)]
#[derive(Clone)]
enum DataPlanInner {
    Rows(Table),
    #[cfg(feature = "polars-engine")]
    Native(NativePlan),
}

#[cfg(feature = "polars-engine")]
#[derive(Clone)]
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
    Window {
        function: DataWindowFunction,
        args: Vec<DataExpr>,
        spec: DataWindowSpec,
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
    Coalesce,
    Concat,
    IfElse,
    Lower,
    Upper,
    Trim,
    Contains,
    StartsWith,
    Replace,
    ToString,
    ToNumber,
    ToBoolean,
    Abs,
    Round { digits: u32 },
    Date,
    Datetime,
    Year,
    Month,
    Day,
    DateFloor,
    DateFormat,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DataWindowSpec {
    pub partition_by: Vec<String>,
    pub order_by: Vec<SortSpec>,
    pub frame: DataWindowFrame,
    pub row_index: Option<String>,
    pub presorted: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DataWindowFrame {
    WholePartition,
    UnboundedPrecedingToCurrentRow,
    CurrentRowToUnboundedFollowing,
    PrecedingToCurrentRow { rows: usize },
    CurrentRowToFollowing { rows: usize },
    PrecedingToFollowing { preceding: usize, following: usize },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DataWindowFunction {
    RowNumber,
    Rank,
    DenseRank,
    PercentRank,
    CumeDist,
    Lag,
    Lead,
    FirstValue,
    LastValue,
    Count,
    Sum,
    Mean,
    Min,
    Max,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DataAggItem {
    pub function: String,
    pub args: Vec<DataExpr>,
    pub alias: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DataJoinKind {
    Inner,
    Left,
    Right,
    Full,
    Semi,
    Anti,
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
            DataPlanInner::Native(plan) => {
                let row_index_name = items
                    .iter()
                    .any(|(_, expr)| data_expr_contains_window(expr))
                    .then(|| native_hidden_column_name(&plan.plan, plan.format))
                    .transpose()?;
                let (direct_items, grouped_items) = native_window_partition_mutate_items(items)?;
                let native_plan = if let Some(name) = &row_index_name {
                    plan.plan.with_row_index(name.clone(), None)
                } else {
                    plan.plan
                };
                let mut output_plan = native_plan.clone();
                if !direct_items.is_empty() {
                    let expressions = direct_items
                        .iter()
                        .map(|(column, expr)| {
                            let expr = row_index_name
                                .as_deref()
                                .map(|name| data_expr_with_window_row_index(expr, name))
                                .unwrap_or_else(|| expr.clone());
                            Ok(native_expr(&expr)?.alias(column))
                        })
                        .collect::<Result<Vec<_>, Diagnostic>>()?;
                    output_plan = output_plan.with_columns(expressions);
                }
                for (group, group_items) in grouped_items {
                    let Some(row_index) = &row_index_name else {
                        return Err(unsupported_native_operation("window row index"));
                    };
                    let mut reserved_names = native_plan
                        .clone()
                        .collect_schema()
                        .map_err(native_collect_error(plan.format))?
                        .iter_names()
                        .map(|name| name.to_string())
                        .collect::<Vec<_>>();
                    reserved_names.push(row_index.clone());
                    let temp_items = group_items
                        .iter()
                        .map(|(column, expr)| {
                            let temp = native_hidden_column_name_from_names(
                                &reserved_names,
                                "__pdl_window_value",
                            );
                            reserved_names.push(temp.clone());
                            (column.clone(), temp, expr.clone())
                        })
                        .collect::<Vec<_>>();
                    let sorted_plan = native_plan.clone().sort(
                        group
                            .sort_specs()
                            .iter()
                            .map(|spec| spec.column.clone())
                            .collect::<Vec<_>>(),
                        native_sort_multiple_options(&group.sort_specs()),
                    );
                    let expressions = temp_items
                        .iter()
                        .map(|(_, temp, expr)| {
                            let expr = data_expr_with_window_row_index(expr, row_index);
                            let expr = data_expr_with_presorted_multi_key_windows(&expr);
                            Ok(native_expr(&expr)?.alias(temp))
                        })
                        .collect::<Result<Vec<_>, Diagnostic>>()?;
                    let right = sorted_plan.with_columns(expressions).select(
                        std::iter::once(native::col(row_index))
                            .chain(temp_items.iter().map(|(_, temp, _)| native::col(temp)))
                            .collect::<Vec<_>>(),
                    );
                    output_plan = output_plan
                        .join_builder()
                        .with(right)
                        .left_on([native::col(row_index)])
                        .right_on([native::col(row_index)])
                        .how(native::JoinType::Left)
                        .suffix("_right")
                        .coalesce(native::JoinCoalesce::CoalesceColumns)
                        .join_nulls(false)
                        .maintain_order(native::MaintainOrderJoin::Left)
                        .finish()
                        .with_columns(
                            temp_items
                                .iter()
                                .map(|(column, temp, _)| native::col(temp).alias(column))
                                .collect::<Vec<_>>(),
                        )
                        .drop(native::cols(
                            temp_items
                                .iter()
                                .map(|(_, temp, _)| temp.clone())
                                .collect::<Vec<_>>(),
                        ));
                }
                let native_plan = if let Some(name) = &row_index_name {
                    output_plan.drop(native::cols([name.as_str()]))
                } else {
                    output_plan
                };
                Ok(Self {
                    inner: DataPlanInner::Native(NativePlan {
                        format: plan.format,
                        plan: native_plan,
                    }),
                })
            }
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
                Ok(Self {
                    inner: DataPlanInner::Native(NativePlan {
                        format: plan.format,
                        plan: plan.plan.sort(columns, native_sort_multiple_options(specs)),
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

    pub fn join(
        self,
        right: DataPlan,
        left_key: &str,
        right_key: &str,
        kind: DataJoinKind,
    ) -> Result<Self, Diagnostic> {
        self.join_on_keys(right, &[(left_key, right_key)], kind)
    }

    pub fn join_on_keys(
        self,
        right: DataPlan,
        keys: &[(&str, &str)],
        kind: DataJoinKind,
    ) -> Result<Self, Diagnostic> {
        #[cfg(not(feature = "polars-engine"))]
        let _ = kind;
        if keys.is_empty() {
            return Err(unsupported_native_operation("join key list"));
        }
        match (self.inner, right.inner) {
            #[cfg(feature = "polars-engine")]
            (DataPlanInner::Native(left), DataPlanInner::Native(right)) => {
                if kind == DataJoinKind::Full {
                    return native_full_join(left, right, keys);
                }
                let output_selection = native_join_output_selection(&left, &right, keys, kind)?;
                let how = match kind {
                    DataJoinKind::Inner => native::JoinType::Inner,
                    DataJoinKind::Left => native::JoinType::Left,
                    DataJoinKind::Right => native::JoinType::Right,
                    DataJoinKind::Full => native::JoinType::Full,
                    DataJoinKind::Semi => native::JoinType::Semi,
                    DataJoinKind::Anti => native::JoinType::Anti,
                };
                let maintain_order = match kind {
                    DataJoinKind::Inner
                    | DataJoinKind::Left
                    | DataJoinKind::Semi
                    | DataJoinKind::Anti => native::MaintainOrderJoin::Left,
                    DataJoinKind::Right => native::MaintainOrderJoin::Right,
                    DataJoinKind::Full => native::MaintainOrderJoin::LeftRight,
                };
                let joined = left
                    .plan
                    .join_builder()
                    .with(right.plan)
                    .left_on(
                        keys.iter()
                            .map(|(left_key, _)| native::col(*left_key))
                            .collect::<Vec<_>>(),
                    )
                    .right_on(
                        keys.iter()
                            .map(|(_, right_key)| native::col(*right_key))
                            .collect::<Vec<_>>(),
                    )
                    .how(how)
                    .suffix("_right")
                    .coalesce(native::JoinCoalesce::CoalesceColumns)
                    .join_nulls(false)
                    .maintain_order(maintain_order)
                    .finish();
                let joined = match output_selection {
                    Some(selection) => joined.select(selection),
                    None => joined,
                };
                Ok(Self {
                    inner: DataPlanInner::Native(NativePlan {
                        format: left.format,
                        plan: joined,
                    }),
                })
            }
            _ => Err(unsupported_native_operation("native join")),
        }
    }

    pub fn union(
        self,
        right: DataPlan,
        _by_name: bool,
        _distinct: bool,
    ) -> Result<Self, Diagnostic> {
        match (self.inner, right.inner) {
            #[cfg(feature = "polars-engine")]
            (DataPlanInner::Native(left), DataPlanInner::Native(right)) => {
                let right_plan = if _by_name {
                    right.plan.select(
                        left.plan
                            .clone()
                            .collect_schema()
                            .map_err(native_collect_error(left.format))?
                            .iter_names()
                            .map(|name| native::col(name.as_str()))
                            .collect::<Vec<_>>(),
                    )
                } else {
                    let left_names = left
                        .plan
                        .clone()
                        .collect_schema()
                        .map_err(native_collect_error(left.format))?
                        .iter_names()
                        .map(|name| name.to_string())
                        .collect::<Vec<_>>();
                    let right_names = right
                        .plan
                        .clone()
                        .collect_schema()
                        .map_err(native_collect_error(right.format))?
                        .iter_names()
                        .map(|name| name.to_string())
                        .collect::<Vec<_>>();
                    right.plan.select(
                        right_names
                            .iter()
                            .zip(left_names)
                            .map(|(right, left)| native::col(right).alias(left))
                            .collect::<Vec<_>>(),
                    )
                };
                let union = native::concat(
                    [left.plan, right_plan],
                    native::UnionArgs {
                        parallel: false,
                        strict: true,
                        maintain_order: true,
                        ..Default::default()
                    },
                )
                .map_err(native_collect_error(left.format))?;
                let plan = if _distinct {
                    union.unique_stable_generic(None, native::UniqueKeepStrategy::First)
                } else {
                    union
                };
                Ok(Self {
                    inner: DataPlanInner::Native(NativePlan {
                        format: left.format,
                        plan,
                    }),
                })
            }
            _ => Err(unsupported_native_operation("native union")),
        }
    }

    /// Native lowering of the `pivot_longer` stage (v0.45). Reshapes the
    /// selected source columns into name/value rows with output order and
    /// column order identical to the row runtime: for each input row, one
    /// output row per selected column in stage order, kept columns first.
    ///
    /// Polars stores one dtype per column while the row runtime keeps
    /// per-cell value types, so value-column sets that span more than one
    /// value class (numeric vs string vs boolean) cannot reproduce row
    /// runtime bytes on a typed engine; they report the unsupported native
    /// operation and stay on the row engine.
    pub fn pivot_longer(
        self,
        columns: &[String],
        names_to: &str,
        values_to: &str,
    ) -> Result<Self, Diagnostic> {
        if columns.is_empty() {
            return Err(unsupported_native_operation("pivot_longer column list"));
        }
        #[cfg(not(feature = "polars-engine"))]
        let _ = (names_to, values_to);
        match self.inner {
            #[cfg(feature = "polars-engine")]
            DataPlanInner::Native(plan) => native_pivot_longer(plan, columns, names_to, values_to),
            _ => Err(unsupported_native_operation("native pivot_longer")),
        }
    }

    /// Native lowering of the `complete` stage (v0.45). Builds the Cartesian
    /// product of first-appearance key domains, preserves existing rows at
    /// their tuple positions, inserts missing tuples with null non-key
    /// columns, and applies fill expressions to inserted rows only — all in
    /// the row runtime's nested key-expansion order.
    ///
    /// Fill expressions that change a column's value class (e.g. a string
    /// fill over a numeric column) cannot reproduce the row runtime's
    /// per-cell value types on a typed engine and stay on the row engine.
    pub fn complete(
        self,
        keys: &[String],
        fills: &[(String, DataExpr)],
    ) -> Result<Self, Diagnostic> {
        if keys.is_empty() {
            return Err(unsupported_native_operation("complete key list"));
        }
        #[cfg(not(feature = "polars-engine"))]
        let _ = fills;
        match self.inner {
            #[cfg(feature = "polars-engine")]
            DataPlanInner::Native(plan) => native_complete(plan, keys, fills),
            _ => Err(unsupported_native_operation("native complete")),
        }
    }

    pub fn collect(self) -> Result<Table, Diagnostic> {
        match self.inner {
            DataPlanInner::Rows(table) => Ok(table),
            #[cfg(feature = "polars-engine")]
            DataPlanInner::Native(plan) => native_collect_to_table(plan),
        }
    }

    pub fn cache(self) -> Self {
        match self.inner {
            DataPlanInner::Rows(table) => Self::from_table(table),
            #[cfg(feature = "polars-engine")]
            DataPlanInner::Native(plan) => Self {
                inner: DataPlanInner::Native(NativePlan {
                    format: plan.format,
                    plan: plan.plan.cache(),
                }),
            },
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

#[cfg(feature = "polars-engine")]
fn native_full_join(
    left: NativePlan,
    right: NativePlan,
    keys: &[(&str, &str)],
) -> Result<DataPlan, Diagnostic> {
    let left_schema = left
        .plan
        .clone()
        .collect_schema()
        .map_err(native_collect_error(left.format))?;
    let right_schema = right
        .plan
        .clone()
        .collect_schema()
        .map_err(native_collect_error(right.format))?;
    let left_names = left_schema
        .iter_names()
        .map(|name| name.to_string())
        .collect::<Vec<_>>();
    let right_names = right_schema
        .iter_names()
        .map(|name| name.to_string())
        .collect::<Vec<_>>();
    let right_keys = keys
        .iter()
        .map(|(_, right_key)| *right_key)
        .collect::<BTreeSet<_>>();
    let right_non_keys = native_join_right_outputs(&left_names, &right_names, &right_keys)?;

    let left_rows = left
        .plan
        .clone()
        .join_builder()
        .with(right.plan.clone())
        .left_on(
            keys.iter()
                .map(|(left_key, _)| native::col(*left_key))
                .collect::<Vec<_>>(),
        )
        .right_on(
            keys.iter()
                .map(|(_, right_key)| native::col(*right_key))
                .collect::<Vec<_>>(),
        )
        .how(native::JoinType::Left)
        .suffix("_right")
        .coalesce(native::JoinCoalesce::CoalesceColumns)
        .join_nulls(false)
        .maintain_order(native::MaintainOrderJoin::Left)
        .finish();

    let mut right_only_selection = left_names
        .iter()
        .map(|column| {
            for (left_key, right_key) in keys {
                if column == left_key {
                    return native::col(*right_key).alias(column);
                }
            }
            native::lit(native::NULL).alias(column)
        })
        .collect::<Vec<_>>();
    right_only_selection.extend(
        right_non_keys
            .iter()
            .map(|(source, output)| native::col(source).alias(output)),
    );
    let output_names = left_names
        .iter()
        .cloned()
        .chain(right_non_keys.iter().map(|(_, output)| output.clone()))
        .collect::<Vec<_>>();
    let sort_key = native_hidden_column_name_from_names(&output_names, "__pdl_full_join_right_key");
    let sort_key_expr = if keys.len() == 1 {
        native::col(keys[0].0).cast(native::DataType::String)
    } else {
        native::concat_str(
            keys.iter()
                .map(|(left_key, _)| native::col(*left_key).cast(native::DataType::String))
                .collect::<Vec<_>>(),
            "|",
            false,
        )
    };
    let right_only = right
        .plan
        .join_builder()
        .with(left.plan)
        .left_on(
            keys.iter()
                .map(|(_, right_key)| native::col(*right_key))
                .collect::<Vec<_>>(),
        )
        .right_on(
            keys.iter()
                .map(|(left_key, _)| native::col(*left_key))
                .collect::<Vec<_>>(),
        )
        .how(native::JoinType::Anti)
        .join_nulls(false)
        .maintain_order(native::MaintainOrderJoin::Left)
        .finish()
        .select(right_only_selection)
        .with_column(sort_key_expr.alias(&sort_key))
        .sort(
            [&sort_key],
            native::SortMultipleOptions {
                descending: vec![false],
                nulls_last: vec![false],
                maintain_order: true,
                ..Default::default()
            },
        )
        .drop(native::cols([sort_key.as_str()]));

    let plan = native::concat(
        [left_rows, right_only],
        native::UnionArgs {
            parallel: false,
            strict: false,
            to_supertypes: true,
            maintain_order: true,
            ..Default::default()
        },
    )
    .map_err(native_collect_error(left.format))?;

    Ok(DataPlan {
        inner: DataPlanInner::Native(NativePlan {
            format: left.format,
            plan,
        }),
    })
}

/// Value classes the native engine can hold in one column without changing
/// row-runtime rendering. Mirrors the row runtime's `Value` classes.
#[cfg(feature = "polars-engine")]
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum NativeValueClass {
    Bool,
    Number,
    String,
}

/// Maps a native dtype to its row-runtime value class. `None` marks an
/// all-null column, which is compatible with every class. Dtypes outside the
/// row value model report the unsupported native operation so automatic mode
/// falls back to rows.
#[cfg(feature = "polars-engine")]
fn native_dtype_class(dtype: &native::DataType) -> Result<Option<NativeValueClass>, Diagnostic> {
    Ok(match dtype {
        native::DataType::Null => None,
        native::DataType::Boolean => Some(NativeValueClass::Bool),
        native::DataType::Int8
        | native::DataType::Int16
        | native::DataType::Int32
        | native::DataType::Int64
        | native::DataType::UInt8
        | native::DataType::UInt16
        | native::DataType::UInt32
        | native::DataType::UInt64
        | native::DataType::Float32
        | native::DataType::Float64 => Some(NativeValueClass::Number),
        native::DataType::String => Some(NativeValueClass::String),
        _ => return Err(unsupported_native_operation("native value class")),
    })
}

#[cfg(feature = "polars-engine")]
fn native_pivot_longer(
    plan: NativePlan,
    columns: &[String],
    names_to: &str,
    values_to: &str,
) -> Result<DataPlan, Diagnostic> {
    let schema = plan
        .plan
        .clone()
        .collect_schema()
        .map_err(native_collect_error(plan.format))?;

    let mut value_classes = BTreeSet::new();
    for column in columns {
        let Some(dtype) = schema.get(column.as_str()) else {
            return Err(Diagnostic::error(
                codes::E1005,
                format!("unknown column `{column}`"),
                Span::zero(),
            ));
        };
        if let Some(class) = native_dtype_class(dtype)? {
            value_classes.insert(class);
        }
    }
    if value_classes.len() > 1 {
        // The row runtime keeps each cell's value type through the reshape;
        // a typed values column would re-render numbers or booleans that
        // share the column with strings. Row-only by design (see the
        // coverage matrix `pivot_longer` row).
        return Err(unsupported_native_operation(
            "mixed-class pivot_longer value columns",
        ));
    }

    let selected: BTreeSet<&str> = columns.iter().map(String::as_str).collect();
    let kept = schema
        .iter_names()
        .map(|name| name.to_string())
        .filter(|name| !selected.contains(name.as_str()))
        .collect::<Vec<_>>();
    if kept
        .iter()
        .any(|column| column == names_to || column == values_to)
        || names_to == values_to
    {
        // The row runtime rejects these collisions with `E1207`; report the
        // unsupported operation so automatic mode falls back to the row
        // engine and surfaces the row diagnostic.
        return Err(unsupported_native_operation(
            "pivot_longer output column collision",
        ));
    }

    let mut output_names = schema
        .iter_names()
        .map(|name| name.to_string())
        .collect::<Vec<_>>();
    output_names.push(names_to.to_string());
    output_names.push(values_to.to_string());
    let row_index = native_hidden_column_name_from_names(&output_names, "__pdl_pivot_row_index");

    let mut index_columns = kept;
    index_columns.push(row_index.clone());

    // Polars unpivot emits all rows for the first value column, then all
    // rows for the second, and so on; the stable sort on the hidden input
    // row index restores the row runtime's interleaved order (input row
    // major, stage column order within each input row).
    let unpivoted = plan
        .plan
        .with_row_index(row_index.clone(), None)
        .unpivot(native::UnpivotArgsDSL {
            on: Some(native::cols(columns.iter().cloned())),
            index: native::cols(index_columns),
            variable_name: Some(names_to.into()),
            value_name: Some(values_to.into()),
        })
        .sort(
            [row_index.as_str()],
            native::SortMultipleOptions {
                descending: vec![false],
                nulls_last: vec![false],
                maintain_order: true,
                ..Default::default()
            },
        )
        .drop(native::cols([row_index.as_str()]));

    Ok(DataPlan {
        inner: DataPlanInner::Native(NativePlan {
            format: plan.format,
            plan: unpivoted,
        }),
    })
}

#[cfg(feature = "polars-engine")]
fn native_complete(
    plan: NativePlan,
    keys: &[String],
    fills: &[(String, DataExpr)],
) -> Result<DataPlan, Diagnostic> {
    let format = plan.format;
    // `complete` needs the whole frame for key domains and the duplicate
    // tuple check, so materialize the input once and continue lazily from
    // the in-memory frame.
    let frame = plan.plan.collect().map_err(native_collect_error(format))?;
    let column_names = native_frame_column_names(&frame);
    let input_schema = frame.schema().clone();

    for key in keys {
        if !column_names.iter().any(|column| column == key) {
            return Err(Diagnostic::error(
                codes::E1005,
                format!("unknown column `{key}`"),
                Span::zero(),
            ));
        }
    }
    for (column, expr) in fills {
        if !column_names.iter().any(|existing| existing == column) {
            return Err(Diagnostic::error(
                codes::E1005,
                format!("unknown column `{column}`"),
                Span::zero(),
            ));
        }
        if keys.iter().any(|key| key == column) {
            // Row runtime rejects key fills with `E1207`; fall back so the
            // row diagnostic surfaces in automatic mode.
            return Err(unsupported_native_operation(
                "complete fill over key column",
            ));
        }
        if data_expr_contains_window(expr) {
            return Err(unsupported_native_operation(
                "complete fill window expression",
            ));
        }
    }

    let key_exprs = keys.iter().map(native::col).collect::<Vec<_>>();
    let distinct_height = frame
        .clone()
        .lazy()
        .select(key_exprs.clone())
        .unique_stable_generic(None, native::UniqueKeepStrategy::First)
        .collect()
        .map_err(native_collect_error(format))?
        .height();
    if distinct_height != frame.height() {
        return Err(Diagnostic::error(
            codes::E1208,
            "complete found duplicate input rows for the same key tuple",
            Span::zero(),
        ));
    }

    // First-appearance domains per key, cross-joined in key order. The cross
    // join repeats each left row across the full right domain, which is
    // exactly the row runtime's nested key-expansion order.
    let mut tuples: Option<native::LazyFrame> = None;
    for key in keys {
        let domain = frame
            .clone()
            .lazy()
            .select([native::col(key)])
            .unique_stable_generic(None, native::UniqueKeepStrategy::First);
        tuples = Some(match tuples {
            None => domain,
            Some(tuples) => tuples
                .join_builder()
                .with(domain)
                .how(native::JoinType::Cross)
                .maintain_order(native::MaintainOrderJoin::LeftRight)
                .finish(),
        });
    }
    let tuples = tuples.expect("complete requires at least one key");

    // Null keys are observed values in the row runtime, so the join must
    // treat null tuple components as matching the original rows.
    let marker = native_hidden_column_name_from_names(&column_names, "__pdl_complete_marker");
    let marked = frame
        .clone()
        .lazy()
        .with_column(native::lit(true).alias(marker.as_str()));
    let joined = tuples
        .join_builder()
        .with(marked)
        .left_on(key_exprs.clone())
        .right_on(key_exprs)
        .how(native::JoinType::Left)
        .suffix("_right")
        .coalesce(native::JoinCoalesce::CoalesceColumns)
        .join_nulls(true)
        .maintain_order(native::MaintainOrderJoin::Left)
        .finish();

    // Existing rows keep their values; inserted rows carry null non-key
    // columns, with fill expressions evaluated against that base row (all
    // fills see the pre-fill frame, matching row runtime semantics).
    let selection = column_names
        .iter()
        .map(|column| {
            for (fill_column, fill_expr) in fills {
                if column == fill_column {
                    return Ok(native::when(native::col(marker.as_str()).is_null())
                        .then(native_expr(fill_expr)?)
                        .otherwise(native::col(column))
                        .alias(column));
                }
            }
            Ok(native::col(column))
        })
        .collect::<Result<Vec<_>, Diagnostic>>()?;
    let completed = joined.select(selection);

    let completed_schema = completed
        .clone()
        .collect_schema()
        .map_err(native_collect_error(format))?;
    for (fill_column, _) in fills {
        let input_class = input_schema
            .get(fill_column.as_str())
            .map(native_dtype_class)
            .transpose()?
            .flatten();
        let output_class = completed_schema
            .get(fill_column.as_str())
            .map(native_dtype_class)
            .transpose()?
            .flatten();
        if let (Some(input_class), Some(output_class)) = (input_class, output_class) {
            if input_class != output_class {
                // A class-changing fill (string fill over a numeric column,
                // for example) would re-render the column's existing values.
                // Row-only by design (see the coverage matrix `complete`
                // row).
                return Err(unsupported_native_operation(
                    "class-changing complete fill expression",
                ));
            }
        }
    }

    Ok(DataPlan {
        inner: DataPlanInner::Native(NativePlan {
            format,
            plan: completed,
        }),
    })
}

#[cfg(feature = "polars-engine")]
fn native_join_output_selection(
    left: &NativePlan,
    right: &NativePlan,
    keys: &[(&str, &str)],
    kind: DataJoinKind,
) -> Result<Option<Vec<native::Expr>>, Diagnostic> {
    if matches!(kind, DataJoinKind::Semi | DataJoinKind::Anti) {
        return Ok(None);
    }
    let left_schema = left
        .plan
        .clone()
        .collect_schema()
        .map_err(native_collect_error(left.format))?;
    let right_schema = right
        .plan
        .clone()
        .collect_schema()
        .map_err(native_collect_error(right.format))?;
    let left_names = left_schema
        .iter_names()
        .map(|name| name.to_string())
        .collect::<Vec<_>>();
    let right_names = right_schema
        .iter_names()
        .map(|name| name.to_string())
        .collect::<Vec<_>>();
    let right_keys = keys
        .iter()
        .map(|(_, right_key)| *right_key)
        .collect::<BTreeSet<_>>();
    let right_outputs = native_join_right_outputs(&left_names, &right_names, &right_keys)?;
    let mut selection = left_names
        .iter()
        .map(|name| {
            for (left_key, right_key) in keys {
                if name == left_key && left_key != right_key {
                    return match kind {
                        DataJoinKind::Right => native::col(*right_key).alias(*left_key),
                        DataJoinKind::Full => {
                            native::coalesce(&[native::col(*left_key), native::col(*right_key)])
                                .alias(*left_key)
                        }
                        DataJoinKind::Inner | DataJoinKind::Left => native::col(name),
                        DataJoinKind::Semi | DataJoinKind::Anti => {
                            unreachable!("semi/anti join has no output selection")
                        }
                    };
                }
            }
            native::col(name)
        })
        .collect::<Vec<_>>();
    selection.extend(right_outputs.iter().map(|(_, output)| native::col(output)));
    Ok(Some(selection))
}

#[cfg(feature = "polars-engine")]
fn native_join_right_outputs(
    left_names: &[String],
    right_names: &[String],
    right_keys: &BTreeSet<&str>,
) -> Result<Vec<(String, String)>, Diagnostic> {
    let mut output_names = left_names.to_vec();
    let mut outputs = Vec::new();
    for column in right_names {
        if right_keys.contains(column.as_str()) {
            continue;
        }
        let mut output = column.clone();
        if output_names.iter().any(|existing| existing == &output) {
            output.push_str("_right");
            if output_names.iter().any(|existing| existing == &output) {
                return Err(unsupported_native_operation("join output column collision"));
            }
        }
        output_names.push(output.clone());
        outputs.push((column.clone(), output));
    }
    Ok(outputs)
}

#[cfg(feature = "polars-engine")]
fn native_hidden_column_name(
    plan: &native::LazyFrame,
    format: DataFormat,
) -> Result<String, Diagnostic> {
    let schema = plan
        .clone()
        .collect_schema()
        .map_err(native_collect_error(format))?;
    let names = schema
        .iter_names()
        .map(|name| name.to_string())
        .collect::<Vec<_>>();
    Ok(native_hidden_column_name_from_names(
        &names,
        "__pdl_window_row_index",
    ))
}

#[cfg(feature = "polars-engine")]
fn native_hidden_column_name_from_names(names: &[String], base: &str) -> String {
    let mut candidate = base.to_string();
    let mut suffix = 1usize;
    while names.iter().any(|name| name == &candidate) {
        candidate = format!("{base}_{suffix}");
        suffix += 1;
    }
    candidate
}

#[cfg(feature = "polars-engine")]
fn data_expr_contains_window(expr: &DataExpr) -> bool {
    match expr {
        DataExpr::Window { .. } => true,
        DataExpr::Unary { expr, .. } => data_expr_contains_window(expr),
        DataExpr::Binary { left, right, .. } => {
            data_expr_contains_window(left) || data_expr_contains_window(right)
        }
        DataExpr::Call { args, .. } => args.iter().any(data_expr_contains_window),
        DataExpr::Column(_) | DataExpr::Literal(_) => false,
    }
}

#[cfg(feature = "polars-engine")]
#[derive(Clone, Debug, PartialEq)]
struct NativeWindowSortGroup {
    partition_by: Vec<String>,
    order_by: Vec<SortSpec>,
}

#[cfg(feature = "polars-engine")]
impl NativeWindowSortGroup {
    fn sort_specs(&self) -> Vec<SortSpec> {
        let mut specs = self
            .partition_by
            .iter()
            .map(|column| SortSpec {
                column: column.clone(),
                direction: SortDirection::Asc,
                nulls: NullsOrder::Last,
            })
            .collect::<Vec<_>>();
        specs.extend(self.order_by.clone());
        specs
    }
}

#[cfg(feature = "polars-engine")]
type NativeMutateItem = (String, DataExpr);

#[cfg(feature = "polars-engine")]
type NativeGroupedMutateItems = Vec<(NativeWindowSortGroup, Vec<NativeMutateItem>)>;

#[cfg(feature = "polars-engine")]
type NativePartitionedMutateItems = (Vec<NativeMutateItem>, NativeGroupedMutateItems);

#[cfg(feature = "polars-engine")]
fn native_window_partition_mutate_items(
    items: &[(String, DataExpr)],
) -> Result<NativePartitionedMutateItems, Diagnostic> {
    let mut direct = Vec::new();
    let mut grouped = NativeGroupedMutateItems::new();
    for (column, expr) in items {
        let mut group = None;
        data_expr_collect_multi_key_window_sort(expr, &mut group)?;
        let item = (column.clone(), expr.clone());
        if let Some(group) = group {
            if let Some((_, items)) = grouped.iter_mut().find(|(current, _)| current == &group) {
                items.push(item);
            } else {
                grouped.push((group, vec![item]));
            }
        } else {
            direct.push(item);
        }
    }
    Ok((direct, grouped))
}

#[cfg(feature = "polars-engine")]
fn data_expr_collect_multi_key_window_sort(
    expr: &DataExpr,
    group: &mut Option<NativeWindowSortGroup>,
) -> Result<(), Diagnostic> {
    match expr {
        DataExpr::Unary { expr, .. } => data_expr_collect_multi_key_window_sort(expr, group),
        DataExpr::Binary { left, right, .. } => {
            data_expr_collect_multi_key_window_sort(left, group)?;
            data_expr_collect_multi_key_window_sort(right, group)
        }
        DataExpr::Call { args, .. } => {
            for arg in args {
                data_expr_collect_multi_key_window_sort(arg, group)?;
            }
            Ok(())
        }
        DataExpr::Window { args, spec, .. } => {
            if spec.order_by.len() > 1 {
                let next = NativeWindowSortGroup {
                    partition_by: spec.partition_by.clone(),
                    order_by: spec.order_by.clone(),
                };
                match group {
                    Some(current) if current != &next => {
                        return Err(unsupported_native_operation(
                            "multiple multi-key window order groups",
                        ));
                    }
                    Some(_) => {}
                    None => *group = Some(next),
                }
            }
            for arg in args {
                data_expr_collect_multi_key_window_sort(arg, group)?;
            }
            Ok(())
        }
        DataExpr::Column(_) | DataExpr::Literal(_) => Ok(()),
    }
}

#[cfg(feature = "polars-engine")]
fn data_expr_with_window_row_index(expr: &DataExpr, row_index: &str) -> DataExpr {
    match expr {
        DataExpr::Unary { op, expr } => DataExpr::Unary {
            op: *op,
            expr: Box::new(data_expr_with_window_row_index(expr, row_index)),
        },
        DataExpr::Binary { left, op, right } => DataExpr::Binary {
            left: Box::new(data_expr_with_window_row_index(left, row_index)),
            op: *op,
            right: Box::new(data_expr_with_window_row_index(right, row_index)),
        },
        DataExpr::Call { function, args } => DataExpr::Call {
            function: *function,
            args: args
                .iter()
                .map(|arg| data_expr_with_window_row_index(arg, row_index))
                .collect(),
        },
        DataExpr::Window {
            function,
            args,
            spec,
        } => {
            let mut spec = spec.clone();
            spec.row_index = Some(row_index.to_string());
            DataExpr::Window {
                function: *function,
                args: args
                    .iter()
                    .map(|arg| data_expr_with_window_row_index(arg, row_index))
                    .collect(),
                spec,
            }
        }
        DataExpr::Column(_) | DataExpr::Literal(_) => expr.clone(),
    }
}

#[cfg(feature = "polars-engine")]
fn data_expr_with_presorted_multi_key_windows(expr: &DataExpr) -> DataExpr {
    match expr {
        DataExpr::Unary { op, expr } => DataExpr::Unary {
            op: *op,
            expr: Box::new(data_expr_with_presorted_multi_key_windows(expr)),
        },
        DataExpr::Binary { left, op, right } => DataExpr::Binary {
            left: Box::new(data_expr_with_presorted_multi_key_windows(left)),
            op: *op,
            right: Box::new(data_expr_with_presorted_multi_key_windows(right)),
        },
        DataExpr::Call { function, args } => DataExpr::Call {
            function: *function,
            args: args
                .iter()
                .map(data_expr_with_presorted_multi_key_windows)
                .collect(),
        },
        DataExpr::Window {
            function,
            args,
            spec,
        } => {
            let mut spec = spec.clone();
            if spec.order_by.len() > 1 {
                spec.presorted = true;
            }
            DataExpr::Window {
                function: *function,
                args: args
                    .iter()
                    .map(data_expr_with_presorted_multi_key_windows)
                    .collect(),
                spec,
            }
        }
        DataExpr::Column(_) | DataExpr::Literal(_) => expr.clone(),
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
    let (format, plan) = match source {
        DataSource::Path { path, format } => {
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
                DataFormat::ArrowFile => native::LazyFrame::scan_ipc(
                    native_path(path)?,
                    Default::default(),
                    native::UnifiedScanArgs::default(),
                )
                .map_err(native_read_error(path, format))?,
                DataFormat::JsonLines => {
                    return Err(unsupported_native_format(format));
                }
            };
            (format, plan)
        }
        DataSource::Bytes {
            logical_path,
            format,
            bytes,
        } => {
            let plan = match format {
                // The byte-backed CSV adapter (v0.46) wraps the in-memory
                // stream in the same lazy CSV scan the path-backed source
                // uses, so schema inference and read semantics cannot drift
                // between path and byte inputs.
                DataFormat::Csv if bytes.is_empty() => {
                    // The row reader yields a zero-column table for empty
                    // CSV input where the native reader rejects it; match
                    // the row engine.
                    native::DataFrame::default().lazy()
                }
                DataFormat::Csv => {
                    let plan = native::LazyCsvReader::new_with_sources(byte_scan_sources(bytes))
                        .with_has_header(true)
                        .finish()
                        .map_err(native_read_error(logical_path, format))?;
                    align_native_csv_header(plan, logical_path, bytes)?
                }
                // Parquet bytes are already buffered to completion, so the
                // footer-driven eager read mirrors what the row engine does
                // with the same stream.
                DataFormat::Parquet => native::ParquetReader::new(Cursor::new(bytes))
                    .finish()
                    .map_err(native_read_error(logical_path, format))?
                    .lazy(),
                DataFormat::ArrowStream => native::IpcStreamReader::new(Cursor::new(bytes))
                    .finish()
                    .map_err(native_read_error(logical_path, format))?
                    .lazy(),
                DataFormat::ArrowFile => native::IpcReader::new(Cursor::new(bytes))
                    .finish()
                    .map_err(native_read_error(logical_path, format))?
                    .lazy(),
                DataFormat::JsonLines => {
                    return Err(unsupported_native_format(format));
                }
            };
            (format, plan)
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
            writer.flush().map_err(output_write_error)?;
            Ok(None)
        }
    }
}

#[cfg(feature = "polars-engine")]
fn write_native_to_writer(
    plan: NativePlan,
    format: DataFormat,
    writer: &mut dyn Write,
) -> Result<(), Diagnostic> {
    let mut frame = plan
        .plan
        .collect()
        .map_err(native_collect_error(plan.format))?;
    match format {
        DataFormat::Parquet => native::ParquetWriter::new(writer)
            .finish(&mut frame)
            .map(|_| ())
            .map_err(native_write_error("Parquet")),
        DataFormat::ArrowFile => native::IpcWriter::new(writer)
            .finish(&mut frame)
            .map_err(native_write_error("Arrow IPC file")),
        DataFormat::ArrowStream => native::IpcStreamWriter::new(writer)
            .finish(&mut frame)
            .map_err(native_write_error("Arrow IPC stream")),
        // The row writers are the byte spec for the text formats. Native
        // emission streams dataframe rows through the row writers' cell
        // encoders so the bytes stay identical without building a row table.
        DataFormat::Csv => write_native_csv(&frame, writer),
        DataFormat::JsonLines => write_native_json_lines(&frame, writer),
    }
}

#[cfg(feature = "polars-engine")]
fn write_native_csv(frame: &native::DataFrame, writer: &mut dyn Write) -> Result<(), Diagnostic> {
    let columns = native_frame_column_names(frame);
    let mut csv_writer = crate::csv::CsvStreamWriter::new(writer, &columns)?;
    let mut values = Vec::with_capacity(columns.len());
    for row_index in 0..frame.height() {
        native_frame_row_values(frame, row_index, &mut values)?;
        csv_writer.write_row(&values)?;
    }
    csv_writer.finish()
}

#[cfg(feature = "polars-engine")]
fn write_native_json_lines(
    frame: &native::DataFrame,
    writer: &mut dyn Write,
) -> Result<(), Diagnostic> {
    let columns = native_frame_column_names(frame);
    let mut values = Vec::with_capacity(columns.len());
    for row_index in 0..frame.height() {
        native_frame_row_values(frame, row_index, &mut values)?;
        crate::jsonl::write_json_lines_record(writer, &columns, &values)?;
    }
    Ok(())
}

#[cfg(feature = "polars-engine")]
fn native_frame_column_names(frame: &native::DataFrame) -> Vec<String> {
    frame
        .get_column_names()
        .iter()
        .map(|name| name.as_str().to_string())
        .collect()
}

#[cfg(feature = "polars-engine")]
fn native_frame_row_values(
    frame: &native::DataFrame,
    row_index: usize,
    values: &mut Vec<Value>,
) -> Result<(), Diagnostic> {
    values.clear();
    for column in frame.columns() {
        values.push(
            column
                .get(row_index)
                .map_err(native_value_error)
                .and_then(native_value_to_pdl)?,
        );
    }
    Ok(())
}

fn output_write_error(error: std::io::Error) -> Diagnostic {
    Diagnostic::error(
        codes::E1704,
        format!("output write failed: {error}"),
        Span::zero(),
    )
}

#[cfg(feature = "polars-engine")]
fn native_write_error(label: &'static str) -> impl FnOnce(native::PolarsError) -> Diagnostic {
    move |error| {
        Diagnostic::error(
            codes::E1704,
            format!("native {label} write failed: {error}"),
            Span::zero(),
        )
    }
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
        DataExpr::Window { .. } => Err(unsupported_native_operation("row data window expression")),
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
    match function {
        DataScalarFunction::Coalesce => {
            for arg in args {
                let value = eval_row_expr(arg, table, row)?;
                if !matches!(value, Value::Null) {
                    return Ok(value);
                }
            }
            Ok(Value::Null)
        }
        DataScalarFunction::Concat => {
            let mut text = String::new();
            for arg in args {
                let value = eval_row_expr(arg, table, row)?;
                if !matches!(value, Value::Null) {
                    text.push_str(&value.to_csv_cell());
                }
            }
            Ok(Value::String(text))
        }
        DataScalarFunction::IfElse => {
            let [condition, when_true, when_false] = args else {
                return Err(Diagnostic::error(
                    codes::E1402,
                    "if_else() expects three arguments",
                    Span::zero(),
                ));
            };
            match eval_row_expr(condition, table, row)? {
                Value::Bool(true) => eval_row_expr(when_true, table, row),
                Value::Bool(false) => eval_row_expr(when_false, table, row),
                Value::Null => Ok(Value::Null),
                _ => Err(type_error("if_else() condition requires a boolean")),
            }
        }
        DataScalarFunction::Contains | DataScalarFunction::StartsWith => {
            let [value, pattern] = args else {
                return Err(Diagnostic::error(
                    codes::E1402,
                    "text predicate expects two arguments",
                    Span::zero(),
                ));
            };
            let value = eval_row_expr(value, table, row)?;
            let pattern = eval_row_expr(pattern, table, row)?;
            Ok(
                match (
                    value_to_optional_text(value),
                    value_to_optional_text(pattern),
                ) {
                    (Some(value), Some(pattern)) => Value::Bool(match function {
                        DataScalarFunction::Contains => value.contains(&pattern),
                        DataScalarFunction::StartsWith => value.starts_with(&pattern),
                        _ => unreachable!(),
                    }),
                    _ => Value::Null,
                },
            )
        }
        DataScalarFunction::Replace => {
            let [value, pattern, replacement] = args else {
                return Err(Diagnostic::error(
                    codes::E1402,
                    "replace() expects three arguments",
                    Span::zero(),
                ));
            };
            let value = eval_row_expr(value, table, row)?;
            let pattern = eval_row_expr(pattern, table, row)?;
            let replacement = eval_row_expr(replacement, table, row)?;
            Ok(
                match (
                    value_to_optional_text(value),
                    value_to_optional_text(pattern),
                    value_to_optional_text(replacement),
                ) {
                    (Some(value), Some(pattern), Some(replacement)) => {
                        Value::String(value.replace(&pattern, &replacement))
                    }
                    _ => Value::Null,
                },
            )
        }
        DataScalarFunction::DateFloor => {
            let [value, unit] = args else {
                return Err(Diagnostic::error(
                    codes::E1402,
                    "date_floor() expects two arguments",
                    Span::zero(),
                ));
            };
            let unit = match eval_row_expr(unit, table, row)? {
                Value::String(unit) => {
                    crate::temporal::parse_temporal_unit(&unit).ok_or_else(|| {
                        Diagnostic::error(
                            codes::E1406,
                            format!(
                                "date_floor() unit `{unit}` is not supported; use \"day\", \
                                 \"week\", \"month\", or \"year\""
                            ),
                            Span::zero(),
                        )
                    })?
                }
                _ => {
                    return Err(Diagnostic::error(
                        codes::E1403,
                        "date_floor() unit must be a string",
                        Span::zero(),
                    ));
                }
            };
            let value = eval_row_expr(value, table, row)?;
            Ok(parse_temporal_value(value)
                .map(|parsed| {
                    let floored = crate::temporal::floor_temporal(&parsed, unit);
                    Value::String(
                        crate::temporal::normalize_datetime(&floored)
                            .unwrap_or_else(|| crate::temporal::normalize_date(&floored)),
                    )
                })
                .unwrap_or(Value::Null))
        }
        DataScalarFunction::DateFormat => {
            let [value, pattern] = args else {
                return Err(Diagnostic::error(
                    codes::E1402,
                    "date_format() expects two arguments",
                    Span::zero(),
                ));
            };
            let pattern = match eval_row_expr(pattern, table, row)? {
                Value::String(pattern) => {
                    crate::temporal::validate_format_pattern(&pattern).map_err(|token| {
                        Diagnostic::error(
                            codes::E1406,
                            format!("date_format() pattern token `{token}` is not supported"),
                            Span::zero(),
                        )
                    })?;
                    pattern
                }
                _ => {
                    return Err(Diagnostic::error(
                        codes::E1403,
                        "date_format() pattern must be a string",
                        Span::zero(),
                    ));
                }
            };
            let value = eval_row_expr(value, table, row)?;
            Ok(parse_temporal_value(value)
                .and_then(|parsed| crate::temporal::format_temporal(&parsed, &pattern))
                .map(Value::String)
                .unwrap_or(Value::Null))
        }
        DataScalarFunction::IsNull
        | DataScalarFunction::NotNull
        | DataScalarFunction::Lower
        | DataScalarFunction::Upper
        | DataScalarFunction::Trim
        | DataScalarFunction::ToString
        | DataScalarFunction::ToNumber
        | DataScalarFunction::ToBoolean
        | DataScalarFunction::Abs
        | DataScalarFunction::Round { .. }
        | DataScalarFunction::Date
        | DataScalarFunction::Datetime
        | DataScalarFunction::Year
        | DataScalarFunction::Month
        | DataScalarFunction::Day => {
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
                DataScalarFunction::Lower => Ok(map_text(value, |text| text.to_lowercase())),
                DataScalarFunction::Upper => Ok(map_text(value, |text| text.to_uppercase())),
                DataScalarFunction::Trim => Ok(map_text(value, |text| text.trim().to_string())),
                DataScalarFunction::ToString => Ok(match value {
                    Value::Null => Value::Null,
                    _ => Value::String(value.to_csv_cell()),
                }),
                DataScalarFunction::ToNumber => Ok(match value {
                    Value::Null => Value::Null,
                    Value::Number(_) => value,
                    _ => value
                        .to_csv_cell()
                        .trim()
                        .parse::<f64>()
                        .map(Value::Number)
                        .unwrap_or(Value::Null),
                }),
                DataScalarFunction::ToBoolean => Ok(match value {
                    Value::Null => Value::Null,
                    Value::Bool(_) => value,
                    _ => match value.to_csv_cell().trim() {
                        "true" => Value::Bool(true),
                        "false" => Value::Bool(false),
                        _ => Value::Null,
                    },
                }),
                DataScalarFunction::Abs => match value {
                    Value::Null => Ok(Value::Null),
                    Value::Number(value) => Ok(Value::Number(value.abs())),
                    _ => Err(type_error("abs() requires a number")),
                },
                DataScalarFunction::Round { digits } => round_value(value, digits),
                DataScalarFunction::Date => Ok(parse_temporal_value(value)
                    .map(|parsed| Value::String(crate::temporal::normalize_date(&parsed)))
                    .unwrap_or(Value::Null)),
                DataScalarFunction::Datetime => Ok(parse_temporal_value(value)
                    .and_then(|parsed| crate::temporal::normalize_datetime(&parsed))
                    .map(Value::String)
                    .unwrap_or(Value::Null)),
                DataScalarFunction::Year => Ok(parse_temporal_value(value)
                    .map(|parsed| Value::Number(f64::from(crate::temporal::temporal_year(&parsed))))
                    .unwrap_or(Value::Null)),
                DataScalarFunction::Month => Ok(parse_temporal_value(value)
                    .map(|parsed| {
                        Value::Number(f64::from(crate::temporal::temporal_month(&parsed)))
                    })
                    .unwrap_or(Value::Null)),
                DataScalarFunction::Day => Ok(parse_temporal_value(value)
                    .map(|parsed| Value::Number(f64::from(crate::temporal::temporal_day(&parsed))))
                    .unwrap_or(Value::Null)),
                DataScalarFunction::Coalesce
                | DataScalarFunction::Concat
                | DataScalarFunction::Contains
                | DataScalarFunction::StartsWith
                | DataScalarFunction::Replace
                | DataScalarFunction::IfElse
                | DataScalarFunction::DateFloor
                | DataScalarFunction::DateFormat => unreachable!(),
            }
        }
    }
}

fn parse_temporal_value(value: Value) -> Option<crate::temporal::TemporalValue> {
    value_to_optional_text(value).and_then(|text| crate::temporal::parse_temporal(&text))
}

fn value_to_optional_text(value: Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(value) => Some(value),
        value => Some(value.to_csv_cell()),
    }
}

fn map_text(value: Value, map: impl FnOnce(String) -> String) -> Value {
    match value {
        Value::Null => Value::Null,
        Value::String(value) => Value::String(map(value)),
        value => Value::String(map(value.to_csv_cell())),
    }
}

fn round_value(value: Value, digits: u32) -> Result<Value, Diagnostic> {
    match value {
        Value::Null => Ok(Value::Null),
        Value::Number(value) => {
            let scale = 10_f64.powi(digits as i32);
            let rounded = (value * scale).round() / scale;
            Ok(Value::Number(if rounded == 0.0 { 0.0 } else { rounded }))
        }
        _ => Err(type_error("round() requires a number")),
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
                DataBinaryOp::Add => {
                    left.cast(native::DataType::Float64) + right.cast(native::DataType::Float64)
                }
                DataBinaryOp::Sub => {
                    left.cast(native::DataType::Float64) - right.cast(native::DataType::Float64)
                }
                DataBinaryOp::Mul => {
                    left.cast(native::DataType::Float64) * right.cast(native::DataType::Float64)
                }
                DataBinaryOp::Div => {
                    left.cast(native::DataType::Float64) / right.cast(native::DataType::Float64)
                }
                DataBinaryOp::Rem => {
                    left.cast(native::DataType::Float64) % right.cast(native::DataType::Float64)
                }
            }
        }
        DataExpr::Call { function, args } => match function {
            // Temporal scalar functions are row-only by design in v0.46.5;
            // the native planner demotes them before lowering reaches this
            // point (`NativeUnsupportedReason::TemporalFunction`).
            DataScalarFunction::Date
            | DataScalarFunction::Datetime
            | DataScalarFunction::Year
            | DataScalarFunction::Month
            | DataScalarFunction::Day
            | DataScalarFunction::DateFloor
            | DataScalarFunction::DateFormat => {
                return Err(unsupported_native_operation("temporal scalar function"));
            }
            DataScalarFunction::Coalesce => {
                let expressions = args
                    .iter()
                    .map(native_expr)
                    .collect::<Result<Vec<_>, _>>()?;
                native::coalesce(&expressions)
            }
            DataScalarFunction::Concat => {
                let expressions = args
                    .iter()
                    .map(|arg| Ok(native_expr(arg)?.cast(native::DataType::String)))
                    .collect::<Result<Vec<_>, Diagnostic>>()?;
                native::concat_str(expressions, "", true)
            }
            DataScalarFunction::IfElse => {
                let [condition, when_true, when_false] = args.as_slice() else {
                    return Err(unsupported_native_operation("if_else arity"));
                };
                let condition = native_expr(condition)?;
                let when_true = native_expr(when_true)?;
                let when_false = native_expr(when_false)?;
                native::when(condition.clone().eq(native::lit(true)))
                    .then(when_true)
                    .otherwise(
                        native::when(condition.is_null())
                            .then(native::lit(native::NULL))
                            .otherwise(when_false),
                    )
            }
            DataScalarFunction::Contains | DataScalarFunction::StartsWith => {
                let [value, pattern] = args.as_slice() else {
                    return Err(unsupported_native_operation("text predicate arity"));
                };
                let value = native_expr(value)?.cast(native::DataType::String);
                let pattern = native_expr(pattern)?.cast(native::DataType::String);
                match function {
                    DataScalarFunction::Contains => value.str().contains_literal(pattern),
                    DataScalarFunction::StartsWith => value.str().starts_with(pattern),
                    _ => unreachable!(),
                }
            }
            DataScalarFunction::Replace => {
                let [value, pattern, replacement] = args.as_slice() else {
                    return Err(unsupported_native_operation("replace arity"));
                };
                let pattern = native_static_text_literal(pattern, "replace pattern")?;
                let replacement = native_static_text_literal(replacement, "replace replacement")?;
                native_expr(value)?
                    .cast(native::DataType::String)
                    .str()
                    .replace_all(
                        native::lit(pattern.as_str()),
                        native::lit(replacement.as_str()),
                        true,
                    )
            }
            DataScalarFunction::IsNull
            | DataScalarFunction::NotNull
            | DataScalarFunction::Lower
            | DataScalarFunction::Upper
            | DataScalarFunction::Trim
            | DataScalarFunction::ToString
            | DataScalarFunction::ToNumber
            | DataScalarFunction::ToBoolean
            | DataScalarFunction::Abs
            | DataScalarFunction::Round { .. } => {
                let [arg_expr] = args.as_slice() else {
                    return Err(unsupported_native_operation("scalar function arity"));
                };
                let arg = native_expr(arg_expr)?;
                match function {
                    DataScalarFunction::IsNull => arg.is_null(),
                    DataScalarFunction::NotNull => arg.is_not_null(),
                    DataScalarFunction::Lower => {
                        arg.cast(native::DataType::String).str().to_lowercase()
                    }
                    DataScalarFunction::Upper => {
                        arg.cast(native::DataType::String).str().to_uppercase()
                    }
                    DataScalarFunction::Trim => arg
                        .cast(native::DataType::String)
                        .str()
                        .strip_chars(native::lit(native::NULL)),
                    DataScalarFunction::ToString => native_to_string_expr(arg_expr, arg)?,
                    DataScalarFunction::ToNumber => arg
                        .cast(native::DataType::String)
                        .str()
                        .strip_chars(native::lit(native::NULL))
                        .cast(native::DataType::Float64),
                    DataScalarFunction::ToBoolean => {
                        let text = arg
                            .cast(native::DataType::String)
                            .str()
                            .strip_chars(native::lit(native::NULL));
                        native::when(text.clone().eq(native::lit("true")))
                            .then(native::lit(true))
                            .otherwise(
                                native::when(text.eq(native::lit("false")))
                                    .then(native::lit(false))
                                    .otherwise(native::lit(native::NULL)),
                            )
                    }
                    DataScalarFunction::Abs => arg.abs(),
                    DataScalarFunction::Round { digits } => {
                        // The row runtime normalizes `-0` to `0`. IEEE 754
                        // compares `-0 == 0`, so the remap catches exactly
                        // the negative-zero results; nulls fall through.
                        let rounded = arg.round(*digits, native::RoundMode::HalfAwayFromZero);
                        native::when(rounded.clone().eq(native::lit(0.0)))
                            .then(native::lit(0.0))
                            .otherwise(rounded)
                    }
                    DataScalarFunction::Coalesce
                    | DataScalarFunction::Concat
                    | DataScalarFunction::Contains
                    | DataScalarFunction::StartsWith
                    | DataScalarFunction::Replace
                    | DataScalarFunction::IfElse
                    | DataScalarFunction::Date
                    | DataScalarFunction::Datetime
                    | DataScalarFunction::Year
                    | DataScalarFunction::Month
                    | DataScalarFunction::Day
                    | DataScalarFunction::DateFloor
                    | DataScalarFunction::DateFormat => {
                        unreachable!()
                    }
                }
            }
        },
        DataExpr::Window {
            function,
            args,
            spec,
        } => native_window_expr(*function, args, spec)?,
    })
}

#[cfg(feature = "polars-engine")]
fn native_static_text_literal(expr: &DataExpr, reason: &'static str) -> Result<String, Diagnostic> {
    match expr {
        DataExpr::Literal(DataLiteral::String(value)) => Ok(value.clone()),
        DataExpr::Literal(DataLiteral::Number(value)) => Ok(Value::Number(*value).to_csv_cell()),
        DataExpr::Literal(DataLiteral::Bool(value)) => Ok(value.to_string()),
        DataExpr::Literal(DataLiteral::Null) => Err(unsupported_native_operation(reason)),
        _ => Err(unsupported_native_operation(reason)),
    }
}

#[cfg(feature = "polars-engine")]
fn native_to_string_expr(
    source: &DataExpr,
    expr: native::Expr,
) -> Result<native::Expr, Diagnostic> {
    if data_expr_native_numeric_result(source) {
        Ok(native_numeric_to_string_expr(expr))
    } else {
        Ok(expr.cast(native::DataType::String))
    }
}

#[cfg(feature = "polars-engine")]
fn native_numeric_to_string_expr(expr: native::Expr) -> native::Expr {
    expr.cast(native::DataType::Float64)
        .cast(native::DataType::String)
        .str()
        .strip_suffix(native::lit(".0"))
}

#[cfg(feature = "polars-engine")]
fn data_expr_native_numeric_result(expr: &DataExpr) -> bool {
    match expr {
        DataExpr::Literal(DataLiteral::Number(_)) => true,
        DataExpr::Literal(DataLiteral::Null | DataLiteral::Bool(_) | DataLiteral::String(_)) => {
            false
        }
        DataExpr::Column(_) => false,
        DataExpr::Unary {
            op: DataUnaryOp::Neg,
            ..
        } => true,
        DataExpr::Unary {
            op: DataUnaryOp::Not,
            ..
        } => false,
        DataExpr::Binary { op, .. } => matches!(
            op,
            DataBinaryOp::Add
                | DataBinaryOp::Sub
                | DataBinaryOp::Mul
                | DataBinaryOp::Div
                | DataBinaryOp::Rem
        ),
        DataExpr::Call { function, args } => match function {
            DataScalarFunction::ToNumber
            | DataScalarFunction::Abs
            | DataScalarFunction::Round { .. } => true,
            DataScalarFunction::Coalesce => args.iter().all(data_expr_native_numeric_result),
            DataScalarFunction::IfElse => {
                let [_, when_true, when_false] = args.as_slice() else {
                    return false;
                };
                data_expr_native_numeric_result(when_true)
                    && data_expr_native_numeric_result(when_false)
            }
            DataScalarFunction::IsNull
            | DataScalarFunction::NotNull
            | DataScalarFunction::Concat
            | DataScalarFunction::Lower
            | DataScalarFunction::Upper
            | DataScalarFunction::Trim
            | DataScalarFunction::Contains
            | DataScalarFunction::StartsWith
            | DataScalarFunction::Replace
            | DataScalarFunction::ToString
            | DataScalarFunction::ToBoolean
            | DataScalarFunction::Date
            | DataScalarFunction::Datetime
            | DataScalarFunction::Year
            | DataScalarFunction::Month
            | DataScalarFunction::Day
            | DataScalarFunction::DateFloor
            | DataScalarFunction::DateFormat => false,
        },
        DataExpr::Window { function, args, .. } => match function {
            DataWindowFunction::RowNumber
            | DataWindowFunction::Rank
            | DataWindowFunction::DenseRank
            | DataWindowFunction::PercentRank
            | DataWindowFunction::CumeDist
            | DataWindowFunction::Count
            | DataWindowFunction::Sum
            | DataWindowFunction::Mean => true,
            DataWindowFunction::Lag
            | DataWindowFunction::Lead
            | DataWindowFunction::FirstValue
            | DataWindowFunction::LastValue
            | DataWindowFunction::Min
            | DataWindowFunction::Max => args.first().is_some_and(data_expr_native_numeric_result),
        },
    }
}

#[cfg(feature = "polars-engine")]
fn native_window_expr(
    function: DataWindowFunction,
    args: &[DataExpr],
    spec: &DataWindowSpec,
) -> Result<native::Expr, Diagnostic> {
    match function {
        DataWindowFunction::RowNumber => {
            if !args.is_empty() {
                return Err(unsupported_native_operation("row_number window arity"));
            }
            native_window_row_position(spec)
        }
        DataWindowFunction::Rank => native_rank_window_expr(args, spec, native::RankMethod::Min),
        DataWindowFunction::DenseRank => {
            native_rank_window_expr(args, spec, native::RankMethod::Dense)
        }
        DataWindowFunction::PercentRank => native_percent_rank_window_expr(args, spec),
        DataWindowFunction::CumeDist => native_cume_dist_window_expr(args, spec),
        DataWindowFunction::Lag => native_offset_window_expr(args, spec, 1),
        DataWindowFunction::Lead => native_offset_window_expr(args, spec, -1),
        DataWindowFunction::FirstValue => native_value_window_expr(args, spec, true),
        DataWindowFunction::LastValue => native_value_window_expr(args, spec, false),
        DataWindowFunction::Count if args.is_empty() => match spec.frame {
            DataWindowFrame::WholePartition => native_window_over(native::len(), spec, false),
            DataWindowFrame::UnboundedPrecedingToCurrentRow => native_window_row_position(spec),
            DataWindowFrame::CurrentRowToUnboundedFollowing
            | DataWindowFrame::PrecedingToCurrentRow { .. }
            | DataWindowFrame::CurrentRowToFollowing { .. }
            | DataWindowFrame::PrecedingToFollowing { .. } => native_frame_row_count_expr(spec),
        },
        DataWindowFunction::Count => {
            let [arg] = args else {
                return Err(unsupported_native_operation("count window arity"));
            };
            match spec.frame {
                DataWindowFrame::WholePartition => {
                    native_window_over(native_expr(arg)?.count(), spec, false)
                }
                DataWindowFrame::UnboundedPrecedingToCurrentRow => {
                    let count = native_expr(arg)?.cum_count(false);
                    native_window_over(count, spec, true)
                }
                DataWindowFrame::CurrentRowToUnboundedFollowing
                | DataWindowFrame::PrecedingToCurrentRow { .. }
                | DataWindowFrame::CurrentRowToFollowing { .. }
                | DataWindowFrame::PrecedingToFollowing { .. } => {
                    native_count_window_expr(native_expr(arg)?, spec)
                }
            }
        }
        DataWindowFunction::Sum
        | DataWindowFunction::Mean
        | DataWindowFunction::Min
        | DataWindowFunction::Max => {
            let [arg] = args else {
                return Err(unsupported_native_operation("aggregate window arity"));
            };
            let expr = native_expr(arg)?;
            match spec.frame {
                DataWindowFrame::WholePartition => {
                    let expr = match function {
                        DataWindowFunction::Sum => expr.sum(),
                        DataWindowFunction::Mean => expr.mean(),
                        DataWindowFunction::Min => expr.min(),
                        DataWindowFunction::Max => expr.max(),
                        DataWindowFunction::RowNumber
                        | DataWindowFunction::Rank
                        | DataWindowFunction::DenseRank
                        | DataWindowFunction::PercentRank
                        | DataWindowFunction::CumeDist
                        | DataWindowFunction::Lag
                        | DataWindowFunction::Lead
                        | DataWindowFunction::FirstValue
                        | DataWindowFunction::LastValue
                        | DataWindowFunction::Count => unreachable!("handled aggregate window"),
                    };
                    native_window_over(expr, spec, false)
                }
                DataWindowFrame::UnboundedPrecedingToCurrentRow => {
                    native_running_aggregate_window_expr(function, expr, spec)
                }
                DataWindowFrame::CurrentRowToUnboundedFollowing
                | DataWindowFrame::PrecedingToCurrentRow { .. }
                | DataWindowFrame::CurrentRowToFollowing { .. }
                | DataWindowFrame::PrecedingToFollowing { .. } => {
                    native_framed_aggregate_window_expr(function, expr, spec)
                }
            }
        }
    }
}

#[cfg(feature = "polars-engine")]
fn native_offset_window_expr(
    args: &[DataExpr],
    spec: &DataWindowSpec,
    shift_multiplier: i64,
) -> Result<native::Expr, Diagnostic> {
    let [value, rest @ ..] = args else {
        return Err(unsupported_native_operation("lag/lead window arity"));
    };
    if rest.len() > 2 {
        return Err(unsupported_native_operation("lag/lead window arity"));
    }
    let offset = native_window_offset(rest.first())?;
    let shifted = native_expr(value)?.shift(native::lit(shift_multiplier * offset as i64));
    let shifted = native_window_over(shifted, spec, true)?;
    let Some(default) = rest.get(1) else {
        return Ok(shifted);
    };
    let position = native_window_row_position(spec)?;
    let out_of_bounds = if shift_multiplier > 0 {
        position.lt_eq(native::lit(offset as u32))
    } else {
        (position + native::lit(offset as u32)).gt(native_window_over(native::len(), spec, false)?)
    };
    Ok(native::when(out_of_bounds)
        .then(native_expr(default)?)
        .otherwise(shifted))
}

#[cfg(feature = "polars-engine")]
fn native_window_offset(expr: Option<&DataExpr>) -> Result<usize, Diagnostic> {
    match expr {
        None => Ok(1),
        Some(DataExpr::Literal(DataLiteral::Number(value)))
            if *value >= 0.0 && value.fract() == 0.0 =>
        {
            Ok(*value as usize)
        }
        Some(_) => Err(unsupported_native_operation("lag/lead window offset")),
    }
}

#[cfg(feature = "polars-engine")]
fn native_value_window_expr(
    args: &[DataExpr],
    spec: &DataWindowSpec,
    first: bool,
) -> Result<native::Expr, Diagnostic> {
    let [arg] = args else {
        return Err(unsupported_native_operation("value window arity"));
    };
    let value = native_expr(arg)?;
    match (first, spec.frame) {
        (true, DataWindowFrame::CurrentRowToUnboundedFollowing)
        | (true, DataWindowFrame::CurrentRowToFollowing { .. })
        | (false, DataWindowFrame::UnboundedPrecedingToCurrentRow)
        | (false, DataWindowFrame::PrecedingToCurrentRow { .. }) => Ok(value),
        (true, DataWindowFrame::WholePartition)
        | (true, DataWindowFrame::UnboundedPrecedingToCurrentRow) => {
            native_window_over(value.first(), spec, true)
        }
        (false, DataWindowFrame::WholePartition)
        | (false, DataWindowFrame::CurrentRowToUnboundedFollowing) => {
            native_window_over(value.last(), spec, true)
        }
        (true, DataWindowFrame::PrecedingToCurrentRow { rows }) => {
            native_shifted_frame_value(value, spec, rows as i64)
        }
        (false, DataWindowFrame::CurrentRowToFollowing { rows }) => {
            native_shifted_frame_value(value, spec, -(rows as i64))
        }
        (
            true,
            DataWindowFrame::PrecedingToFollowing {
                preceding,
                following: _,
            },
        ) => native_shifted_frame_value(value, spec, preceding as i64),
        (
            false,
            DataWindowFrame::PrecedingToFollowing {
                preceding: _,
                following,
            },
        ) => native_shifted_frame_value(value, spec, -(following as i64)),
    }
}

#[cfg(feature = "polars-engine")]
fn native_frame_row_count_expr(spec: &DataWindowSpec) -> Result<native::Expr, Diagnostic> {
    let position = native_window_row_position(spec)?;
    let partition_len = native_window_over(native::len(), spec, false)?;
    let expr = match spec.frame {
        DataWindowFrame::WholePartition => partition_len,
        DataWindowFrame::UnboundedPrecedingToCurrentRow => position,
        DataWindowFrame::CurrentRowToUnboundedFollowing => {
            partition_len - position + native::lit(1u32)
        }
        DataWindowFrame::PrecedingToCurrentRow { rows } => {
            native_min_u32_expr(position, native::lit((rows + 1) as u32))
        }
        DataWindowFrame::CurrentRowToFollowing { rows } => {
            let available = partition_len - position + native::lit(1u32);
            native_min_u32_expr(available, native::lit((rows + 1) as u32))
        }
        DataWindowFrame::PrecedingToFollowing {
            preceding,
            following,
        } => {
            let start = native::when(position.clone().gt(native::lit(preceding as u32)))
                .then(position.clone() - native::lit(preceding as u32))
                .otherwise(native::lit(1u32));
            let unclamped_end = position + native::lit(following as u32);
            let end = native_min_u32_expr(unclamped_end, partition_len);
            native::when(start.clone().lt_eq(end.clone()))
                .then(end - start + native::lit(1u32))
                .otherwise(native::lit(0u32))
        }
    };
    Ok(expr)
}

#[cfg(feature = "polars-engine")]
fn native_count_window_expr(
    expr: native::Expr,
    spec: &DataWindowSpec,
) -> Result<native::Expr, Diagnostic> {
    let present = expr.is_not_null().cast(native::DataType::UInt32);
    let expr = match spec.frame {
        DataWindowFrame::WholePartition => present.sum(),
        DataWindowFrame::UnboundedPrecedingToCurrentRow => present.cum_sum(false),
        DataWindowFrame::CurrentRowToUnboundedFollowing => {
            native_reverse_running_count_expr(present)
        }
        DataWindowFrame::PrecedingToCurrentRow { rows } => {
            present.rolling_sum(native_fixed_window_options(rows + 1, false))
        }
        DataWindowFrame::CurrentRowToFollowing { rows } => present
            .reverse()
            .rolling_sum(native_fixed_window_options(rows + 1, false))
            .reverse(),
        DataWindowFrame::PrecedingToFollowing {
            preceding,
            following,
        } if preceding == following => {
            present.rolling_sum(native_fixed_window_options(preceding + following + 1, true))
        }
        DataWindowFrame::PrecedingToFollowing { .. } => {
            return Err(unsupported_native_operation(
                "asymmetric bounded window frame",
            ));
        }
    };
    native_window_over(expr, spec, true)
}

#[cfg(feature = "polars-engine")]
fn native_framed_aggregate_window_expr(
    function: DataWindowFunction,
    expr: native::Expr,
    spec: &DataWindowSpec,
) -> Result<native::Expr, Diagnostic> {
    let expr = match spec.frame {
        DataWindowFrame::WholePartition => match function {
            DataWindowFunction::Sum => expr.sum(),
            DataWindowFunction::Mean => expr.mean(),
            DataWindowFunction::Min => expr.min(),
            DataWindowFunction::Max => expr.max(),
            _ => return Err(unsupported_native_operation("aggregate window function")),
        },
        DataWindowFrame::UnboundedPrecedingToCurrentRow => {
            return native_running_aggregate_window_expr(function, expr, spec);
        }
        DataWindowFrame::CurrentRowToUnboundedFollowing => {
            native_reverse_running_aggregate_expr(function, expr)?
        }
        DataWindowFrame::PrecedingToCurrentRow { rows } => native_rolling_aggregate_expr(
            function,
            expr,
            native_fixed_window_options(rows + 1, false),
        )?,
        DataWindowFrame::CurrentRowToFollowing { rows } => native_rolling_aggregate_expr(
            function,
            expr.reverse(),
            native_fixed_window_options(rows + 1, false),
        )?
        .reverse(),
        DataWindowFrame::PrecedingToFollowing {
            preceding,
            following,
        } if preceding == following => native_rolling_aggregate_expr(
            function,
            expr,
            native_fixed_window_options(preceding + following + 1, true),
        )?,
        DataWindowFrame::PrecedingToFollowing { .. } => {
            return Err(unsupported_native_operation(
                "asymmetric bounded window frame",
            ));
        }
    };
    native_window_over(expr, spec, true)
}

#[cfg(feature = "polars-engine")]
fn native_reverse_running_count_expr(expr: native::Expr) -> native::Expr {
    expr.reverse().cum_sum(false).reverse()
}

#[cfg(feature = "polars-engine")]
fn native_reverse_running_aggregate_expr(
    function: DataWindowFunction,
    expr: native::Expr,
) -> Result<native::Expr, Diagnostic> {
    Ok(match function {
        DataWindowFunction::Sum => {
            native_running_fill_nulls(expr.reverse().cum_sum(false)).reverse()
        }
        DataWindowFunction::Min => {
            native_running_fill_nulls(expr.reverse().cum_min(false)).reverse()
        }
        DataWindowFunction::Max => {
            native_running_fill_nulls(expr.reverse().cum_max(false)).reverse()
        }
        DataWindowFunction::Mean => {
            let reversed = expr.reverse();
            let sum = native_running_fill_nulls(reversed.clone().cum_sum(false));
            let count = reversed.cum_count(false);
            native::when(count.clone().gt(native::lit(0u32)))
                .then(sum.cast(native::DataType::Float64) / count.cast(native::DataType::Float64))
                .otherwise(native::lit(native::NULL))
                .reverse()
        }
        _ => return Err(unsupported_native_operation("aggregate window function")),
    })
}

#[cfg(feature = "polars-engine")]
fn native_rolling_aggregate_expr(
    function: DataWindowFunction,
    expr: native::Expr,
    options: native::RollingOptionsFixedWindow,
) -> Result<native::Expr, Diagnostic> {
    Ok(match function {
        DataWindowFunction::Sum => expr.rolling_sum(options),
        DataWindowFunction::Mean => expr.rolling_mean(options),
        DataWindowFunction::Min => expr.rolling_min(options),
        DataWindowFunction::Max => expr.rolling_max(options),
        _ => return Err(unsupported_native_operation("aggregate window function")),
    })
}

#[cfg(feature = "polars-engine")]
fn native_fixed_window_options(
    window_size: usize,
    center: bool,
) -> native::RollingOptionsFixedWindow {
    native::RollingOptionsFixedWindow {
        window_size,
        min_periods: 1,
        center,
        ..Default::default()
    }
}

#[cfg(feature = "polars-engine")]
fn native_shifted_frame_value(
    value: native::Expr,
    spec: &DataWindowSpec,
    offset: i64,
) -> Result<native::Expr, Diagnostic> {
    if offset == 0 {
        return Ok(value);
    }
    let shifted = native_window_over(value.clone().shift(native::lit(offset)), spec, true)?;
    let position = native_window_row_position(spec)?;
    let (in_bounds, boundary) = if offset > 0 {
        (
            position.gt(native::lit(offset as u32)),
            native_window_over(value.first(), spec, true)?,
        )
    } else {
        let rows = (-offset) as u32;
        let partition_len = native_window_over(native::len(), spec, false)?;
        (
            (position + native::lit(rows)).lt_eq(partition_len),
            native_window_over(value.last(), spec, true)?,
        )
    };
    Ok(native::when(in_bounds).then(shifted).otherwise(boundary))
}

#[cfg(feature = "polars-engine")]
fn native_min_u32_expr(left: native::Expr, right: native::Expr) -> native::Expr {
    native::when(left.clone().lt_eq(right.clone()))
        .then(left)
        .otherwise(right)
}

#[cfg(feature = "polars-engine")]
fn native_percent_rank_window_expr(
    args: &[DataExpr],
    spec: &DataWindowSpec,
) -> Result<native::Expr, Diagnostic> {
    let rank = native_rank_window_expr(args, spec, native::RankMethod::Min)?;
    let partition_len = native_window_over(native::len(), spec, false)?;
    Ok(native::when(partition_len.clone().lt_eq(native::lit(1u32)))
        .then(native::lit(0.0f64))
        .otherwise(
            (rank.cast(native::DataType::Float64) - native::lit(1.0f64))
                / (partition_len.cast(native::DataType::Float64) - native::lit(1.0f64)),
        ))
}

#[cfg(feature = "polars-engine")]
fn native_cume_dist_window_expr(
    args: &[DataExpr],
    spec: &DataWindowSpec,
) -> Result<native::Expr, Diagnostic> {
    let rank = native_rank_window_expr(args, spec, native::RankMethod::Max)?;
    let partition_len = native_window_over(native::len(), spec, false)?;
    Ok(rank.cast(native::DataType::Float64) / partition_len.cast(native::DataType::Float64))
}

#[cfg(feature = "polars-engine")]
fn native_running_aggregate_window_expr(
    function: DataWindowFunction,
    expr: native::Expr,
    spec: &DataWindowSpec,
) -> Result<native::Expr, Diagnostic> {
    let expr = match function {
        DataWindowFunction::Sum => native_running_fill_nulls(expr.cum_sum(false)),
        DataWindowFunction::Min => native_running_fill_nulls(expr.cum_min(false)),
        DataWindowFunction::Max => native_running_fill_nulls(expr.cum_max(false)),
        DataWindowFunction::Mean => {
            let sum = native_running_fill_nulls(expr.clone().cum_sum(false));
            let count = expr.cum_count(false);
            native::when(count.clone().gt(native::lit(0u32)))
                .then(sum.cast(native::DataType::Float64) / count.cast(native::DataType::Float64))
                .otherwise(native::lit(native::NULL))
        }
        DataWindowFunction::RowNumber
        | DataWindowFunction::Rank
        | DataWindowFunction::DenseRank
        | DataWindowFunction::PercentRank
        | DataWindowFunction::CumeDist
        | DataWindowFunction::Lag
        | DataWindowFunction::Lead
        | DataWindowFunction::FirstValue
        | DataWindowFunction::LastValue
        | DataWindowFunction::Count => {
            unreachable!("running aggregate helper only receives sum/mean/min/max")
        }
    };
    native_window_over(expr, spec, true)
}

#[cfg(feature = "polars-engine")]
fn native_running_fill_nulls(expr: native::Expr) -> native::Expr {
    expr.fill_null_with_strategy(native::FillNullStrategy::Forward(
        native::FillNullLimit::None,
    ))
}

#[cfg(feature = "polars-engine")]
fn native_running_backward_fill_nulls(expr: native::Expr) -> native::Expr {
    expr.fill_null_with_strategy(native::FillNullStrategy::Backward(
        native::FillNullLimit::None,
    ))
}

#[cfg(feature = "polars-engine")]
fn native_window_row_position(spec: &DataWindowSpec) -> Result<native::Expr, Diagnostic> {
    native_window_over(native_window_row_position_expr(spec)?, spec, true)
}

#[cfg(feature = "polars-engine")]
fn native_window_row_position_expr(spec: &DataWindowSpec) -> Result<native::Expr, Diagnostic> {
    let Some(row_index) = &spec.row_index else {
        return Err(unsupported_native_operation("window row index"));
    };
    Ok(native::col(row_index).cum_count(false))
}

#[cfg(feature = "polars-engine")]
fn native_rank_window_expr(
    args: &[DataExpr],
    spec: &DataWindowSpec,
    method: native::RankMethod,
) -> Result<native::Expr, Diagnostic> {
    if !args.is_empty() {
        return Err(unsupported_native_operation("rank window arity"));
    }
    if spec.presorted && spec.order_by.len() > 1 {
        return native_presorted_rank_window_expr(spec, method);
    }
    let [order] = spec.order_by.as_slice() else {
        return Err(unsupported_native_operation("rank window order"));
    };
    let order_expr = native::col(&order.column);
    let partition_by = native_partition_exprs(spec);
    let descending = order.direction == SortDirection::Desc;
    let nulls_first = !native_sort_nulls_last(order.direction, order.nulls);
    let rank = native_window_over_partition(
        order_expr
            .clone()
            .rank(native::RankOptions { method, descending }, None),
        partition_by.clone(),
    )?;
    let null_count =
        native_window_over_partition(order_expr.clone().is_null().sum(), partition_by.clone())?;
    let non_null_count =
        native_window_over_partition(order_expr.clone().is_not_null().sum(), partition_by.clone())?;

    let expr = match method {
        native::RankMethod::Min if nulls_first => native::when(order_expr.clone().is_null())
            .then(native::lit(1u32))
            .otherwise(rank + null_count),
        native::RankMethod::Min => native::when(order_expr.clone().is_null())
            .then(non_null_count + native::lit(1u32))
            .otherwise(rank),
        native::RankMethod::Max if nulls_first => native::when(order_expr.clone().is_null())
            .then(null_count.clone())
            .otherwise(rank + null_count),
        native::RankMethod::Max => native::when(order_expr.clone().is_null())
            .then(null_count + non_null_count)
            .otherwise(rank),
        native::RankMethod::Dense if nulls_first => {
            let null_offset = native::when(null_count.clone().gt(native::lit(0u32)))
                .then(native::lit(1u32))
                .otherwise(native::lit(0u32));
            native::when(order_expr.clone().is_null())
                .then(native::lit(1u32))
                .otherwise(rank + null_offset)
        }
        native::RankMethod::Dense => {
            let non_null_unique = native_window_over_partition(
                order_expr.clone().drop_nulls().n_unique(),
                partition_by,
            )?;
            native::when(order_expr.is_null())
                .then(non_null_unique + native::lit(1u32))
                .otherwise(rank)
        }
        _ => return Err(unsupported_native_operation("rank method")),
    };
    Ok(expr)
}

#[cfg(feature = "polars-engine")]
fn native_presorted_rank_window_expr(
    spec: &DataWindowSpec,
    method: native::RankMethod,
) -> Result<native::Expr, Diagnostic> {
    let position = native_window_row_position_expr(spec)?;
    match method {
        native::RankMethod::Dense => {
            let peer_start = native_presorted_peer_boundary_expr(spec, false)?;
            let dense_rank = peer_start.cast(native::DataType::UInt32).cum_sum(false);
            native_window_over(dense_rank, spec, true)
        }
        native::RankMethod::Min => {
            let peer_start = native_presorted_peer_boundary_expr(spec, false)?;
            let starts = native::when(peer_start)
                .then(position)
                .otherwise(native::lit(native::NULL));
            native_window_over(native_running_fill_nulls(starts), spec, true)
        }
        native::RankMethod::Max => {
            let peer_end = native_presorted_peer_boundary_expr(spec, true)?;
            let ends = native::when(peer_end)
                .then(position)
                .otherwise(native::lit(native::NULL));
            native_window_over(native_running_backward_fill_nulls(ends), spec, true)
        }
        _ => Err(unsupported_native_operation("rank method")),
    }
}

#[cfg(feature = "polars-engine")]
fn native_presorted_peer_boundary_expr(
    spec: &DataWindowSpec,
    next: bool,
) -> Result<native::Expr, Diagnostic> {
    if spec.order_by.len() <= 1 {
        return Err(unsupported_native_operation("multi-key peer boundary"));
    }
    let mut differs = native::lit(false);
    let shift = if next { -1i64 } else { 1i64 };
    for order in &spec.order_by {
        let current = native::col(&order.column);
        let adjacent = current.clone().shift(native::lit(shift));
        differs = differs.or(current.neq_missing(adjacent));
    }
    let position = native_window_row_position_expr(spec)?;
    let edge = if next {
        position.eq(native::len())
    } else {
        position.eq(native::lit(1u32))
    };
    Ok(edge.or(differs))
}

#[cfg(feature = "polars-engine")]
fn native_window_over(
    expr: native::Expr,
    spec: &DataWindowSpec,
    include_order: bool,
) -> Result<native::Expr, Diagnostic> {
    let partition_by = native_partition_exprs(spec);
    let order_by = if include_order {
        native_window_order(spec)?
    } else {
        None
    };
    expr.over_with_options(
        Some(partition_by),
        order_by,
        native::WindowMapping::GroupsToRows,
    )
    .map_err(native_window_error)
}

#[cfg(feature = "polars-engine")]
fn native_window_over_partition(
    expr: native::Expr,
    partition_by: Vec<native::Expr>,
) -> Result<native::Expr, Diagnostic> {
    expr.over_with_options(
        Some(partition_by),
        None::<(Vec<native::Expr>, native::SortOptions)>,
        native::WindowMapping::GroupsToRows,
    )
    .map_err(native_window_error)
}

#[cfg(feature = "polars-engine")]
fn native_partition_exprs(spec: &DataWindowSpec) -> Vec<native::Expr> {
    if spec.partition_by.is_empty() {
        vec![native::lit(1i32)]
    } else {
        spec.partition_by.iter().map(native::col).collect()
    }
}

#[cfg(feature = "polars-engine")]
fn native_window_order(
    spec: &DataWindowSpec,
) -> Result<Option<(Vec<native::Expr>, native::SortOptions)>, Diagnostic> {
    if spec.presorted && spec.order_by.len() > 1 {
        return Ok(None);
    }
    match spec.order_by.as_slice() {
        [] => Ok(None),
        [item] => {
            let options = native::SortOptions::default()
                .with_order_descending(item.direction == SortDirection::Desc)
                .with_nulls_last(native_sort_nulls_last(item.direction, item.nulls))
                .with_multithreaded(false)
                .with_maintain_order(true);
            Ok(Some((vec![native::col(&item.column)], options)))
        }
        _ => Err(unsupported_native_operation("multi-key window order")),
    }
}

#[cfg(feature = "polars-engine")]
fn native_sort_multiple_options(specs: &[SortSpec]) -> native::SortMultipleOptions {
    native::SortMultipleOptions {
        descending: specs
            .iter()
            .map(|spec| spec.direction == SortDirection::Desc)
            .collect(),
        nulls_last: specs
            .iter()
            .map(|spec| native_sort_nulls_last(spec.direction, spec.nulls))
            .collect(),
        maintain_order: true,
        ..Default::default()
    }
}

#[cfg(feature = "polars-engine")]
fn native_sort_nulls_last(direction: SortDirection, nulls: NullsOrder) -> bool {
    match direction {
        SortDirection::Asc => nulls == NullsOrder::Last,
        SortDirection::Desc => nulls == NullsOrder::First,
    }
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
        "count_distinct" => native_unary_agg(item, |expr| expr.drop_nulls().n_unique())?,
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

/// Wraps in-memory source bytes (stdin or host-supplied file contents) as a
/// Polars scan source so byte-backed CSV inputs run through the same lazy
/// scan implementation as path-backed inputs.
#[cfg(feature = "polars-engine")]
fn byte_scan_sources(bytes: &[u8]) -> native::ScanSources {
    native::ScanSources::Buffers(std::sync::Arc::from([polars_buffer::Buffer::from(
        bytes.to_vec(),
    )]))
}

/// The native CSV reader keeps doubled-quote escapes inside quoted header
/// cells verbatim; the row reader unescapes them. Rename the scanned columns
/// positionally to the row reader's header parse so byte-backed native CSV
/// scans carry row-engine column names.
#[cfg(feature = "polars-engine")]
fn align_native_csv_header(
    mut plan: native::LazyFrame,
    logical_path: &Path,
    bytes: &[u8],
) -> Result<native::LazyFrame, Diagnostic> {
    let row_headers = crate::csv::read_csv_schema_from_bytes(logical_path, bytes)?;
    let native_headers: Vec<String> = plan
        .collect_schema()
        .map_err(native_read_error(logical_path, DataFormat::Csv))?
        .iter_names()
        .map(|name| name.to_string())
        .collect();
    if native_headers == row_headers || native_headers.len() != row_headers.len() {
        return Ok(plan);
    }
    Ok(plan.rename(&native_headers, &row_headers, true))
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
    let columns = native_frame_column_names(frame);
    let mut rows = Vec::with_capacity(frame.height());
    let mut values = Vec::with_capacity(columns.len());
    for row_index in 0..frame.height() {
        native_frame_row_values(frame, row_index, &mut values)?;
        rows.push(Row {
            values: values.clone(),
        });
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
fn native_window_error(error: native::PolarsError) -> Diagnostic {
    Diagnostic::error(
        codes::E1211,
        format!("native window expression failed: {error}"),
        Span::zero(),
    )
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

    /// v0.46.5: the data facade's row path mirrors the `pdl-exec` row
    /// runtime for temporal scalar functions: normalization, calendar
    /// fields, flooring, formatting, and null on unparseable input.
    #[test]
    fn temporal_scalar_functions_row_path_mirrors_row_runtime() {
        let source = DataSource::Bytes {
            logical_path: Path::new("memory.csv"),
            format: DataFormat::Csv,
            bytes: b"stamp\n2025-02-17T14:20:59Z\n2025-02-17T14:20:59+00:00\n2024-01-15T10:22:33.123-05:00\n2024-01-15\nnot-a-date\n",
        };
        let call = |function, args| DataExpr::Call { function, args };
        let stamp = || DataExpr::Column("stamp".to_string());
        let text = |value: &str| DataExpr::Literal(DataLiteral::String(value.to_string()));
        let plan = DataPlan::scan(source)
            .expect("row plan")
            .mutate(&[
                (
                    "date".to_string(),
                    call(DataScalarFunction::Date, vec![stamp()]),
                ),
                (
                    "datetime".to_string(),
                    call(DataScalarFunction::Datetime, vec![stamp()]),
                ),
                (
                    "y".to_string(),
                    call(DataScalarFunction::Year, vec![stamp()]),
                ),
                (
                    "m".to_string(),
                    call(DataScalarFunction::Month, vec![stamp()]),
                ),
                (
                    "d".to_string(),
                    call(DataScalarFunction::Day, vec![stamp()]),
                ),
                (
                    "floored".to_string(),
                    call(DataScalarFunction::DateFloor, vec![stamp(), text("month")]),
                ),
                (
                    "month_key".to_string(),
                    call(DataScalarFunction::DateFormat, vec![stamp(), text("%Y-%m")]),
                ),
            ])
            .expect("mutate")
            .drop_columns(&["stamp".to_string()])
            .expect("drop");
        let mut bytes = Vec::new();
        plan.write_to_sink(DataSink::Writer {
            format: DataFormat::Csv,
            writer: &mut bytes,
        })
        .expect("write");
        assert_eq!(
            String::from_utf8(bytes).expect("utf8"),
            "date,datetime,y,m,d,floored,month_key\n\
             2025-02-17,2025-02-17T14:20:59Z,2025,2,17,2025-02-01T00:00:00Z,2025-02\n\
             2025-02-17,2025-02-17T14:20:59Z,2025,2,17,2025-02-01T00:00:00Z,2025-02\n\
             2024-01-15,2024-01-15T10:22:33-05:00,2024,1,15,2024-01-01T00:00:00-05:00,2024-01\n\
             2024-01-15,,2024,1,15,2024-01-01,2024-01\n\
             ,,,,,,\n"
        );
    }

    /// v0.46.5: invalid `date_floor` units and `date_format` patterns
    /// report `E1406`; non-string units and patterns report `E1403`.
    #[test]
    fn temporal_scalar_functions_unit_and_pattern_diagnostics() {
        let scan = || {
            DataPlan::scan(DataSource::Bytes {
                logical_path: Path::new("memory.csv"),
                format: DataFormat::Csv,
                bytes: b"stamp\n2024-01-15\n",
            })
            .expect("row plan")
        };
        let stamp = || DataExpr::Column("stamp".to_string());
        let mutate = |function, arg| {
            scan().mutate(&[(
                "out".to_string(),
                DataExpr::Call {
                    function,
                    args: vec![stamp(), arg],
                },
            )])
        };

        let week = mutate(
            DataScalarFunction::DateFloor,
            DataExpr::Literal(DataLiteral::String("week".to_string())),
        );
        assert!(week.is_ok(), "week is a supported unit since v0.46.5");

        let fortnight = mutate(
            DataScalarFunction::DateFloor,
            DataExpr::Literal(DataLiteral::String("fortnight".to_string())),
        );
        assert_eq!(
            fortnight.err().expect("fortnight unit fails").code,
            codes::E1406
        );

        let numeric_unit = mutate(
            DataScalarFunction::DateFloor,
            DataExpr::Literal(DataLiteral::Number(1.0)),
        );
        assert_eq!(
            numeric_unit.err().expect("numeric unit fails").code,
            codes::E1403
        );

        let bad_token = mutate(
            DataScalarFunction::DateFormat,
            DataExpr::Literal(DataLiteral::String("%B".to_string())),
        );
        assert_eq!(bad_token.err().expect("%B fails").code, codes::E1406);

        let null_pattern = mutate(
            DataScalarFunction::DateFormat,
            DataExpr::Literal(DataLiteral::Null),
        );
        assert_eq!(
            null_pattern.err().expect("null pattern fails").code,
            codes::E1403
        );
    }

    /// Parity corpus for the v0.44 native text writers: embedded delimiters,
    /// quotes, newlines, multibyte UTF-8 in headers and cells, explicit
    /// nulls, booleans, and numeric edges (int64-scale magnitudes, f64
    /// subnormals, negative fractions).
    #[cfg(feature = "polars-engine")]
    fn text_writer_parity_table() -> Table {
        let columns = vec![
            "región, área".to_string(),
            "notes \"q\"".to_string(),
            "amount".to_string(),
            "active".to_string(),
        ];
        let rows = vec![
            Row {
                values: vec![
                    Value::String("West, upper".to_string()),
                    Value::String("said \"hi\"\nsecond line".to_string()),
                    Value::Number(9.007199254740991e15),
                    Value::Bool(true),
                ],
            },
            Row {
                values: vec![
                    Value::String("北区 ❄".to_string()),
                    Value::Null,
                    Value::Number(f64::MIN_POSITIVE / 2.0),
                    Value::Bool(false),
                ],
            },
            Row {
                values: vec![
                    Value::String(String::new()),
                    Value::String("plain".to_string()),
                    Value::Number(-1234.5),
                    Value::Null,
                ],
            },
        ];
        Table::new(columns, rows)
    }

    #[cfg(feature = "polars-engine")]
    fn engine_plan(backend: DataBackend, arrow_bytes: &[u8]) -> DataPlan {
        DataPlan::scan_with_backend(
            DataSource::Bytes {
                logical_path: Path::new("memory.arrows"),
                format: DataFormat::ArrowStream,
                bytes: arrow_bytes,
            },
            backend,
        )
        .expect("scan arrow-stream bytes")
    }

    #[cfg(feature = "polars-engine")]
    fn assert_native_text_writer_parity(table: &Table, format: DataFormat) {
        let arrow_bytes =
            crate::write_table_to_bytes(DataFormat::ArrowStream, table).expect("encode arrow");

        let row_bytes = engine_plan(DataBackend::PortableRows, &arrow_bytes)
            .write_to_sink(DataSink::Bytes { format })
            .expect("row bytes sink")
            .expect("row bytes");
        let native_bytes = engine_plan(DataBackend::NativePolars, &arrow_bytes)
            .write_to_sink(DataSink::Bytes { format })
            .expect("native bytes sink")
            .expect("native bytes");
        assert_eq!(
            String::from_utf8_lossy(&row_bytes),
            String::from_utf8_lossy(&native_bytes),
            "{} bytes-sink output differs between engines",
            format.canonical_name()
        );

        let mut native_writer_bytes = Vec::new();
        engine_plan(DataBackend::NativePolars, &arrow_bytes)
            .write_to_sink(DataSink::Writer {
                format,
                writer: &mut native_writer_bytes,
            })
            .expect("native writer sink");
        assert_eq!(row_bytes, native_writer_bytes);

        static NONCE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let directory = std::env::temp_dir().join(format!(
            "pdl-data-native-writer-{}-{}-{}",
            format.canonical_name(),
            std::process::id(),
            NONCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&directory).expect("create temp dir");
        let path = directory.join("native-output");
        engine_plan(DataBackend::NativePolars, &arrow_bytes)
            .write_to_sink(DataSink::Path {
                path: &path,
                format,
            })
            .expect("native path sink");
        let native_path_bytes = std::fs::read(&path).expect("read native path output");
        std::fs::remove_dir_all(&directory).expect("clean temp dir");
        assert_eq!(row_bytes, native_path_bytes);
    }

    #[test]
    #[cfg(feature = "polars-engine")]
    fn native_csv_writer_matches_row_writer_bytes() {
        assert_native_text_writer_parity(&text_writer_parity_table(), DataFormat::Csv);
    }

    #[test]
    #[cfg(feature = "polars-engine")]
    fn native_json_lines_writer_matches_row_writer_bytes() {
        assert_native_text_writer_parity(&text_writer_parity_table(), DataFormat::JsonLines);
    }

    /// v0.46 byte-backed scan parity: scanning the same in-memory bytes on
    /// both engines must produce byte-identical CSV output. This is the
    /// unit-level corpus behind the stdin / host-byte source promotions.
    #[cfg(feature = "polars-engine")]
    fn assert_byte_scan_parity(name: &str, format: DataFormat, bytes: &[u8]) {
        let scan = |backend| {
            DataPlan::scan_with_backend(
                DataSource::Bytes {
                    logical_path: Path::new(name),
                    format,
                    bytes,
                },
                backend,
            )
            .unwrap_or_else(|error| panic!("{name}: byte-backed scan failed: {error:?}"))
        };
        let csv_bytes = |plan: DataPlan| {
            plan.write_to_sink(DataSink::Bytes {
                format: DataFormat::Csv,
            })
            .expect("csv bytes sink")
            .expect("csv bytes")
        };
        let row = csv_bytes(scan(DataBackend::PortableRows));
        let native = csv_bytes(scan(DataBackend::NativePolars));
        assert_eq!(
            String::from_utf8_lossy(&row),
            String::from_utf8_lossy(&native),
            "{name}: byte-backed scan output differs between engines"
        );
    }

    #[test]
    #[cfg(feature = "polars-engine")]
    fn native_byte_backed_csv_scan_matches_rows_for_empty_input() {
        assert_byte_scan_parity("empty.csv", DataFormat::Csv, b"");
    }

    #[test]
    #[cfg(feature = "polars-engine")]
    fn native_byte_backed_csv_scan_matches_rows_for_header_only_input() {
        assert_byte_scan_parity(
            "header_only.csv",
            DataFormat::Csv,
            b"order_id,region,amount\n",
        );
    }

    #[test]
    #[cfg(feature = "polars-engine")]
    fn native_byte_backed_csv_scan_matches_rows_for_tricky_cells() {
        // Embedded delimiters, quotes, and newlines; multibyte UTF-8 in
        // headers and cells; empty-cell nulls; boolean-shaped strings; and
        // numeric edge values (int53 boundary, f64 subnormal, exponent
        // notation, negative fraction).
        let csv = "\"regi\u{f3}n, \u{e1}rea\",\"notes \"\"q\"\"\",amount,flag\n\
                   \"West, upper\",\"said \"\"hi\"\"\nsecond line\",9007199254740991,true\n\
                   \u{5317}\u{533a} \u{2744},,5e-324,false\n\
                   ,plain,-1234.5,\n\
                   plain too,\u{2744},1e3,true\n";
        assert_byte_scan_parity("tricky.csv", DataFormat::Csv, csv.as_bytes());
    }

    #[test]
    #[cfg(feature = "polars-engine")]
    fn native_byte_backed_csv_scan_matches_rows_for_large_input() {
        // Large enough to cross internal reader buffer boundaries and the
        // default native schema-inference window.
        let mut csv = String::from("id,segment,value\n");
        for id in 0..50_000 {
            csv.push_str(&format!("{id},seg{},{}.5\n", id % 7, id));
        }
        assert_byte_scan_parity("large.csv", DataFormat::Csv, csv.as_bytes());
    }

    #[test]
    #[cfg(all(feature = "polars-engine", feature = "parquet"))]
    fn native_byte_backed_parquet_scan_matches_rows_for_empty_table() {
        let empty = Table::new(vec!["region".to_string(), "amount".to_string()], vec![]);
        let bytes =
            crate::write_table_to_bytes(DataFormat::Parquet, &empty).expect("encode parquet");
        assert_byte_scan_parity("empty.parquet", DataFormat::Parquet, &bytes);
    }

    #[test]
    #[cfg(all(feature = "polars-engine", feature = "parquet"))]
    fn native_byte_backed_parquet_scan_matches_rows_for_nullable_columns() {
        let bytes = crate::write_table_to_bytes(DataFormat::Parquet, &text_writer_parity_table())
            .expect("encode parquet");
        assert_byte_scan_parity("nullable.parquet", DataFormat::Parquet, &bytes);
    }

    #[test]
    #[cfg(all(feature = "polars-engine", feature = "parquet"))]
    fn native_byte_backed_parquet_scan_matches_rows_for_multi_row_group_file() {
        let rows = (0..512)
            .map(|id| Row {
                values: vec![
                    Value::Number(f64::from(id)),
                    Value::String(format!("seg{}", id % 5)),
                ],
            })
            .collect();
        let table = Table::new(vec!["id".to_string(), "segment".to_string()], rows);
        let batch = crate::arrow::table_to_batch(&table).expect("arrow batch");
        let properties = ::parquet::file::properties::WriterProperties::builder()
            .set_max_row_group_size(64)
            .build();
        let mut bytes = Vec::new();
        let mut writer =
            ::parquet::arrow::ArrowWriter::try_new(&mut bytes, batch.schema(), Some(properties))
                .expect("parquet writer");
        writer.write(&batch).expect("write batch");
        writer.close().expect("close writer");
        assert_byte_scan_parity("multi_row_group.parquet", DataFormat::Parquet, &bytes);
    }

    #[cfg(feature = "polars-engine")]
    fn native_csv_bytes(plan: DataPlan) -> String {
        let bytes = plan
            .write_to_sink(DataSink::Bytes {
                format: DataFormat::Csv,
            })
            .expect("csv bytes sink")
            .expect("csv bytes");
        String::from_utf8(bytes).expect("utf8 csv")
    }

    #[cfg(feature = "polars-engine")]
    fn native_plan_from_table(table: &Table) -> DataPlan {
        let arrow_bytes =
            crate::write_table_to_bytes(DataFormat::ArrowStream, table).expect("encode arrow");
        engine_plan(DataBackend::NativePolars, &arrow_bytes)
    }

    /// Row-runtime `pivot_longer` order is input-row major with stage column
    /// order inside each input row; kept columns keep table order ahead of
    /// `names_to`/`values_to`.
    #[test]
    #[cfg(feature = "polars-engine")]
    fn native_pivot_longer_matches_row_runtime_order() {
        let table = Table::new(
            vec![
                "region".to_string(),
                "q1".to_string(),
                "q2".to_string(),
                "year".to_string(),
            ],
            vec![
                Row {
                    values: vec![
                        Value::String("West".to_string()),
                        Value::Number(10.0),
                        Value::Number(20.0),
                        Value::Number(2026.0),
                    ],
                },
                Row {
                    values: vec![
                        Value::String("East".to_string()),
                        Value::Number(5.0),
                        Value::Null,
                        Value::Number(2026.0),
                    ],
                },
            ],
        );
        let plan = native_plan_from_table(&table)
            .pivot_longer(&["q2".to_string(), "q1".to_string()], "quarter", "amount")
            .expect("native pivot_longer");
        assert_eq!(
            native_csv_bytes(plan),
            "region,year,quarter,amount\n\
             West,2026,q2,20\n\
             West,2026,q1,10\n\
             East,2026,q2,\n\
             East,2026,q1,5\n"
        );
    }

    #[test]
    #[cfg(feature = "polars-engine")]
    fn native_pivot_longer_handles_empty_and_all_null_input() {
        let empty = Table::new(
            vec!["region".to_string(), "q1".to_string(), "q2".to_string()],
            Vec::new(),
        );
        let plan = native_plan_from_table(&empty)
            .pivot_longer(&["q1".to_string(), "q2".to_string()], "quarter", "amount")
            .expect("native pivot_longer on empty input");
        assert_eq!(native_csv_bytes(plan), "region,quarter,amount\n");

        let all_null = Table::new(
            vec!["region".to_string(), "q1".to_string()],
            vec![Row {
                values: vec![Value::String("West".to_string()), Value::Null],
            }],
        );
        let plan = native_plan_from_table(&all_null)
            .pivot_longer(&["q1".to_string()], "quarter", "amount")
            .expect("native pivot_longer on all-null column");
        assert_eq!(native_csv_bytes(plan), "region,quarter,amount\nWest,q1,\n");
    }

    /// Mixed value classes cannot keep row-runtime per-cell typing on a
    /// typed engine; the lowering must refuse so automatic mode falls back.
    #[test]
    #[cfg(feature = "polars-engine")]
    fn native_pivot_longer_rejects_mixed_class_value_columns() {
        let table = Table::new(
            vec!["id".to_string(), "label".to_string(), "amount".to_string()],
            vec![Row {
                values: vec![
                    Value::Number(1.0),
                    Value::String("a".to_string()),
                    Value::Number(2.0),
                ],
            }],
        );
        let error = native_plan_from_table(&table)
            .pivot_longer(
                &["label".to_string(), "amount".to_string()],
                "name",
                "value",
            )
            .err()
            .expect("mixed-class pivot must stay row-only");
        assert_eq!(error.code, "E1211");
        assert!(error.message.contains("mixed-class"), "{}", error.message);
    }

    /// Row-runtime `complete` order is the Cartesian product of
    /// first-appearance key domains, outer key first.
    #[test]
    #[cfg(feature = "polars-engine")]
    fn native_complete_matches_row_runtime_order_and_fills() {
        let table = Table::new(
            vec![
                "region".to_string(),
                "day".to_string(),
                "visits".to_string(),
                "note".to_string(),
            ],
            vec![
                Row {
                    values: vec![
                        Value::String("West".to_string()),
                        Value::String("mon".to_string()),
                        Value::Number(12.0),
                        Value::String("ok".to_string()),
                    ],
                },
                Row {
                    values: vec![
                        Value::String("East".to_string()),
                        Value::String("tue".to_string()),
                        Value::Number(4.0),
                        Value::String("ok".to_string()),
                    ],
                },
            ],
        );
        let plan = native_plan_from_table(&table)
            .complete(
                &["region".to_string(), "day".to_string()],
                &[(
                    "visits".to_string(),
                    DataExpr::Literal(DataLiteral::Number(0.0)),
                )],
            )
            .expect("native complete");
        assert_eq!(
            native_csv_bytes(plan),
            "region,day,visits,note\n\
             West,mon,12,ok\n\
             West,tue,0,\n\
             East,mon,0,\n\
             East,tue,4,ok\n"
        );
    }

    #[test]
    #[cfg(feature = "polars-engine")]
    fn native_complete_handles_empty_input() {
        let empty = Table::new(vec!["region".to_string(), "visits".to_string()], Vec::new());
        let plan = native_plan_from_table(&empty)
            .complete(&["region".to_string()], &[])
            .expect("native complete on empty input");
        assert_eq!(native_csv_bytes(plan), "region,visits\n");
    }

    #[test]
    #[cfg(feature = "polars-engine")]
    fn native_complete_rejects_duplicate_key_tuples() {
        let table = Table::new(
            vec!["region".to_string(), "visits".to_string()],
            vec![
                Row {
                    values: vec![Value::String("West".to_string()), Value::Number(1.0)],
                },
                Row {
                    values: vec![Value::String("West".to_string()), Value::Number(2.0)],
                },
            ],
        );
        let error = native_plan_from_table(&table)
            .complete(&["region".to_string()], &[])
            .err()
            .expect("duplicate key tuples must fail");
        assert_eq!(error.code, "E1208");
    }

    /// A fill that changes the column's value class would re-render existing
    /// values on a typed engine; the lowering must refuse so automatic mode
    /// falls back.
    #[test]
    #[cfg(feature = "polars-engine")]
    fn native_complete_rejects_class_changing_fill() {
        let table = Table::new(
            vec![
                "region".to_string(),
                "day".to_string(),
                "visits".to_string(),
            ],
            vec![
                Row {
                    values: vec![
                        Value::String("West".to_string()),
                        Value::String("mon".to_string()),
                        Value::Number(12.0),
                    ],
                },
                Row {
                    values: vec![
                        Value::String("East".to_string()),
                        Value::String("tue".to_string()),
                        Value::Number(4.0),
                    ],
                },
            ],
        );
        let error = native_plan_from_table(&table)
            .complete(
                &["region".to_string(), "day".to_string()],
                &[(
                    "visits".to_string(),
                    DataExpr::Literal(DataLiteral::String("none".to_string())),
                )],
            )
            .err()
            .expect("class-changing fill must stay row-only");
        assert_eq!(error.code, "E1211");
        assert!(
            error.message.contains("class-changing"),
            "{}",
            error.message
        );
    }

    #[test]
    #[cfg(feature = "polars-engine")]
    fn native_text_writers_match_row_writer_bytes_for_empty_input() {
        let empty = Table::new(vec!["región".to_string(), "amount".to_string()], Vec::new());
        assert_native_text_writer_parity(&empty, DataFormat::Csv);
        assert_native_text_writer_parity(&empty, DataFormat::JsonLines);
    }
}
