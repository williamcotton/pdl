use pdl_core::{codes, Diagnostic, Span};
use pdl_data::{
    compare_values, read_table_from_bytes, sniff_format_from_bytes, DataAggItem, DataBackend,
    DataBinaryOp, DataExpr, DataFormat, DataLiteral, DataPlan, DataScalarFunction, DataSink,
    DataSource, DataUnaryOp, NullsOrder as DataNullsOrder, Row, SortDirection as DataSortDirection,
    SortSpec, Table, Value,
};
use pdl_driver::{
    DriverIo, FormatDecision, OsDriverIo, PlanInputSource, PlanOutputSink, PreparedProgram,
    SinkDescriptor, SourceDescriptor,
};
use pdl_semantics::{
    decode_context_column_ref_ir, AggItemIr, BinaryOpIr, CompleteFillItemIr, ContextKindIr, ExprIr,
    FrameBoundIr, JoinKindIr, MutateItemIr, NullsOrderIr, PipelineIr, PipelineStartIr,
    SortDirectionIr, StageIr, UnaryOpIr, WindowFrameIr, WindowSpecIr,
};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use crate::output::{emit_stdout, write_output};
use crate::planning::{plan_prepared, ExecutionPlan, PlanningOptions};

#[derive(Clone, Debug)]
pub struct RunOptions {
    pub stdout_format: Option<String>,
    pub dry_run: bool,
    pub allow_binary_stdout: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ExecutionEngine {
    #[default]
    Auto,
    Row,
    Native,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            stdout_format: None,
            dry_run: false,
            allow_binary_stdout: true,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct RunResult {
    pub stdout: Option<Vec<u8>>,
    pub named_outputs: Vec<NamedOutput>,
    pub diagnostics: Vec<Diagnostic>,
    pub backend: DataBackend,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NamedOutput {
    pub name: String,
    pub table: Table,
}

pub fn run_prepared(prepared: &PreparedProgram, options: RunOptions) -> RunResult {
    let io = OsDriverIo;
    run_prepared_with_io(prepared, options, &io)
}

pub fn run_prepared_with_io(
    prepared: &PreparedProgram,
    options: RunOptions,
    io: &dyn DriverIo,
) -> RunResult {
    run_prepared_with_io_and_context(prepared, options, io, BTreeMap::new())
}

pub fn run_prepared_with_engine(
    prepared: &PreparedProgram,
    options: RunOptions,
    engine: ExecutionEngine,
) -> RunResult {
    let io = OsDriverIo;
    run_prepared_with_io_and_context_and_engine(prepared, options, &io, BTreeMap::new(), engine)
}

pub fn run_prepared_with_io_and_context(
    prepared: &PreparedProgram,
    options: RunOptions,
    io: &dyn DriverIo,
    context: BTreeMap<String, Value>,
) -> RunResult {
    run_prepared_with_io_and_context_and_engine(
        prepared,
        options,
        io,
        context,
        ExecutionEngine::Auto,
    )
}

pub fn run_prepared_with_io_and_context_and_engine(
    prepared: &PreparedProgram,
    options: RunOptions,
    io: &dyn DriverIo,
    context: BTreeMap<String, Value>,
    engine: ExecutionEngine,
) -> RunResult {
    let plan = match plan_prepared(
        prepared,
        PlanningOptions {
            stdout_format: options.stdout_format.clone(),
            dry_run: options.dry_run,
            allow_binary_stdout: options.allow_binary_stdout,
        },
    ) {
        Ok(plan) => plan,
        Err(diagnostics) => {
            return RunResult {
                stdout: None,
                named_outputs: Vec::new(),
                diagnostics,
                backend: DataBackend::PortableRows,
            };
        }
    };

    let Some(ir) = prepared.analysis.ir.as_ref() else {
        let mut diagnostics = prepared.diagnostics();
        diagnostics.push(Diagnostic::error(
            codes::E1505,
            "semantic IR is unavailable for execution",
            Span::zero(),
        ));
        return RunResult {
            stdout: None,
            named_outputs: Vec::new(),
            diagnostics,
            backend: DataBackend::PortableRows,
        };
    };
    let mut diagnostics = prepared.diagnostics();
    let context = build_context_values(ir, context, &mut diagnostics);
    if diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == pdl_core::Severity::Error)
    {
        return RunResult {
            stdout: None,
            named_outputs: Vec::new(),
            diagnostics,
            backend: DataBackend::PortableRows,
        };
    }

    if !matches!(engine, ExecutionEngine::Row) {
        match try_execute_native(prepared, ir, &plan, &context) {
            Ok(result) => return result,
            Err(diagnostic) if matches!(engine, ExecutionEngine::Native) => {
                return RunResult {
                    stdout: None,
                    named_outputs: Vec::new(),
                    diagnostics: {
                        let mut diagnostics = prepared.diagnostics();
                        diagnostics.push(diagnostic);
                        diagnostics
                    },
                    backend: DataBackend::NativePolars,
                };
            }
            Err(_) => {}
        }
    }

    let mut runtime = Runtime {
        prepared,
        diagnostics,
        cache: BTreeMap::new(),
        active_bindings: Vec::new(),
        context,
        dry_run: plan.dry_run,
        stdout: None,
        io,
    };

    let mut named_outputs = Vec::new();
    let final_table = if ir.outputs.is_empty() {
        let Some(main) = &ir.main else {
            runtime.diagnostics.push(Diagnostic::error(
                codes::E1502,
                "no runnable main pipeline",
                Span::zero(),
            ));
            return RunResult {
                stdout: None,
                named_outputs,
                diagnostics: runtime.diagnostics,
                backend: DataBackend::PortableRows,
            };
        };
        match runtime.execute_pipeline(main) {
            Ok(table) => Some(table),
            Err(diagnostic) => {
                runtime.diagnostics.push(diagnostic);
                return RunResult {
                    stdout: None,
                    named_outputs,
                    diagnostics: runtime.diagnostics,
                    backend: DataBackend::PortableRows,
                };
            }
        }
    } else {
        let mut last = None;
        for output in &ir.outputs {
            match runtime.execute_pipeline(&output.pipeline) {
                Ok(table) => {
                    last = Some(table.clone());
                    named_outputs.push(NamedOutput {
                        name: output.name.clone(),
                        table,
                    });
                }
                Err(diagnostic) => {
                    runtime.diagnostics.push(diagnostic);
                    return RunResult {
                        stdout: None,
                        named_outputs,
                        diagnostics: runtime.diagnostics,
                        backend: DataBackend::PortableRows,
                    };
                }
            }
        }
        last
    };

    let stdout = if let Some(format) = plan.stdout_format {
        final_table
            .as_ref()
            .and_then(|table| match emit_stdout(format, table) {
                Ok(bytes) => Some(bytes),
                Err(diagnostic) => {
                    runtime.diagnostics.push(diagnostic);
                    None
                }
            })
    } else {
        runtime.stdout.take()
    };

    RunResult {
        stdout,
        named_outputs,
        diagnostics: runtime.diagnostics,
        backend: DataBackend::PortableRows,
    }
}

fn build_context_values(
    ir: &pdl_semantics::ProgramIr,
    mut overrides: BTreeMap<String, Value>,
    diagnostics: &mut Vec<Diagnostic>,
) -> BTreeMap<String, Value> {
    let mut values = BTreeMap::new();
    for context in &ir.contexts {
        let default = literal_ir_value(&context.default).unwrap_or(Value::Null);
        let value = match overrides.remove(&context.name) {
            Some(value) => {
                if context_value_type_matches(&default, &value) {
                    value
                } else {
                    diagnostics.push(Diagnostic::error(
                        codes::E2005,
                        format!(
                            "external value for {} `{}` has the wrong type",
                            context_kind_label(context.kind),
                            context.name
                        ),
                        context.span,
                    ));
                    default.clone()
                }
            }
            None => default.clone(),
        };
        values.insert(context.name.clone(), value);
    }
    for name in overrides.into_keys() {
        diagnostics.push(Diagnostic::error(
            codes::E2002,
            format!("unknown context value `{name}`"),
            Span::zero(),
        ));
    }
    values
}

fn literal_ir_value(expr: &ExprIr) -> Option<Value> {
    match expr {
        ExprIr::Quoted { value, .. } => Some(Value::String(value.clone())),
        ExprIr::Number { value, .. } => Some(Value::Number(*value)),
        ExprIr::Bool { value, .. } => Some(Value::Bool(*value)),
        ExprIr::Null { .. } => Some(Value::Null),
        ExprIr::Ident { .. }
        | ExprIr::Context { .. }
        | ExprIr::Call { .. }
        | ExprIr::Window { .. }
        | ExprIr::Unary { .. }
        | ExprIr::Binary { .. } => None,
    }
}

fn context_value_type_matches(default: &Value, value: &Value) -> bool {
    matches!(
        (default, value),
        (Value::Null, Value::Null)
            | (Value::Bool(_), Value::Bool(_))
            | (Value::Number(_), Value::Number(_))
            | (Value::String(_), Value::String(_))
    )
}

fn context_kind_label(kind: ContextKindIr) -> &'static str {
    match kind {
        ContextKindIr::Param => "parameter",
        ContextKindIr::State => "state",
    }
}

fn try_execute_native(
    prepared: &PreparedProgram,
    ir: &pdl_semantics::ProgramIr,
    plan: &ExecutionPlan,
    context: &BTreeMap<String, Value>,
) -> Result<RunResult, Diagnostic> {
    check_native_program_eligibility(prepared, ir, plan, context)?;
    let main = ir
        .main
        .as_ref()
        .ok_or_else(|| unsupported_native_pipeline("no runnable main pipeline"))?;
    let stdout = match execute_native_pipeline(prepared, main, plan, context)? {
        NativePipelineResult::Plan(data_plan) => {
            if let Some(stdout_format) = plan.stdout_format {
                data_plan
                    .write_to_sink(DataSink::Bytes {
                        format: stdout_format,
                    })?
                    .ok_or_else(|| {
                        unsupported_native_pipeline("native stdout bytes were not returned")
                    })?
                    .into()
            } else {
                None
            }
        }
        NativePipelineResult::Completed { stdout } => stdout,
    };
    Ok(RunResult {
        stdout,
        named_outputs: Vec::new(),
        diagnostics: prepared.diagnostics(),
        backend: DataBackend::NativePolars,
    })
}

fn check_native_program_eligibility(
    prepared: &PreparedProgram,
    ir: &pdl_semantics::ProgramIr,
    plan: &ExecutionPlan,
    context: &BTreeMap<String, Value>,
) -> Result<(), Diagnostic> {
    if !ir.outputs.is_empty() {
        return Err(unsupported_native_pipeline(
            "native execution for named outputs is deferred",
        ));
    }
    let main = ir
        .main
        .as_ref()
        .ok_or_else(|| unsupported_native_pipeline("no runnable main pipeline"))?;
    check_native_pipeline_eligibility(prepared, main, plan, context)
}

fn check_native_pipeline_eligibility(
    prepared: &PreparedProgram,
    pipeline: &PipelineIr,
    execution_plan: &ExecutionPlan,
    context: &BTreeMap<String, Value>,
) -> Result<(), Diagnostic> {
    match &pipeline.start {
        PipelineStartIr::Load { format, span, .. } => {
            check_native_load_eligibility(prepared, *span, format.as_deref())?;
        }
        PipelineStartIr::Binding { .. } => {
            return Err(unsupported_native_pipeline(
                "native execution from bindings is deferred",
            ));
        }
    }

    for (stage_index, stage) in pipeline.stages.iter().enumerate() {
        let is_terminal = stage_index + 1 == pipeline.stages.len();
        match stage {
            StageIr::Filter { expr, .. } => {
                lower_data_expr(expr, context)?;
            }
            StageIr::Select { items, .. } => {
                for item in items {
                    resolve_native_column_name(&item.source, item.span, context)?;
                    resolve_native_column_name(&item.output, item.span, context)?;
                }
            }
            StageIr::Drop { columns, span } | StageIr::Distinct { columns, span } => {
                resolve_native_column_names(columns, *span, context)?;
            }
            StageIr::Rename { items, .. } => {
                for item in items {
                    resolve_native_column_name(&item.old, item.span, context)?;
                    resolve_native_column_name(&item.new, item.span, context)?;
                }
            }
            StageIr::GroupBy { columns, span } => {
                resolve_native_column_names(columns, *span, context)?;
            }
            StageIr::Agg { items, .. } => {
                lower_data_agg_items(items, context)?;
            }
            StageIr::Sort { items, .. } => {
                for item in items {
                    resolve_native_column_name(&item.column, item.span, context)?;
                }
            }
            StageIr::Limit { .. } => {}
            StageIr::Save { format, span, .. } => {
                if !is_terminal {
                    return Err(unsupported_native_pipeline(
                        "native save stages are supported only as terminal stages",
                    ));
                }
                check_native_save_eligibility(prepared, execution_plan, *span, format.as_deref())?;
            }
            StageIr::Mutate { .. }
            | StageIr::Join { .. }
            | StageIr::Union { .. }
            | StageIr::PivotLonger { .. }
            | StageIr::Complete { .. }
            | StageIr::Unsupported { .. } => {
                return Err(unsupported_native_pipeline(
                    "pipeline stage is not supported by native execution",
                ));
            }
        }
    }
    Ok(())
}

fn check_native_load_eligibility(
    prepared: &PreparedProgram,
    stage_span: Span,
    explicit_format: Option<&str>,
) -> Result<(), Diagnostic> {
    let Some(input) = prepared.driver_plan.input_for_stage_span(stage_span) else {
        return Err(Diagnostic::error(
            codes::E1505,
            "driver source facts are unavailable for native execution",
            stage_span,
        ));
    };
    if !matches!(input.source, SourceDescriptor::Path { .. }) {
        return Err(unsupported_native_pipeline(
            "native execution requires a path-backed input",
        ));
    }
    let format = resolve_input_format(input, explicit_format, None, None, stage_span)?;
    if !matches!(format, DataFormat::Csv | DataFormat::Parquet) {
        return Err(unsupported_native_pipeline(
            "input format is not supported by native execution",
        ));
    }
    Ok(())
}

fn check_native_save_eligibility(
    prepared: &PreparedProgram,
    _execution_plan: &ExecutionPlan,
    stage_span: Span,
    explicit_format: Option<&str>,
) -> Result<(), Diagnostic> {
    let Some(sink) = prepared.driver_plan.sink_for_stage_span(stage_span) else {
        return Err(Diagnostic::error(
            codes::E1505,
            "driver sink facts are unavailable for native execution",
            stage_span,
        ));
    };
    resolve_output_format(sink, explicit_format, stage_span)?;
    Ok(())
}

enum NativePipelineResult {
    Plan(DataPlan),
    Completed { stdout: Option<Vec<u8>> },
}

fn execute_native_pipeline(
    prepared: &PreparedProgram,
    pipeline: &PipelineIr,
    execution_plan: &ExecutionPlan,
    context: &BTreeMap<String, Value>,
) -> Result<NativePipelineResult, Diagnostic> {
    let mut plan = match &pipeline.start {
        PipelineStartIr::Load { format, span, .. } => {
            native_load_plan(prepared, *span, format.as_deref())?
        }
        PipelineStartIr::Binding { .. } => {
            return Err(unsupported_native_pipeline(
                "native execution from bindings is deferred",
            ));
        }
    };
    let mut grouping: Option<Vec<String>> = None;

    for (stage_index, stage) in pipeline.stages.iter().enumerate() {
        let is_terminal = stage_index + 1 == pipeline.stages.len();
        plan = match stage {
            StageIr::Filter { expr, .. } => {
                grouping = None;
                plan.filter(lower_data_expr(expr, context)?)?
            }
            StageIr::Select { items, .. } => {
                grouping = None;
                let selection = items
                    .iter()
                    .map(|item| {
                        Ok((
                            resolve_native_column_name(&item.source, item.span, context)?,
                            resolve_native_column_name(&item.output, item.span, context)?,
                        ))
                    })
                    .collect::<Result<Vec<_>, Diagnostic>>()?;
                plan.select(&selection)?
            }
            StageIr::Drop { columns, span } => {
                grouping = None;
                let columns = resolve_native_column_names(columns, *span, context)?;
                plan.drop_columns(&columns)?
            }
            StageIr::Rename { items, .. } => {
                grouping = None;
                let renames = items
                    .iter()
                    .map(|item| {
                        Ok((
                            resolve_native_column_name(&item.old, item.span, context)?,
                            resolve_native_column_name(&item.new, item.span, context)?,
                        ))
                    })
                    .collect::<Result<Vec<_>, Diagnostic>>()?;
                plan.rename_columns(&renames)?
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
                        Ok(SortSpec {
                            column: resolve_native_column_name(&item.column, item.span, context)?,
                            direction,
                            nulls,
                        })
                    })
                    .collect::<Result<Vec<_>, Diagnostic>>()?;
                plan.sort(&specs)?
            }
            StageIr::Limit { n, .. } => plan.limit(*n)?,
            StageIr::Distinct { columns, span } => {
                grouping = None;
                let columns = resolve_native_column_names(columns, *span, context)?;
                plan.distinct(&columns)?
            }
            StageIr::GroupBy { columns, span } => {
                grouping = Some(resolve_native_column_names(columns, *span, context)?);
                plan
            }
            StageIr::Agg { items, .. } => {
                let items = lower_data_agg_items(items, context)?;
                plan.aggregate(&grouping.take().unwrap_or_default(), &items)?
            }
            StageIr::Save { format, span, .. } => {
                if !is_terminal {
                    return Err(unsupported_native_pipeline(
                        "native save stages are supported only as terminal stages",
                    ));
                }
                return execute_native_save(
                    prepared,
                    execution_plan,
                    plan,
                    *span,
                    format.as_deref(),
                );
            }
            StageIr::Mutate { .. }
            | StageIr::Join { .. }
            | StageIr::Union { .. }
            | StageIr::PivotLonger { .. }
            | StageIr::Complete { .. }
            | StageIr::Unsupported { .. } => {
                return Err(unsupported_native_pipeline(
                    "pipeline stage is not supported by native execution",
                ));
            }
        };
    }
    Ok(NativePipelineResult::Plan(plan))
}

