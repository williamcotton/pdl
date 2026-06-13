use pdl_core::{codes, has_errors, Diagnostic, Span};
use pdl_data::{DataFormat, NativeMaterializationReason};
use pdl_driver::{PreparedProgram, SinkDescriptor, SourceDescriptor};
use pdl_semantics::{
    AggItemIr, ExprIr, FrameBoundIr, PipelineIr, PipelineStartIr, ProgramIr, StageIr,
    WindowFrameIr, WindowSpecIr,
};
use std::collections::{BTreeMap, BTreeSet};

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
    /// Row execution that additionally asserts no part of the run silently
    /// used native lowering. Only ever a requested engine; the selected
    /// engine for a row-strict request is `Row`.
    RowStrict,
    Native,
    NativeStrict,
    /// Program-level observability for named-output programs where automatic
    /// planning selects different engines per output. Not a CLI request value.
    Mixed,
}

impl PlannedEngine {
    pub fn as_str(self) -> &'static str {
        match self {
            PlannedEngine::Auto => "auto",
            PlannedEngine::Row => "row",
            PlannedEngine::RowStrict => "row-strict",
            PlannedEngine::Native => "native",
            PlannedEngine::NativeStrict => "native-strict",
            PlannedEngine::Mixed => "mixed",
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
    pub materialization_reasons: Vec<NativeMaterializationReason>,
    pub native_bridge_count: usize,
    pub estimated_row_bridge_stages: Vec<String>,
    pub dynamic_window_strategy: Option<String>,
    pub performance_classification: String,
    pub required_source_columns: Option<Vec<String>>,
    pub outputs: Vec<OutputPlanObservability>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutputPlanObservability {
    pub name: String,
    pub selected_engine: PlannedEngine,
    pub fallback_reason: Option<NativeUnsupportedReason>,
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

/// Typed observability values explaining why a pipeline is not eligible for
/// native execution. These are not diagnostic codes; they surface through
/// `pdl plan`, `pdl plan --json`, and `pdl manifest` under
/// `execution.observability.fallback_reason`.
///
/// The v0.43 refinement split the coarse v0.40–v0.42 categories into
/// coverage-boundary variants. Since v0.49, promoted language-feature variants
/// are retained only for stable observability vocabulary, defensive paths, and
/// invalid-program cases.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeUnsupportedReason {
    /// No runnable main pipeline exists.
    NoRunnableMain,
    /// Input format has no native scan. JSON Lines is native-eligible since
    /// v0.49; this is reserved for genuinely unknown formats.
    InputFormat,
    /// Geospatial input or geometry-carrying data. The native (Polars) engine
    /// does not support geometry in v0.53; these pipelines run on the row
    /// runtime (PDL_SPEC §10.13).
    Geometry,
    /// Scalar function is outside the native allowlist.
    ScalarFunction,
    /// Scalar function arity is outside the native contract.
    ScalarFunctionArity,
    /// Aggregate function is outside the native allowlist.
    AggregateFunction,
    /// Aggregate arity is outside the native contract.
    AggregateArity,
    /// Window function, frame, or arity is outside the language/native subset.
    WindowExpression,
    /// Retired for valid v0.49 language-feature pipelines: data-dependent
    /// `col(...)` is native-eligible.
    DataDependentColIndirection,
    /// Retired for valid v0.49 language-feature pipelines: expression-valued
    /// `replace` pattern and replacement arguments are native-eligible.
    DataDependentReplacePattern,
    /// Retired for valid v0.49 language-feature pipelines.
    UnsupportedNumericCoercion,
    /// Retired for valid v0.49 language-feature pipelines: temporal scalar
    /// functions are native-eligible.
    TemporalFunction,
    /// Retired for valid v0.49 language-feature pipelines: heterogeneous
    /// `union` null-padding is native-eligible.
    UnionNullPadding,
    /// Reserved for future unshipped non-equi join syntax.
    NonEquiJoin,
    /// Pipeline-start binding references a binding the native planner cannot
    /// lower.
    BindingStartNotEligible,
    /// Forced-native named-output program has at least one row-only output.
    NamedOutputMixedEngines,
    /// Non-terminal `save` requires a fan-out subcase the native planner cannot
    /// preserve in row-runtime write order.
    NonTerminalSaveFanout,
    /// Retired in v0.46 and retained for stable observability vocabulary.
    StdinBytesBackedScan,
    /// Retired in v0.46 and retained for stable observability vocabulary.
    HostBytesBackedScan,
    /// Sink format is not wired to `NativeDirectWriter`. Since the v0.44
    /// CSV/NDJSON writer promotions every format has a native direct writer;
    /// the variant survives as the defensive boundary for native sink writes
    /// that fail to return bytes.
    NativeSinkWriter,
    /// Stage is `row-only by design` with no narrower variant. Catch-all for
    /// stages the coverage matrix declares row-only.
    RowOnlyStage,
    /// Driver source or sink facts are unavailable for native planning.
    DriverFacts,
    /// Non-execution observability boundary for the WASM contract. Not
    /// produced by the planner at runtime; documents the WASM boundary in the
    /// coverage matrix and tests.
    WasmTargetGraph,
    /// Non-execution observability boundary for the LSP / editor-services
    /// surface. Same use as `WasmTargetGraph`.
    EditorService,
}

impl NativeUnsupportedReason {
    pub fn code(self) -> &'static str {
        match self {
            NativeUnsupportedReason::NoRunnableMain => "no-runnable-main",
            NativeUnsupportedReason::InputFormat => "input-format",
            NativeUnsupportedReason::Geometry => "geometry",
            NativeUnsupportedReason::ScalarFunction => "scalar-function",
            NativeUnsupportedReason::ScalarFunctionArity => "scalar-function-arity",
            NativeUnsupportedReason::AggregateFunction => "aggregate-function",
            NativeUnsupportedReason::AggregateArity => "aggregate-arity",
            NativeUnsupportedReason::WindowExpression => "window-expression",
            NativeUnsupportedReason::DataDependentColIndirection => {
                "data-dependent-col-indirection"
            }
            NativeUnsupportedReason::DataDependentReplacePattern => {
                "data-dependent-replace-pattern"
            }
            NativeUnsupportedReason::UnsupportedNumericCoercion => "unsupported-numeric-coercion",
            NativeUnsupportedReason::TemporalFunction => "temporal-function",
            NativeUnsupportedReason::UnionNullPadding => "union-null-padding",
            NativeUnsupportedReason::NonEquiJoin => "non-equi-join",
            NativeUnsupportedReason::BindingStartNotEligible => "binding-start-not-eligible",
            NativeUnsupportedReason::NamedOutputMixedEngines => "named-output-mixed-engines",
            NativeUnsupportedReason::NonTerminalSaveFanout => "non-terminal-save-fanout",
            NativeUnsupportedReason::StdinBytesBackedScan => "stdin-bytes-backed-scan",
            NativeUnsupportedReason::HostBytesBackedScan => "host-bytes-backed-scan",
            NativeUnsupportedReason::NativeSinkWriter => "native-sink-writer",
            NativeUnsupportedReason::RowOnlyStage => "row-only-stage",
            NativeUnsupportedReason::DriverFacts => "driver-facts",
            NativeUnsupportedReason::WasmTargetGraph => "wasm-target-graph",
            NativeUnsupportedReason::EditorService => "editor-service",
        }
    }

    /// Every variant the planner may attach to a runnable row-only pipeline,
    /// plus the two non-execution boundary variants. Used by the parity
    /// harness to assert reported reasons stay inside the typed surface.
    pub fn all_codes() -> &'static [&'static str] {
        &[
            "no-runnable-main",
            "input-format",
            "geometry",
            "scalar-function",
            "scalar-function-arity",
            "aggregate-function",
            "aggregate-arity",
            "window-expression",
            "data-dependent-col-indirection",
            "data-dependent-replace-pattern",
            "unsupported-numeric-coercion",
            "temporal-function",
            "union-null-padding",
            "non-equi-join",
            "binding-start-not-eligible",
            "named-output-mixed-engines",
            "non-terminal-save-fanout",
            "stdin-bytes-backed-scan",
            "host-bytes-backed-scan",
            "native-sink-writer",
            "row-only-stage",
            "driver-facts",
            "wasm-target-graph",
            "editor-service",
        ]
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
    let output_observability = output_observability(prepared, ir, options.engine);
    let mut native_reason = native_unsupported_reason(prepared, ir);
    if !ir.outputs.is_empty()
        && matches!(
            options.engine,
            PlannedEngine::Native | PlannedEngine::NativeStrict
        )
        && output_observability
            .iter()
            .any(|output| output.fallback_reason.is_some())
    {
        native_reason = Some(NativeUnsupportedReason::NamedOutputMixedEngines);
    }
    let native_eligible = if ir.outputs.is_empty() {
        native_reason.is_none()
    } else {
        output_observability
            .iter()
            .all(|output| output.fallback_reason.is_none())
    };
    let eligible_engine = eligible_engine(ir, native_eligible, &output_observability);
    let selected_engine = match options.engine {
        PlannedEngine::Auto => eligible_engine,
        PlannedEngine::Row | PlannedEngine::RowStrict => PlannedEngine::Row,
        PlannedEngine::Native | PlannedEngine::NativeStrict => PlannedEngine::Native,
        PlannedEngine::Mixed => PlannedEngine::Mixed,
    };
    let output_format = plan_output_format(prepared, stdout_format, steps);
    let sink_strategy = sink_strategy(prepared, stdout_format, selected_engine, &output_format);
    let materialization_reasons = if selected_engine == PlannedEngine::Native {
        native_materialization_reasons(prepared, ir)
    } else {
        Vec::new()
    };
    let native_bridge_count = materialization_reasons.len();
    // Since v0.44 the native CSV/NDJSON writers stream dataframe rows through
    // the row writers' cell encoders, so text output no longer forces a row
    // materialization on the native engine.
    let row_materialization =
        selected_engine != PlannedEngine::Native || !materialization_reasons.is_empty();
    let estimated_row_bridge_stages = if selected_engine == PlannedEngine::Native {
        native_estimated_row_bridge_stages(prepared, ir)
    } else {
        Vec::new()
    };
    let dynamic_window_strategy = native_dynamic_window_strategy(ir);
    let performance_classification =
        native_performance_classification(selected_engine, sink_strategy, &materialization_reasons);

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
        materialization_reasons,
        native_bridge_count,
        estimated_row_bridge_stages,
        dynamic_window_strategy,
        performance_classification,
        required_source_columns: ir
            .main
            .as_ref()
            .and_then(|pipeline| required_source_columns(prepared, pipeline)),
        outputs: output_observability,
    }
}

