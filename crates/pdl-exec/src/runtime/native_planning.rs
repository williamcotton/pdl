// Native eligibility checks and native pipeline orchestration extracted from
// `runtime.rs` as part of the v0.42 split. See `runtime.rs` for the
// cross-module layout overview.

use pdl_core::{codes, Diagnostic, Span};
use pdl_data::{
    DataBackend, DataFormat, DataJoinKind, DataPlan, DataSink, DataSource,
    NullsOrder as DataNullsOrder, SortDirection as DataSortDirection, SortSpec, Value,
};
use pdl_driver::{DriverIo, PreparedProgram, SinkDescriptor, SourceDescriptor};
use pdl_semantics::{
    decode_context_column_ref_ir, ContextKindIr, JoinKeyIr, JoinKindIr, NullsOrderIr, PipelineIr,
    PipelineStartIr, SortDirectionIr, StageIr,
};
use std::collections::BTreeMap;

use crate::planning::{ExecutionPlan, NativeUnsupportedReason};
use crate::runtime::native_lowering::{
    lower_data_agg_items, lower_data_expr, lower_data_mutate_items,
};
use crate::runtime::{resolve_input_format, resolve_output_format, RunResult};

pub(crate) fn try_execute_native(
    prepared: &PreparedProgram,
    ir: &pdl_semantics::ProgramIr,
    plan: &ExecutionPlan,
    context: &BTreeMap<String, Value>,
    io: &dyn DriverIo,
) -> Result<RunResult, Diagnostic> {
    check_native_program_eligibility(prepared, ir, plan, context)?;
    let main = ir.main.as_ref().ok_or_else(|| {
        unsupported_native_pipeline(
            NativeUnsupportedReason::NoRunnableMain,
            "no runnable main pipeline",
        )
    })?;
    let stdout =
        match execute_native_pipeline(prepared, ir, main, plan, context, io, &mut Vec::new())? {
            NativePipelineResult::Plan(data_plan) => {
                if let Some(stdout_format) = plan.stdout_format {
                    data_plan
                        .write_to_sink(DataSink::Bytes {
                            format: stdout_format,
                        })?
                        .ok_or_else(|| {
                            unsupported_native_pipeline(
                                NativeUnsupportedReason::NativeSinkWriter,
                                "native stdout bytes were not returned",
                            )
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

pub(crate) fn check_native_program_eligibility(
    prepared: &PreparedProgram,
    ir: &pdl_semantics::ProgramIr,
    plan: &ExecutionPlan,
    context: &BTreeMap<String, Value>,
) -> Result<(), Diagnostic> {
    if !ir.outputs.is_empty() {
        return Err(unsupported_native_pipeline(
            NativeUnsupportedReason::NamedOutputMixedEngines,
            "native execution for named outputs is deferred",
        ));
    }
    let main = ir.main.as_ref().ok_or_else(|| {
        unsupported_native_pipeline(
            NativeUnsupportedReason::NoRunnableMain,
            "no runnable main pipeline",
        )
    })?;
    check_native_pipeline_eligibility(prepared, main, plan, context)
}

pub(crate) fn check_native_pipeline_eligibility(
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
                NativeUnsupportedReason::BindingStartNotEligible,
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
            StageIr::Mutate { items, .. } => {
                lower_data_mutate_items(items, context)?;
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
                        NativeUnsupportedReason::NonTerminalSaveFanout,
                        "native save stages are supported only as terminal stages",
                    ));
                }
                check_native_save_eligibility(prepared, execution_plan, *span, format.as_deref())?;
            }
            StageIr::Join { source, .. } => {
                let binding = ir_binding(prepared, source).ok_or_else(|| {
                    Diagnostic::error(
                        codes::E1007,
                        format!("unknown binding `{source}`"),
                        Span::zero(),
                    )
                })?;
                check_native_pipeline_eligibility(
                    prepared,
                    &binding.pipeline,
                    execution_plan,
                    context,
                )?;
            }
            StageIr::Union { source, .. } => {
                let binding = ir_binding(prepared, source).ok_or_else(|| {
                    Diagnostic::error(
                        codes::E1007,
                        format!("unknown binding `{source}`"),
                        Span::zero(),
                    )
                })?;
                check_native_pipeline_eligibility(
                    prepared,
                    &binding.pipeline,
                    execution_plan,
                    context,
                )?;
            }
            StageIr::PivotLonger {
                columns,
                names_to,
                values_to,
                span,
            } => {
                if columns.is_empty() {
                    // The row runtime rejects the empty column list with
                    // `E1203`; keep the pipeline on the row engine so the
                    // row diagnostic surfaces.
                    return Err(unsupported_native_pipeline(
                        NativeUnsupportedReason::RowOnlyStage,
                        "pivot_longer requires at least one source column",
                    ));
                }
                resolve_native_column_names(columns, *span, context)?;
                resolve_native_column_name(names_to, *span, context)?;
                resolve_native_column_name(values_to, *span, context)?;
            }
            StageIr::Complete { keys, fills, span } => {
                if keys.is_empty() {
                    // Same as `pivot_longer`: the row runtime owns the
                    // `E1203` diagnostic for an empty key list.
                    return Err(unsupported_native_pipeline(
                        NativeUnsupportedReason::RowOnlyStage,
                        "complete requires at least one key column",
                    ));
                }
                resolve_native_column_names(keys, *span, context)?;
                for fill in fills {
                    resolve_native_column_name(&fill.column, fill.span, context)?;
                    if crate::planning::expr_ir_contains_window(&fill.expr) {
                        // Fill expressions evaluate against the inserted base
                        // row; window semantics over the completed frame have
                        // no row-runtime counterpart.
                        return Err(unsupported_native_pipeline(
                            NativeUnsupportedReason::RowOnlyStage,
                            "complete fill window expressions are row-only",
                        ));
                    }
                    lower_data_expr(&fill.expr, context)?;
                }
            }
            StageIr::Unsupported { .. } => {
                return Err(unsupported_native_pipeline(
                    NativeUnsupportedReason::RowOnlyStage,
                    "pipeline stage is not supported by native execution",
                ));
            }
        }
    }
    Ok(())
}

pub(crate) fn check_native_load_eligibility(
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
    let format = match &input.source {
        SourceDescriptor::Path { .. } => {
            resolve_input_format(input, explicit_format, None, None, stage_span)?
        }
        // Since v0.46 the byte-backed scan adapters make stdin CSV and
        // Parquet native alongside Arrow IPC; only JSON Lines stays
        // row-only by design.
        SourceDescriptor::Stdin => resolve_input_format(
            input,
            explicit_format,
            prepared.stdin_format.as_deref(),
            prepared.stdin_bytes.as_deref(),
            stage_span,
        )?,
    };
    if !matches!(
        format,
        DataFormat::Csv | DataFormat::Parquet | DataFormat::ArrowFile | DataFormat::ArrowStream
    ) {
        return Err(unsupported_native_pipeline(
            NativeUnsupportedReason::InputFormat,
            "input format is not supported by native execution",
        ));
    }
    Ok(())
}

pub(crate) fn check_native_save_eligibility(
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

// `DataPlan` embeds the native engine's lazy plan, which grew past the
// clippy variant-size threshold when the v0.45 reshape lowerings enabled
// additional native engine features. These results are short-lived, moved
// values; boxing would add indirection without a measurable win.
#[allow(clippy::large_enum_variant)]
pub(crate) enum NativePipelineResult {
    Plan(DataPlan),
    Completed { stdout: Option<Vec<u8>> },
}

pub(crate) fn execute_native_pipeline(
    prepared: &PreparedProgram,
    ir: &pdl_semantics::ProgramIr,
    pipeline: &PipelineIr,
    execution_plan: &ExecutionPlan,
    context: &BTreeMap<String, Value>,
    io: &dyn DriverIo,
    active_bindings: &mut Vec<String>,
) -> Result<NativePipelineResult, Diagnostic> {
    let mut plan = match &pipeline.start {
        PipelineStartIr::Load { format, span, .. } => {
            native_load_plan(prepared, *span, format.as_deref(), io)?
        }
        PipelineStartIr::Binding { .. } => {
            return Err(unsupported_native_pipeline(
                NativeUnsupportedReason::BindingStartNotEligible,
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
            StageIr::Mutate { items, .. } => {
                grouping = None;
                let items = lower_data_mutate_items(items, context)?;
                plan.mutate(&items)?
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
                        NativeUnsupportedReason::NonTerminalSaveFanout,
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
            StageIr::Join {
                source,
                source_span,
                keys,
                kind,
                span,
                ..
            } => {
                grouping = None;
                let right = execute_native_binding(
                    prepared,
                    ir,
                    NativeBindingRef {
                        name: source,
                        span: *source_span,
                    },
                    execution_plan,
                    context,
                    io,
                    active_bindings,
                )?;
                let kind = match kind {
                    JoinKindIr::Inner => DataJoinKind::Inner,
                    JoinKindIr::Left => DataJoinKind::Left,
                    JoinKindIr::Right => DataJoinKind::Right,
                    JoinKindIr::Full => DataJoinKind::Full,
                    JoinKindIr::Semi => DataJoinKind::Semi,
                    JoinKindIr::Anti => DataJoinKind::Anti,
                };
                let resolved_keys = resolve_native_join_keys(keys, *span, context)?;
                let key_refs = resolved_keys
                    .iter()
                    .map(|(left, right)| (left.as_str(), right.as_str()))
                    .collect::<Vec<_>>();
                plan.join_on_keys(right, &key_refs, kind)?
            }
            StageIr::Union {
                source,
                source_span,
                by_name,
                distinct,
                ..
            } => {
                grouping = None;
                let right = execute_native_binding(
                    prepared,
                    ir,
                    NativeBindingRef {
                        name: source,
                        span: *source_span,
                    },
                    execution_plan,
                    context,
                    io,
                    active_bindings,
                )?;
                plan.union(right, *by_name, *distinct)?
            }
            StageIr::PivotLonger {
                columns,
                names_to,
                values_to,
                span,
            } => {
                grouping = None;
                let columns = resolve_native_column_names(columns, *span, context)?;
                let names_to = resolve_native_column_name(names_to, *span, context)?;
                let values_to = resolve_native_column_name(values_to, *span, context)?;
                plan.pivot_longer(&columns, &names_to, &values_to)?
            }
            StageIr::Complete { keys, fills, span } => {
                grouping = None;
                let keys = resolve_native_column_names(keys, *span, context)?;
                let fills = fills
                    .iter()
                    .map(|fill| {
                        Ok((
                            resolve_native_column_name(&fill.column, fill.span, context)?,
                            lower_data_expr(&fill.expr, context)?,
                        ))
                    })
                    .collect::<Result<Vec<_>, Diagnostic>>()?;
                plan.complete(&keys, &fills)?
            }
            StageIr::Unsupported { .. } => {
                return Err(unsupported_native_pipeline(
                    NativeUnsupportedReason::RowOnlyStage,
                    "pipeline stage is not supported by native execution",
                ));
            }
        };
    }
    Ok(NativePipelineResult::Plan(plan))
}

pub(crate) fn execute_native_binding(
    prepared: &PreparedProgram,
    ir: &pdl_semantics::ProgramIr,
    binding_ref: NativeBindingRef<'_>,
    execution_plan: &ExecutionPlan,
    context: &BTreeMap<String, Value>,
    io: &dyn DriverIo,
    active_bindings: &mut Vec<String>,
) -> Result<DataPlan, Diagnostic> {
    let name = binding_ref.name;
    if let Some(index) = active_bindings.iter().position(|active| active == name) {
        let mut path = active_bindings[index..].to_vec();
        path.push(name.to_string());
        return Err(Diagnostic::error(
            codes::E1501,
            format!("binding dependency cycle: {}", path.join(" -> ")),
            binding_ref.span,
        ));
    }
    let binding = ir
        .bindings
        .iter()
        .find(|binding| binding.name == name)
        .ok_or_else(|| {
            Diagnostic::error(
                codes::E1007,
                format!("unknown binding `{name}`"),
                binding_ref.span,
            )
        })?;
    active_bindings.push(name.to_string());
    let result = execute_native_pipeline(
        prepared,
        ir,
        &binding.pipeline,
        execution_plan,
        context,
        io,
        active_bindings,
    );
    active_bindings.pop();
    match result? {
        NativePipelineResult::Plan(plan) => Ok(plan),
        NativePipelineResult::Completed { .. } => Err(unsupported_native_pipeline(
            NativeUnsupportedReason::NonTerminalSaveFanout,
            "native binding outputs must remain table plans",
        )),
    }
}

pub(crate) struct NativeBindingRef<'a> {
    pub(crate) name: &'a str,
    pub(crate) span: Span,
}

pub(crate) fn ir_binding<'a>(
    prepared: &'a PreparedProgram,
    name: &str,
) -> Option<&'a pdl_semantics::BindingIr> {
    prepared
        .analysis
        .ir
        .as_ref()?
        .bindings
        .iter()
        .find(|binding| binding.name == name)
}

pub(crate) fn execute_native_save(
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
                    unsupported_native_pipeline(
                        NativeUnsupportedReason::NativeSinkWriter,
                        "native stdout bytes were not returned",
                    )
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

pub(crate) fn native_load_plan(
    prepared: &PreparedProgram,
    stage_span: Span,
    explicit_format: Option<&str>,
    io: &dyn DriverIo,
) -> Result<DataPlan, Diagnostic> {
    let Some(input) = prepared.driver_plan.input_for_stage_span(stage_span) else {
        return Err(Diagnostic::error(
            codes::E1505,
            "driver source facts are unavailable for native execution",
            stage_span,
        ));
    };
    match &input.source {
        SourceDescriptor::Path { resolved_path, .. } => {
            let format = resolve_input_format(input, explicit_format, None, None, stage_span)?;
            if resolved_path.exists() {
                return DataPlan::scan_with_backend(
                    DataSource::Path {
                        path: resolved_path,
                        format,
                    },
                    DataBackend::NativePolars,
                );
            }
            if !matches!(
                format,
                DataFormat::Csv
                    | DataFormat::Parquet
                    | DataFormat::ArrowFile
                    | DataFormat::ArrowStream
            ) {
                return Err(unsupported_native_pipeline(
                    NativeUnsupportedReason::InputFormat,
                    "input format is not supported by native execution",
                ));
            }
            let bytes = io.read_path_bytes(resolved_path)?;
            DataPlan::scan_with_backend(
                DataSource::Bytes {
                    logical_path: resolved_path,
                    format,
                    bytes: &bytes,
                },
                DataBackend::NativePolars,
            )
        }
        SourceDescriptor::Stdin => {
            let owned_bytes;
            let bytes = if let Some(bytes) = prepared.stdin_bytes.as_deref() {
                bytes
            } else {
                owned_bytes = io.read_stdin_bytes()?;
                &owned_bytes
            };
            let format = resolve_input_format(
                input,
                explicit_format,
                prepared.stdin_format.as_deref(),
                Some(bytes),
                stage_span,
            )?;
            if !matches!(
                format,
                DataFormat::Csv
                    | DataFormat::Parquet
                    | DataFormat::ArrowFile
                    | DataFormat::ArrowStream
            ) {
                return Err(unsupported_native_pipeline(
                    NativeUnsupportedReason::InputFormat,
                    "input format is not supported by native execution",
                ));
            }
            DataPlan::scan_with_backend(
                DataSource::Bytes {
                    logical_path: std::path::Path::new("stdin"),
                    format,
                    bytes,
                },
                DataBackend::NativePolars,
            )
        }
    }
}

pub(crate) fn resolve_native_column_names(
    columns: &[String],
    span: Span,
    context: &BTreeMap<String, Value>,
) -> Result<Vec<String>, Diagnostic> {
    columns
        .iter()
        .map(|column| resolve_native_column_name(column, span, context))
        .collect()
}

pub(crate) fn resolve_native_column_name(
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

pub(crate) fn resolve_native_join_keys(
    keys: &[JoinKeyIr],
    span: Span,
    context: &BTreeMap<String, Value>,
) -> Result<Vec<(String, String)>, Diagnostic> {
    keys.iter()
        .map(|key| {
            Ok((
                resolve_native_column_name(&key.left, span, context)?,
                resolve_native_column_name(&key.right, span, context)?,
            ))
        })
        .collect()
}

pub(crate) fn unsupported_native_pipeline(
    reason: NativeUnsupportedReason,
    detail: &'static str,
) -> Diagnostic {
    Diagnostic::error(
        codes::E1211,
        format!("native execution unsupported [{}]: {detail}", reason.code()),
        Span::zero(),
    )
}

fn context_kind_label(kind: ContextKindIr) -> &'static str {
    match kind {
        ContextKindIr::Param => "parameter",
        ContextKindIr::State => "state",
    }
}
