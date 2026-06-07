use pdl_core::{codes, has_errors, Diagnostic, Span};
use pdl_data::DataFormat;
use pdl_driver::{PreparedProgram, SinkDescriptor, SourceDescriptor};
use pdl_semantics::{
    AggItemIr, ExprIr, JoinKindIr, PipelineIr, PipelineStartIr, ProgramIr, StageIr,
};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlanningOptions {
    pub stdout_format: Option<String>,
    pub dry_run: bool,
    pub allow_binary_stdout: bool,
    pub engine: PlannedEngine,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExecutionPlan {
    pub steps: Vec<ExecutionPlanStep>,
    pub stdout_format: Option<DataFormat>,
    pub dry_run: bool,
    pub observability: PlanObservability,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ExecutionPlanStep {
    Output { name: String },
    Load { source: String, format: String },
    Binding { name: String },
    Transform { stage: String },
    Save { sink: String, format: String },
    Stdout { format: String },
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PlannedEngine {
    #[default]
    Auto,
    Row,
    Native,
}

impl PlannedEngine {
    pub fn as_str(self) -> &'static str {
        match self {
            PlannedEngine::Auto => "auto",
            PlannedEngine::Row => "row",
            PlannedEngine::Native => "native",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlanObservability {
    pub requested_engine: PlannedEngine,
    pub selected_engine: PlannedEngine,
    pub eligible_engine: PlannedEngine,
    pub native_eligible: bool,
    pub fallback_reason: Option<NativeUnsupportedReason>,
    pub source_boundary: Option<String>,
    pub input_format: Option<String>,
    pub output_format: Option<String>,
    pub sink_strategy: SinkStrategy,
    pub blocking_stages: Vec<String>,
    pub row_materialization: bool,
    pub required_source_columns: Option<Vec<String>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SinkStrategy {
    None,
    RowFormatWriter,
    NativeDirectWriter,
    BytesSink,
    StdoutWriter,
    FilesystemWriter,
}

impl SinkStrategy {
    pub fn as_str(self) -> &'static str {
        match self {
            SinkStrategy::None => "none",
            SinkStrategy::RowFormatWriter => "row-format-writer",
            SinkStrategy::NativeDirectWriter => "native-direct-writer",
            SinkStrategy::BytesSink => "bytes-sink",
            SinkStrategy::StdoutWriter => "stdout-writer",
            SinkStrategy::FilesystemWriter => "filesystem-writer",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeUnsupportedReason {
    NamedOutputs,
    NoRunnableMain,
    BindingStart,
    SourceBoundary,
    SourcePathMissing,
    InputFormat,
    SaveNotTerminal,
    Stage,
    ScalarFunction,
    ScalarFunctionArity,
    AggregateFunction,
    AggregateArity,
    WindowExpression,
    DynamicColumn,
    DriverFacts,
    NativeStdoutBytes,
}

impl NativeUnsupportedReason {
    pub fn code(self) -> &'static str {
        match self {
            NativeUnsupportedReason::NamedOutputs => "named-outputs",
            NativeUnsupportedReason::NoRunnableMain => "no-runnable-main",
            NativeUnsupportedReason::BindingStart => "binding-start",
            NativeUnsupportedReason::SourceBoundary => "source-boundary",
            NativeUnsupportedReason::SourcePathMissing => "source-path-missing",
            NativeUnsupportedReason::InputFormat => "input-format",
            NativeUnsupportedReason::SaveNotTerminal => "save-not-terminal",
            NativeUnsupportedReason::Stage => "stage",
            NativeUnsupportedReason::ScalarFunction => "scalar-function",
            NativeUnsupportedReason::ScalarFunctionArity => "scalar-function-arity",
            NativeUnsupportedReason::AggregateFunction => "aggregate-function",
            NativeUnsupportedReason::AggregateArity => "aggregate-arity",
            NativeUnsupportedReason::WindowExpression => "window-expression",
            NativeUnsupportedReason::DynamicColumn => "dynamic-column",
            NativeUnsupportedReason::DriverFacts => "driver-facts",
            NativeUnsupportedReason::NativeStdoutBytes => "native-stdout-bytes",
        }
    }
}

pub fn plan_prepared(
    prepared: &PreparedProgram,
    options: PlanningOptions,
) -> Result<ExecutionPlan, Vec<Diagnostic>> {
    let mut diagnostics = prepared.diagnostics();
    if has_errors(&diagnostics) {
        return Err(diagnostics);
    }

    let stdout_format = if let Some(format) = &options.stdout_format {
        let Some(data_format) = DataFormat::from_name(format) else {
            diagnostics.push(Diagnostic::error(
                codes::E1705,
                format!("stdout format `{format}` is not supported in 0.26.0"),
                Span::zero(),
            ));
            return Err(diagnostics);
        };
        if !data_format.is_supported_output() {
            diagnostics.push(Diagnostic::error(
                codes::E1705,
                format!(
                    "stdout format `{}` is not supported in 0.26.0",
                    data_format.canonical_name()
                ),
                Span::zero(),
            ));
            return Err(diagnostics);
        }
        if data_format.is_binary() && !options.allow_binary_stdout {
            diagnostics.push(Diagnostic::error(
                codes::E1705,
                format!(
                    "{} stdout is not supported by this host",
                    data_format.canonical_name()
                ),
                Span::zero(),
            ));
            return Err(diagnostics);
        }
        Some(data_format)
    } else {
        None
    };

    let Some(ir) = prepared.analysis.ir.as_ref() else {
        diagnostics.push(Diagnostic::error(
            codes::E1505,
            "semantic IR is unavailable for planning",
            Span::zero(),
        ));
        return Err(diagnostics);
    };

    let mut steps = Vec::new();
    let mut planned_bindings = BTreeSet::new();
    if ir.outputs.is_empty() {
        let Some(main) = &ir.main else {
            diagnostics.push(Diagnostic::error(
                codes::E1502,
                "no runnable main pipeline",
                Span::zero(),
            ));
            return Err(diagnostics);
        };
        if let Err(diagnostic) =
            append_pipeline_steps(prepared, ir, main, &mut planned_bindings, &mut steps)
        {
            diagnostics.push(diagnostic);
            return Err(diagnostics);
        }
    } else {
        if stdout_format.is_some() && ir.outputs.len() > 1 {
            diagnostics.push(Diagnostic::error(
                codes::E1607,
                "multiple output declarations cannot share one stdout stream",
                Span::zero(),
            ));
            return Err(diagnostics);
        }
        if prepared.driver_plan.stdout_writes().len() > 1 {
            diagnostics.push(Diagnostic::error(
                codes::E1607,
                "multiple output declarations cannot write separate tables to stdout",
                Span::zero(),
            ));
            return Err(diagnostics);
        }
        for output in &ir.outputs {
            steps.push(ExecutionPlanStep::Output {
                name: output.name.clone(),
            });
            if let Err(diagnostic) = append_pipeline_steps(
                prepared,
                ir,
                &output.pipeline,
                &mut planned_bindings,
                &mut steps,
            ) {
                diagnostics.push(diagnostic);
                return Err(diagnostics);
            }
        }
    }

    if let Some(format) = stdout_format {
        steps.push(ExecutionPlanStep::Stdout {
            format: format.canonical_name().to_string(),
        });
    }

    let observability = build_observability(prepared, ir, &steps, stdout_format, &options);

    Ok(ExecutionPlan {
        steps,
        stdout_format,
        dry_run: options.dry_run,
        observability,
    })
}

fn append_pipeline_steps(
    prepared: &PreparedProgram,
    ir: &ProgramIr,
    pipeline: &PipelineIr,
    planned_bindings: &mut BTreeSet<String>,
    steps: &mut Vec<ExecutionPlanStep>,
) -> Result<(), Diagnostic> {
    match &pipeline.start {
        PipelineStartIr::Load { span, .. } => steps.push(load_step(prepared, *span)?),
        PipelineStartIr::Binding { name, span } => {
            append_binding_steps(prepared, ir, name, *span, planned_bindings, steps)?;
        }
    }

    for stage in &pipeline.stages {
        match stage {
            StageIr::Join {
                source,
                source_span,
                ..
            }
            | StageIr::Union {
                source,
                source_span,
                ..
            } => {
                append_binding_steps(prepared, ir, source, *source_span, planned_bindings, steps)?;
                steps.push(ExecutionPlanStep::Transform {
                    stage: stage_name(stage).to_string(),
                });
            }
            StageIr::Save { span, .. } => steps.push(save_step(prepared, *span)?),
            _ => steps.push(ExecutionPlanStep::Transform {
                stage: stage_name(stage).to_string(),
            }),
        }
    }
    Ok(())
}

fn append_binding_steps(
    prepared: &PreparedProgram,
    ir: &ProgramIr,
    name: &str,
    span: Span,
    planned_bindings: &mut BTreeSet<String>,
    steps: &mut Vec<ExecutionPlanStep>,
) -> Result<(), Diagnostic> {
    if !planned_bindings.insert(name.to_string()) {
        return Ok(());
    }
    let binding = ir
        .bindings
        .iter()
        .find(|binding| binding.name == name)
        .ok_or_else(|| {
            Diagnostic::error(codes::E1007, format!("unknown binding `{name}`"), span)
        })?;
    append_pipeline_steps(prepared, ir, &binding.pipeline, planned_bindings, steps)?;
    steps.push(ExecutionPlanStep::Binding {
        name: name.to_string(),
    });
    Ok(())
}

fn load_step(prepared: &PreparedProgram, span: Span) -> Result<ExecutionPlanStep, Diagnostic> {
    let input = prepared
        .driver_plan
        .input_for_stage_span(span)
        .ok_or_else(|| {
            Diagnostic::error(
                codes::E1505,
                "driver source facts are unavailable for planning",
                span,
            )
        })?;
    Ok(ExecutionPlanStep::Load {
        source: match &input.source {
            SourceDescriptor::Path { logical_path, .. } => logical_path.clone(),
            SourceDescriptor::Stdin => "stdin".to_string(),
        },
        format: input.format.effective_name(),
    })
}

fn save_step(prepared: &PreparedProgram, span: Span) -> Result<ExecutionPlanStep, Diagnostic> {
    let sink = prepared
        .driver_plan
        .sink_for_stage_span(span)
        .ok_or_else(|| {
            Diagnostic::error(
                codes::E1505,
                "driver sink facts are unavailable for planning",
                span,
            )
        })?;
    Ok(ExecutionPlanStep::Save {
        sink: match &sink.sink {
            SinkDescriptor::Path { logical_path, .. } => logical_path.clone(),
            SinkDescriptor::Stdout => "stdout".to_string(),
        },
        format: sink.format.effective_name(),
    })
}

fn stage_name(stage: &StageIr) -> &'static str {
    match stage {
        StageIr::Filter { .. } => "filter",
        StageIr::Select { .. } => "select",
        StageIr::Drop { .. } => "drop",
        StageIr::Rename { .. } => "rename",
        StageIr::Mutate { .. } => "mutate",
        StageIr::GroupBy { .. } => "group_by",
        StageIr::Agg { .. } => "agg",
        StageIr::Sort { .. } => "sort",
        StageIr::Limit { .. } => "limit",
        StageIr::Join { .. } => "join",
        StageIr::Union { .. } => "union",
        StageIr::Distinct { .. } => "distinct",
        StageIr::PivotLonger { .. } => "pivot_longer",
        StageIr::Complete { .. } => "complete",
        StageIr::Save { .. } => "save",
        StageIr::Unsupported { name, .. } => match name.as_str() {
            "join" => "join",
            "union" => "union",
            _ => "unknown",
        },
    }
}

fn build_observability(
    prepared: &PreparedProgram,
    ir: &ProgramIr,
    steps: &[ExecutionPlanStep],
    stdout_format: Option<DataFormat>,
    options: &PlanningOptions,
) -> PlanObservability {
    let native_reason = native_unsupported_reason(prepared, ir);
    let native_eligible = native_reason.is_none();
    let eligible_engine = if native_eligible {
        PlannedEngine::Native
    } else {
        PlannedEngine::Row
    };
    let selected_engine = match options.engine {
        PlannedEngine::Auto => eligible_engine,
        PlannedEngine::Row => PlannedEngine::Row,
        PlannedEngine::Native => PlannedEngine::Native,
    };
    let output_format = plan_output_format(prepared, stdout_format, steps);
    let sink_strategy = sink_strategy(prepared, stdout_format, selected_engine, &output_format);
    let row_materialization = selected_engine == PlannedEngine::Row
        || matches!(output_format.as_deref(), Some("csv" | "jsonl" | "ndjson"));

    PlanObservability {
        requested_engine: options.engine,
        selected_engine,
        eligible_engine,
        native_eligible,
        fallback_reason: native_reason,
        source_boundary: prepared
            .driver_plan
            .inputs
            .first()
            .map(|input| match input.source {
                SourceDescriptor::Path { .. } => "path".to_string(),
                SourceDescriptor::Stdin => "stdin".to_string(),
            }),
        input_format: prepared
            .driver_plan
            .inputs
            .first()
            .map(|input| input.format.effective_name()),
        output_format,
        sink_strategy,
        blocking_stages: ir.main.as_ref().map(blocking_stages).unwrap_or_default(),
        row_materialization,
        required_source_columns: ir
            .main
            .as_ref()
            .and_then(|pipeline| required_source_columns(prepared, pipeline)),
    }
}

fn plan_output_format(
    prepared: &PreparedProgram,
    stdout_format: Option<DataFormat>,
    steps: &[ExecutionPlanStep],
) -> Option<String> {
    if let Some(format) = stdout_format {
        return Some(format.canonical_name().to_string());
    }
    steps
        .iter()
        .rev()
        .find_map(|step| match step {
            ExecutionPlanStep::Save { format, .. } | ExecutionPlanStep::Stdout { format } => {
                Some(format.clone())
            }
            ExecutionPlanStep::Output { .. }
            | ExecutionPlanStep::Load { .. }
            | ExecutionPlanStep::Binding { .. }
            | ExecutionPlanStep::Transform { .. } => None,
        })
        .or_else(|| {
            prepared
                .driver_plan
                .sinks
                .last()
                .map(|sink| sink.format.effective_name())
        })
}

fn sink_strategy(
    prepared: &PreparedProgram,
    stdout_format: Option<DataFormat>,
    selected_engine: PlannedEngine,
    output_format: &Option<String>,
) -> SinkStrategy {
    let format = output_format
        .as_deref()
        .and_then(DataFormat::from_name)
        .unwrap_or(DataFormat::Csv);
    if stdout_format.is_some() {
        return if selected_engine == PlannedEngine::Native && format.is_binary() {
            SinkStrategy::BytesSink
        } else {
            SinkStrategy::RowFormatWriter
        };
    }
    let Some(sink) = prepared.driver_plan.sinks.last() else {
        return SinkStrategy::None;
    };
    if selected_engine == PlannedEngine::Native && format.is_binary() {
        return SinkStrategy::NativeDirectWriter;
    }
    if matches!(format, DataFormat::Csv | DataFormat::JsonLines) {
        return SinkStrategy::RowFormatWriter;
    }
    match sink.sink {
        SinkDescriptor::Stdout => SinkStrategy::StdoutWriter,
        SinkDescriptor::Path { .. } => SinkStrategy::FilesystemWriter,
    }
}

fn native_unsupported_reason(
    prepared: &PreparedProgram,
    ir: &ProgramIr,
) -> Option<NativeUnsupportedReason> {
    if !ir.outputs.is_empty() {
        return Some(NativeUnsupportedReason::NamedOutputs);
    }
    let Some(main) = ir.main.as_ref() else {
        return Some(NativeUnsupportedReason::NoRunnableMain);
    };
    native_pipeline_unsupported_reason(prepared, main)
}

fn native_pipeline_unsupported_reason(
    prepared: &PreparedProgram,
    pipeline: &PipelineIr,
) -> Option<NativeUnsupportedReason> {
    match &pipeline.start {
        PipelineStartIr::Load { format, span, .. } => {
            if let Some(reason) = native_load_unsupported_reason(prepared, *span, format.as_deref())
            {
                return Some(reason);
            }
        }
        PipelineStartIr::Binding { .. } => return Some(NativeUnsupportedReason::BindingStart),
    }

    for (stage_index, stage) in pipeline.stages.iter().enumerate() {
        let is_terminal = stage_index + 1 == pipeline.stages.len();
        match stage {
            StageIr::Filter { expr, .. } => {
                if let Some(reason) = native_expr_unsupported_reason(expr) {
                    return Some(reason);
                }
            }
            StageIr::Select { .. }
            | StageIr::Drop { .. }
            | StageIr::Rename { .. }
            | StageIr::GroupBy { .. }
            | StageIr::Sort { .. }
            | StageIr::Limit { .. }
            | StageIr::Distinct { .. } => {}
            StageIr::Mutate { items, .. } => {
                for item in items {
                    native_expr_unsupported_reason(&item.expr)?;
                }
            }
            StageIr::Agg { items, .. } => {
                for item in items {
                    native_agg_unsupported_reason(item)?;
                }
            }
            StageIr::Save { .. } if !is_terminal => {
                return Some(NativeUnsupportedReason::SaveNotTerminal);
            }
            StageIr::Save { .. } => {}
            StageIr::Join { source, kind, .. } => {
                if !matches!(
                    kind,
                    JoinKindIr::Inner | JoinKindIr::Left | JoinKindIr::Semi | JoinKindIr::Anti
                ) {
                    return Some(NativeUnsupportedReason::Stage);
                }
                let Some(binding) = prepared
                    .analysis
                    .ir
                    .as_ref()
                    .and_then(|ir| ir.bindings.iter().find(|binding| binding.name == *source))
                else {
                    return Some(NativeUnsupportedReason::DriverFacts);
                };
                if let Some(reason) =
                    native_pipeline_unsupported_reason(prepared, &binding.pipeline)
                {
                    return Some(reason);
                }
            }
            StageIr::Union { source, .. } => {
                let Some(binding) = prepared
                    .analysis
                    .ir
                    .as_ref()
                    .and_then(|ir| ir.bindings.iter().find(|binding| binding.name == *source))
                else {
                    return Some(NativeUnsupportedReason::DriverFacts);
                };
                if let Some(reason) =
                    native_pipeline_unsupported_reason(prepared, &binding.pipeline)
                {
                    return Some(reason);
                }
            }
            StageIr::PivotLonger { .. }
            | StageIr::Complete { .. }
            | StageIr::Unsupported { .. } => return Some(NativeUnsupportedReason::Stage),
        }
    }
    None
}

fn native_load_unsupported_reason(
    prepared: &PreparedProgram,
    stage_span: Span,
    explicit_format: Option<&str>,
) -> Option<NativeUnsupportedReason> {
    let Some(input) = prepared.driver_plan.input_for_stage_span(stage_span) else {
        return Some(NativeUnsupportedReason::DriverFacts);
    };
    let format = explicit_format
        .and_then(DataFormat::from_name)
        .or_else(|| {
            if matches!(input.source, SourceDescriptor::Stdin) {
                prepared
                    .stdin_format
                    .as_deref()
                    .and_then(DataFormat::from_name)
            } else {
                None
            }
        })
        .or(input.format.inferred_from_path)
        .or_else(|| {
            if matches!(input.source, SourceDescriptor::Stdin) {
                prepared
                    .stdin_bytes
                    .as_deref()
                    .and_then(|bytes| pdl_data::sniff_format_from_bytes(bytes).ok())
            } else {
                None
            }
        })
        .unwrap_or(DataFormat::Csv);
    match input.source {
        SourceDescriptor::Path { .. } => {
            if matches!(
                format,
                DataFormat::Csv
                    | DataFormat::Parquet
                    | DataFormat::ArrowFile
                    | DataFormat::ArrowStream
            ) {
                None
            } else {
                Some(NativeUnsupportedReason::InputFormat)
            }
        }
        SourceDescriptor::Stdin => {
            if matches!(format, DataFormat::ArrowFile | DataFormat::ArrowStream) {
                None
            } else {
                Some(NativeUnsupportedReason::SourceBoundary)
            }
        }
    }
}

fn native_agg_unsupported_reason(item: &AggItemIr) -> Option<NativeUnsupportedReason> {
    match item.function.as_str() {
        "count" if item.args.is_empty() => None,
        "count" | "sum" | "mean" | "min" | "max" | "count_distinct" => {
            let [arg] = item.args.as_slice() else {
                return Some(NativeUnsupportedReason::AggregateArity);
            };
            native_expr_unsupported_reason(arg)
        }
        _ => Some(NativeUnsupportedReason::AggregateFunction),
    }
}

fn native_expr_unsupported_reason(expr: &ExprIr) -> Option<NativeUnsupportedReason> {
    match expr {
        ExprIr::Quoted { .. }
        | ExprIr::Number { .. }
        | ExprIr::Bool { .. }
        | ExprIr::Null { .. }
        | ExprIr::Ident { .. }
        | ExprIr::Context { .. } => None,
        ExprIr::Unary { expr, .. } => native_expr_unsupported_reason(expr),
        ExprIr::Binary { left, right, .. } => {
            native_expr_unsupported_reason(left).or_else(|| native_expr_unsupported_reason(right))
        }
        ExprIr::Window { .. } => Some(NativeUnsupportedReason::WindowExpression),
        ExprIr::Call { name, args, .. } => {
            if name == "col" {
                return match args.as_slice() {
                    [ExprIr::Quoted { .. } | ExprIr::Context { .. }] => None,
                    [_] => Some(NativeUnsupportedReason::DynamicColumn),
                    _ => Some(NativeUnsupportedReason::ScalarFunctionArity),
                };
            }
            match name.as_str() {
                "is_null" | "not_null" | "lower" | "upper" | "trim" | "to_number" | "abs" => {
                    let [arg] = args.as_slice() else {
                        return Some(NativeUnsupportedReason::ScalarFunctionArity);
                    };
                    native_expr_unsupported_reason(arg)
                }
                "coalesce" | "concat" => args.iter().find_map(native_expr_unsupported_reason),
                "if_else" => match args.as_slice() {
                    [condition, when_true, when_false] => native_expr_unsupported_reason(condition)
                        .or_else(|| native_expr_unsupported_reason(when_true))
                        .or_else(|| native_expr_unsupported_reason(when_false)),
                    _ => Some(NativeUnsupportedReason::ScalarFunctionArity),
                },
                "round" => match args.as_slice() {
                    [arg] => native_expr_unsupported_reason(arg),
                    [arg, ExprIr::Number { .. }] => native_expr_unsupported_reason(arg),
                    [_, _] => Some(NativeUnsupportedReason::ScalarFunctionArity),
                    _ => Some(NativeUnsupportedReason::ScalarFunctionArity),
                },
                _ => Some(NativeUnsupportedReason::ScalarFunction),
            }
        }
    }
}

fn blocking_stages(pipeline: &PipelineIr) -> Vec<String> {
    pipeline
        .stages
        .iter()
        .filter_map(|stage| match stage {
            StageIr::Agg { .. } => Some("agg"),
            StageIr::Sort { .. } => Some("sort"),
            StageIr::Distinct { .. } => Some("distinct"),
            StageIr::Join { .. } => Some("join"),
            StageIr::Union { .. } => Some("union"),
            StageIr::PivotLonger { .. } => Some("pivot_longer"),
            StageIr::Complete { .. } => Some("complete"),
            StageIr::Filter { .. }
            | StageIr::Select { .. }
            | StageIr::Drop { .. }
            | StageIr::Rename { .. }
            | StageIr::Mutate { .. }
            | StageIr::GroupBy { .. }
            | StageIr::Limit { .. }
            | StageIr::Save { .. }
            | StageIr::Unsupported { .. } => None,
        })
        .map(ToString::to_string)
        .collect()
}

fn required_source_columns(
    prepared: &PreparedProgram,
    pipeline: &PipelineIr,
) -> Option<Vec<String>> {
    let source_columns = prepared
        .analysis
        .traces
        .iter()
        .find_map(|trace| trace.input_schema.clone())?;
    let mut required: Option<BTreeSet<String>> = None;
    let mut pending_group_keys: Vec<String> = Vec::new();

    for stage in pipeline.stages.iter().rev() {
        match stage {
            StageIr::Save { .. } | StageIr::Limit { .. } => {}
            StageIr::Sort { items, .. } => {
                if let Some(required) = &mut required {
                    required.extend(items.iter().map(|item| item.column.clone()));
                }
            }
            StageIr::Distinct { columns, .. } => {
                if let Some(required) = &mut required {
                    required.extend(columns.iter().cloned());
                }
            }
            StageIr::Agg { items, .. } => {
                let mut next = BTreeSet::new();
                next.extend(pending_group_keys.iter().cloned());
                for item in items {
                    for arg in &item.args {
                        collect_expr_columns(arg, &mut next);
                    }
                }
                required = Some(next);
            }
            StageIr::GroupBy { columns, .. } => {
                pending_group_keys = columns.clone();
                if let Some(required) = &mut required {
                    required.extend(columns.iter().cloned());
                }
            }
            StageIr::Mutate { items, .. } => {
                if let Some(current) = required.take() {
                    let targets = items
                        .iter()
                        .map(|item| item.column.clone())
                        .collect::<BTreeSet<_>>();
                    let mut next = current
                        .iter()
                        .filter(|column| !targets.contains(*column))
                        .cloned()
                        .collect::<BTreeSet<_>>();
                    for item in items {
                        if current.contains(&item.column) {
                            collect_expr_columns(&item.expr, &mut next);
                        }
                    }
                    required = Some(next);
                }
            }
            StageIr::Rename { items, .. } => {
                if let Some(current) = required.take() {
                    let mut next = current.clone();
                    for item in items {
                        if current.contains(&item.new) {
                            next.remove(&item.new);
                            next.insert(item.old.clone());
                        }
                    }
                    required = Some(next);
                }
            }
            StageIr::Drop { columns, .. } => {
                if required.is_none() {
                    let mut next = source_columns.iter().cloned().collect::<BTreeSet<_>>();
                    for column in columns {
                        next.remove(column);
                    }
                    required = Some(next);
                }
            }
            StageIr::Select { items, .. } => {
                let selected = items
                    .iter()
                    .filter(|item| {
                        required
                            .as_ref()
                            .map(|required| required.contains(&item.output))
                            .unwrap_or(true)
                    })
                    .map(|item| item.source.clone())
                    .collect::<BTreeSet<_>>();
                required = Some(selected);
            }
            StageIr::Filter { expr, .. } => {
                let mut next = required
                    .unwrap_or_else(|| source_columns.iter().cloned().collect::<BTreeSet<_>>());
                collect_expr_columns(expr, &mut next);
                required = Some(next);
            }
            StageIr::Join { .. }
            | StageIr::Union { .. }
            | StageIr::PivotLonger { .. }
            | StageIr::Complete { .. }
            | StageIr::Unsupported { .. } => {
                return None;
            }
        }
    }

    let required = required.unwrap_or_else(|| source_columns.iter().cloned().collect());
    Some(required.into_iter().collect())
}

fn collect_expr_columns(expr: &ExprIr, columns: &mut BTreeSet<String>) {
    match expr {
        ExprIr::Ident { value, .. } => {
            columns.insert(value.clone());
        }
        ExprIr::Call { name, args, .. } if name == "col" => {
            if let [ExprIr::Quoted { value, .. }] = args.as_slice() {
                columns.insert(value.clone());
            } else {
                for arg in args {
                    collect_expr_columns(arg, columns);
                }
            }
        }
        ExprIr::Call { args, .. } | ExprIr::Window { args, .. } => {
            for arg in args {
                collect_expr_columns(arg, columns);
            }
        }
        ExprIr::Unary { expr, .. } => collect_expr_columns(expr, columns),
        ExprIr::Binary { left, right, .. } => {
            collect_expr_columns(left, columns);
            collect_expr_columns(right, columns);
        }
        ExprIr::Quoted { .. }
        | ExprIr::Number { .. }
        | ExprIr::Bool { .. }
        | ExprIr::Null { .. }
        | ExprIr::Context { .. } => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdl_driver::{prepare_source_with_io, InMemoryDriverIo};

    #[test]
    fn planning_records_sources_transforms_and_stdout_without_emitting() {
        let io = InMemoryDriverIo::default().with_schema("memory/sales.csv", ["amount", "region"]);
        let prepared = prepare_source_with_io(
            "memory/main.pdl",
            r#"load "sales.csv" | filter amount > 0 | select region"#,
            &io,
        );

        let plan = plan_prepared(
            &prepared,
            PlanningOptions {
                stdout_format: Some("csv".to_string()),
                dry_run: true,
                allow_binary_stdout: true,
                engine: PlannedEngine::Auto,
            },
        )
        .expect("execution plan");

        assert_eq!(
            plan.steps,
            vec![
                ExecutionPlanStep::Load {
                    source: "sales.csv".to_string(),
                    format: "csv".to_string(),
                },
                ExecutionPlanStep::Transform {
                    stage: "filter".to_string(),
                },
                ExecutionPlanStep::Transform {
                    stage: "select".to_string(),
                },
                ExecutionPlanStep::Stdout {
                    format: "csv".to_string(),
                },
            ]
        );
        assert_eq!(plan.observability.requested_engine, PlannedEngine::Auto);
        assert_eq!(plan.observability.selected_engine, PlannedEngine::Native);
        assert_eq!(plan.observability.eligible_engine, PlannedEngine::Native);
        assert_eq!(plan.observability.fallback_reason, None);
        assert_eq!(
            plan.observability.required_source_columns,
            Some(vec!["amount".to_string(), "region".to_string()])
        );
        assert_eq!(
            plan.observability.sink_strategy,
            SinkStrategy::RowFormatWriter
        );
        assert!(plan.observability.row_materialization);
    }

    #[test]
    fn planning_records_binding_dependencies_once_before_multi_input_stage() {
        let io = InMemoryDriverIo::default()
            .with_schema("memory/sales.csv", ["customer_id", "amount"])
            .with_schema("memory/customers.csv", ["customer_id", "segment"]);
        let prepared = prepare_source_with_io(
            "memory/main.pdl",
            r#"let customers =
  load "customers.csv"

load "sales.csv"
  | join customers on customer_id
  | join customers on customer_id"#,
            &io,
        );

        let plan = plan_prepared(
            &prepared,
            PlanningOptions {
                stdout_format: Some("csv".to_string()),
                dry_run: true,
                allow_binary_stdout: true,
                engine: PlannedEngine::Auto,
            },
        )
        .expect("execution plan");

        assert_eq!(
            plan.steps,
            vec![
                ExecutionPlanStep::Load {
                    source: "sales.csv".to_string(),
                    format: "csv".to_string(),
                },
                ExecutionPlanStep::Load {
                    source: "customers.csv".to_string(),
                    format: "csv".to_string(),
                },
                ExecutionPlanStep::Binding {
                    name: "customers".to_string(),
                },
                ExecutionPlanStep::Transform {
                    stage: "join".to_string(),
                },
                ExecutionPlanStep::Transform {
                    stage: "join".to_string(),
                },
                ExecutionPlanStep::Stdout {
                    format: "csv".to_string(),
                },
            ]
        );
        assert_eq!(plan.observability.fallback_reason, None);
        assert_eq!(plan.observability.selected_engine, PlannedEngine::Native);
    }

    #[test]
    fn forced_native_plan_reports_unsupported_reason_without_selecting_rows() {
        let io = InMemoryDriverIo::default()
            .with_schema("memory/sales.csv", ["customer_id", "amount"])
            .with_schema("memory/customers.csv", ["customer_id", "segment"]);
        let prepared = prepare_source_with_io(
            "memory/main.pdl",
            r#"let customers =
  load "customers.csv"

load "sales.csv"
  | join customers on customer_id kind full"#,
            &io,
        );

        let plan = plan_prepared(
            &prepared,
            PlanningOptions {
                stdout_format: Some("csv".to_string()),
                dry_run: true,
                allow_binary_stdout: true,
                engine: PlannedEngine::Native,
            },
        )
        .expect("execution plan");

        assert_eq!(plan.observability.requested_engine, PlannedEngine::Native);
        assert_eq!(plan.observability.selected_engine, PlannedEngine::Native);
        assert_eq!(plan.observability.eligible_engine, PlannedEngine::Row);
        assert_eq!(
            plan.observability.fallback_reason,
            Some(NativeUnsupportedReason::Stage)
        );
    }

    #[test]
    fn native_coverage_matrix_uses_known_statuses_and_tracks_stage_rows() {
        let matrix = include_str!("../../../docs/PDL_NATIVE_COVERAGE.csv");
        let mut stage_rows = BTreeSet::new();
        let mut planned_native_rows = Vec::new();
        for (index, line) in matrix.lines().enumerate() {
            if index == 0 {
                assert_eq!(line, "area,item,status,notes");
                continue;
            }
            let fields = line.splitn(4, ',').collect::<Vec<_>>();
            assert_eq!(fields.len(), 4, "{line}");
            assert!(
                matches!(
                    fields[2],
                    "native parity"
                        | "native partial"
                        | "row-only by design"
                        | "planned native"
                        | "unsupported"
                        | "deferred"
                ),
                "unknown status in {line}"
            );
            if fields[0] == "stage" {
                stage_rows.insert(fields[1]);
            }
            if fields[2] == "planned native" {
                planned_native_rows.push(line);
            }
        }
        assert!(
            planned_native_rows.is_empty(),
            "v0.37 coverage matrix must close planned-native rows: {planned_native_rows:?}"
        );
        for stage in [
            "load",
            "filter",
            "select",
            "drop",
            "rename",
            "mutate",
            "group_by",
            "agg",
            "sort",
            "limit",
            "distinct",
            "join",
            "union",
            "pivot_longer",
            "complete",
            "save",
        ] {
            assert!(stage_rows.contains(stage), "missing matrix row for {stage}");
        }
    }
}
