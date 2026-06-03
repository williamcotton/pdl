use pdl_core::{Diagnostic, Span};
use pdl_driver::{
    DriverPlan, FormatDecision, PipelineLabel, PreparedProgram, SinkDescriptor, SniffingDecision,
    SniffingReason, SourceDescriptor, StreamDirection, StreamKind,
};
use pdl_exec::{ExecutionPlan, ExecutionPlanStep};
use pdl_semantics::{
    BinaryOpIr, ExprIr, FrameBoundIr, JoinKindIr, PipelineSchemaLabel, PipelineStartIr, ProgramIr,
    SinkIr, SortDirectionIr, StageIr, UnaryOpIr, WindowFrameIr, WindowSpecIr,
};
use pdl_syntax::{
    AggItem, BinaryOp, Binding, Expr, FrameBound, JoinOn, MutateItem, Pipeline, PipelineStart,
    Program, SaveStage, SinkRef, SortDirection, SourceRef, Spanned, Stage, UnaryOp,
    UnionOptionKind, WindowFrame, WindowSpec,
};
use serde::Serialize;

pub fn final_schema_columns(prepared: &PreparedProgram) -> Option<Vec<String>> {
    prepared
        .analysis
        .outputs
        .iter()
        .rev()
        .find(|output| matches!(output.label, PipelineSchemaLabel::Main))
        .map(|output| output.columns.clone())
}

pub fn render_schema_text(columns: &[String]) -> String {
    let mut text = String::from("columns:\n");
    for column in columns {
        text.push_str("  - ");
        text.push_str(column);
        text.push('\n');
    }
    text
}

pub fn render_plan_text(prepared: &PreparedProgram, plan: &ExecutionPlan) -> String {
    let mut text = String::new();
    text.push_str(&format!("source: {}\n", prepared.path.display()));
    text.push_str("inputs:\n");
    if prepared.driver_plan.inputs.is_empty() {
        text.push_str("  none\n");
    } else {
        for input in &prepared.driver_plan.inputs {
            text.push_str(&format!(
                "  - {} load {} format {}\n",
                pipeline_label_text(&input.pipeline),
                source_text(&input.source),
                input.format.effective_name()
            ));
        }
    }
    text.push_str("sinks:\n");
    if prepared.driver_plan.sinks.is_empty() && plan.stdout_format.is_none() {
        text.push_str("  none\n");
    } else {
        for sink in &prepared.driver_plan.sinks {
            text.push_str(&format!(
                "  - {} save {} format {}\n",
                pipeline_label_text(&sink.pipeline),
                sink_text(&sink.sink),
                sink.format.effective_name()
            ));
        }
        if let Some(format) = plan.stdout_format {
            text.push_str(&format!("  - stdout format {}\n", format.canonical_name()));
        }
    }
    text.push_str("execution:\n");
    for step in &plan.steps {
        text.push_str("  - ");
        text.push_str(&execution_step_text(step));
        text.push('\n');
    }
    text
}

pub fn schema_json(prepared: &PreparedProgram, binding: Option<&str>) -> SchemaOutputJson {
    let columns = final_schema_columns(prepared).unwrap_or_default();
    SchemaOutputJson {
        source_path: prepared.path.display().to_string(),
        binding: binding.map(ToString::to_string),
        schema: SchemaJson::from_columns(columns),
        stage_traces: prepared
            .analysis
            .traces
            .iter()
            .map(|trace| StageTraceJson {
                stage_id: trace.stage_id,
                stage_name: trace.stage_name.clone(),
                span: trace.span,
                input_columns: trace.input_schema.clone().unwrap_or_default(),
                output_columns: trace.output_schema.clone().unwrap_or_default(),
                grouping_columns: trace.grouping.columns.clone(),
            })
            .collect(),
        diagnostics: prepared.diagnostics(),
    }
}

pub fn plan_json(prepared: &PreparedProgram, plan: &ExecutionPlan) -> PlanOutputJson {
    PlanOutputJson {
        source_path: prepared.path.display().to_string(),
        driver: driver_plan_json(&prepared.driver_plan),
        execution: execution_plan_json(plan),
        final_schema: final_schema_columns(prepared).map(SchemaJson::from_columns),
        diagnostics: prepared.diagnostics(),
    }
}

pub fn manifest_json(prepared: &PreparedProgram, plan: &ExecutionPlan) -> ManifestJson {
    let stdout_format = plan.stdout_format.map(|format| format.canonical_name());
    ManifestJson {
        manifest_version: "0.16.0",
        implementation_version: env!("CARGO_PKG_VERSION"),
        language_version: "0.16.0",
        source_path: prepared.path.display().to_string(),
        driver: driver_plan_json(&prepared.driver_plan),
        execution: execution_plan_json(plan),
        final_schema: final_schema_columns(prepared).map(SchemaJson::from_columns),
        diagnostics: prepared.diagnostics(),
        algraf_interop: (stdout_format == Some("arrow-stream")).then_some(AlgrafInteropJson {
            stdout_format: "arrow-stream",
            stdin_format_hint: "arrow-stream",
        }),
    }
}

pub fn ast_json(
    source_path: String,
    program: &Program,
    diagnostics: Vec<Diagnostic>,
) -> AstOutputJson {
    AstOutputJson {
        source_path,
        program: program_json(program),
        diagnostics,
    }
}