fn output_observability(
    prepared: &PreparedProgram,
    ir: &ProgramIr,
    requested_engine: PlannedEngine,
) -> Vec<OutputPlanObservability> {
    ir.outputs
        .iter()
        .map(|output| {
            let fallback_reason = native_pipeline_unsupported_reason(prepared, &output.pipeline);
            let eligible_engine = if fallback_reason.is_none() {
                PlannedEngine::Native
            } else {
                PlannedEngine::Row
            };
            let selected_engine = match requested_engine {
                PlannedEngine::Auto => eligible_engine,
                PlannedEngine::Row | PlannedEngine::RowStrict => PlannedEngine::Row,
                PlannedEngine::Native | PlannedEngine::NativeStrict => PlannedEngine::Native,
                PlannedEngine::Mixed => eligible_engine,
            };
            OutputPlanObservability {
                name: output.name.clone(),
                selected_engine,
                fallback_reason,
            }
        })
        .collect()
}

fn eligible_engine(
    ir: &ProgramIr,
    native_eligible: bool,
    outputs: &[OutputPlanObservability],
) -> PlannedEngine {
    if ir.outputs.is_empty() {
        if native_eligible {
            PlannedEngine::Native
        } else {
            PlannedEngine::Row
        }
    } else {
        let native_outputs = outputs
            .iter()
            .filter(|output| output.fallback_reason.is_none())
            .count();
        match native_outputs {
            0 => PlannedEngine::Row,
            count if count == outputs.len() => PlannedEngine::Native,
            _ => PlannedEngine::Mixed,
        }
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
        // Since v0.44 every format has a native direct writer, so the native
        // engine always hands stdout payloads over as bytes.
        return if selected_engine == PlannedEngine::Native {
            SinkStrategy::BytesSink
        } else {
            SinkStrategy::RowFormatWriter
        };
    }
    let Some(sink) = prepared.driver_plan.sinks.last() else {
        return SinkStrategy::None;
    };
    if selected_engine == PlannedEngine::Native {
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

fn native_materialization_reasons(
    prepared: &PreparedProgram,
    ir: &ProgramIr,
) -> Vec<NativeMaterializationReason> {
    let mut reasons = Vec::new();
    for input in &prepared.driver_plan.inputs {
        if input.format.effective_name() == "jsonl" {
            reasons.push(NativeMaterializationReason::JsonLinesScan);
        }
    }
    collect_program_expr_reasons(ir, &mut reasons);
    reasons.sort();
    reasons.dedup();
    reasons
}

fn native_estimated_row_bridge_stages(prepared: &PreparedProgram, ir: &ProgramIr) -> Vec<String> {
    let mut stages = Vec::new();
    if prepared
        .driver_plan
        .inputs
        .iter()
        .any(|input| input.format.effective_name() == "jsonl")
    {
        stages.push("load:json_lines_scan".to_string());
    }
    collect_program_bridge_stages(ir, &mut stages);
    stages.sort();
    stages.dedup();
    stages
}

fn native_dynamic_window_strategy(ir: &ProgramIr) -> Option<String> {
    program_exprs(ir)
        .iter()
        .any(|expr| expr_contains_dynamic_offset_window(expr))
        .then(|| "cached-row-bridge".to_string())
        .or_else(|| {
            program_exprs(ir)
                .iter()
                .any(|expr| expr_contains_window(expr))
                .then(|| "polars-native".to_string())
        })
}

fn native_performance_classification(
    selected_engine: PlannedEngine,
    sink_strategy: SinkStrategy,
    materialization_reasons: &[NativeMaterializationReason],
) -> String {
    if selected_engine != PlannedEngine::Native {
        return "row-engine".to_string();
    }
    if materialization_reasons.contains(&NativeMaterializationReason::WindowDynamicOffset) {
        return "cached-row-bridge".to_string();
    }
    if !materialization_reasons.is_empty() {
        return "native-bridge".to_string();
    }
    if matches!(
        sink_strategy,
        SinkStrategy::NativeDirectWriter | SinkStrategy::BytesSink
    ) {
        return "polars-native".to_string();
    }
    "polars-native".to_string()
}

fn collect_program_expr_reasons(ir: &ProgramIr, reasons: &mut Vec<NativeMaterializationReason>) {
    for pipeline in ir
        .main
        .iter()
        .chain(ir.bindings.iter().map(|binding| &binding.pipeline))
        .chain(ir.outputs.iter().map(|output| &output.pipeline))
    {
        collect_pipeline_expr_reasons(pipeline, reasons);
    }
}

fn collect_pipeline_expr_reasons(
    pipeline: &PipelineIr,
    reasons: &mut Vec<NativeMaterializationReason>,
) {
    for expr in pipeline_exprs(pipeline) {
        collect_expr_materialization_reasons(expr, reasons);
    }
}

fn collect_expr_materialization_reasons(
    expr: &ExprIr,
    reasons: &mut Vec<NativeMaterializationReason>,
) {
    if let Some(reason) = expr_materialization_reason(expr) {
        reasons.push(reason);
    }
    match expr {
        ExprIr::Unary { expr, .. } => collect_expr_materialization_reasons(expr, reasons),
        ExprIr::Binary { left, right, .. } => {
            collect_expr_materialization_reasons(left, reasons);
            collect_expr_materialization_reasons(right, reasons);
        }
        ExprIr::Call { args, .. } | ExprIr::Window { args, .. } => {
            for arg in args {
                collect_expr_materialization_reasons(arg, reasons);
            }
        }
        ExprIr::Quoted { .. }
        | ExprIr::Number { .. }
        | ExprIr::Bool { .. }
        | ExprIr::Null { .. }
        | ExprIr::Ident { .. }
        | ExprIr::Context { .. } => {}
    }
}

fn expr_materialization_reason(expr: &ExprIr) -> Option<NativeMaterializationReason> {
    match expr {
        ExprIr::Call { name, args, .. } if name == "col" => match args.as_slice() {
            [ExprIr::Quoted { .. }] => None,
            [_] => Some(NativeMaterializationReason::DynamicColumnLookup),
            _ => None,
        },
        ExprIr::Call { name, args, .. } if name == "replace" => {
            let dynamic_pattern_or_replacement = args
                .get(1..)
                .is_some_and(|rest| rest.iter().any(|arg| !expr_is_static_text(arg)));
            dynamic_pattern_or_replacement
                .then_some(NativeMaterializationReason::DynamicReplaceText)
        }
        ExprIr::Call { name, args, .. } if name == "if_else" && expr_if_else_homogeneous(args) => {
            None
        }
        ExprIr::Call { name, .. } if name == "if_else" => {
            Some(NativeMaterializationReason::MixedClassConditional)
        }
        ExprIr::Call { name, .. } if is_temporal_function(name) => {
            Some(NativeMaterializationReason::TemporalScalar)
        }
        ExprIr::Window {
            function,
            args,
            spec,
            ..
        } if matches!(function.as_str(), "lag" | "lead")
            && matches!(args.get(1), Some(expr) if !matches!(expr, ExprIr::Number { .. })) =>
        {
            Some(NativeMaterializationReason::WindowDynamicOffset)
        }
        ExprIr::Window { spec, .. } if spec.order_by.len() > 1 => {
            Some(NativeMaterializationReason::WindowMultiOrder)
        }
        ExprIr::Quoted { .. }
        | ExprIr::Number { .. }
        | ExprIr::Bool { .. }
        | ExprIr::Null { .. }
        | ExprIr::Ident { .. }
        | ExprIr::Context { .. }
        | ExprIr::Unary { .. }
        | ExprIr::Binary { .. }
        | ExprIr::Call { .. }
        | ExprIr::Window { .. } => None,
    }
}

fn collect_program_bridge_stages(ir: &ProgramIr, stages: &mut Vec<String>) {
    for pipeline in ir
        .main
        .iter()
        .chain(ir.bindings.iter().map(|binding| &binding.pipeline))
        .chain(ir.outputs.iter().map(|output| &output.pipeline))
    {
        collect_pipeline_bridge_stages(pipeline, stages);
    }
}

fn collect_pipeline_bridge_stages(pipeline: &PipelineIr, stages: &mut Vec<String>) {
    for stage in &pipeline.stages {
        match stage {
            StageIr::Filter { expr, .. } => {
                if expr_contains_materialization_reason(expr) {
                    stages.push("filter".to_string());
                }
            }
            StageIr::Mutate { items, .. } => {
                if items
                    .iter()
                    .any(|item| expr_contains_materialization_reason(&item.expr))
                {
                    stages.push("mutate".to_string());
                }
            }
            StageIr::Agg { items, .. } => {
                if items
                    .iter()
                    .any(|item| item.args.iter().any(expr_contains_materialization_reason))
                {
                    stages.push("agg".to_string());
                }
            }
            StageIr::PivotLonger { .. } => stages.push("pivot_longer:maybe".to_string()),
            StageIr::Complete { .. } => stages.push("complete:maybe".to_string()),
            StageIr::Union { .. } => stages.push("union:maybe".to_string()),
            StageIr::Select { .. }
            | StageIr::Drop { .. }
            | StageIr::Rename { .. }
            | StageIr::GroupBy { .. }
            | StageIr::Sort { .. }
            | StageIr::Limit { .. }
            | StageIr::Distinct { .. }
            | StageIr::Save { .. }
            | StageIr::Join { .. }
            | StageIr::Unsupported { .. } => {}
        }
    }
}

fn program_exprs(ir: &ProgramIr) -> Vec<&ExprIr> {
    ir.main
        .iter()
        .chain(ir.bindings.iter().map(|binding| &binding.pipeline))
        .chain(ir.outputs.iter().map(|output| &output.pipeline))
        .flat_map(pipeline_exprs)
        .collect()
}

fn pipeline_exprs(pipeline: &PipelineIr) -> Vec<&ExprIr> {
    let mut exprs = Vec::new();
    for stage in &pipeline.stages {
        match stage {
            StageIr::Filter { expr, .. } => exprs.push(expr),
            StageIr::Mutate { items, .. } => {
                exprs.extend(items.iter().map(|item| &item.expr));
            }
            StageIr::Agg { items, .. } => {
                exprs.extend(items.iter().flat_map(|item| item.args.iter()));
            }
            StageIr::Complete { fills, .. } => {
                exprs.extend(fills.iter().map(|fill| &fill.expr));
            }
            StageIr::Select { .. }
            | StageIr::Drop { .. }
            | StageIr::Rename { .. }
            | StageIr::GroupBy { .. }
            | StageIr::Sort { .. }
            | StageIr::Limit { .. }
            | StageIr::Join { .. }
            | StageIr::Union { .. }
            | StageIr::Distinct { .. }
            | StageIr::PivotLonger { .. }
            | StageIr::Save { .. }
            | StageIr::Unsupported { .. } => {}
        }
    }
    exprs
}

fn expr_contains_dynamic_offset_window(expr: &ExprIr) -> bool {
    match expr {
        ExprIr::Window { function, args, .. }
            if matches!(function.as_str(), "lag" | "lead")
                && matches!(args.get(1), Some(expr) if !matches!(expr, ExprIr::Number { .. })) =>
        {
            true
        }
        ExprIr::Unary { expr, .. } => expr_contains_dynamic_offset_window(expr),
        ExprIr::Binary { left, right, .. } => {
            expr_contains_dynamic_offset_window(left) || expr_contains_dynamic_offset_window(right)
        }
        ExprIr::Call { args, .. } | ExprIr::Window { args, .. } => {
            args.iter().any(expr_contains_dynamic_offset_window)
        }
        ExprIr::Quoted { .. }
        | ExprIr::Number { .. }
        | ExprIr::Bool { .. }
        | ExprIr::Null { .. }
        | ExprIr::Ident { .. }
        | ExprIr::Context { .. } => false,
    }
}

fn expr_contains_materialization_reason(expr: &ExprIr) -> bool {
    if expr_materialization_reason(expr).is_some() {
        return true;
    }
    match expr {
        ExprIr::Unary { expr, .. } => expr_contains_materialization_reason(expr),
        ExprIr::Binary { left, right, .. } => {
            expr_contains_materialization_reason(left)
                || expr_contains_materialization_reason(right)
        }
        ExprIr::Call { args, .. } | ExprIr::Window { args, .. } => {
            args.iter().any(expr_contains_materialization_reason)
        }
        ExprIr::Quoted { .. }
        | ExprIr::Number { .. }
        | ExprIr::Bool { .. }
        | ExprIr::Null { .. }
        | ExprIr::Ident { .. }
        | ExprIr::Context { .. } => false,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ExprValueClass {
    Null,
    Bool,
    Number,
    String,
}

fn expr_if_else_homogeneous(args: &[ExprIr]) -> bool {
    let [_, when_true, when_false] = args else {
        return false;
    };
    match (
        expr_proven_value_class(when_true),
        expr_proven_value_class(when_false),
    ) {
        (Some(ExprValueClass::Null), Some(_)) | (Some(_), Some(ExprValueClass::Null)) => true,
        (Some(left), Some(right)) => left == right,
        _ => false,
    }
}

fn expr_proven_value_class(expr: &ExprIr) -> Option<ExprValueClass> {
    match expr {
        ExprIr::Quoted { .. } => Some(ExprValueClass::String),
        ExprIr::Number { .. } => Some(ExprValueClass::Number),
        ExprIr::Bool { .. } => Some(ExprValueClass::Bool),
        ExprIr::Null { .. } => Some(ExprValueClass::Null),
        ExprIr::Ident { .. } | ExprIr::Context { .. } => None,
        ExprIr::Unary { op, .. } => match op {
            pdl_semantics::UnaryOpIr::Not => Some(ExprValueClass::Bool),
            pdl_semantics::UnaryOpIr::Neg => Some(ExprValueClass::Number),
        },
        ExprIr::Binary { op, .. } => match op {
            pdl_semantics::BinaryOpIr::Or
            | pdl_semantics::BinaryOpIr::And
            | pdl_semantics::BinaryOpIr::Eq
            | pdl_semantics::BinaryOpIr::Ne
            | pdl_semantics::BinaryOpIr::Lt
            | pdl_semantics::BinaryOpIr::Lte
            | pdl_semantics::BinaryOpIr::Gt
            | pdl_semantics::BinaryOpIr::Gte => Some(ExprValueClass::Bool),
            pdl_semantics::BinaryOpIr::Add
            | pdl_semantics::BinaryOpIr::Sub
            | pdl_semantics::BinaryOpIr::Mul
            | pdl_semantics::BinaryOpIr::Div
            | pdl_semantics::BinaryOpIr::Rem => Some(ExprValueClass::Number),
        },
        ExprIr::Call { name, args, .. } => match name.as_str() {
            "is_null" | "not_null" | "contains" | "starts_with" | "to_boolean" => {
                Some(ExprValueClass::Bool)
            }
            "to_number" | "abs" | "round" | "year" | "month" | "day" => {
                Some(ExprValueClass::Number)
            }
            "concat" | "lower" | "upper" | "trim" | "replace" | "to_string" | "date"
            | "datetime" | "date_floor" | "date_format" => Some(ExprValueClass::String),
            "coalesce" => {
                let mut class = None;
                for arg in args {
                    let next = expr_proven_value_class(arg)?;
                    if next == ExprValueClass::Null {
                        continue;
                    }
                    match class {
                        Some(current) if current != next => return None,
                        Some(_) => {}
                        None => class = Some(next),
                    }
                }
                Some(class.unwrap_or(ExprValueClass::Null))
            }
            "if_else" if expr_if_else_homogeneous(args) => {
                let [_, when_true, when_false] = args.as_slice() else {
                    return None;
                };
                match expr_proven_value_class(when_true) {
                    Some(ExprValueClass::Null) => expr_proven_value_class(when_false),
                    value => value,
                }
            }
            _ => None,
        },
        ExprIr::Window { function, args, .. } => match function.as_str() {
            "row_number" | "rank" | "dense_rank" | "percent_rank" | "cume_dist" | "count"
            | "sum" | "mean" => Some(ExprValueClass::Number),
            "lag" | "lead" | "first_value" | "last_value" | "min" | "max" => {
                args.first().and_then(expr_proven_value_class)
            }
            _ => None,
        },
    }
}

fn expr_is_static_text(expr: &ExprIr) -> bool {
    matches!(
        expr,
        ExprIr::Quoted { .. } | ExprIr::Number { .. } | ExprIr::Bool { .. }
    )
}

fn is_temporal_function(name: &str) -> bool {
    matches!(
        name,
        "date" | "datetime" | "year" | "month" | "day" | "date_floor" | "date_format"
    )
}

fn native_unsupported_reason(
    prepared: &PreparedProgram,
    ir: &ProgramIr,
) -> Option<NativeUnsupportedReason> {
    if ir.outputs.is_empty() {
        let Some(main) = ir.main.as_ref() else {
            return Some(NativeUnsupportedReason::NoRunnableMain);
        };
        return native_pipeline_unsupported_reason(prepared, main);
    }

    let mut first_reason = None;
    let mut native_outputs = 0usize;
    for output in &ir.outputs {
        match native_pipeline_unsupported_reason(prepared, &output.pipeline) {
            Some(reason) => {
                first_reason.get_or_insert(reason);
            }
            None => {
                native_outputs += 1;
            }
        };
    }
    if native_outputs > 0 {
        None
    } else {
        first_reason
    }
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
        PipelineStartIr::Binding { name, .. } => {
            let Some(binding) = prepared
                .analysis
                .ir
                .as_ref()
                .and_then(|ir| ir.bindings.iter().find(|binding| binding.name == *name))
            else {
                return Some(NativeUnsupportedReason::BindingStartNotEligible);
            };
            if native_pipeline_unsupported_reason(prepared, &binding.pipeline).is_some() {
                return Some(NativeUnsupportedReason::BindingStartNotEligible);
            }
        }
    }

    for stage in &pipeline.stages {
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
                    if let Some(reason) = native_expr_unsupported_reason(&item.expr) {
                        return Some(reason);
                    }
                }
            }
            StageIr::Agg { items, .. } => {
                for item in items {
                    if let Some(reason) = native_agg_unsupported_reason(item) {
                        return Some(reason);
                    }
                }
            }
            StageIr::Save { .. } => {}
            StageIr::Join { source, .. } => {
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
            StageIr::PivotLonger { columns, .. } => {
                // Promoted in v0.45. The empty column list stays on the row
                // engine so the row runtime's `E1203` diagnostic surfaces.
                if columns.is_empty() {
                    return Some(NativeUnsupportedReason::RowOnlyStage);
                }
            }
            StageIr::Complete { keys, fills, .. } => {
                if keys.is_empty() {
                    return Some(NativeUnsupportedReason::RowOnlyStage);
                }
                for fill in fills {
                    if let Some(reason) = native_expr_unsupported_reason(&fill.expr) {
                        return Some(reason);
                    }
                }
            }
            StageIr::Unsupported { .. } => return Some(NativeUnsupportedReason::RowOnlyStage),
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
    // Since v0.46 the byte-backed scan adapters give stdin and host-byte
    // CSV / Parquet inputs the same native coverage as path-backed inputs,
    // so every source kind shares one format gate. JSON Lines stays
    // row-only by design and reports `input-format` everywhere.
    if matches!(
        format,
        DataFormat::Csv
            | DataFormat::Parquet
            | DataFormat::ArrowFile
            | DataFormat::ArrowStream
            | DataFormat::JsonLines
    ) {
        None
    } else {
        Some(NativeUnsupportedReason::InputFormat)
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
        ExprIr::Window {
            function,
            args,
            spec,
            ..
        } => native_window_unsupported_reason(function, args, spec),
        ExprIr::Call { name, args, .. } => {
            if name == "col" {
                return match args.as_slice() {
                    [arg] => native_expr_unsupported_reason(arg),
                    _ => Some(NativeUnsupportedReason::ScalarFunctionArity),
                };
            }
            match name.as_str() {
                "is_null" | "not_null" | "lower" | "upper" | "trim" | "to_string" | "to_number"
                | "to_boolean" | "abs" => match args.as_slice() {
                    [arg] => native_expr_unsupported_reason(arg),
                    _ => Some(NativeUnsupportedReason::ScalarFunctionArity),
                },
                "contains" | "starts_with" => match args.as_slice() {
                    [value, pattern] => native_expr_unsupported_reason(value)
                        .or_else(|| native_expr_unsupported_reason(pattern)),
                    _ => Some(NativeUnsupportedReason::ScalarFunctionArity),
                },
                "replace" => match args.as_slice() {
                    [value, pattern, replacement] => native_expr_unsupported_reason(value)
                        .or_else(|| native_expr_unsupported_reason(pattern))
                        .or_else(|| native_expr_unsupported_reason(replacement)),
                    _ => Some(NativeUnsupportedReason::ScalarFunctionArity),
                },
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
                "date" | "datetime" | "year" | "month" | "day" | "date_floor" | "date_format" => {
                    args.iter().find_map(native_expr_unsupported_reason)
                }
                _ => Some(NativeUnsupportedReason::ScalarFunction),
            }
        }
    }
}

fn native_window_unsupported_reason(
    function: &str,
    args: &[ExprIr],
    spec: &WindowSpecIr,
) -> Option<NativeUnsupportedReason> {
    match function {
        "row_number" if args.is_empty() => native_supported_window_frame_reason(spec),
        "rank" | "dense_rank" if args.is_empty() && !spec.order_by.is_empty() => {
            native_supported_window_frame_reason(spec)
        }
        "percent_rank" | "cume_dist" if args.is_empty() && !spec.order_by.is_empty() => {
            native_supported_window_frame_reason(spec)
        }
        "lag" | "lead" if !args.is_empty() && args.len() <= 3 && !spec.order_by.is_empty() => {
            native_supported_window_frame_reason(spec)
                .or_else(|| native_expr_unsupported_reason(&args[0]))
                .or_else(|| args.get(1).and_then(native_expr_unsupported_reason))
                .or_else(|| native_offset_default_reason(args.get(2)))
        }
        "first_value" | "last_value" if args.len() == 1 => {
            native_supported_window_frame_reason(spec)
                .or_else(|| native_expr_unsupported_reason(&args[0]))
        }
        "count" if args.is_empty() => native_supported_window_frame_reason(spec),
        "count" if args.len() == 1 => native_supported_window_frame_reason(spec)
            .or_else(|| native_expr_unsupported_reason(&args[0])),
        "sum" | "mean" | "min" | "max" if args.len() == 1 => {
            native_supported_window_frame_reason(spec)
                .or_else(|| native_expr_unsupported_reason(&args[0]))
        }
        _ => Some(NativeUnsupportedReason::WindowExpression),
    }
}

fn native_supported_window_frame_reason(spec: &WindowSpecIr) -> Option<NativeUnsupportedReason> {
    match spec.frame.as_ref() {
        None => None,
        Some(WindowFrameIr {
            start: FrameBoundIr::UnboundedPreceding { .. },
            end: FrameBoundIr::UnboundedFollowing { .. },
            ..
        }) => None,
        Some(WindowFrameIr {
            start: FrameBoundIr::UnboundedPreceding { .. },
            end: FrameBoundIr::CurrentRow { .. },
            ..
        }) => None,
        Some(WindowFrameIr {
            start: FrameBoundIr::CurrentRow { .. },
            end: FrameBoundIr::UnboundedFollowing { .. },
            ..
        }) => None,
        Some(WindowFrameIr {
            start: FrameBoundIr::Preceding { .. },
            end: FrameBoundIr::CurrentRow { .. },
            ..
        }) => None,
        Some(WindowFrameIr {
            start: FrameBoundIr::CurrentRow { .. },
            end: FrameBoundIr::Following { .. },
            ..
        }) => None,
        Some(WindowFrameIr {
            start: FrameBoundIr::Preceding { .. },
            end: FrameBoundIr::Following { .. },
            ..
        }) => None,
        Some(_) => Some(NativeUnsupportedReason::WindowExpression),
    }
}

fn native_offset_default_reason(default: Option<&ExprIr>) -> Option<NativeUnsupportedReason> {
    match default {
        None => None,
        Some(default) => native_expr_unsupported_reason(default),
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
            StageIr::Mutate { items, .. }
                if items.iter().any(|item| expr_contains_window(&item.expr)) =>
            {
                Some("window")
            }
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

fn expr_contains_window(expr: &ExprIr) -> bool {
    match expr {
        ExprIr::Window { .. } => true,
        ExprIr::Call { args, .. } => args.iter().any(expr_contains_window),
        ExprIr::Unary { expr, .. } => expr_contains_window(expr),
        ExprIr::Binary { left, right, .. } => {
            expr_contains_window(left) || expr_contains_window(right)
        }
        ExprIr::Quoted { .. }
        | ExprIr::Number { .. }
        | ExprIr::Bool { .. }
        | ExprIr::Null { .. }
        | ExprIr::Ident { .. }
        | ExprIr::Context { .. } => false,
    }
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
    let context_columns = prepared
        .analysis
        .ir
        .as_ref()
        .map(context_string_defaults)
        .unwrap_or_default();
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
                        collect_expr_columns(arg, &context_columns, &mut next);
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
                            collect_expr_columns(&item.expr, &context_columns, &mut next);
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
                collect_expr_columns(expr, &context_columns, &mut next);
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

fn context_string_defaults(ir: &ProgramIr) -> BTreeMap<String, String> {
    ir.contexts
        .iter()
        .filter_map(|context| match &context.default {
            ExprIr::Quoted { value, .. } => Some((context.name.clone(), value.clone())),
            _ => None,
        })
        .collect()
}

fn collect_expr_columns(
    expr: &ExprIr,
    context_columns: &BTreeMap<String, String>,
    columns: &mut BTreeSet<String>,
) {
    match expr {
        ExprIr::Ident { value, .. } => {
            columns.insert(value.clone());
        }
        ExprIr::Call { name, args, .. } if name == "col" => match args.as_slice() {
            [ExprIr::Quoted { value, .. }] => {
                columns.insert(value.clone());
            }
            [ExprIr::Context { name, .. }] => {
                if let Some(value) = context_columns.get(name) {
                    columns.insert(value.clone());
                }
            }
            _ => {
                for arg in args {
                    collect_expr_columns(arg, context_columns, columns);
                }
            }
        },
        ExprIr::Call { args, .. } => {
            for arg in args {
                collect_expr_columns(arg, context_columns, columns);
            }
        }
        ExprIr::Window { args, spec, .. } => {
            for arg in args {
                collect_expr_columns(arg, context_columns, columns);
            }
            columns.extend(spec.partition_by.iter().cloned());
            columns.extend(spec.order_by.iter().map(|item| item.column.clone()));
        }
        ExprIr::Unary { expr, .. } => collect_expr_columns(expr, context_columns, columns),
        ExprIr::Binary { left, right, .. } => {
            collect_expr_columns(left, context_columns, columns);
            collect_expr_columns(right, context_columns, columns);
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
        assert_eq!(plan.observability.sink_strategy, SinkStrategy::BytesSink);
        assert!(!plan.observability.row_materialization);
    }

    /// v0.44: CSV and JSON Lines saves route through the native direct
    /// writer on the native engine and keep the row-format writer on the row
    /// engine.
    #[test]
    fn planning_routes_text_saves_through_native_direct_writer() {
        for (sink, format_name) in [("top.csv", "csv"), ("top.jsonl", "jsonl")] {
            let io =
                InMemoryDriverIo::default().with_schema("memory/sales.csv", ["amount", "region"]);
            let prepared = prepare_source_with_io(
                "memory/main.pdl",
                format!(r#"load "sales.csv" | filter amount > 0 | save "{sink}""#),
                &io,
            );

            let plan = |engine: PlannedEngine| {
                plan_prepared(
                    &prepared,
                    PlanningOptions {
                        stdout_format: None,
                        dry_run: true,
                        allow_binary_stdout: false,
                        engine,
                    },
                )
                .expect("execution plan")
            };

            let auto = plan(PlannedEngine::Auto);
            assert_eq!(
                auto.observability.selected_engine,
                PlannedEngine::Native,
                "{format_name}"
            );
            assert_eq!(
                auto.observability.sink_strategy,
                SinkStrategy::NativeDirectWriter,
                "{format_name}"
            );
            assert!(!auto.observability.row_materialization, "{format_name}");

            let row = plan(PlannedEngine::Row);
            assert_eq!(
                row.observability.sink_strategy,
                SinkStrategy::RowFormatWriter,
                "{format_name}"
            );
            assert!(row.observability.row_materialization, "{format_name}");
        }
    }

    #[test]
    fn planning_selects_native_for_v0_40_expressions_and_context_col_defaults() {
        let io = InMemoryDriverIo::default()
            .with_schema("memory/sales.csv", ["amount", "region", "status"]);
        let prepared = prepare_source_with_io(
            "memory/main.pdl",
            r#"param metric = "amount"

load "sales.csv"
  | mutate
      selected = col($metric),
      label = concat(replace(region, "W", "West"), ":", to_string(col($metric))),
      parsed = to_boolean("true"),
      seq = row_number() over (partition_by region order_by amount desc, status asc),
      prior_amount = lag(amount, 1, 0) over (partition_by region order_by amount desc, status asc)
  | filter contains(label, "West") or starts_with(region, "E")
  | select selected, parsed, seq, prior_amount"#,
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

        assert_eq!(plan.observability.selected_engine, PlannedEngine::Native);
        assert_eq!(plan.observability.eligible_engine, PlannedEngine::Native);
        assert_eq!(plan.observability.fallback_reason, None);
        assert_eq!(
            plan.observability.required_source_columns,
            Some(vec![
                "amount".to_string(),
                "region".to_string(),
                "status".to_string()
            ])
        );
    }

    #[test]
    fn planning_accepts_mixed_multi_key_window_order_groups() {
        let io = InMemoryDriverIo::default()
            .with_schema("memory/sales.csv", ["amount", "region", "status"]);
        let prepared = prepare_source_with_io(
            "memory/main.pdl",
            r#"load "sales.csv"
  | mutate
      amount_seq = row_number() over (partition_by region order_by amount desc, status asc),
      status_seq = row_number() over (partition_by region order_by status asc, amount desc)"#,
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

        assert_eq!(plan.observability.selected_engine, PlannedEngine::Native);
        assert_eq!(plan.observability.fallback_reason, None);
    }

    #[test]
    fn planning_window_frame_named_native_eligibility() {
        let plan_for_frame = |clause: &str| {
            let io = InMemoryDriverIo::default()
                .with_schema("memory/sales.csv", ["amount", "region", "status"]);
            let source = format!(
                r#"load "sales.csv"
  | mutate value = sum(amount) over (partition_by region order_by amount {clause})"#
            );
            let prepared = prepare_source_with_io("memory/main.pdl", &source, &io);
            plan_prepared(
                &prepared,
                PlanningOptions {
                    stdout_format: Some("csv".to_string()),
                    dry_run: true,
                    allow_binary_stdout: true,
                    engine: PlannedEngine::Auto,
                },
            )
            .expect("execution plan")
        };

        for clause in [
            "frame whole_partition",
            "frame running",
            "frame remaining",
            "frame trailing 2",
            "frame leading 2",
            "frame centered 1",
        ] {
            let plan = plan_for_frame(clause);
            assert_eq!(
                plan.observability.selected_engine,
                PlannedEngine::Native,
                "`{clause}` must stay native parity"
            );
            assert_eq!(plan.observability.fallback_reason, None, "`{clause}`");
        }
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
    fn planning_selects_native_for_binding_start_pipeline() {
        let io = InMemoryDriverIo::default().with_schema("memory/sales.csv", ["status", "amount"]);
        let prepared = prepare_source_with_io(
            "memory/main.pdl",
            r#"let completed =
  load "sales.csv"
  | filter status == "completed"

completed
  | select amount"#,
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

        assert_eq!(plan.observability.selected_engine, PlannedEngine::Native);
        assert_eq!(plan.observability.fallback_reason, None);
    }

    #[test]
    fn planning_selects_native_for_non_terminal_save() {
        let io = InMemoryDriverIo::default().with_schema("memory/sales.csv", ["status", "region"]);
        let prepared = prepare_source_with_io(
            "memory/main.pdl",
            r#"load "sales.csv"
  | filter status == "completed"
  | save "completed.csv"
  | select region"#,
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

        assert_eq!(plan.observability.selected_engine, PlannedEngine::Native);
        assert_eq!(plan.observability.fallback_reason, None);
    }

    #[test]
    fn planning_records_per_output_engine_selection() {
        let io = InMemoryDriverIo::default()
            .with_schema("memory/sales.csv", ["region", "amount"])
            .with_file_bytes(
                "memory/events.jsonl",
                b"{\"region\":\"West\",\"amount\":1}\n",
            );
        let prepared = prepare_source_with_io(
            "memory/main.pdl",
            r#"output native_report =
  load "sales.csv"
  | select region, amount
  | save "native_report.csv"

output row_report =
  load "events.jsonl"
  | select region, amount
  | save "row_report.csv""#,
            &io,
        );

        let plan = plan_prepared(
            &prepared,
            PlanningOptions {
                stdout_format: None,
                dry_run: true,
                allow_binary_stdout: true,
                engine: PlannedEngine::Auto,
            },
        )
        .expect("execution plan");

        assert_eq!(plan.observability.selected_engine, PlannedEngine::Native);
        assert_eq!(plan.observability.eligible_engine, PlannedEngine::Native);
        assert_eq!(plan.observability.fallback_reason, None);
        assert_eq!(
            plan.observability.outputs,
            vec![
                OutputPlanObservability {
                    name: "native_report".to_string(),
                    selected_engine: PlannedEngine::Native,
                    fallback_reason: None,
                },
                OutputPlanObservability {
                    name: "row_report".to_string(),
                    selected_engine: PlannedEngine::Native,
                    fallback_reason: None,
                },
            ]
        );

        let forced = plan_prepared(
            &prepared,
            PlanningOptions {
                stdout_format: None,
                dry_run: true,
                allow_binary_stdout: true,
                engine: PlannedEngine::Native,
            },
        )
        .expect("forced native execution plan");
        assert_eq!(forced.observability.fallback_reason, None);
    }

    #[test]
    fn forced_native_plan_reports_temporal_functions_as_native_eligible() {
        let io = InMemoryDriverIo::default().with_schema("memory/sales.csv", ["region", "amount"]);
        let prepared = prepare_source_with_io(
            "memory/main.pdl",
            r#"load "sales.csv"
  | mutate author_day = date(amount)"#,
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
        assert_eq!(plan.observability.eligible_engine, PlannedEngine::Native);
        assert_eq!(plan.observability.fallback_reason, None);
    }

    /// v0.49: temporal scalar functions are native-eligible.
    #[test]
    fn planning_accepts_temporal_functions_as_native() {
        for expr in [
            "date(stamp)",
            "datetime(stamp)",
            "year(stamp)",
            "month(stamp)",
            "day(stamp)",
            r#"date_floor(stamp, "month")"#,
            r#"date_format(stamp, "%Y-%m")"#,
        ] {
            let io =
                InMemoryDriverIo::default().with_schema("memory/commits.csv", ["repo", "stamp"]);
            let prepared = prepare_source_with_io(
                "memory/main.pdl",
                format!(
                    r#"load "commits.csv"
  | mutate out = {expr}"#
                ),
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
                plan.observability.selected_engine,
                PlannedEngine::Native,
                "`{expr}` must be native-eligible"
            );
            assert_eq!(plan.observability.fallback_reason, None, "`{expr}`");
        }
    }

    #[test]
    fn native_coverage_matrix_uses_v0_49_status_vocabulary() {
        let matrix = include_str!("../../../docs/PDL_NATIVE_COVERAGE.csv");
        let mut stage_rows = BTreeSet::new();
        let mut row_only_rows = BTreeSet::new();
        for (index, line) in matrix.lines().enumerate() {
            if index == 0 {
                assert_eq!(line, "area,item,status,notes");
                continue;
            }
            let fields = line.splitn(4, ',').collect::<Vec<_>>();
            assert_eq!(fields.len(), 4, "{line}");
            assert!(
                matches!(fields[2], "native parity" | "row-only by design"),
                "v0.49 coverage matrix status must be `native parity` or \
                 `row-only by design`: {line}"
            );
            if fields[0] == "stage" {
                stage_rows.insert(fields[1]);
            }
            if fields[2] == "row-only by design" {
                row_only_rows.insert((fields[0], fields[1]));
            }
        }
        assert_eq!(
            row_only_rows,
            BTreeSet::from([("host", "WASM"), ("host", "LSP/editor")]),
            "v0.49 row-only coverage is limited to the two non-execution \
             host boundaries"
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