fn execute_native_save(
    prepared: &PreparedProgram,
    execution_plan: &ExecutionPlan,
    plan: DataPlan,
    stage_span: Span,
    explicit_format: Option<&str>,
) -> Result<NativePipelineResult, Diagnostic> {
    if execution_plan.dry_run {
        return Ok(NativePipelineResult::Completed { stdout: None });
    }
    let Some(sink) = prepared.driver_plan.sink_for_stage_span(stage_span) else {
        return Err(Diagnostic::error(
            codes::E1505,
            "driver sink facts are unavailable for native execution",
            stage_span,
        ));
    };
    let format = resolve_output_format(sink, explicit_format, stage_span)?;
    match &sink.sink {
        SinkDescriptor::Stdout => {
            let stdout = plan
                .write_to_sink(DataSink::Bytes { format })?
                .ok_or_else(|| {
                    unsupported_native_pipeline("native stdout bytes were not returned")
                })?;
            Ok(NativePipelineResult::Completed {
                stdout: Some(stdout),
            })
        }
        SinkDescriptor::Path { resolved_path, .. } => {
            plan.write_to_sink(DataSink::Path {
                path: resolved_path,
                format,
            })?;
            Ok(NativePipelineResult::Completed { stdout: None })
        }
    }
}

fn native_load_plan(
    prepared: &PreparedProgram,
    stage_span: Span,
    explicit_format: Option<&str>,
) -> Result<DataPlan, Diagnostic> {
    let Some(input) = prepared.driver_plan.input_for_stage_span(stage_span) else {
        return Err(Diagnostic::error(
            codes::E1505,
            "driver source facts are unavailable for native execution",
            stage_span,
        ));
    };
    let SourceDescriptor::Path { resolved_path, .. } = &input.source else {
        return Err(unsupported_native_pipeline(
            "native execution requires a path-backed input",
        ));
    };
    if !resolved_path.exists() {
        return Err(unsupported_native_pipeline(
            "native execution requires a real filesystem path",
        ));
    }
    let format = resolve_input_format(input, explicit_format, None, None, stage_span)?;
    DataPlan::scan_with_backend(
        DataSource::Path {
            path: resolved_path,
            format,
        },
        DataBackend::NativePolars,
    )
}

fn resolve_native_column_names(
    columns: &[String],
    span: Span,
    context: &BTreeMap<String, Value>,
) -> Result<Vec<String>, Diagnostic> {
    columns
        .iter()
        .map(|column| resolve_native_column_name(column, span, context))
        .collect()
}