pub fn ir_json(source_path: String, ir: &ProgramIr, diagnostics: Vec<Diagnostic>) -> IrOutputJson {
    IrOutputJson {
        source_path,
        ir: program_ir_json(ir),
        diagnostics,
    }
}

#[derive(Serialize)]
pub struct SchemaOutputJson {
    source_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    binding: Option<String>,
    schema: SchemaJson,
    stage_traces: Vec<StageTraceJson>,
    diagnostics: Vec<Diagnostic>,
}

#[derive(Serialize)]
pub struct PlanOutputJson {
    source_path: String,
    driver: DriverPlanJson,
    execution: ExecutionPlanJson,
    #[serde(skip_serializing_if = "Option::is_none")]
    final_schema: Option<SchemaJson>,
    diagnostics: Vec<Diagnostic>,
}

#[derive(Serialize)]
pub struct ManifestJson {
    manifest_version: &'static str,
    implementation_version: &'static str,
    language_version: &'static str,
    source_path: String,
    driver: DriverPlanJson,
    execution: ExecutionPlanJson,
    #[serde(skip_serializing_if = "Option::is_none")]
    final_schema: Option<SchemaJson>,
    diagnostics: Vec<Diagnostic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    algraf_interop: Option<AlgrafInteropJson>,
}

#[derive(Serialize)]
pub struct AstOutputJson {
    source_path: String,
    program: ProgramJson,
    diagnostics: Vec<Diagnostic>,
}

#[derive(Serialize)]
pub struct IrOutputJson {
    source_path: String,
    ir: ProgramIrJson,
    diagnostics: Vec<Diagnostic>,
}

#[derive(Serialize)]
struct SchemaJson {
    columns: Vec<ColumnJson>,
}

impl SchemaJson {
    fn from_columns(columns: Vec<String>) -> Self {
        Self {
            columns: columns
                .into_iter()
                .map(|name| ColumnJson {
                    name,
                    logical_type: "unknown",
                    nullable: true,
                })
                .collect(),
        }
    }
}

#[derive(Serialize)]
struct ColumnJson {
    name: String,
    logical_type: &'static str,
    nullable: bool,
}

#[derive(Serialize)]
struct StageTraceJson {
    stage_id: usize,
    stage_name: String,
    span: Span,
    input_columns: Vec<String>,
    output_columns: Vec<String>,
    grouping_columns: Vec<String>,
}

#[derive(Serialize)]
struct DriverPlanJson {
    source_path: String,
    base_dir: String,
    inputs: Vec<InputJson>,
    sinks: Vec<SinkJson>,
    dependencies: Vec<DependencyJson>,
    streams: Vec<StreamJson>,
}

#[derive(Serialize)]
struct InputJson {
    pipeline: String,
    source: String,
    format: FormatDecisionJson,
    span: Span,
    stage_span: Span,
}

#[derive(Serialize)]
struct SinkJson {
    pipeline: String,
    sink: String,
    format: FormatDecisionJson,
    span: Span,
    stage_span: Span,
}

#[derive(Serialize)]
struct DependencyJson {
    logical_path: String,
    resolved_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    inferred_format: Option<String>,
    span: Span,
}

#[derive(Serialize)]
struct StreamJson {
    pipeline: String,
    stream: &'static str,
    direction: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    explicit_format: Option<String>,
    span: Span,
}

#[derive(Serialize)]
struct FormatDecisionJson {
    #[serde(skip_serializing_if = "Option::is_none")]
    explicit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    inferred_from_path: Option<String>,
    effective: String,
    sniffing: SniffingJson,
}

