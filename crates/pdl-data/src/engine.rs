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
                let window_presort = native_window_presort_specs(items)?;
                let native_plan = if let Some(name) = &row_index_name {
                    plan.plan.with_row_index(name.clone(), None)
                } else {
                    plan.plan
                };
                let native_plan = if let Some(specs) = &window_presort {
                    native_plan.sort(
                        specs
                            .iter()
                            .map(|spec| spec.column.clone())
                            .collect::<Vec<_>>(),
                        native_sort_multiple_options(specs),
                    )
                } else {
                    native_plan
                };
                let expressions = items
                    .iter()
                    .map(|(column, expr)| {
                        let expr = row_index_name
                            .as_deref()
                            .map(|name| data_expr_with_window_row_index(expr, name))
                            .unwrap_or_else(|| expr.clone());
                        let expr = if window_presort.is_some() {
                            data_expr_with_presorted_multi_key_windows(&expr)
                        } else {
                            expr
                        };
                        Ok(native_expr(&expr)?.alias(column))
                    })
                    .collect::<Result<Vec<_>, Diagnostic>>()?;
                let native_plan = native_plan.with_columns(expressions);
                let native_plan = if let Some(name) = &row_index_name {
                    let native_plan = if window_presort.is_some() {
                        native_plan.sort(
                            [name.as_str()],
                            native::SortMultipleOptions {
                                descending: vec![false],
                                nulls_last: vec![false],
                                maintain_order: true,
                                ..Default::default()
                            },
                        )
                    } else {
                        native_plan
                    };
                    native_plan.drop(native::cols([name.as_str()]))
                } else {
                    native_plan
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
fn native_window_presort_specs(
    items: &[(String, DataExpr)],
) -> Result<Option<Vec<SortSpec>>, Diagnostic> {
    let mut group = None;
    for (_, expr) in items {
        data_expr_collect_multi_key_window_sort(expr, &mut group)?;
    }
    let Some(group) = group else {
        return Ok(None);
    };
    let mut specs = group
        .partition_by
        .iter()
        .map(|column| SortSpec {
            column: column.clone(),
            direction: SortDirection::Asc,
            nulls: NullsOrder::Last,
        })
        .collect::<Vec<_>>();
    specs.extend(group.order_by);
    Ok(Some(specs))
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
                DataFormat::ArrowFile => {
                    let file = std::fs::File::open(path).map_err(|error| {
                        Diagnostic::error(
                            codes::E1802,
                            format!("could not read data file `{}`: {error}", path.display()),
                            Span::zero(),
                        )
                    })?;
                    native::IpcReader::new(file)
                        .finish()
                        .map_err(native_read_error(path, format))?
                        .lazy()
                }
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
                DataFormat::ArrowStream => native::IpcStreamReader::new(Cursor::new(bytes))
                    .finish()
                    .map_err(native_read_error(logical_path, format))?
                    .lazy(),
                DataFormat::ArrowFile => native::IpcReader::new(Cursor::new(bytes))
                    .finish()
                    .map_err(native_read_error(logical_path, format))?
                    .lazy(),
                DataFormat::Csv | DataFormat::Parquet | DataFormat::JsonLines => {
                    return Err(unsupported_native_operation("byte-backed input"));
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
            if native_direct_writer_format(format) {
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
    if !native_direct_writer_format(format) {
        let table = native_collect_to_table(plan)?;
        let bytes = write_table_to_bytes(format, &table)?;
        writer.write_all(&bytes).map_err(output_write_error)?;
        return Ok(());
    }

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
        DataFormat::Csv | DataFormat::JsonLines => unreachable!("handled by row-format fallback"),
    }
}

#[cfg(feature = "polars-engine")]
fn native_direct_writer_format(format: DataFormat) -> bool {
    matches!(
        format,
        DataFormat::Parquet | DataFormat::ArrowFile | DataFormat::ArrowStream
    )
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
                DataScalarFunction::Coalesce
                | DataScalarFunction::Concat
                | DataScalarFunction::Contains
                | DataScalarFunction::StartsWith
                | DataScalarFunction::Replace
                | DataScalarFunction::IfElse => unreachable!(),
            }
        }
    }
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
                let [arg] = args.as_slice() else {
                    return Err(unsupported_native_operation("scalar function arity"));
                };
                let arg = native_expr(arg)?;
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
                    DataScalarFunction::ToString => arg.cast(native::DataType::String),
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
                        arg.round(*digits, native::RoundMode::HalfAwayFromZero)
                    }
                    DataScalarFunction::Coalesce
                    | DataScalarFunction::Concat
                    | DataScalarFunction::Contains
                    | DataScalarFunction::StartsWith
                    | DataScalarFunction::Replace
                    | DataScalarFunction::IfElse => {
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
    if !first && spec.frame == DataWindowFrame::UnboundedPrecedingToCurrentRow {
        return native_expr(arg);
    }
    let expr = if first {
        native_expr(arg)?.first()
    } else {
        native_expr(arg)?.last()
    };
    native_window_over(expr, spec, true)
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
}
