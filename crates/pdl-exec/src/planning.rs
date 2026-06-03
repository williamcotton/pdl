use pdl_core::{codes, has_errors, Diagnostic, Span};
use pdl_data::DataFormat;
use pdl_driver::{PreparedProgram, SinkDescriptor, SourceDescriptor};
use pdl_semantics::{PipelineIr, PipelineStartIr, ProgramIr, StageIr};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlanningOptions {
    pub stdout_format: Option<String>,
    pub dry_run: bool,
    pub allow_binary_stdout: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExecutionPlan {
    pub steps: Vec<ExecutionPlanStep>,
    pub stdout_format: Option<DataFormat>,
    pub dry_run: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ExecutionPlanStep {
    Load { source: String, format: String },
    Binding { name: String },
    Transform { stage: String },
    Save { sink: String, format: String },
    Stdout { format: String },
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
                format!("stdout format `{format}` is not supported in 0.16.0"),
                Span::zero(),
            ));
            return Err(diagnostics);
        };
        if !data_format.is_supported_output() {
            diagnostics.push(Diagnostic::error(
                codes::E1705,
                format!(
                    "stdout format `{}` is not supported in 0.16.0",
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

    let Some(main) = &ir.main else {
        diagnostics.push(Diagnostic::error(
            codes::E1502,
            "no runnable main pipeline",
            Span::zero(),
        ));
        return Err(diagnostics);
    };

    let mut steps = Vec::new();
    let mut planned_bindings = BTreeSet::new();
    if let Err(diagnostic) =
        append_pipeline_steps(prepared, ir, main, &mut planned_bindings, &mut steps)
    {
        diagnostics.push(diagnostic);
        return Err(diagnostics);
    }

    if let Some(format) = stdout_format {
        steps.push(ExecutionPlanStep::Stdout {
            format: format.canonical_name().to_string(),
        });
    }

    Ok(ExecutionPlan {
        steps,
        stdout_format,
        dry_run: options.dry_run,
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
        StageIr::Save { .. } => "save",
        StageIr::Unsupported { name, .. } => match name.as_str() {
            "join" => "join",
            "union" => "union",
            _ => "unknown",
        },
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
            r#"load "sales.csv" | filter "amount" > 0 | select "region""#,
            &io,
        );

        let plan = plan_prepared(
            &prepared,
            PlanningOptions {
                stdout_format: Some("csv".to_string()),
                dry_run: true,
                allow_binary_stdout: true,
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
  | join customers on "customer_id"
  | join customers on "customer_id""#,
            &io,
        );

        let plan = plan_prepared(
            &prepared,
            PlanningOptions {
                stdout_format: Some("csv".to_string()),
                dry_run: true,
                allow_binary_stdout: true,
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
    }
}
