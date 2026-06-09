// Plan/manifest JSON types, builders, and text helpers extracted from
// `render.rs` as part of the v0.42 split. See `render.rs` for the cross-module
// layout overview.

use pdl_core::Span;
use pdl_driver::{
    DriverPlan, FormatDecision, PipelineLabel, PreparedProgram, SinkDescriptor, SniffingDecision,
    SniffingReason, SourceDescriptor, StreamDirection, StreamKind,
};
use pdl_exec::{ExecutionPlan, ExecutionPlanStep, NativeUnsupportedReason, PlanObservability};
use serde::Serialize;

use crate::render::schema_render::{output_schema_json, SchemaJson};
use crate::render::{final_schema_columns, ManifestJson};

#[derive(Serialize)]
pub(crate) struct DriverPlanJson {
    pub(crate) source_path: String,
    pub(crate) base_dir: String,
    pub(crate) inputs: Vec<InputJson>,
    pub(crate) sinks: Vec<SinkJson>,
    pub(crate) dependencies: Vec<DependencyJson>,
    pub(crate) streams: Vec<StreamJson>,
}

#[derive(Serialize)]
pub(crate) struct InputJson {
    pub(crate) pipeline: String,
    pub(crate) source: String,
    pub(crate) format: FormatDecisionJson,
    pub(crate) span: Span,
    pub(crate) stage_span: Span,
}

#[derive(Serialize)]
pub(crate) struct SinkJson {
    pub(crate) pipeline: String,
    pub(crate) sink: String,
    pub(crate) format: FormatDecisionJson,
    pub(crate) span: Span,
    pub(crate) stage_span: Span,
}

#[derive(Serialize)]
pub(crate) struct DependencyJson {
    pub(crate) logical_path: String,
    pub(crate) resolved_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) inferred_format: Option<String>,
    pub(crate) span: Span,
}

#[derive(Serialize)]
pub(crate) struct StreamJson {
    pub(crate) pipeline: String,
    pub(crate) stream: &'static str,
    pub(crate) direction: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) explicit_format: Option<String>,
    pub(crate) span: Span,
}

#[derive(Serialize)]
pub(crate) struct FormatDecisionJson {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) explicit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) inferred_from_path: Option<String>,
    pub(crate) effective: String,
    pub(crate) sniffing: SniffingJson,
}

#[derive(Serialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub(crate) enum SniffingJson {
    NotNeeded,
    Deferred { reason: &'static str },
}

#[derive(Serialize)]
pub(crate) struct ExecutionPlanJson {
    pub(crate) dry_run: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) stdout_format: Option<String>,
    pub(crate) observability: PlanObservabilityJson,
    pub(crate) steps: Vec<ExecutionStepJson>,
}

#[derive(Serialize)]
pub(crate) struct PlanObservabilityJson {
    pub(crate) requested_engine: &'static str,
    pub(crate) selected_engine: &'static str,
    pub(crate) eligible_engine: &'static str,
    pub(crate) native_eligible: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) fallback_reason: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) source_boundary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) input_format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) output_format: Option<String>,
    pub(crate) sink_strategy: &'static str,
    pub(crate) blocking_stages: Vec<String>,
    pub(crate) row_materialization: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) required_source_columns: Option<Vec<String>>,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum ExecutionStepJson {
    Output { name: String },
    Load { source: String, format: String },
    Binding { name: String },
    Transform { stage: String },
    Save { sink: String, format: String },
    Stdout { format: String },
}

#[derive(Serialize)]
pub(crate) struct StreamInteropJson {
    pub(crate) stdout_format: &'static str,
    pub(crate) stdin_format_hint: &'static str,
}