#[derive(Serialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
enum SniffingJson {
    NotNeeded,
    Deferred { reason: &'static str },
}

#[derive(Serialize)]
struct ExecutionPlanJson {
    dry_run: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    stdout_format: Option<String>,
    steps: Vec<ExecutionStepJson>,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ExecutionStepJson {
    Load { source: String, format: String },
    Binding { name: String },
    Transform { stage: String },
    Save { sink: String, format: String },
    Stdout { format: String },
}

#[derive(Serialize)]
struct AlgrafInteropJson {
    stdout_format: &'static str,
    stdin_format_hint: &'static str,
}

fn driver_plan_json(plan: &DriverPlan) -> DriverPlanJson {
    DriverPlanJson {
        source_path: plan.source_path.display().to_string(),
        base_dir: plan.base_dir.display().to_string(),
        inputs: plan
            .inputs
            .iter()
            .map(|input| InputJson {
                pipeline: pipeline_label_text(&input.pipeline),
                source: source_text(&input.source),
                format: format_decision_json(&input.format),
                span: input.span,
                stage_span: input.stage_span,
            })
            .collect(),
        sinks: plan
            .sinks
            .iter()
            .map(|sink| SinkJson {
                pipeline: pipeline_label_text(&sink.pipeline),
                sink: sink_text(&sink.sink),
                format: format_decision_json(&sink.format),
                span: sink.span,
                stage_span: sink.stage_span,
            })
            .collect(),
        dependencies: plan
            .dependencies
            .iter()
            .map(|dependency| DependencyJson {
                logical_path: dependency.logical_path.clone(),
                resolved_path: dependency.resolved_path.display().to_string(),
                inferred_format: dependency
                    .inferred_format
                    .map(|format| format.canonical_name().to_string()),
                span: dependency.span,
            })
            .collect(),
        streams: plan
            .streams
            .iter()
            .map(|stream| StreamJson {
                pipeline: pipeline_label_text(&stream.pipeline),
                stream: stream_kind_text(stream.stream),
                direction: stream_direction_text(stream.direction),
                explicit_format: stream.explicit_format.clone(),
                span: stream.span,
            })
            .collect(),
    }
}

fn execution_plan_json(plan: &ExecutionPlan) -> ExecutionPlanJson {
    ExecutionPlanJson {
        dry_run: plan.dry_run,
        stdout_format: plan
            .stdout_format
            .map(|format| format.canonical_name().to_string()),
        steps: plan.steps.iter().map(execution_step_json).collect(),
    }
}

fn execution_step_json(step: &ExecutionPlanStep) -> ExecutionStepJson {
    match step {
        ExecutionPlanStep::Load { source, format } => ExecutionStepJson::Load {
            source: source.clone(),
            format: format.clone(),
        },
        ExecutionPlanStep::Binding { name } => ExecutionStepJson::Binding { name: name.clone() },
        ExecutionPlanStep::Transform { stage } => ExecutionStepJson::Transform {
            stage: stage.clone(),
        },
        ExecutionPlanStep::Save { sink, format } => ExecutionStepJson::Save {
            sink: sink.clone(),
            format: format.clone(),
        },
        ExecutionPlanStep::Stdout { format } => ExecutionStepJson::Stdout {
            format: format.clone(),
        },
    }
}

fn format_decision_json(format: &FormatDecision) -> FormatDecisionJson {
    FormatDecisionJson {
        explicit: format.explicit.clone(),
        inferred_from_path: format
            .inferred_from_path
            .map(|format| format.canonical_name().to_string()),
        effective: format.effective_name(),
        sniffing: match &format.sniffing {
            SniffingDecision::NotNeeded => SniffingJson::NotNeeded,
            SniffingDecision::Deferred { reason } => SniffingJson::Deferred {
                reason: sniffing_reason_text(reason),
            },
        },
    }
}

fn pipeline_label_text(label: &PipelineLabel) -> String {
    match label {
        PipelineLabel::Main => "main".to_string(),
        PipelineLabel::Binding(name) => format!("binding:{name}"),
    }
}

fn source_text(source: &SourceDescriptor) -> String {
    match source {
        SourceDescriptor::Path { logical_path, .. } => logical_path.clone(),
        SourceDescriptor::Stdin => "stdin".to_string(),
    }
}

fn sink_text(sink: &SinkDescriptor) -> String {
    match sink {
        SinkDescriptor::Path { logical_path, .. } => logical_path.clone(),
        SinkDescriptor::Stdout => "stdout".to_string(),
    }
}

fn sniffing_reason_text(reason: &SniffingReason) -> &'static str {
    match reason {
        SniffingReason::StdinWithoutExplicitFormat => "stdin-without-explicit-format",
        SniffingReason::StdoutWithoutExplicitFormat => "stdout-without-explicit-format",
        SniffingReason::PathWithoutKnownExtension => "path-without-known-extension",
    }
}

fn stream_kind_text(stream: StreamKind) -> &'static str {
    match stream {
        StreamKind::Stdin => "stdin",
        StreamKind::Stdout => "stdout",
    }
}

fn stream_direction_text(direction: StreamDirection) -> &'static str {
    match direction {
        StreamDirection::Read => "read",
        StreamDirection::Write => "write",
    }
}

fn execution_step_text(step: &ExecutionPlanStep) -> String {
    match step {
        ExecutionPlanStep::Load { source, format } => format!("load {source} format {format}"),
        ExecutionPlanStep::Binding { name } => format!("binding {name}"),
        ExecutionPlanStep::Transform { stage } => stage.clone(),
        ExecutionPlanStep::Save { sink, format } => format!("save {sink} format {format}"),
        ExecutionPlanStep::Stdout { format } => format!("stdout format {format}"),
    }
}

#[derive(Serialize)]
struct ProgramJson {
    bindings: Vec<BindingJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    main: Option<PipelineJson>,
}

#[derive(Serialize)]
struct BindingJson {
    name: SpannedJson<String>,
    pipeline: PipelineJson,
}

