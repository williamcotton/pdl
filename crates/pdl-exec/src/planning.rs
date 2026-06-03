use pdl_core::{codes, has_errors, Diagnostic, Span};
use pdl_driver::{PreparedProgram, SinkDescriptor, SourceDescriptor};
use pdl_semantics::{PipelineStartIr, StageIr};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlanningOptions {
    pub stdout_format: Option<String>,
    pub dry_run: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExecutionPlan {
    pub steps: Vec<ExecutionPlanStep>,
    pub stdout_format: Option<String>,
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

    if let Some(format) = &options.stdout_format {
        if format != "csv" {
            diagnostics.push(Diagnostic::error(
                codes::E1705,
                format!("stdout format `{format}` is not supported in 0.11.0"),
                Span::zero(),
            ));
            return Err(diagnostics);
        }
    }

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
    match &main.start {
        PipelineStartIr::Load { span, .. } => {
            let Some(input) = prepared.driver_plan.input_for_stage_span(*span) else {
                diagnostics.push(Diagnostic::error(
                    codes::E1505,
                    "driver source facts are unavailable for planning",
                    *span,
                ));
                return Err(diagnostics);
            };
            steps.push(ExecutionPlanStep::Load {
                source: match &input.source {
                    SourceDescriptor::Path { logical_path, .. } => logical_path.clone(),
                    SourceDescriptor::Stdin => "stdin".to_string(),
                },
                format: input.format.effective_name(),
            });
        }
        PipelineStartIr::Binding { name, .. } => {
            steps.push(ExecutionPlanStep::Binding { name: name.clone() })
        }
    }

    for stage in &main.stages {
        match stage {
            StageIr::Save { span, .. } => {
                let Some(sink) = prepared.driver_plan.sink_for_stage_span(*span) else {
                    diagnostics.push(Diagnostic::error(
                        codes::E1505,
                        "driver sink facts are unavailable for planning",
                        *span,
                    ));
                    return Err(diagnostics);
                };
                steps.push(ExecutionPlanStep::Save {
                    sink: match &sink.sink {
                        SinkDescriptor::Path { logical_path, .. } => logical_path.clone(),
                        SinkDescriptor::Stdout => "stdout".to_string(),
                    },
                    format: sink.format.effective_name(),
                });
            }
            _ => steps.push(ExecutionPlanStep::Transform {
                stage: stage_name(stage).to_string(),
            }),
        }
    }

    if let Some(format) = &options.stdout_format {
        steps.push(ExecutionPlanStep::Stdout {
            format: format.clone(),
        });
    }

    Ok(ExecutionPlan {
        steps,
        stdout_format: options.stdout_format,
        dry_run: options.dry_run,
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
}
