use pdl_core::{codes, has_errors, Diagnostic, Span};
use pdl_data::DataFormat;
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
}

impl PlannedEngine {
    pub fn as_str(self) -> &'static str {
        match self {
            PlannedEngine::Auto => "auto",
            PlannedEngine::Row => "row",
            PlannedEngine::RowStrict => "row-strict",
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

/// Typed observability values explaining why a pipeline is not eligible for
/// native execution. These are not diagnostic codes; they surface through
/// `pdl plan`, `pdl plan --json`, and `pdl manifest` under
/// `execution.observability.fallback_reason`.
///
/// The v0.43 refinement split the coarse v0.40–v0.42 categories into
/// coverage-boundary variants. Variants marked "reserve" are defined ahead of
/// the v0.44–v0.49 native-coverage promotions and are not yet produced by the
/// planner.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeUnsupportedReason {
    /// No runnable main pipeline exists.
    NoRunnableMain,
    /// Path-backed input format has no native scan (e.g. JSON Lines paths).
    InputFormat,
    /// Scalar function is outside the native allowlist.
    ScalarFunction,
    /// Scalar function arity is outside the native contract.
    ScalarFunctionArity,
    /// Aggregate function is outside the native allowlist.
    AggregateFunction,
    /// Aggregate arity is outside the native contract.
    AggregateArity,
    /// Window function, frame, or order grouping is outside the native subset.
    WindowExpression,
    /// `col(...)` argument is not a string literal or string-typed context
    /// default; the column reference is data-dependent.
    DataDependentColIndirection,
    /// `replace` pattern or replacement is not a string literal or
    /// string-typed context default; the pattern is data-dependent.
    DataDependentReplacePattern,
    /// `to_number` / `to_string` / `to_boolean` input falls outside the v0.47
    /// coercion contract (v0.47 reserve).
    UnsupportedNumericCoercion,
    /// Temporal scalar functions (`date`, `datetime`, `year`, `month`,
    /// `day`, `date_floor`, `date_format`) are row-only by design in
    /// v0.46.5; native lowering is deferred to a later native-coverage
    /// release.
    TemporalFunction,
    /// `union` participants have heterogeneous schemas and require
    /// null-padding alignment (v0.41 row-only reserve).
    UnionNullPadding,
    /// Join predicate is not an equality on columns (v0.41 row-only reserve).
    NonEquiJoin,
    /// Pipeline-start binding references a binding the native planner cannot
    /// lower (v0.48 reserve refines this further).
    BindingStartNotEligible,
    /// Multi-output program has at least one row-only output and per-output
    /// observability is not enabled (v0.48 reserve refines this further).
    NamedOutputMixedEngines,
    /// Non-terminal `save` requires fan-out the native planner does not yet
    /// support (v0.48 reserve refines this further).
    NonTerminalSaveFanout,
    /// Retired in v0.46: the byte-backed scan adapters promoted stdin CSV
    /// and Parquet, so the planner no longer produces this variant. JSON
    /// Lines stdin reports `input-format` like path-backed JSON Lines. The
    /// variant stays in the vocabulary until the v0.49 cleanup.
    StdinBytesBackedScan,
    /// Retired in v0.46: host-supplied CSV / Parquet bytes scan natively
    /// through the same byte-backed adapters, so the planner no longer
    /// produces this variant. The variant stays in the vocabulary until the
    /// v0.49 cleanup.
    HostBytesBackedScan,
    /// Sink format is not wired to `NativeDirectWriter`. Since the v0.44
    /// CSV/NDJSON writer promotions every format has a native direct writer;
    /// the variant survives as the defensive boundary for native sink writes
    /// that fail to return bytes. Vocabulary cleanup is v0.49 work.
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
    let native_reason = native_unsupported_reason(prepared, ir);
    let native_eligible = native_reason.is_none();
    let eligible_engine = if native_eligible {
        PlannedEngine::Native
    } else {
        PlannedEngine::Row
    };
    let selected_engine = match options.engine {
        PlannedEngine::Auto => eligible_engine,
        PlannedEngine::Row | PlannedEngine::RowStrict => PlannedEngine::Row,
        PlannedEngine::Native => PlannedEngine::Native,
    };
    let output_format = plan_output_format(prepared, stdout_format, steps);
    let sink_strategy = sink_strategy(prepared, stdout_format, selected_engine, &output_format);
    // Since v0.44 the native CSV/NDJSON writers stream dataframe rows through
    // the row writers' cell encoders, so text output no longer forces a row
    // materialization on the native engine.
    let row_materialization = selected_engine == PlannedEngine::Row;

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

fn native_unsupported_reason(
    prepared: &PreparedProgram,
    ir: &ProgramIr,
) -> Option<NativeUnsupportedReason> {
    if !ir.outputs.is_empty() {
        return Some(NativeUnsupportedReason::NamedOutputMixedEngines);
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
        PipelineStartIr::Binding { .. } => {
            return Some(NativeUnsupportedReason::BindingStartNotEligible)
        }
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
                    if native_multi_key_window_order_reason(item).is_some() {
                        return Some(NativeUnsupportedReason::WindowExpression);
                    }
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
            StageIr::Save { .. } if !is_terminal => {
                return Some(NativeUnsupportedReason::NonTerminalSaveFanout);
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
                // Promoted in v0.45. Empty key lists and window-bearing fill
                // expressions stay on the row engine by design.
                if keys.is_empty() {
                    return Some(NativeUnsupportedReason::RowOnlyStage);
                }
                for fill in fills {
                    if expr_ir_contains_window(&fill.expr) {
                        return Some(NativeUnsupportedReason::RowOnlyStage);
                    }
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

pub(crate) fn expr_ir_contains_window(expr: &ExprIr) -> bool {
    match expr {
        ExprIr::Window { .. } => true,
        ExprIr::Unary { expr, .. } => expr_ir_contains_window(expr),
        ExprIr::Binary { left, right, .. } => {
            expr_ir_contains_window(left) || expr_ir_contains_window(right)
        }
        ExprIr::Call { args, .. } => args.iter().any(expr_ir_contains_window),
        ExprIr::Quoted { .. }
        | ExprIr::Number { .. }
        | ExprIr::Bool { .. }
        | ExprIr::Null { .. }
        | ExprIr::Ident { .. }
        | ExprIr::Context { .. } => false,
    }
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
        DataFormat::Csv | DataFormat::Parquet | DataFormat::ArrowFile | DataFormat::ArrowStream
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
                    [ExprIr::Quoted { .. } | ExprIr::Context { .. }] => None,
                    [_] => Some(NativeUnsupportedReason::DataDependentColIndirection),
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
                        .or_else(|| native_static_text_arg_reason(pattern))
                        .or_else(|| native_static_text_arg_reason(replacement)),
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
                // Temporal scalar functions are row-only by design in
                // v0.46.5 (see docs/PDL_NATIVE_COVERAGE.md, "temporal
                // functions"); native lowering is deferred.
                "date" | "datetime" | "year" | "month" | "day" | "date_floor" | "date_format" => {
                    Some(NativeUnsupportedReason::TemporalFunction)
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
                .or_else(|| native_offset_arg_reason(args.get(1)))
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

#[derive(Clone, Debug, PartialEq)]
struct NativeWindowSortGroupIr {
    partition_by: Vec<String>,
    order_by: Vec<(
        String,
        pdl_semantics::SortDirectionIr,
        Option<pdl_semantics::NullsOrderIr>,
    )>,
}

fn native_multi_key_window_order_reason(
    item: &pdl_semantics::MutateItemIr,
) -> Option<NativeUnsupportedReason> {
    let mut group = None;
    if native_expr_multi_key_window_order_incompatible(&item.expr, &mut group) {
        return Some(NativeUnsupportedReason::WindowExpression);
    }
    None
}

fn native_expr_multi_key_window_order_incompatible(
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
                .any(|arg| native_expr_multi_key_window_order_incompatible(arg, group))
        }
        ExprIr::Call { args, .. } => args
            .iter()
            .any(|arg| native_expr_multi_key_window_order_incompatible(arg, group)),
        ExprIr::Unary { expr, .. } => native_expr_multi_key_window_order_incompatible(expr, group),
        ExprIr::Binary { left, right, .. } => {
            native_expr_multi_key_window_order_incompatible(left, group)
                || native_expr_multi_key_window_order_incompatible(right, group)
        }
        ExprIr::Quoted { .. }
        | ExprIr::Number { .. }
        | ExprIr::Bool { .. }
        | ExprIr::Null { .. }
        | ExprIr::Ident { .. }
        | ExprIr::Context { .. } => false,
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

fn native_offset_arg_reason(offset: Option<&ExprIr>) -> Option<NativeUnsupportedReason> {
    match offset {
        None => None,
        Some(ExprIr::Number { value, .. }) if *value >= 0.0 && value.fract() == 0.0 => None,
        Some(_) => Some(NativeUnsupportedReason::WindowExpression),
    }
}

fn native_static_text_arg_reason(arg: &ExprIr) -> Option<NativeUnsupportedReason> {
    match arg {
        ExprIr::Quoted { .. }
        | ExprIr::Number { .. }
        | ExprIr::Bool { .. }
        | ExprIr::Context { .. } => None,
        _ => Some(NativeUnsupportedReason::DataDependentReplacePattern),
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
    fn forced_native_plan_reports_unsupported_reason_without_selecting_rows() {
        let io = InMemoryDriverIo::default().with_schema("memory/sales.csv", ["region", "amount"]);
        // Temporal scalar functions stay row-only by design, so they serve as
        // the forced-native unsupported specimen after v0.47 promotes bounded
        // named frames.
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
        assert_eq!(plan.observability.eligible_engine, PlannedEngine::Row);
        assert_eq!(
            plan.observability.fallback_reason,
            Some(NativeUnsupportedReason::TemporalFunction)
        );
    }

    /// v0.46.5: temporal scalar functions stay row-only by design; the
    /// planner must demote them with the typed `temporal-function` reason.
    #[test]
    fn planning_demotes_temporal_functions_with_temporal_function_reason() {
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
                PlannedEngine::Row,
                "`{expr}` must stay row-only"
            );
            assert_eq!(
                plan.observability.fallback_reason,
                Some(NativeUnsupportedReason::TemporalFunction),
                "`{expr}`"
            );
        }
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
            "v0.40 coverage matrix must close planned-native rows: {planned_native_rows:?}"
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