#[derive(Serialize)]
struct PipelineJson {
    start: PipelineStartJson,
    stages: Vec<StageJson>,
    span: Span,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum PipelineStartJson {
    Load {
        source: SourceJson,
        #[serde(skip_serializing_if = "Option::is_none")]
        format: Option<SpannedJson<String>>,
        span: Span,
    },
    Binding {
        name: SpannedJson<String>,
    },
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum SourceJson {
    Path { path: SpannedJson<String> },
    Stdin { span: Span },
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum SinkJsonAst {
    Path { path: SpannedJson<String> },
    Stdout { span: Span },
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum StageJson {
    Filter {
        expr: ExprJson,
        span: Span,
    },
    Select {
        items: Vec<SelectItemJson>,
        span: Span,
    },
    Drop {
        columns: Vec<SpannedJson<String>>,
        span: Span,
    },
    Rename {
        items: Vec<RenameItemJson>,
        span: Span,
    },
    Mutate {
        items: Vec<MutateItemJson>,
        span: Span,
    },
    GroupBy {
        columns: Vec<SpannedJson<String>>,
        span: Span,
    },
    Agg {
        items: Vec<AggItemJson>,
        span: Span,
    },
    Sort {
        items: Vec<SortItemJson>,
        span: Span,
    },
    Limit {
        n: usize,
        span: Span,
    },
    Join {
        source: SpannedJson<String>,
        on: JoinOnJson,
        join_kind: &'static str,
        #[serde(skip_serializing_if = "Option::is_none")]
        kind_span: Option<Span>,
        span: Span,
    },
    Union {
        source: SpannedJson<String>,
        options: Vec<UnionOptionJson>,
        span: Span,
    },
    Distinct {
        columns: Vec<SpannedJson<String>>,
        span: Span,
    },
    Save {
        sink: SinkJsonAst,
        #[serde(skip_serializing_if = "Option::is_none")]
        format: Option<SpannedJson<String>>,
        span: Span,
    },
    Unsupported {
        name: SpannedJson<String>,
        span: Span,
    },
}

#[derive(Serialize)]
struct SelectItemJson {
    column: SpannedJson<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    alias: Option<SpannedJson<String>>,
}

#[derive(Serialize)]
struct RenameItemJson {
    old: SpannedJson<String>,
    new: SpannedJson<String>,
}

#[derive(Serialize)]
struct MutateItemJson {
    column: SpannedJson<String>,
    expr: ExprJson,
    span: Span,
}

#[derive(Serialize)]
struct AggItemJson {
    function: SpannedJson<String>,
    args: Vec<ExprJson>,
    alias: SpannedJson<String>,
    span: Span,
}

#[derive(Serialize)]
struct SortItemJson {
    column: SpannedJson<String>,
    direction: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    nulls: Option<&'static str>,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum JoinOnJson {
    Same {
        column: SpannedJson<String>,
    },
    Pair {
        left: SpannedJson<String>,
        right: SpannedJson<String>,
        span: Span,
    },
}

#[derive(Serialize)]
struct UnionOptionJson {
    option: &'static str,
    value: SpannedJson<bool>,
    span: Span,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ExprJson {
    Quoted {
        value: SpannedJson<String>,
    },
    Number {
        value: SpannedJson<f64>,
    },
    Bool {
        value: SpannedJson<bool>,
    },
    Null {
        span: Span,
    },
    Ident {
        value: SpannedJson<String>,
    },
    Call {
        name: SpannedJson<String>,
        args: Vec<ExprJson>,
        span: Span,
    },
    Window {
        function: SpannedJson<String>,
        args: Vec<ExprJson>,
        spec: WindowSpecJson,
        span: Span,
    },
    Unary {
        op: &'static str,
        expr: Box<ExprJson>,
        span: Span,
    },
    Binary {
        left: Box<ExprJson>,
        op: &'static str,
        right: Box<ExprJson>,
        span: Span,
    },
}

#[derive(Serialize)]
struct WindowSpecJson {
    partition_by: Vec<SpannedJson<String>>,
    order_by: Vec<SortItemJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    frame: Option<WindowFrameJson>,
    span: Span,
}

#[derive(Serialize)]
struct WindowFrameJson {
    start: FrameBoundJson,
    end: FrameBoundJson,
    span: Span,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum FrameBoundJson {
    UnboundedPreceding { span: Span },
    Preceding { rows: usize, span: Span },
    CurrentRow { span: Span },
    Following { rows: usize, span: Span },
    UnboundedFollowing { span: Span },
}

#[derive(Serialize)]
struct SpannedJson<T>
where
    T: Serialize,
{
    value: T,
    span: Span,
}

fn spanned_json<T>(spanned: &Spanned<T>) -> SpannedJson<T>
where
    T: Clone + Serialize,
{
    SpannedJson {
        value: spanned.value.clone(),
        span: spanned.span,
    }
}

fn program_json(program: &Program) -> ProgramJson {
    ProgramJson {
        bindings: program.bindings.iter().map(binding_json).collect(),
        main: program.main.as_ref().map(pipeline_json),
    }
}

fn binding_json(binding: &Binding) -> BindingJson {
    BindingJson {
        name: spanned_json(&binding.name),
        pipeline: pipeline_json(&binding.pipeline),
    }
}

fn pipeline_json(pipeline: &Pipeline) -> PipelineJson {
    PipelineJson {
        start: pipeline_start_json(&pipeline.start),
        stages: pipeline.stages.iter().map(stage_json).collect(),
        span: pipeline.span,
    }
}

fn pipeline_start_json(start: &PipelineStart) -> PipelineStartJson {
    match start {
        PipelineStart::Load(load) => PipelineStartJson::Load {
            source: source_json(&load.source),
            format: load.format.as_ref().map(spanned_json),
            span: load.span,
        },
        PipelineStart::Binding(name) => PipelineStartJson::Binding {
            name: spanned_json(name),
        },
    }
}

fn source_json(source: &SourceRef) -> SourceJson {
    match source {
        SourceRef::Path(path) => SourceJson::Path {
            path: spanned_json(path),
        },
        SourceRef::Stdin(span) => SourceJson::Stdin { span: *span },
    }
}

fn sink_ast_json(sink: &SinkRef) -> SinkJsonAst {
    match sink {
        SinkRef::Path(path) => SinkJsonAst::Path {
            path: spanned_json(path),
        },
        SinkRef::Stdout(span) => SinkJsonAst::Stdout { span: *span },
    }
}

fn stage_json(stage: &Stage) -> StageJson {
    match stage {
        Stage::Filter { expr, span } => StageJson::Filter {
            expr: expr_json(expr),
            span: *span,
        },
        Stage::Select { items, span } => StageJson::Select {
            items: items
                .iter()
                .map(|item| SelectItemJson {
                    column: spanned_json(&item.column),
                    alias: item.alias.as_ref().map(spanned_json),
                })
                .collect(),
            span: *span,
        },
        Stage::Drop { columns, span } => StageJson::Drop {
            columns: columns.iter().map(spanned_json).collect(),
            span: *span,
        },
        Stage::Rename { items, span } => StageJson::Rename {
            items: items
                .iter()
                .map(|item| RenameItemJson {
                    old: spanned_json(&item.old),
                    new: spanned_json(&item.new),
                })
                .collect(),
            span: *span,
        },
        Stage::Mutate { items, span } => StageJson::Mutate {
            items: items.iter().map(mutate_item_json).collect(),
            span: *span,
        },
        Stage::GroupBy { columns, span } => StageJson::GroupBy {
            columns: columns.iter().map(spanned_json).collect(),
            span: *span,
        },
        Stage::Agg { items, span } => StageJson::Agg {
            items: items.iter().map(agg_item_json).collect(),
            span: *span,
        },
        Stage::Sort { items, span } => StageJson::Sort {
            items: items
                .iter()
                .map(|item| SortItemJson {
                    column: spanned_json(&item.column),
                    direction: sort_direction_text(item.direction),
                    nulls: item.nulls.map(|nulls| match nulls {
                        pdl_syntax::NullsOrder::First => "first",
                        pdl_syntax::NullsOrder::Last => "last",
                    }),
                })
                .collect(),
            span: *span,
        },
        Stage::Limit { n, span } => StageJson::Limit { n: *n, span: *span },
        Stage::Join {
            source,
            on,
            kind,
            kind_span,
            span,
        } => StageJson::Join {
            source: spanned_json(source),
            on: join_on_json(on),
            join_kind: kind.as_str(),
            kind_span: *kind_span,
            span: *span,
        },
        Stage::Union {
            source,
            options,
            span,
        } => StageJson::Union {
            source: spanned_json(source),
            options: options
                .iter()
                .map(|option| UnionOptionJson {
                    option: match option.kind {
                        UnionOptionKind::ByName => "by_name",
                        UnionOptionKind::Distinct => "distinct",
                    },
                    value: spanned_json(&option.value),
                    span: option.span,
                })
                .collect(),
            span: *span,
        },
        Stage::Distinct { columns, span } => StageJson::Distinct {
            columns: columns.iter().map(spanned_json).collect(),
            span: *span,
        },
        Stage::Save(save) => save_stage_json(save),
        Stage::Unsupported { name, span } => StageJson::Unsupported {
            name: spanned_json(name),
            span: *span,
        },
    }
}

fn save_stage_json(save: &SaveStage) -> StageJson {
    StageJson::Save {
        sink: sink_ast_json(&save.sink),
        format: save.format.as_ref().map(spanned_json),
        span: save.span,
    }
}

fn mutate_item_json(item: &MutateItem) -> MutateItemJson {
    MutateItemJson {
        column: spanned_json(&item.column),
        expr: expr_json(&item.expr),
        span: item.span,
    }
}

fn agg_item_json(item: &AggItem) -> AggItemJson {
    AggItemJson {
        function: spanned_json(&item.function),
        args: item.args.iter().map(expr_json).collect(),
        alias: spanned_json(&item.alias),
        span: item.span,
    }
}

fn join_on_json(on: &JoinOn) -> JoinOnJson {
    match on {
        JoinOn::Same(column) => JoinOnJson::Same {
            column: spanned_json(column),
        },
        JoinOn::Pair { left, right, span } => JoinOnJson::Pair {
            left: spanned_json(left),
            right: spanned_json(right),
            span: *span,
        },
    }
}

fn expr_json(expr: &Expr) -> ExprJson {
    match expr {
        Expr::Quoted(value) => ExprJson::Quoted {
            value: spanned_json(value),
        },
        Expr::Number(value) => ExprJson::Number {
            value: spanned_json(value),
        },
        Expr::Bool(value) => ExprJson::Bool {
            value: spanned_json(value),
        },
        Expr::Null(span) => ExprJson::Null { span: *span },
        Expr::Ident(value) => ExprJson::Ident {
            value: spanned_json(value),
        },
        Expr::Call { name, args, span } => ExprJson::Call {
            name: spanned_json(name),
            args: args.iter().map(expr_json).collect(),
            span: *span,
        },
        Expr::Window {
            function,
            args,
            spec,
            span,
        } => ExprJson::Window {
            function: spanned_json(function),
            args: args.iter().map(expr_json).collect(),
            spec: window_spec_json(spec),
            span: *span,
        },
        Expr::Unary { op, expr, span } => ExprJson::Unary {
            op: unary_op_text(*op),
            expr: Box::new(expr_json(expr)),
            span: *span,
        },
        Expr::Binary {
            left,
            op,
            right,
            span,
        } => ExprJson::Binary {
            left: Box::new(expr_json(left)),
            op: binary_op_text(*op),
            right: Box::new(expr_json(right)),
            span: *span,
        },
    }
}

fn window_spec_json(spec: &WindowSpec) -> WindowSpecJson {
    WindowSpecJson {
        partition_by: spec.partition_by.iter().map(spanned_json).collect(),
        order_by: spec
            .order_by
            .iter()
            .map(|item| SortItemJson {
                column: spanned_json(&item.column),
                direction: sort_direction_text(item.direction),
                nulls: item.nulls.map(|nulls| match nulls {
                    pdl_syntax::NullsOrder::First => "first",
                    pdl_syntax::NullsOrder::Last => "last",
                }),
            })
            .collect(),
        frame: spec.frame.as_ref().map(window_frame_json),
        span: spec.span,
    }
}

fn window_frame_json(frame: &WindowFrame) -> WindowFrameJson {
    WindowFrameJson {
        start: frame_bound_json(&frame.start),
        end: frame_bound_json(&frame.end),
        span: frame.span,
    }
}

fn frame_bound_json(bound: &FrameBound) -> FrameBoundJson {
    match bound {
        FrameBound::UnboundedPreceding { span } => {
            FrameBoundJson::UnboundedPreceding { span: *span }
        }
        FrameBound::Preceding { rows, span } => FrameBoundJson::Preceding {
            rows: *rows,
            span: *span,
        },
        FrameBound::CurrentRow { span } => FrameBoundJson::CurrentRow { span: *span },
        FrameBound::Following { rows, span } => FrameBoundJson::Following {
            rows: *rows,
            span: *span,
        },
        FrameBound::UnboundedFollowing { span } => {
            FrameBoundJson::UnboundedFollowing { span: *span }
        }
    }
}

fn sort_direction_text(direction: SortDirection) -> &'static str {
    match direction {
        SortDirection::Asc => "asc",
        SortDirection::Desc => "desc",
    }
}

fn unary_op_text(op: UnaryOp) -> &'static str {
    match op {
        UnaryOp::Not => "not",
        UnaryOp::Neg => "neg",
    }
}

fn binary_op_text(op: BinaryOp) -> &'static str {
    match op {
        BinaryOp::Or => "or",
        BinaryOp::And => "and",
        BinaryOp::Eq => "eq",
        BinaryOp::Ne => "ne",
        BinaryOp::Lt => "lt",
        BinaryOp::Lte => "lte",
        BinaryOp::Gt => "gt",
        BinaryOp::Gte => "gte",
        BinaryOp::Add => "add",
        BinaryOp::Sub => "sub",
        BinaryOp::Mul => "mul",
        BinaryOp::Div => "div",
        BinaryOp::Rem => "rem",
    }
}

#[derive(Serialize)]
struct ProgramIrJson {
    bindings: Vec<BindingIrJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    main: Option<PipelineIrJson>,
}

#[derive(Serialize)]
struct BindingIrJson {
    name: String,
    span: Span,
    pipeline: PipelineIrJson,
}

#[derive(Serialize)]
struct PipelineIrJson {
    start: PipelineStartIrJson,
    stages: Vec<StageIrJson>,
    span: Span,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum PipelineStartIrJson {
    Load {
        source: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        format: Option<String>,
        span: Span,
    },
    Binding {
        name: String,
        span: Span,
    },
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum StageIrJson {
    Filter {
        expr: ExprIrJson,
        span: Span,
    },
    Select {
        items: Vec<SelectItemIrJson>,
        span: Span,
    },
    Drop {
        columns: Vec<String>,
        span: Span,
    },
    Rename {
        items: Vec<RenameItemIrJson>,
        span: Span,
    },
    Mutate {
        items: Vec<MutateItemIrJson>,
        span: Span,
    },
    GroupBy {
        columns: Vec<String>,
        span: Span,
    },
    Agg {
        items: Vec<AggItemIrJson>,
        span: Span,
    },
    Sort {
        items: Vec<SortItemIrJson>,
        span: Span,
    },
    Limit {
        n: usize,
        span: Span,
    },
    Join {
        source: String,
        source_span: Span,
        left_key: String,
        right_key: String,
        join_kind: &'static str,
        span: Span,
    },
    Union {
        source: String,
        source_span: Span,
        by_name: bool,
        distinct: bool,
        span: Span,
    },
    Distinct {
        columns: Vec<String>,
        span: Span,
    },
    Save {
        sink: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        format: Option<String>,
        span: Span,
    },
    Unsupported {
        name: String,
        span: Span,
    },
}

#[derive(Serialize)]
struct SelectItemIrJson {
    source: String,
    output: String,
    span: Span,
}

#[derive(Serialize)]
struct RenameItemIrJson {
    old: String,
    new: String,
    span: Span,
}

#[derive(Serialize)]
struct MutateItemIrJson {
    column: String,
    expr: ExprIrJson,
    span: Span,
}

#[derive(Serialize)]
struct AggItemIrJson {
    function: String,
    args: Vec<ExprIrJson>,
    alias: String,
    span: Span,
}

#[derive(Serialize)]
struct SortItemIrJson {
    column: String,
    direction: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    nulls: Option<&'static str>,
    span: Span,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ExprIrJson {
    Quoted {
        value: String,
        span: Span,
    },
    Number {
        value: f64,
        span: Span,
    },
    Bool {
        value: bool,
        span: Span,
    },
    Null {
        span: Span,
    },
    Ident {
        value: String,
        span: Span,
    },
    Call {
        name: String,
        args: Vec<ExprIrJson>,
        span: Span,
    },
    Window {
        function: String,
        args: Vec<ExprIrJson>,
        spec: WindowSpecIrJson,
        span: Span,
    },
    Unary {
        op: &'static str,
        expr: Box<ExprIrJson>,
        span: Span,
    },
    Binary {
        left: Box<ExprIrJson>,
        op: &'static str,
        right: Box<ExprIrJson>,
        span: Span,
    },
}

#[derive(Serialize)]
struct WindowSpecIrJson {
    partition_by: Vec<String>,
    order_by: Vec<SortItemIrJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    frame: Option<WindowFrameIrJson>,
    span: Span,
}

#[derive(Serialize)]
struct WindowFrameIrJson {
    start: FrameBoundIrJson,
    end: FrameBoundIrJson,
    span: Span,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum FrameBoundIrJson {
    UnboundedPreceding { span: Span },
    Preceding { rows: usize, span: Span },
    CurrentRow { span: Span },
    Following { rows: usize, span: Span },
    UnboundedFollowing { span: Span },
}

fn program_ir_json(ir: &ProgramIr) -> ProgramIrJson {
    ProgramIrJson {
        bindings: ir
            .bindings
            .iter()
            .map(|binding| BindingIrJson {
                name: binding.name.clone(),
                span: binding.span,
                pipeline: PipelineIrJson {
                    start: pipeline_start_ir_json(&binding.pipeline.start),
                    stages: binding.pipeline.stages.iter().map(stage_ir_json).collect(),
                    span: binding.pipeline.span,
                },
            })
            .collect(),
        main: ir.main.as_ref().map(|pipeline| PipelineIrJson {
            start: pipeline_start_ir_json(&pipeline.start),
            stages: pipeline.stages.iter().map(stage_ir_json).collect(),
            span: pipeline.span,
        }),
    }
}

fn pipeline_start_ir_json(start: &PipelineStartIr) -> PipelineStartIrJson {
    match start {
        PipelineStartIr::Load {
            source,
            format,
            span,
        } => PipelineStartIrJson::Load {
            source: match source {
                pdl_semantics::SourceIr::Path(path) => path.clone(),
                pdl_semantics::SourceIr::Stdin => "stdin".to_string(),
            },
            format: format.clone(),
            span: *span,
        },
        PipelineStartIr::Binding { name, span } => PipelineStartIrJson::Binding {
            name: name.clone(),
            span: *span,
        },
    }
}

fn stage_ir_json(stage: &StageIr) -> StageIrJson {
    match stage {
        StageIr::Filter { expr, span } => StageIrJson::Filter {
            expr: expr_ir_json(expr),
            span: *span,
        },
        StageIr::Select { items, span } => StageIrJson::Select {
            items: items
                .iter()
                .map(|item| SelectItemIrJson {
                    source: item.source.clone(),
                    output: item.output.clone(),
                    span: item.span,
                })
                .collect(),
            span: *span,
        },
        StageIr::Drop { columns, span } => StageIrJson::Drop {
            columns: columns.clone(),
            span: *span,
        },
        StageIr::Rename { items, span } => StageIrJson::Rename {
            items: items
                .iter()
                .map(|item| RenameItemIrJson {
                    old: item.old.clone(),
                    new: item.new.clone(),
                    span: item.span,
                })
                .collect(),
            span: *span,
        },
        StageIr::Mutate { items, span } => StageIrJson::Mutate {
            items: items
                .iter()
                .map(|item| MutateItemIrJson {
                    column: item.column.clone(),
                    expr: expr_ir_json(&item.expr),
                    span: item.span,
                })
                .collect(),
            span: *span,
        },
        StageIr::GroupBy { columns, span } => StageIrJson::GroupBy {
            columns: columns.clone(),
            span: *span,
        },
        StageIr::Agg { items, span } => StageIrJson::Agg {
            items: items
                .iter()
                .map(|item| AggItemIrJson {
                    function: item.function.clone(),
                    args: item.args.iter().map(expr_ir_json).collect(),
                    alias: item.alias.clone(),
                    span: item.span,
                })
                .collect(),
            span: *span,
        },
        StageIr::Sort { items, span } => StageIrJson::Sort {
            items: items
                .iter()
                .map(|item| SortItemIrJson {
                    column: item.column.clone(),
                    direction: match item.direction {
                        SortDirectionIr::Asc => "asc",
                        SortDirectionIr::Desc => "desc",
                    },
                    nulls: item.nulls.map(|nulls| match nulls {
                        pdl_semantics::NullsOrderIr::First => "first",
                        pdl_semantics::NullsOrderIr::Last => "last",
                    }),
                    span: item.span,
                })
                .collect(),
            span: *span,
        },
        StageIr::Limit { n, span } => StageIrJson::Limit { n: *n, span: *span },
        StageIr::Join {
            source,
            source_span,
            left_key,
            right_key,
            kind,
            span,
        } => StageIrJson::Join {
            source: source.clone(),
            source_span: *source_span,
            left_key: left_key.clone(),
            right_key: right_key.clone(),
            join_kind: join_kind_ir_text(*kind),
            span: *span,
        },
        StageIr::Union {
            source,
            source_span,
            by_name,
            distinct,
            span,
        } => StageIrJson::Union {
            source: source.clone(),
            source_span: *source_span,
            by_name: *by_name,
            distinct: *distinct,
            span: *span,
        },
        StageIr::Distinct { columns, span } => StageIrJson::Distinct {
            columns: columns.clone(),
            span: *span,
        },
        StageIr::Save { sink, format, span } => StageIrJson::Save {
            sink: match sink {
                SinkIr::Path(path) => path.clone(),
                SinkIr::Stdout => "stdout".to_string(),
            },
            format: format.clone(),
            span: *span,
        },
        StageIr::Unsupported { name, span } => StageIrJson::Unsupported {
            name: name.clone(),
            span: *span,
        },
    }
}

fn expr_ir_json(expr: &ExprIr) -> ExprIrJson {
    match expr {
        ExprIr::Quoted { value, span } => ExprIrJson::Quoted {
            value: value.clone(),
            span: *span,
        },
        ExprIr::Number { value, span } => ExprIrJson::Number {
            value: *value,
            span: *span,
        },
        ExprIr::Bool { value, span } => ExprIrJson::Bool {
            value: *value,
            span: *span,
        },
        ExprIr::Null { span } => ExprIrJson::Null { span: *span },
        ExprIr::Ident { value, span } => ExprIrJson::Ident {
            value: value.clone(),
            span: *span,
        },
        ExprIr::Call { name, args, span } => ExprIrJson::Call {
            name: name.clone(),
            args: args.iter().map(expr_ir_json).collect(),
            span: *span,
        },
        ExprIr::Window {
            function,
            args,
            spec,
            span,
        } => ExprIrJson::Window {
            function: function.clone(),
            args: args.iter().map(expr_ir_json).collect(),
            spec: window_spec_ir_json(spec),
            span: *span,
        },
        ExprIr::Unary { op, expr, span } => ExprIrJson::Unary {
            op: unary_op_ir_text(*op),
            expr: Box::new(expr_ir_json(expr)),
            span: *span,
        },
        ExprIr::Binary {
            left,
            op,
            right,
            span,
        } => ExprIrJson::Binary {
            left: Box::new(expr_ir_json(left)),
            op: binary_op_ir_text(*op),
            right: Box::new(expr_ir_json(right)),
            span: *span,
        },
    }
}

fn window_spec_ir_json(spec: &WindowSpecIr) -> WindowSpecIrJson {
    WindowSpecIrJson {
        partition_by: spec.partition_by.clone(),
        order_by: spec
            .order_by
            .iter()
            .map(|item| SortItemIrJson {
                column: item.column.clone(),
                direction: match item.direction {
                    SortDirectionIr::Asc => "asc",
                    SortDirectionIr::Desc => "desc",
                },
                nulls: item.nulls.map(|nulls| match nulls {
                    pdl_semantics::NullsOrderIr::First => "first",
                    pdl_semantics::NullsOrderIr::Last => "last",
                }),
                span: item.span,
            })
            .collect(),
        frame: spec.frame.as_ref().map(window_frame_ir_json),
        span: spec.span,
    }
}

fn window_frame_ir_json(frame: &WindowFrameIr) -> WindowFrameIrJson {
    WindowFrameIrJson {
        start: frame_bound_ir_json(&frame.start),
        end: frame_bound_ir_json(&frame.end),
        span: frame.span,
    }
}

fn frame_bound_ir_json(bound: &FrameBoundIr) -> FrameBoundIrJson {
    match bound {
        FrameBoundIr::UnboundedPreceding { span } => {
            FrameBoundIrJson::UnboundedPreceding { span: *span }
        }
        FrameBoundIr::Preceding { rows, span } => FrameBoundIrJson::Preceding {
            rows: *rows,
            span: *span,
        },
        FrameBoundIr::CurrentRow { span } => FrameBoundIrJson::CurrentRow { span: *span },
        FrameBoundIr::Following { rows, span } => FrameBoundIrJson::Following {
            rows: *rows,
            span: *span,
        },
        FrameBoundIr::UnboundedFollowing { span } => {
            FrameBoundIrJson::UnboundedFollowing { span: *span }
        }
    }
}

fn join_kind_ir_text(kind: JoinKindIr) -> &'static str {
    match kind {
        JoinKindIr::Inner => "inner",
        JoinKindIr::Left => "left",
        JoinKindIr::Right => "right",
        JoinKindIr::Full => "full",
        JoinKindIr::Semi => "semi",
        JoinKindIr::Anti => "anti",
    }
}

fn unary_op_ir_text(op: UnaryOpIr) -> &'static str {
    match op {
        UnaryOpIr::Not => "not",
        UnaryOpIr::Neg => "neg",
    }
}

fn binary_op_ir_text(op: BinaryOpIr) -> &'static str {
    match op {
        BinaryOpIr::Or => "or",
        BinaryOpIr::And => "and",
        BinaryOpIr::Eq => "eq",
        BinaryOpIr::Ne => "ne",
        BinaryOpIr::Lt => "lt",
        BinaryOpIr::Lte => "lte",
        BinaryOpIr::Gt => "gt",
        BinaryOpIr::Gte => "gte",
        BinaryOpIr::Add => "add",
        BinaryOpIr::Sub => "sub",
        BinaryOpIr::Mul => "mul",
        BinaryOpIr::Div => "div",
        BinaryOpIr::Rem => "rem",
    }
}
