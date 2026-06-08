// `render.rs` is the CLI rendering public surface. Per the v0.42 split, the
// `Serialize`-deriving JSON structs and their builders live in sibling modules
// under `crates/pdl-cli/src/render/`:
//
// * [`render::schema_render`] — `SchemaJson`, `NamedSchemaJson`, `ColumnJson`,
//   `output_schema_json`.
// * [`render::plan_render`] — driver/execution plan and manifest types
//   (`DriverPlanJson`, `ExecutionPlanJson`, `PlanObservabilityJson`,
//   `ExecutionStepJson`, etc.), their builders, the text helpers
//   (`pipeline_label_text`, `source_text`, etc.), and `manifest_json`.
// * [`render::ast_serialize`] — `ProgramJson`/`StageJson`/`ExprJson`/...
//   AST-to-JSON conversion and op/sort-direction text helpers.
// * [`render::ir_serialize`] — `ProgramIrJson`/`StageIrJson`/`ExprIrJson`/...
//   IR-to-JSON conversion and IR op text helpers.
// * [`render::span_json`] — the shared `SpannedJson<T>` generic wrapper.

use pdl_core::{Diagnostic, Span};
use pdl_driver::PreparedProgram;
use pdl_exec::ExecutionPlan;
use pdl_semantics::{PipelineSchemaLabel, ProgramIr};
use pdl_syntax::Program;
use serde::Serialize;

mod ast_serialize;
mod ir_serialize;
mod plan_render;
mod schema_render;
mod span_json;

use ast_serialize::{program_json, ProgramJson};
use ir_serialize::{program_ir_json, ProgramIrJson};
use plan_render::{
    driver_plan_json, execution_plan_json, execution_step_text, pipeline_label_text, sink_text,
    source_text, DriverPlanJson, ExecutionPlanJson, StreamInteropJson,
};
use schema_render::{output_schema_json, NamedSchemaJson, SchemaJson};

pub fn final_schema_columns(prepared: &PreparedProgram) -> Option<Vec<String>> {
    prepared
        .analysis
        .outputs
        .iter()
        .rev()
        .find(|output| {
            matches!(
                output.label,
                PipelineSchemaLabel::Main | PipelineSchemaLabel::Output(_)
            )
        })
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
    text.push_str(&format!(
        "  requested engine: {}\n",
        plan.observability.requested_engine.as_str()
    ));
    text.push_str(&format!(
        "  selected engine: {}\n",
        plan.observability.selected_engine.as_str()
    ));
    text.push_str(&format!(
        "  eligible engine: {}\n",
        plan.observability.eligible_engine.as_str()
    ));
    if let Some(reason) = plan.observability.fallback_reason {
        text.push_str(&format!("  fallback reason: {}\n", reason.code()));
    }
    text.push_str(&format!(
        "  sink strategy: {}\n",
        plan.observability.sink_strategy.as_str()
    ));
    text.push_str(&format!(
        "  row materialization: {}\n",
        plan.observability.row_materialization
    ));
    if let Some(columns) = &plan.observability.required_source_columns {
        text.push_str(&format!(
            "  required source columns: {}\n",
            columns.join(", ")
        ));
    }
    if !plan.observability.blocking_stages.is_empty() {
        text.push_str(&format!(
            "  blocking stages: {}\n",
            plan.observability.blocking_stages.join(", ")
        ));
    }
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
        output_schemas: output_schema_json(prepared),
        final_schema: final_schema_columns(prepared).map(SchemaJson::from_columns),
        diagnostics: prepared.diagnostics(),
    }
}

pub fn manifest_json(prepared: &PreparedProgram, plan: &ExecutionPlan) -> ManifestJson {
    plan_render::manifest_json(prepared, plan)
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
    output_schemas: Vec<NamedSchemaJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    final_schema: Option<SchemaJson>,
    diagnostics: Vec<Diagnostic>,
}

#[derive(Serialize)]
pub struct ManifestJson {
    pub(crate) manifest_version: &'static str,
    pub(crate) implementation_version: &'static str,
    pub(crate) language_version: &'static str,
    pub(crate) source_path: String,
    pub(crate) driver: DriverPlanJson,
    pub(crate) execution: ExecutionPlanJson,
    pub(crate) output_schemas: Vec<NamedSchemaJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) final_schema: Option<SchemaJson>,
    pub(crate) diagnostics: Vec<Diagnostic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) stream_interop: Option<StreamInteropJson>,
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
struct StageTraceJson {
    stage_id: usize,
    stage_name: String,
    span: Span,
    input_columns: Vec<String>,
    output_columns: Vec<String>,
    grouping_columns: Vec<String>,
}