pub(crate) fn driver_plan_json(plan: &DriverPlan) -> DriverPlanJson {
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

pub(crate) fn execution_plan_json(plan: &ExecutionPlan) -> ExecutionPlanJson {
    ExecutionPlanJson {
        dry_run: plan.dry_run,
        stdout_format: plan
            .stdout_format
            .map(|format| format.canonical_name().to_string()),
        observability: plan_observability_json(&plan.observability),
        steps: plan.steps.iter().map(execution_step_json).collect(),
    }
}

pub(crate) fn plan_observability_json(observability: &PlanObservability) -> PlanObservabilityJson {
    PlanObservabilityJson {
        requested_engine: observability.requested_engine.as_str(),
        selected_engine: observability.selected_engine.as_str(),
        eligible_engine: observability.eligible_engine.as_str(),
        native_eligible: observability.native_eligible,
        fallback_reason: observability
            .fallback_reason
            .map(NativeUnsupportedReason::code),
        source_boundary: observability.source_boundary.clone(),
        input_format: observability.input_format.clone(),
        output_format: observability.output_format.clone(),
        sink_strategy: observability.sink_strategy.as_str(),
        blocking_stages: observability.blocking_stages.clone(),
        row_materialization: observability.row_materialization,
        required_source_columns: observability.required_source_columns.clone(),
    }
}

pub(crate) fn execution_step_json(step: &ExecutionPlanStep) -> ExecutionStepJson {
    match step {
        ExecutionPlanStep::Output { name } => ExecutionStepJson::Output { name: name.clone() },
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

pub(crate) fn format_decision_json(format: &FormatDecision) -> FormatDecisionJson {
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

pub(crate) fn pipeline_label_text(label: &PipelineLabel) -> String {
    match label {
        PipelineLabel::Main => "main".to_string(),
        PipelineLabel::Binding(name) => format!("binding:{name}"),
        PipelineLabel::Output(name) => format!("output:{name}"),
    }
}

pub(crate) fn source_text(source: &SourceDescriptor) -> String {
    match source {
        SourceDescriptor::Path { logical_path, .. } => logical_path.clone(),
        SourceDescriptor::Stdin => "stdin".to_string(),
    }
}

pub(crate) fn sink_text(sink: &SinkDescriptor) -> String {
    match sink {
        SinkDescriptor::Path { logical_path, .. } => logical_path.clone(),
        SinkDescriptor::Stdout => "stdout".to_string(),
    }
}

pub(crate) fn sniffing_reason_text(reason: &SniffingReason) -> &'static str {
    match reason {
        SniffingReason::StdinWithoutExplicitFormat => "stdin-without-explicit-format",
        SniffingReason::StdoutWithoutExplicitFormat => "stdout-without-explicit-format",
        SniffingReason::PathWithoutKnownExtension => "path-without-known-extension",
    }
}

pub(crate) fn stream_kind_text(stream: StreamKind) -> &'static str {
    match stream {
        StreamKind::Stdin => "stdin",
        StreamKind::Stdout => "stdout",
    }
}

pub(crate) fn stream_direction_text(direction: StreamDirection) -> &'static str {
    match direction {
        StreamDirection::Read => "read",
        StreamDirection::Write => "write",
    }
}

pub(crate) fn execution_step_text(step: &ExecutionPlanStep) -> String {
    match step {
        ExecutionPlanStep::Output { name } => format!("output {name}"),
        ExecutionPlanStep::Load { source, format } => format!("load {source} format {format}"),
        ExecutionPlanStep::Binding { name } => format!("binding {name}"),
        ExecutionPlanStep::Transform { stage } => stage.clone(),
        ExecutionPlanStep::Save { sink, format } => format!("save {sink} format {format}"),
        ExecutionPlanStep::Stdout { format } => format!("stdout format {format}"),
    }
}

pub(crate) fn manifest_json(prepared: &PreparedProgram, plan: &ExecutionPlan) -> ManifestJson {
    let stdout_format = plan.stdout_format.map(|format| format.canonical_name());
    ManifestJson {
        manifest_version: "0.43.0",
        implementation_version: env!("CARGO_PKG_VERSION"),
        language_version: "0.43.0",
        source_path: prepared.path.display().to_string(),
        driver: driver_plan_json(&prepared.driver_plan),
        execution: execution_plan_json(plan),
        output_schemas: output_schema_json(prepared),
        final_schema: final_schema_columns(prepared).map(SchemaJson::from_columns),
        diagnostics: prepared.diagnostics(),
        stream_interop: (stdout_format == Some("arrow-stream")).then_some(StreamInteropJson {
            stdout_format: "arrow-stream",
            stdin_format_hint: "arrow-stream",
        }),
    }
}