fn resolve_native_column_name(
    column: &str,
    span: Span,
    context: &BTreeMap<String, Value>,
) -> Result<String, Diagnostic> {
    let Some((kind, name)) = decode_context_column_ref_ir(column) else {
        return Ok(column.to_string());
    };
    let Some(value) = context.get(name) else {
        return Err(Diagnostic::error(
            codes::E2002,
            format!("unknown {} `{name}`", context_kind_label(kind)),
            span,
        ));
    };
    match value {
        Value::String(value) => Ok(value.clone()),
        _ => Err(Diagnostic::error(
            codes::E2004,
            format!("context value `{name}` must be a string to resolve a column name"),
            span,
        )),
    }
}

fn lower_data_expr(
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
        ExprIr::Window { .. } => Err(unsupported_native_pipeline(
            "window expressions are not supported by native execution",
        )),
    }
}

fn lower_data_agg_items(
    items: &[AggItemIr],
    context: &BTreeMap<String, Value>,
) -> Result<Vec<DataAggItem>, Diagnostic> {
    items
        .iter()
        .map(|item| {
            let args = match item.function.as_str() {
                "count" if item.args.is_empty() => Vec::new(),
                "count" | "sum" | "mean" | "min" | "max" => {
                    let [arg] = item.args.as_slice() else {
                        return Err(unsupported_native_pipeline(
                            "aggregate arity is not supported by native execution",
                        ));
                    };
                    vec![lower_data_agg_arg(arg, context)?]
                }
                _ => {
                    return Err(unsupported_native_pipeline(
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

fn lower_data_agg_arg(
    expr: &ExprIr,
    context: &BTreeMap<String, Value>,
) -> Result<DataExpr, Diagnostic> {
    let expr = lower_data_expr(expr, context)?;
    if matches!(expr, DataExpr::Column(_)) {
        Ok(expr)
    } else {
        Err(unsupported_native_pipeline(
            "aggregate argument is not supported by native execution",
        ))
    }
}

fn lower_data_call(
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
                "native col() requires a string literal or context string",
            )),
        };
    }

    let function = match name {
        "is_null" => DataScalarFunction::IsNull,
        "not_null" => DataScalarFunction::NotNull,
        "abs" => DataScalarFunction::Abs,
        _ => {
            return Err(unsupported_native_pipeline(
                "scalar function is not supported by native execution",
            ));
        }
    };
    Ok(DataExpr::Call {
        function,
        args: args
            .iter()
            .map(|arg| lower_data_expr(arg, context))
            .collect::<Result<Vec<_>, _>>()?,
    })
}

fn value_to_data_literal(value: &Value) -> DataExpr {
    DataExpr::Literal(match value {
        Value::Null => DataLiteral::Null,
        Value::Bool(value) => DataLiteral::Bool(*value),
        Value::Number(value) => DataLiteral::Number(*value),
        Value::String(value) => DataLiteral::String(value.clone()),
    })
}

fn unsupported_native_pipeline(reason: &'static str) -> Diagnostic {
    Diagnostic::error(codes::E1211, reason, Span::zero())
}

struct Runtime<'a> {
    prepared: &'a PreparedProgram,
    diagnostics: Vec<Diagnostic>,
    cache: BTreeMap<String, Table>,
    active_bindings: Vec<String>,
    context: BTreeMap<String, Value>,
    dry_run: bool,
    stdout: Option<Vec<u8>>,
    io: &'a dyn DriverIo,
}

impl Runtime<'_> {
    fn execute_pipeline(&mut self, pipeline: &PipelineIr) -> Result<Table, Diagnostic> {
        let mut table = match &pipeline.start {
            PipelineStartIr::Load { format, span, .. } => {
                self.execute_load(*span, format.as_deref())?
            }
            PipelineStartIr::Binding { name, span } => self.execute_binding(name, *span)?,
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
                        .map(|item| {
                            Ok((
                                self.resolve_column_name(&item.source, item.span)?,
                                self.resolve_column_name(&item.output, item.span)?,
                            ))
                        })
                        .collect::<Result<_, Diagnostic>>()?;
                    table = table.select(&selection);
                    grouping = None;
                }
                StageIr::Drop { columns, span } => {
                    let columns = self.resolve_column_names(columns, *span)?;
                    table = table.drop_columns(&columns);
                    grouping = None;
                }
                StageIr::Rename { items, .. } => {
                    let renames: Vec<(String, String)> = items
                        .iter()
                        .map(|item| {
                            Ok((
                                self.resolve_column_name(&item.old, item.span)?,
                                self.resolve_column_name(&item.new, item.span)?,
                            ))
                        })
                        .collect::<Result<_, Diagnostic>>()?;
                    table = table.rename_columns(&renames);
                    grouping = None;
                }
                StageIr::Mutate { items, .. } => {
                    table = self.mutate(table, items)?;
                    grouping = None;
                }
                StageIr::GroupBy { columns, span } => {
                    grouping = Some(self.resolve_column_names(columns, *span)?);
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
                            Ok(SortSpec {
                                column: self.resolve_column_name(&item.column, item.span)?,
                                direction,
                                nulls,
                            })
                        })
                        .collect::<Result<Vec<_>, Diagnostic>>()?;
                    table.stable_sort(&specs);
                }
                StageIr::Limit { n, .. } => {
                    table = table.limit(*n);
                }
                StageIr::Join {
                    source,
                    source_span,
                    left_key,
                    right_key,
                    kind,
                    span,
                } => {
                    let right = self.execute_binding(source, *source_span)?;
                    let left_key = self.resolve_column_name(left_key, *span)?;
                    let right_key = self.resolve_column_name(right_key, *span)?;
                    table = self.join(table, right, &left_key, &right_key, *kind, *span)?;
                    grouping = None;
                }
                StageIr::Union {
                    source,
                    source_span,
                    by_name,
                    distinct,
                    span,
                } => {
                    let right = self.execute_binding(source, *source_span)?;
                    table = self.union(table, right, *by_name, *distinct, *span)?;
                    grouping = None;
                }
                StageIr::Distinct { columns, span } => {
                    let columns = self.resolve_column_names(columns, *span)?;
                    table = table.distinct(&columns);
                    grouping = None;
                }
                StageIr::PivotLonger {
                    columns,
                    names_to,
                    values_to,
                    span,
                } => {
                    let columns = self.resolve_column_names(columns, *span)?;
                    let names_to = self.resolve_column_name(names_to, *span)?;
                    let values_to = self.resolve_column_name(values_to, *span)?;
                    table = pivot_longer(table, &columns, &names_to, &values_to, *span)?;
                    grouping = None;
                }
                StageIr::Complete { keys, fills, span } => {
                    let keys = self.resolve_column_names(keys, *span)?;
                    let fills = fills
                        .iter()
                        .map(|fill| {
                            Ok(CompleteFillItemIr {
                                column: self.resolve_column_name(&fill.column, fill.span)?,
                                expr: fill.expr.clone(),
                                span: fill.span,
                            })
                        })
                        .collect::<Result<Vec<_>, Diagnostic>>()?;
                    table = complete(table, &keys, &fills, *span, &self.context)?;
                    grouping = None;
                }
                StageIr::Save { format, span, .. } => {
                    self.execute_save(*span, format.as_deref(), &table)?;
                }
                StageIr::Unsupported { name, span } => {
                    return Err(Diagnostic::error(
                        codes::E1211,
                        format!("stage `{name}` is deferred in 0.26.0"),
                        *span,
                    ));
                }
            }
        }

        Ok(table)
    }

    fn resolve_column_names(
        &self,
        columns: &[String],
        span: Span,
    ) -> Result<Vec<String>, Diagnostic> {
        columns
            .iter()
            .map(|column| self.resolve_column_name(column, span))
            .collect()
    }

    fn resolve_column_name(&self, column: &str, span: Span) -> Result<String, Diagnostic> {
        let Some((kind, name)) = decode_context_column_ref_ir(column) else {
            return Ok(column.to_string());
        };
        let Some(value) = self.context.get(name) else {
            return Err(Diagnostic::error(
                codes::E2002,
                format!("unknown {} `{name}`", context_kind_label(kind)),
                span,
            ));
        };
        match value {
            Value::String(value) => Ok(value.clone()),
            _ => Err(Diagnostic::error(
                codes::E2004,
                format!("context value `{name}` must be a string to resolve a column name"),
                span,
            )),
        }
    }

    fn execute_binding(&mut self, name: &str, reference_span: Span) -> Result<Table, Diagnostic> {
        if let Some(table) = self.cache.get(name) {
            return Ok(table.clone());
        }
        if let Some(index) = self
            .active_bindings
            .iter()
            .position(|active| active == name)
        {
            let mut path = self.active_bindings[index..].to_vec();
            path.push(name.to_string());
            return Err(Diagnostic::error(
                codes::E1501,
                format!("binding dependency cycle: {}", path.join(" -> ")),
                reference_span,
            ));
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
                    reference_span,
                )
            })?;
        self.active_bindings.push(name.to_string());
        let table = self.execute_pipeline(&binding.pipeline)?;
        self.active_bindings.pop();
        self.cache.insert(name.to_string(), table.clone());
        Ok(table)
    }

    fn execute_load(
        &self,
        stage_span: Span,
        explicit_format: Option<&str>,
    ) -> Result<Table, Diagnostic> {
        let Some(input) = self.prepared.driver_plan.input_for_stage_span(stage_span) else {
            return Err(Diagnostic::error(
                codes::E1505,
                "driver source facts are unavailable for execution",
                stage_span,
            ));
        };
        match &input.source {
            SourceDescriptor::Path { resolved_path, .. } => {
                let bytes = self.io.read_path_bytes(resolved_path)?;
                let format =
                    resolve_input_format(input, explicit_format, None, Some(&bytes), stage_span)?;
                read_table_from_bytes(resolved_path, format, &bytes)
            }
            SourceDescriptor::Stdin => {
                let owned_bytes;
                let bytes = if let Some(bytes) = self.prepared.stdin_bytes.as_deref() {
                    bytes
                } else {
                    owned_bytes = self.io.read_stdin_bytes()?;
                    &owned_bytes
                };
                let format = resolve_input_format(
                    input,
                    explicit_format,
                    self.prepared.stdin_format.as_deref(),
                    Some(bytes),
                    stage_span,
                )?;
                read_table_from_bytes(std::path::Path::new("stdin"), format, bytes)
            }
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
        let Some(sink) = self.prepared.driver_plan.sink_for_stage_span(stage_span) else {
            return Err(Diagnostic::error(
                codes::E1505,
                "driver sink facts are unavailable for execution",
                stage_span,
            ));
        };
        let format = resolve_output_format(sink, explicit_format, stage_span)?;
        match &sink.sink {
            SinkDescriptor::Path { resolved_path, .. } => {
                write_output(resolved_path, format, table)
            }
            SinkDescriptor::Stdout => {
                let bytes = emit_stdout(format, table)?;
                self.stdout = Some(bytes);
                Ok(())
            }
        }
    }

    fn filter(&self, table: Table, expr: &ExprIr) -> Result<Table, Diagnostic> {
        let rows = table
            .rows
            .iter()
            .filter_map(|row| {
                match eval_row_expr(
                    expr,
                    &table,
                    row,
                    ExprRole::PredicateRoot,
                    None,
                    &self.context,
                ) {
                    Ok(value) if value.is_truthy_true() => Some(Ok(row.clone())),
                    Ok(_) => None,
                    Err(diagnostic) => Some(Err(diagnostic)),
                }
            })
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
                values.push(eval_aggregate(item, table, &group_rows, &self.context)?);
            }
            rows.push(Row { values });
        }

        Ok(Table { columns, rows })
    }

    fn mutate(&self, table: Table, items: &[MutateItemIr]) -> Result<Table, Diagnostic> {
        let input_columns = table.columns.clone();
        let mut columns = input_columns.clone();
        for item in items {
            if !columns.iter().any(|column| column == &item.column) {
                columns.push(item.column.clone());
            }
        }

        let rows = table
            .rows
            .iter()
            .enumerate()
            .map(|(row_index, row)| {
                let mut values = row.values.clone();
                for item in items {
                    let value = eval_row_expr(
                        &item.expr,
                        &table,
                        row,
                        ExprRole::Default,
                        Some(row_index),
                        &self.context,
                    )?;
                    if let Some(index) = input_columns
                        .iter()
                        .position(|column| column == &item.column)
                    {
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

    fn join(
        &self,
        left: Table,
        right: Table,
        left_key: &str,
        right_key: &str,
        kind: JoinKindIr,
        span: Span,
    ) -> Result<Table, Diagnostic> {
        ensure_key_types_compatible(&left, left_key, &right, right_key, span)?;
        let output_columns = join_columns(&left.columns, &right.columns, right_key, kind, span)?;
        if matches!(kind, JoinKindIr::Semi | JoinKindIr::Anti) {
            return Ok(join_semi_anti(left, &right, left_key, right_key, kind));
        }

        let left_key_index = left.column_index(left_key).ok_or_else(|| {
            Diagnostic::error(codes::E1005, format!("unknown column `{left_key}`"), span)
        })?;
        let right_key_index = right.column_index(right_key).ok_or_else(|| {
            Diagnostic::error(codes::E1005, format!("unknown column `{right_key}`"), span)
        })?;
        let left_matches = join_index(&left, left_key);
        let right_matches = join_index(&right, right_key);
        let right_value_indices = right_non_key_indices(&right.columns, right_key);
        let mut rows = Vec::new();

        match kind {
            JoinKindIr::Inner | JoinKindIr::Left | JoinKindIr::Full => {
                let mut matched_right = vec![false; right.rows.len()];
                for left_row in &left.rows {
                    let key = row_key(left_row, left_key_index);
                    let matches = key.as_ref().and_then(|key| right_matches.get(key));
                    if let Some(matches) = matches {
                        for right_index in matches {
                            matched_right[*right_index] = true;
                            rows.push(combine_rows(
                                left_row,
                                Some(&right.rows[*right_index]),
                                &right_value_indices,
                                left.columns.len(),
                            ));
                        }
                    } else if matches!(kind, JoinKindIr::Left | JoinKindIr::Full) {
                        rows.push(combine_rows(
                            left_row,
                            None,
                            &right_value_indices,
                            left.columns.len(),
                        ));
                    }
                }
                if matches!(kind, JoinKindIr::Full) {
                    let mut unmatched_right = right
                        .rows
                        .iter()
                        .enumerate()
                        .filter(|(index, _)| !matched_right[*index])
                        .collect::<Vec<_>>();
                    unmatched_right.sort_by(|(_, left_row), (_, right_row)| {
                        row_key(left_row, right_key_index).cmp(&row_key(right_row, right_key_index))
                    });
                    for (_, right_row) in unmatched_right {
                        rows.push(right_only_row(
                            right_row,
                            right_key_index,
                            left_key_index,
                            left.columns.len(),
                            &right_value_indices,
                        ));
                    }
                }
            }
            JoinKindIr::Right => {
                for right_row in &right.rows {
                    let key = row_key(right_row, right_key_index);
                    let matches = key.as_ref().and_then(|key| left_matches.get(key));
                    if let Some(matches) = matches {
                        for left_index in matches {
                            rows.push(combine_rows(
                                &left.rows[*left_index],
                                Some(right_row),
                                &right_value_indices,
                                left.columns.len(),
                            ));
                        }
                    } else {
                        rows.push(right_only_row(
                            right_row,
                            right_key_index,
                            left_key_index,
                            left.columns.len(),
                            &right_value_indices,
                        ));
                    }
                }
            }
            JoinKindIr::Semi | JoinKindIr::Anti => unreachable!("handled earlier"),
        }

        Ok(Table {
            columns: output_columns,
            rows,
        })
    }

    fn union(
        &self,
        left: Table,
        right: Table,
        by_name: bool,
        distinct: bool,
        span: Span,
    ) -> Result<Table, Diagnostic> {
        ensure_union_compatible(&left, &right, by_name, span)?;
        let columns = left.columns.clone();
        let mut rows = left.rows.clone();
        if by_name {
            let right_indices = columns
                .iter()
                .map(|column| right.column_index(column))
                .collect::<Vec<_>>();
            rows.extend(right.rows.iter().map(|row| {
                Row {
                    values: right_indices
                        .iter()
                        .map(|index| {
                            index
                                .and_then(|index| row.values.get(index))
                                .cloned()
                                .unwrap_or(Value::Null)
                        })
                        .collect(),
                }
            }));
        } else {
            rows.extend(right.rows.iter().map(|row| {
                Row {
                    values: (0..columns.len())
                        .map(|index| row.values.get(index).cloned().unwrap_or(Value::Null))
                        .collect(),
                }
            }));
        }
        let table = Table { columns, rows };
        Ok(if distinct { table.distinct(&[]) } else { table })
    }
}

fn pivot_longer(
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

fn complete(
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

struct CompleteContext<'a> {
    table: &'a Table,
    observed_by_key: &'a [Vec<Value>],
    key_indices: &'a [usize],
    fills: &'a [CompleteFillItemIr],
    fill_indices: &'a [usize],
    existing: &'a BTreeMap<Vec<String>, Row>,
    runtime_context: &'a BTreeMap<String, Value>,
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

fn resolve_input_format(
    input: &PlanInputSource,
    explicit_format: Option<&str>,
    stdin_format: Option<&str>,
    bytes: Option<&[u8]>,
    span: Span,
) -> Result<DataFormat, Diagnostic> {
    if let Some(format) = explicit_format {
        return DataFormat::from_name(format).ok_or_else(|| {
            Diagnostic::error(
                codes::E1215,
                format!("format `{format}` is not supported in 0.26.0"),
                span,
            )
        });
    }
    if matches!(&input.source, SourceDescriptor::Stdin) {
        if let Some(format) = stdin_format {
            return DataFormat::from_name(format).ok_or_else(|| {
                Diagnostic::error(
                    codes::E1215,
                    format!("stdin format `{format}` is not supported in 0.26.0"),
                    input.span,
                )
            });
        }
    }
    if let Some(format) = input.format.inferred_from_path {
        return Ok(format);
    }
    if let Some(bytes) = bytes {
        return sniff_format_from_bytes(bytes);
    }
    Ok(DataFormat::Csv)
}

fn resolve_output_format(
    sink: &PlanOutputSink,
    explicit_format: Option<&str>,
    span: Span,
) -> Result<DataFormat, Diagnostic> {
    if let Some(format) = explicit_format {
        return DataFormat::from_name(format).ok_or_else(|| {
            Diagnostic::error(
                codes::E1705,
                format!("output format `{format}` is not supported in 0.26.0"),
                span,
            )
        });
    }
    format_from_decision(&sink.format).ok_or_else(|| {
        Diagnostic::error(
            codes::E1705,
            "could not infer supported output format",
            sink.span,
        )
    })
}

fn format_from_decision(decision: &FormatDecision) -> Option<DataFormat> {
    decision
        .explicit
        .as_deref()
        .and_then(DataFormat::from_name)
        .or(decision.inferred_from_path)
        .or(Some(DataFormat::Csv))
}

fn join_columns(
    left_columns: &[String],
    right_columns: &[String],
    right_key: &str,
    kind: JoinKindIr,
    span: Span,
) -> Result<Vec<String>, Diagnostic> {
    if matches!(kind, JoinKindIr::Semi | JoinKindIr::Anti) {
        return Ok(left_columns.to_vec());
    }

    let mut columns = left_columns.to_vec();
    for column in right_columns {
        if column == right_key {
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

fn right_non_key_indices(columns: &[String], right_key: &str) -> Vec<usize> {
    columns
        .iter()
        .enumerate()
        .filter_map(|(index, column)| (column != right_key).then_some(index))
        .collect()
}

fn join_index(table: &Table, key: &str) -> BTreeMap<String, Vec<usize>> {
    let Some(index) = table.column_index(key) else {
        return BTreeMap::new();
    };
    let mut matches = BTreeMap::new();
    for (row_index, row) in table.rows.iter().enumerate() {
        if let Some(key) = row_key(row, index) {
            matches.entry(key).or_insert_with(Vec::new).push(row_index);
        }
    }
    matches
}

fn row_key(row: &Row, index: usize) -> Option<String> {
    match row.values.get(index).unwrap_or(&Value::Null) {
        Value::Null => None,
        value => Some(value.to_csv_cell()),
    }
}

fn combine_rows(
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

fn right_only_row(
    right_row: &Row,
    right_key_index: usize,
    left_key_index: usize,
    left_width: usize,
    right_value_indices: &[usize],
) -> Row {
    let mut values = vec![Value::Null; left_width];
    if let Some(value) = right_row.values.get(right_key_index) {
        if let Some(left_key) = values.get_mut(left_key_index) {
            *left_key = value.clone();
        }
    }
    values.extend(
        right_value_indices
            .iter()
            .map(|index| right_row.values.get(*index).cloned().unwrap_or(Value::Null)),
    );
    Row { values }
}

fn join_semi_anti(
    left: Table,
    right: &Table,
    left_key: &str,
    right_key: &str,
    kind: JoinKindIr,
) -> Table {
    let Some(left_index) = left.column_index(left_key) else {
        return left;
    };
    let right_matches = join_index(right, right_key);
    let rows = left
        .rows
        .iter()
        .filter(|row| {
            let matched = row_key(row, left_index)
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

fn ensure_key_types_compatible(
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

fn ensure_union_compatible(
    left: &Table,
    right: &Table,
    by_name: bool,
    span: Span,
) -> Result<(), Diagnostic> {
    if by_name {
        let left_names: BTreeSet<&String> = left.columns.iter().collect();
        let right_names: BTreeSet<&String> = right.columns.iter().collect();
        if left_names != right_names {
            return Err(Diagnostic::error(
                codes::E1209,
                "union schemas have different column names",
                span,
            ));
        }
        for column in &left.columns {
            ensure_union_column_compatible(left, column, right, column, span)?;
        }
    } else {
        if left.columns.len() != right.columns.len() {
            return Err(Diagnostic::error(
                codes::E1209,
                "union schemas have different column counts",
                span,
            ));
        }
        for (left_column, right_column) in left.columns.iter().zip(&right.columns) {
            ensure_union_column_compatible(left, left_column, right, right_column, span)?;
        }
    }
    Ok(())
}

fn ensure_union_column_compatible(
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
enum ValueClass {
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

#[derive(Clone, Copy)]
enum ExprRole {
    PredicateRoot,
    Default,
    ComparisonLeft,
    ComparisonRight,
}

#[derive(Clone, Copy)]
struct EvalScope<'a> {
    window_row_index: Option<usize>,
    runtime_context: &'a BTreeMap<String, Value>,
}

fn eval_row_expr(
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

fn eval_call(
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
            |value| Ok(map_text(value, |text| text.to_ascii_lowercase())),
        ),
        "upper" => eval_single_arg(
            args,
            table,
            row,
            span,
            window_row_index,
            runtime_context,
            |value| Ok(map_text(value, |text| text.to_ascii_uppercase())),
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
        _ => Err(Diagnostic::error(
            codes::E1401,
            format!("unknown function `{name}`"),
            span,
        )),
    }
}

fn round_digits(expr: &ExprIr, span: Span) -> Result<i32, Diagnostic> {
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

fn round_value(value: Value, digits: i32, span: Span) -> Result<Value, Diagnostic> {
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

fn eval_window_expr(
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

fn ordered_partition_indices(
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

fn partition_key(table: &Table, spec: &WindowSpecIr, row_index: usize) -> Vec<Value> {
    let row = &table.rows[row_index];
    spec.partition_by
        .iter()
        .map(|column| table.value(row, column).cloned().unwrap_or(Value::Null))
        .collect()
}

fn compare_rows_for_window_order(
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

fn compare_values_for_window_sort(left: &Value, right: &Value, nulls: DataNullsOrder) -> Ordering {
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

fn rank_value(table: &Table, spec: &WindowSpecIr, partition: &[usize], position: usize) -> usize {
    let current_key = order_key(table, spec, partition[position]);
    partition
        .iter()
        .position(|index| order_key(table, spec, *index) == current_key)
        .map_or(position + 1, |index| index + 1)
}

fn dense_rank_value(
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

fn last_peer_position(
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

fn order_key(table: &Table, spec: &WindowSpecIr, row_index: usize) -> Vec<Value> {
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

fn eval_offset_window(
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
    let offset = window_offset(args.get(1), span)? as isize;
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

fn window_offset(offset: Option<&ExprIr>, span: Span) -> Result<usize, Diagnostic> {
    match offset {
        None => Ok(1),
        Some(ExprIr::Number { value, .. }) if *value >= 0.0 && value.fract() == 0.0 => {
            Ok(*value as usize)
        }
        Some(expr) => Err(Diagnostic::error(
            codes::E1206,
            "lag/lead offset must be a non-negative integer literal",
            expr.span(),
        )),
    }
    .map_err(|mut diagnostic| {
        if diagnostic.span == Span::zero() {
            diagnostic.span = span;
        }
        diagnostic
    })
}

fn frame_indices(
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
    table
        .value(row, column)
        .cloned()
        .ok_or_else(|| Diagnostic::error(codes::E1005, format!("unknown column `{column}`"), span))
}

fn eval_aggregate(
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

#[cfg(test)]
mod tests {
    use super::*;
    use pdl_driver::{
        prepare_source_for_run_with_io, prepare_source_with_io, InMemoryDriverIo, OsDriverIo,
    };
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn runs_csv_stdin_with_explicit_format() {
        let io = InMemoryDriverIo::default()
            .with_stdin_bytes("status,amount\ncompleted,10\npending,20\n");
        let prepared = prepare_source_for_run_with_io(
            "memory/main.pdl",
            r#"load stdin format "csv"
  | filter status == "completed"
  | select amount"#,
            None,
            &io,
        );

        let result = run_prepared_with_io(
            &prepared,
            RunOptions {
                stdout_format: Some("csv".to_string()),
                dry_run: false,
                allow_binary_stdout: true,
            },
            &io,
        );

        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert_eq!(
            String::from_utf8(result.stdout.expect("csv stdout")).expect("utf8 csv"),
            "amount\n10\n"
        );
    }

    #[test]
    fn native_engine_runs_supported_path_backed_pipeline() {
        let workspace = temp_workspace("native-supported");
        fs::write(
            workspace.join("sales.csv"),
            "status,region,amount\ncompleted,West,30\npending,East,10\ncompleted,North,40\n",
        )
        .expect("write csv");
        let program_path = workspace.join("main.pdl");
        let io = OsDriverIo;
        let prepared = prepare_source_with_io(
            &program_path,
            r#"load "sales.csv"
  | filter status == "completed"
  | select region, amount
  | sort amount desc
  | limit 1"#,
            &io,
        );

        let result = run_prepared_with_io_and_context_and_engine(
            &prepared,
            RunOptions {
                stdout_format: Some("csv".to_string()),
                dry_run: false,
                allow_binary_stdout: true,
            },
            &io,
            BTreeMap::new(),
            ExecutionEngine::Native,
        );

        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert_eq!(result.backend, DataBackend::NativePolars);
        assert_eq!(
            String::from_utf8(result.stdout.expect("csv stdout")).expect("utf8 csv"),
            "region,amount\nNorth,40\n"
        );
        fs::remove_dir_all(workspace).expect("clean temp workspace");
    }

    #[test]
    fn auto_engine_falls_back_to_rows_for_unsupported_native_stage() {
        let workspace = temp_workspace("native-fallback");
        fs::write(
            workspace.join("sales.csv"),
            "region,amount\nWest,30\nEast,10\n",
        )
        .expect("write csv");
        let program_path = workspace.join("main.pdl");
        let io = OsDriverIo;
        let prepared = prepare_source_with_io(
            &program_path,
            r#"load "sales.csv"
  | mutate doubled = amount * 2
  | select region, doubled
  | sort region"#,
            &io,
        );

        let auto = run_prepared_with_io_and_context_and_engine(
            &prepared,
            RunOptions {
                stdout_format: Some("csv".to_string()),
                dry_run: false,
                allow_binary_stdout: true,
            },
            &io,
            BTreeMap::new(),
            ExecutionEngine::Auto,
        );
        assert!(auto.diagnostics.is_empty(), "{:?}", auto.diagnostics);
        assert_eq!(auto.backend, DataBackend::PortableRows);
        assert_eq!(
            String::from_utf8(auto.stdout.expect("csv stdout")).expect("utf8 csv"),
            "region,doubled\nEast,20\nWest,60\n"
        );

        let forced = run_prepared_with_io_and_context_and_engine(
            &prepared,
            RunOptions {
                stdout_format: Some("csv".to_string()),
                dry_run: false,
                allow_binary_stdout: true,
            },
            &io,
            BTreeMap::new(),
            ExecutionEngine::Native,
        );
        assert!(forced.stdout.is_none());
        assert_eq!(forced.backend, DataBackend::NativePolars);
        assert!(forced
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "E1211"));
        fs::remove_dir_all(workspace).expect("clean temp workspace");
    }

    #[test]
    fn native_eligibility_rejects_unsupported_stage_before_execution() {
        let workspace = temp_workspace("native-eligibility");
        fs::write(workspace.join("sales.csv"), "amount\n10\n").expect("write csv");
        let program_path = workspace.join("main.pdl");
        let io = OsDriverIo;
        let prepared = prepare_source_with_io(
            &program_path,
            r#"load "sales.csv"
  | mutate doubled = amount * 2"#,
            &io,
        );
        let plan = plan_prepared(
            &prepared,
            PlanningOptions {
                stdout_format: Some(DataFormat::Csv.canonical_name().to_string()),
                dry_run: false,
                allow_binary_stdout: true,
            },
        )
        .expect("execution plan");
        let ir = prepared.analysis.ir.as_ref().expect("ir");

        let diagnostic = check_native_program_eligibility(&prepared, ir, &plan, &BTreeMap::new())
            .expect_err("unsupported native pipeline");

        assert_eq!(diagnostic.code, "E1211");
        assert!(diagnostic.message.contains("pipeline stage"));
        fs::remove_dir_all(workspace).expect("clean temp workspace");
    }

    #[test]
    fn native_engine_runs_grouped_aggregate_csv_and_parquet() {
        let workspace = temp_workspace("native-aggregate");
        let csv = "region,score,latency_ms\nWest,30,100\nEast,10,80\nWest,50,120\nEast,30,90\n";
        fs::write(workspace.join("sales.csv"), csv).expect("write csv");
        pdl_data::write_table_to_path(
            &workspace.join("sales.parquet"),
            DataFormat::Parquet,
            &Table::new(
                vec![
                    "region".to_string(),
                    "score".to_string(),
                    "latency_ms".to_string(),
                ],
                vec![
                    Row {
                        values: vec![
                            Value::String("West".to_string()),
                            Value::Number(30.0),
                            Value::Number(100.0),
                        ],
                    },
                    Row {
                        values: vec![
                            Value::String("East".to_string()),
                            Value::Number(10.0),
                            Value::Number(80.0),
                        ],
                    },
                    Row {
                        values: vec![
                            Value::String("West".to_string()),
                            Value::Number(50.0),
                            Value::Number(120.0),
                        ],
                    },
                    Row {
                        values: vec![
                            Value::String("East".to_string()),
                            Value::Number(30.0),
                            Value::Number(90.0),
                        ],
                    },
                ],
            ),
        )
        .expect("write parquet");

        for input in ["sales.csv", "sales.parquet"] {
            let program_path = workspace.join(format!("{input}.pdl"));
            let io = OsDriverIo;
            let prepared = prepare_source_with_io(
                &program_path,
                format!(
                    r#"load "{input}"
  | group_by region
  | agg
      row_count = count(),
      total_score = sum(score),
      avg_score = mean(score),
      min_latency_ms = min(latency_ms),
      max_latency_ms = max(latency_ms)
  | sort region"#
                ),
                &io,
            );
            let options = RunOptions {
                stdout_format: Some("csv".to_string()),
                dry_run: false,
                allow_binary_stdout: true,
            };
            let row = run_prepared_with_io_and_context_and_engine(
                &prepared,
                options.clone(),
                &io,
                BTreeMap::new(),
                ExecutionEngine::Row,
            );
            let native = run_prepared_with_io_and_context_and_engine(
                &prepared,
                options,
                &io,
                BTreeMap::new(),
                ExecutionEngine::Native,
            );

            assert!(row.diagnostics.is_empty(), "{:?}", row.diagnostics);
            assert!(
                native.diagnostics.is_empty(),
                "{input}: {:?}",
                native.diagnostics
            );
            assert_eq!(native.backend, DataBackend::NativePolars);
            assert_eq!(
                String::from_utf8(native.stdout.expect("native csv")).expect("utf8"),
                String::from_utf8(row.stdout.expect("row csv")).expect("utf8"),
                "{input}"
            );
        }
        fs::remove_dir_all(workspace).expect("clean temp workspace");
    }

    #[test]
    fn native_engine_writes_readable_arrow_stream_stdout() {
        let workspace = temp_workspace("native-arrow-stream");
        fs::write(
            workspace.join("sales.csv"),
            "region,amount\nWest,30\nEast,10\n",
        )
        .expect("write csv");
        let program_path = workspace.join("main.pdl");
        let io = OsDriverIo;
        let prepared = prepare_source_with_io(
            &program_path,
            r#"load "sales.csv"
  | select region, amount
  | sort amount desc"#,
            &io,
        );

        let result = run_prepared_with_io_and_context_and_engine(
            &prepared,
            RunOptions {
                stdout_format: Some("arrow-stream".to_string()),
                dry_run: false,
                allow_binary_stdout: true,
            },
            &io,
            BTreeMap::new(),
            ExecutionEngine::Native,
        );

        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert_eq!(result.backend, DataBackend::NativePolars);
        let stdout = result.stdout.expect("arrow stdout");
        assert!(stdout.starts_with(&[0xff, 0xff, 0xff, 0xff]));
        assert_eq!(
            pdl_data::read_table_from_bytes(
                Path::new("stdout.arrow"),
                DataFormat::ArrowStream,
                &stdout,
            )
            .expect("read arrow stdout"),
            Table::new(
                vec!["region".to_string(), "amount".to_string()],
                vec![
                    Row {
                        values: vec![Value::String("West".to_string()), Value::Number(30.0)],
                    },
                    Row {
                        values: vec![Value::String("East".to_string()), Value::Number(10.0)],
                    },
                ],
            )
        );
        fs::remove_dir_all(workspace).expect("clean temp workspace");
    }

    #[test]
    fn native_engine_supports_terminal_save_stdout_arrow_stream() {
        let workspace = temp_workspace("native-save-stdout");
        fs::write(
            workspace.join("sales.csv"),
            "region,amount\nWest,30\nEast,10\n",
        )
        .expect("write csv");
        let program_path = workspace.join("main.pdl");
        let io = OsDriverIo;
        let prepared = prepare_source_with_io(
            &program_path,
            r#"load "sales.csv"
  | select region, amount
  | sort amount desc
  | save stdout format "arrow-stream""#,
            &io,
        );

        let result = run_prepared_with_io_and_context_and_engine(
            &prepared,
            RunOptions {
                stdout_format: None,
                dry_run: false,
                allow_binary_stdout: true,
            },
            &io,
            BTreeMap::new(),
            ExecutionEngine::Native,
        );

        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert_eq!(result.backend, DataBackend::NativePolars);
        let stdout = result.stdout.expect("arrow stdout");
        assert!(stdout.starts_with(&[0xff, 0xff, 0xff, 0xff]));
        assert_eq!(
            pdl_data::read_table_from_bytes(
                Path::new("stdout.arrow"),
                DataFormat::ArrowStream,
                &stdout,
            )
            .expect("read arrow stdout")
            .columns,
            vec!["region", "amount"]
        );
        fs::remove_dir_all(workspace).expect("clean temp workspace");
    }

    #[test]
    fn reactive_context_defaults_and_overrides_drive_named_outputs() {
        let io = InMemoryDriverIo::default().with_file_bytes(
            "memory/trips.csv",
            "zone,station,fleet,revenue,duration_min\nDowntown,A,bus,100,12\nDowntown,B,rail,50,30\nRiverfront,C,bus,80,20\nRiverfront,D,rail,120,40\n",
        );
        let source = r#"param active_fleet = "all"
state selected_zone = "Downtown"
param metric_column = "revenue"

let trips =
  load "trips.csv"
  | filter $active_fleet == "all" or fleet == $active_fleet

output zone_summary =
  trips
  | group_by zone
  | agg total_revenue = sum(revenue)
  | save "zone_summary.csv"

output active_rankings =
  trips
  | filter zone == @selected_zone
  | group_by station
  | agg total = sum(col($metric_column))
  | sort total desc
  | save "active_rankings.csv""#;
        let prepared = prepare_source_with_io("memory/main.pdl", source, &io);

        let run_options = RunOptions {
            dry_run: true,
            ..RunOptions::default()
        };
        let defaults = run_prepared_with_io(&prepared, run_options.clone(), &io);
        assert!(
            defaults.diagnostics.is_empty(),
            "{:?}",
            defaults.diagnostics
        );
        assert_eq!(
            named_output_csv(&defaults, "active_rankings"),
            "station,total\nA,100\nB,50\n"
        );

        let mut context = BTreeMap::new();
        context.insert("active_fleet".to_string(), Value::String("bus".to_string()));
        context.insert(
            "selected_zone".to_string(),
            Value::String("Riverfront".to_string()),
        );
        context.insert(
            "metric_column".to_string(),
            Value::String("duration_min".to_string()),
        );
        let overridden = run_prepared_with_io_and_context(&prepared, run_options, &io, context);
        assert!(
            overridden.diagnostics.is_empty(),
            "{:?}",
            overridden.diagnostics
        );
        assert_eq!(
            named_output_csv(&overridden, "active_rankings"),
            "station,total\nC,20\n"
        );
    }

    #[test]
    fn sniffs_arrow_stream_stdin_and_preserves_bytes_for_execution() {
        let input_table = Table::new(
            vec!["region".to_string(), "amount".to_string()],
            vec![
                Row {
                    values: vec![Value::String("West".to_string()), Value::Number(30.0)],
                },
                Row {
                    values: vec![Value::String("East".to_string()), Value::Number(10.0)],
                },
            ],
        );
        let stdin = pdl_data::write_table_to_bytes(DataFormat::ArrowStream, &input_table)
            .expect("arrow stdin");
        let io = InMemoryDriverIo::default().with_stdin_bytes(stdin);
        let prepared = prepare_source_for_run_with_io(
            "memory/main.pdl",
            r#"load stdin
  | sort amount desc"#,
            None,
            &io,
        );

        let result = run_prepared_with_io(
            &prepared,
            RunOptions {
                stdout_format: Some("csv".to_string()),
                dry_run: false,
                allow_binary_stdout: true,
            },
            &io,
        );

        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert_eq!(
            String::from_utf8(result.stdout.expect("csv stdout")).expect("utf8 csv"),
            "region,amount\nWest,30\nEast,10\n"
        );
    }

    #[test]
    fn stdin_format_conflict_reports_e1217_before_reading_stdin() {
        let io = InMemoryDriverIo::default();
        let prepared = prepare_source_for_run_with_io(
            "memory/main.pdl",
            r#"load stdin format "csv""#,
            Some("arrow-stream".to_string()),
            &io,
        );
        let diagnostics = prepared.diagnostics();

        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "E1217"),
            "{diagnostics:?}"
        );
        assert!(
            !diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "E1806"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn emits_deterministic_arrow_stream_stdout() {
        let io = InMemoryDriverIo::default()
            .with_file_bytes("memory/sales.csv", "region,amount\nWest,30\nEast,10\n");
        let prepared = prepare_source_with_io(
            "memory/main.pdl",
            r#"load "sales.csv"
  | sort amount desc"#,
            &io,
        );

        let first = run_prepared_with_io(
            &prepared,
            RunOptions {
                stdout_format: Some("arrow-stream".to_string()),
                dry_run: false,
                allow_binary_stdout: true,
            },
            &io,
        );
        let second = run_prepared_with_io(
            &prepared,
            RunOptions {
                stdout_format: Some("arrow-stream".to_string()),
                dry_run: false,
                allow_binary_stdout: true,
            },
            &io,
        );

        assert!(first.diagnostics.is_empty(), "{:?}", first.diagnostics);
        assert!(second.diagnostics.is_empty(), "{:?}", second.diagnostics);
        let first_stdout = first.stdout.expect("arrow stdout");
        let second_stdout = second.stdout.expect("arrow stdout");
        assert_eq!(first_stdout, second_stdout);
        assert!(first_stdout.starts_with(&[0xff, 0xff, 0xff, 0xff]));
        assert_eq!(
            pdl_data::read_table_from_bytes(
                Path::new("stdout.arrow"),
                DataFormat::ArrowStream,
                &first_stdout,
            )
            .expect("read arrow stdout"),
            Table::new(
                vec!["region".to_string(), "amount".to_string()],
                vec![
                    Row {
                        values: vec![Value::String("West".to_string()), Value::Number(30.0)],
                    },
                    Row {
                        values: vec![Value::String("East".to_string()), Value::Number(10.0)],
                    },
                ],
            )
        );
    }

    #[test]
    fn save_stdout_writes_arrow_stream_bytes() {
        let io = InMemoryDriverIo::default()
            .with_file_bytes("memory/sales.csv", "region,amount\nWest,30\n");
        let prepared = prepare_source_with_io(
            "memory/main.pdl",
            r#"load "sales.csv"
  | save stdout format "arrow-stream""#,
            &io,
        );

        let result = run_prepared_with_io(
            &prepared,
            RunOptions {
                stdout_format: None,
                dry_run: false,
                allow_binary_stdout: true,
            },
            &io,
        );

        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        let stdout = result.stdout.expect("arrow stdout");
        assert!(stdout.starts_with(&[0xff, 0xff, 0xff, 0xff]));
    }

    #[test]
    fn executes_mutate_distinct_and_scalar_functions() {
        let io = InMemoryDriverIo::default().with_file_bytes(
            "memory/orders.csv",
            "order_id,region,channel,gross,discount,status\nA1,North,web,120,20,completed\nA1,North,web,120,20,completed\nA2,South,store,80,5,pending\nA3,West,Web,200,50,completed\n",
        );
        let prepared = prepare_source_with_io(
            "memory/main.pdl",
            r#"load "orders.csv"
  | filter status == "completed"
  | mutate net_amount = gross - discount, region_channel = concat(upper(region), ":", lower(channel)), priority = if_else(gross >= 150, "high", "standard")
  | distinct order_id
  | select order_id, region_channel, net_amount, priority
  | sort order_id"#,
            &io,
        );

        let result = run_prepared_with_io(
            &prepared,
            RunOptions {
                stdout_format: Some("csv".to_string()),
                dry_run: true,
                allow_binary_stdout: true,
            },
            &io,
        );

        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert_eq!(
            String::from_utf8(result.stdout.expect("csv stdout")).expect("utf8 csv"),
            "order_id,region_channel,net_amount,priority\nA1,NORTH:web,100,standard\nA3,WEST:web,150,high\n"
        );
    }

    #[test]
    fn executes_decimal_rounding_and_count_distinct() {
        let io = InMemoryDriverIo::default().with_file_bytes(
            "memory/events.csv",
            "group,user,amount\nA,u1,1.234\nA,u1,2.345\nA,u2,-0.004\nA,,4.0\nB,u3,10.005\n",
        );
        let prepared = prepare_source_with_io(
            "memory/main.pdl",
            r#"load "events.csv"
  | group_by `group`
  | agg users = count_distinct(user), total = sum(amount)
  | mutate rounded = round(total, 2), nearest = round(total)
  | sort `group`"#,
            &io,
        );

        let result = run_prepared_with_io(
            &prepared,
            RunOptions {
                stdout_format: Some("csv".to_string()),
                dry_run: true,
                allow_binary_stdout: true,
            },
            &io,
        );

        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert_eq!(
            String::from_utf8(result.stdout.expect("csv stdout")).expect("utf8 csv"),
            "group,users,total,rounded,nearest\nA,2,7.575,7.58,8\nB,1,10.005,10.01,10\n"
        );
    }

    #[test]
    fn round_propagates_null_and_normalizes_negative_zero() {
        let io = InMemoryDriverIo::default()
            .with_file_bytes("memory/values.csv", "id,value\nnegative,-0.004\nempty,\n");
        let prepared = prepare_source_with_io(
            "memory/main.pdl",
            r#"load "values.csv"
  | mutate rounded = round(value, 2)
  | sort id"#,
            &io,
        );

        let result = run_prepared_with_io(
            &prepared,
            RunOptions {
                stdout_format: Some("csv".to_string()),
                dry_run: true,
                allow_binary_stdout: true,
            },
            &io,
        );

        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert_eq!(
            String::from_utf8(result.stdout.expect("csv stdout")).expect("utf8 csv"),
            "id,value,rounded\nempty,,\nnegative,-0.004,0\n"
        );
    }

    #[test]
    fn invalid_round_digits_are_semantic_diagnostics() {
        let io = InMemoryDriverIo::default().with_file_bytes("memory/values.csv", "value\n1.234\n");
        let prepared = prepare_source_with_io(
            "memory/main.pdl",
            r#"load "values.csv"
  | mutate rounded = round(value, 13)"#,
            &io,
        );
        let diagnostics = prepared.diagnostics();

        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "E1206"
                    && diagnostic.message.contains("round() digits")),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn executes_pivot_longer_with_stable_order() {
        let io = InMemoryDriverIo::default().with_file_bytes(
            "memory/wide.csv",
            "rider_type,Share of rides,Share of revenue\nmember,65.96,39.01\nvisitor,34.04,60.99\n",
        );
        let prepared = prepare_source_with_io(
            "memory/main.pdl",
            r#"load "wide.csv"
  | pivot_longer `Share of rides`, `Share of revenue` names_to metric values_to share"#,
            &io,
        );

        let result = run_prepared_with_io(
            &prepared,
            RunOptions {
                stdout_format: Some("csv".to_string()),
                dry_run: true,
                allow_binary_stdout: true,
            },
            &io,
        );

        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert_eq!(
            String::from_utf8(result.stdout.expect("csv stdout")).expect("utf8 csv"),
            "rider_type,metric,share\nmember,Share of rides,65.96\nmember,Share of revenue,39.01\nvisitor,Share of rides,34.04\nvisitor,Share of revenue,60.99\n"
        );
    }

    #[test]
    fn executes_complete_with_deterministic_fill_rows() {
        let io = InMemoryDriverIo::default().with_file_bytes(
            "memory/daily.csv",
            "trip_date,rider_type,trips,revenue\n2026-04-01,member,3,13.85\n2026-04-01,visitor,2,33.13\n2026-04-03,member,2,8.35\n",
        );
        let prepared = prepare_source_with_io(
            "memory/main.pdl",
            r#"load "daily.csv"
  | complete trip_date, rider_type fill trips = 0, revenue = 0"#,
            &io,
        );

        let result = run_prepared_with_io(
            &prepared,
            RunOptions {
                stdout_format: Some("csv".to_string()),
                dry_run: true,
                allow_binary_stdout: true,
            },
            &io,
        );

        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert_eq!(
            String::from_utf8(result.stdout.expect("csv stdout")).expect("utf8 csv"),
            "trip_date,rider_type,trips,revenue\n2026-04-01,member,3,13.85\n2026-04-01,visitor,2,33.13\n2026-04-03,member,2,8.35\n2026-04-03,visitor,0,0\n"
        );
    }

    #[test]
    fn complete_rejects_duplicate_key_tuples() {
        let io = InMemoryDriverIo::default().with_file_bytes(
            "memory/daily.csv",
            "trip_date,rider_type,trips\n2026-04-01,member,3\n2026-04-01,member,4\n",
        );
        let prepared = prepare_source_with_io(
            "memory/main.pdl",
            r#"load "daily.csv"
  | complete trip_date, rider_type fill trips = 0"#,
            &io,
        );

        let result = run_prepared_with_io(
            &prepared,
            RunOptions {
                stdout_format: Some("csv".to_string()),
                dry_run: true,
                allow_binary_stdout: true,
            },
            &io,
        );

        assert!(result.stdout.is_none());
        assert!(result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "E1208"));
    }

    #[test]
    fn executes_named_outputs_in_source_order() {
        let io = InMemoryDriverIo::default()
            .with_file_bytes("memory/sales.csv", "region,amount\nWest,30\nEast,10\n");
        let prepared = prepare_source_with_io(
            "memory/main.pdl",
            r#"let sales =
  load "sales.csv"

output west =
  sales
  | filter region == "West"
  | save "west.csv"

output totals =
  sales
  | agg total = sum(amount)
  | save "totals.csv""#,
            &io,
        );

        let result = run_prepared_with_io(
            &prepared,
            RunOptions {
                stdout_format: None,
                dry_run: true,
                allow_binary_stdout: true,
            },
            &io,
        );

        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert_eq!(
            result
                .named_outputs
                .iter()
                .map(|output| output.name.as_str())
                .collect::<Vec<_>>(),
            vec!["west", "totals"]
        );
        assert_eq!(
            result.named_outputs[0].table.columns,
            vec!["region", "amount"]
        );
        assert_eq!(result.named_outputs[0].table.rows.len(), 1);
        assert_eq!(result.named_outputs[1].table.columns, vec!["total"]);
    }

    #[test]
    fn multiple_named_outputs_reject_stdout_format() {
        let io = InMemoryDriverIo::default()
            .with_file_bytes("memory/sales.csv", "region,amount\nWest,30\n");
        let prepared = prepare_source_with_io(
            "memory/main.pdl",
            r#"output one =
  load "sales.csv"

output two =
  load "sales.csv""#,
            &io,
        );

        let result = run_prepared_with_io(
            &prepared,
            RunOptions {
                stdout_format: Some("csv".to_string()),
                dry_run: true,
                allow_binary_stdout: true,
            },
            &io,
        );

        assert!(result.stdout.is_none());
        assert!(result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "E1607"));
    }

    #[test]
    fn prepares_bikeshare_story_named_outputs() {
        let io = InMemoryDriverIo::default().with_file_bytes(
            "memory/trips.csv",
            "trip_id,trip_date,rider_type,weather,dock_id,bike_id,fare,tip,trip_status\nT1,2026-04-01,member,clear,D1,B1,10.005,0.5,valid\nT2,2026-04-01,visitor,clear,D2,B2,20.125,1.0,valid\nT3,2026-04-02,visitor,rain,D2,B2,8.1,0,invalid\nT4,2026-04-03,member,rain,D1,B1,5.333,0.25,valid\n",
        );
        let prepared = prepare_source_with_io(
            "memory/main.pdl",
            r#"let cleaned =
  load "trips.csv"
  | filter trip_status == "valid"
  | mutate revenue = round(fare + tip, 2)

output daily_rider_trips =
  cleaned
  | group_by trip_date, rider_type
  | agg trips = count(), revenue = sum(revenue)
  | complete trip_date, rider_type fill trips = 0, revenue = 0
  | sort trip_date, rider_type
  | save "daily_rider_trips.csv"

output valid_trips =
  cleaned
  | select trip_id, trip_date, rider_type, weather, dock_id, revenue
  | sort trip_id
  | save "valid_trips.csv"

output revenue_inversion =
  cleaned
  | group_by rider_type
  | agg trips = count(), revenue = sum(revenue)
  | mutate `Share of rides` = round(trips, 2), `Share of revenue` = round(revenue, 2)
  | select rider_type, `Share of rides`, `Share of revenue`
  | pivot_longer `Share of rides`, `Share of revenue` names_to metric values_to value
  | save "revenue_inversion.csv"

output weather_split =
  cleaned
  | group_by weather, rider_type
  | agg trips = count()
  | sort weather, rider_type
  | save "weather_split.csv"

output dock_priority =
  cleaned
  | group_by dock_id
  | agg trips = count(), bikes = count_distinct(bike_id), revenue = sum(revenue)
  | mutate revenue = round(revenue, 2)
  | sort dock_id
  | save "dock_priority.csv""#,
            &io,
        );

        let result = run_prepared_with_io(
            &prepared,
            RunOptions {
                stdout_format: None,
                dry_run: true,
                allow_binary_stdout: true,
            },
            &io,
        );

        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert_eq!(
            result
                .named_outputs
                .iter()
                .map(|output| output.name.as_str())
                .collect::<Vec<_>>(),
            vec![
                "daily_rider_trips",
                "valid_trips",
                "revenue_inversion",
                "weather_split",
                "dock_priority"
            ]
        );
        assert_eq!(
            named_output_csv(&result, "daily_rider_trips"),
            "trip_date,rider_type,trips,revenue\n2026-04-01,member,1,10.51\n2026-04-01,visitor,1,21.13\n2026-04-03,member,1,5.58\n2026-04-03,visitor,0,0\n"
        );
        assert_eq!(
            named_output_csv(&result, "valid_trips"),
            "trip_id,trip_date,rider_type,weather,dock_id,revenue\nT1,2026-04-01,member,clear,D1,10.51\nT2,2026-04-01,visitor,clear,D2,21.13\nT4,2026-04-03,member,rain,D1,5.58\n"
        );
        assert_eq!(
            named_output_csv(&result, "revenue_inversion"),
            "rider_type,metric,value\nmember,Share of rides,2\nmember,Share of revenue,16.09\nvisitor,Share of rides,1\nvisitor,Share of revenue,21.13\n"
        );
        assert_eq!(
            named_output_csv(&result, "weather_split"),
            "weather,rider_type,trips\nclear,member,1\nclear,visitor,1\nrain,member,1\n"
        );
        assert_eq!(
            named_output_csv(&result, "dock_priority"),
            "dock_id,trips,bikes,revenue\nD1,2,1,16.09\nD2,1,1,21.13\n"
        );
    }

    fn named_output_csv(result: &RunResult, name: &str) -> String {
        let table = &result
            .named_outputs
            .iter()
            .find(|output| output.name == name)
            .unwrap_or_else(|| panic!("missing named output `{name}`"))
            .table;
        String::from_utf8(
            pdl_data::write_table_to_bytes(DataFormat::Csv, table).expect("csv output"),
        )
        .expect("utf8 csv")
    }

    fn temp_workspace(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("pdl-{name}-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&path).expect("create temp workspace");
        path
    }

    #[test]
    fn executes_window_mutations_with_rank_offsets_and_frames() {
        let io = InMemoryDriverIo::default().with_file_bytes(
            "memory/orders.csv",
            "order_id,customer_id,region,order_date,amount\nA1,C1,North,2026-02-01,10\nA2,C1,North,2026-02-03,25\nA3,C2,North,2026-02-02,15\nA4,C2,South,2026-02-01,40\nA5,C1,North,2026-02-04,5\n",
        );
        let prepared = prepare_source_with_io(
            "memory/main.pdl",
            r#"load "orders.csv"
  | mutate customer_row = row_number() over (partition_by customer_id order_by order_date), customer_running_amount = sum(amount) over (partition_by customer_id order_by order_date rows between unbounded_preceding and current_row), previous_amount = lag(amount) over (partition_by customer_id order_by order_date), region_amount_rank = dense_rank() over (partition_by region order_by amount desc)
  | select order_id, customer_id, amount, customer_row, customer_running_amount, previous_amount, region_amount_rank"#,
            &io,
        );

        let result = run_prepared_with_io(
            &prepared,
            RunOptions {
                stdout_format: Some("csv".to_string()),
                dry_run: true,
                allow_binary_stdout: true,
            },
            &io,
        );

        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert_eq!(
            String::from_utf8(result.stdout.expect("csv stdout")).expect("utf8 csv"),
            "order_id,customer_id,amount,customer_row,customer_running_amount,previous_amount,region_amount_rank\nA1,C1,10,1,10,,3\nA2,C1,25,2,35,10,1\nA3,C2,15,2,55,40,2\nA4,C2,40,1,40,,1\nA5,C1,5,3,40,25,4\n"
        );
    }

    #[test]
    fn executes_window_distribution_value_and_lead_functions() {
        let io = InMemoryDriverIo::default().with_file_bytes(
            "memory/orders.csv",
            "order_id,customer_id,region,order_date,amount\nA1,C1,North,2026-02-01,10\nA2,C1,North,2026-02-03,25\nA3,C2,North,2026-02-02,15\nA4,C2,South,2026-02-01,40\nA5,C1,North,2026-02-04,5\n",
        );
        let prepared = prepare_source_with_io(
            "memory/main.pdl",
            r#"load "orders.csv"
  | mutate region_count = count() over (partition_by region), region_top_order = first_value(order_id) over (partition_by region order_by amount desc), region_low_order = last_value(order_id) over (partition_by region order_by amount desc rows between unbounded_preceding and unbounded_following), next_amount = lead(amount, 1, "none") over (partition_by customer_id order_by order_date), region_percent_rank = percent_rank() over (partition_by region order_by amount desc)
  | select order_id, region_count, region_top_order, region_low_order, next_amount, region_percent_rank"#,
            &io,
        );

        let result = run_prepared_with_io(
            &prepared,
            RunOptions {
                stdout_format: Some("csv".to_string()),
                dry_run: true,
                allow_binary_stdout: true,
            },
            &io,
        );

        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert_eq!(
            String::from_utf8(result.stdout.expect("csv stdout")).expect("utf8 csv"),
            "order_id,region_count,region_top_order,region_low_order,next_amount,region_percent_rank\nA1,4,A2,A5,25,0.6666666666666666\nA2,4,A2,A5,5,0\nA3,4,A2,A5,none,0.3333333333333333\nA4,1,A4,A4,15,0\nA5,4,A2,A5,none,1\n"
        );
    }

    #[test]
    fn executes_left_join_with_binding_and_suffixes() {
        let io = InMemoryDriverIo::default()
            .with_file_bytes(
                "memory/sales.csv",
                "sale_id,customer_id,amount,segment\nS1,C001,120,Direct\nS2,C999,50,Unknown\nS3,C003,200,Direct\n",
            )
            .with_file_bytes(
                "memory/customers.csv",
                "customer_id,segment\nC001,Enterprise\nC003,Consumer\n",
            );
        let prepared = prepare_source_with_io(
            "memory/main.pdl",
            r#"let customers =
  load "customers.csv"

load "sales.csv"
  | join customers on customer_id kind left
  | select sale_id, customer_id, segment, segment_right
  | sort sale_id"#,
            &io,
        );

        let result = run_prepared_with_io(
            &prepared,
            RunOptions {
                stdout_format: Some("csv".to_string()),
                dry_run: true,
                allow_binary_stdout: true,
            },
            &io,
        );

        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert_eq!(
            String::from_utf8(result.stdout.expect("csv stdout")).expect("utf8 csv"),
            "sale_id,customer_id,segment,segment_right\nS1,C001,Direct,Enterprise\nS2,C999,Unknown,\nS3,C003,Direct,Consumer\n"
        );
    }

    #[test]
    fn executes_full_join_with_unmatched_right_rows_sorted_by_key() {
        let io = InMemoryDriverIo::default()
            .with_file_bytes("memory/left.csv", "id,left_value\nB,left-b\n")
            .with_file_bytes(
                "memory/right.csv",
                "id,right_value\nC,right-c\nA,right-a\nB,right-b\n",
            );
        let prepared = prepare_source_with_io(
            "memory/main.pdl",
            r#"let right_side =
  load "right.csv"

load "left.csv"
  | join right_side on id kind full"#,
            &io,
        );

        let result = run_prepared_with_io(
            &prepared,
            RunOptions {
                stdout_format: Some("csv".to_string()),
                dry_run: true,
                allow_binary_stdout: true,
            },
            &io,
        );

        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert_eq!(
            String::from_utf8(result.stdout.expect("csv stdout")).expect("utf8 csv"),
            "id,left_value,right_value\nB,left-b,right-b\nA,,right-a\nC,,right-c\n"
        );
    }

    #[test]
    fn executes_union_by_name_and_distinct() {
        let io = InMemoryDriverIo::default()
            .with_file_bytes(
                "memory/day1.csv",
                "order_id,region,amount\nA1,North,10\nA2,South,20\n",
            )
            .with_file_bytes(
                "memory/day2.csv",
                "amount,region,order_id\n20,South,A2\n30,West,A3\n",
            );
        let prepared = prepare_source_with_io(
            "memory/main.pdl",
            r#"let day2 =
  load "day2.csv"

load "day1.csv"
  | union day2 by_name true distinct true
  | sort order_id"#,
            &io,
        );

        let result = run_prepared_with_io(
            &prepared,
            RunOptions {
                stdout_format: Some("csv".to_string()),
                dry_run: true,
                allow_binary_stdout: true,
            },
            &io,
        );

        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert_eq!(
            String::from_utf8(result.stdout.expect("csv stdout")).expect("utf8 csv"),
            "order_id,region,amount\nA1,North,10\nA2,South,20\nA3,West,30\n"
        );
    }

    #[test]
    fn incompatible_join_key_types_report_e1208() {
        let io = InMemoryDriverIo::default()
            .with_file_bytes("memory/left.csv", "id,value\n1,left\n")
            .with_file_bytes("memory/right.csv", "id,label\nA,right\n");
        let prepared = prepare_source_with_io(
            "memory/main.pdl",
            r#"let right_side =
  load "right.csv"

load "left.csv"
  | join right_side on id"#,
            &io,
        );

        let result = run_prepared_with_io(
            &prepared,
            RunOptions {
                stdout_format: Some("csv".to_string()),
                dry_run: true,
                allow_binary_stdout: true,
            },
            &io,
        );

        assert!(result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "E1208"));
        assert!(result.stdout.is_none());
    }
}
