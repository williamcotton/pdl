use pdl_core::{codes, has_errors, Diagnostic, Span};
use pdl_driver::{program, PreparedProgram};
use pdl_syntax::{PipelineStart, SinkRef, SourceRef, Stage};

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
                format!("stdout format `{format}` is not supported in 0.4.0"),
                Span::zero(),
            ));
            return Err(diagnostics);
        }
    }

    let Some(main) = &program(prepared).main else {
        diagnostics.push(Diagnostic::error(
            codes::E1502,
            "no runnable main pipeline",
            Span::zero(),
        ));
        return Err(diagnostics);
    };

    let mut steps = Vec::new();
    match &main.start {
        PipelineStart::Load(load) => steps.push(ExecutionPlanStep::Load {
            source: match &load.source {
                SourceRef::Path(path) => path.value.clone(),
                SourceRef::Stdin(_) => "stdin".to_string(),
            },
            format: load
                .format
                .as_ref()
                .map_or_else(|| "csv".to_string(), |format| format.value.clone()),
        }),
        PipelineStart::Binding(name) => steps.push(ExecutionPlanStep::Binding {
            name: name.value.clone(),
        }),
    }

    for stage in &main.stages {
        match stage {
            Stage::Save(save) => steps.push(ExecutionPlanStep::Save {
                sink: match &save.sink {
                    SinkRef::Path(path) => path.value.clone(),
                    SinkRef::Stdout(_) => "stdout".to_string(),
                },
                format: save
                    .format
                    .as_ref()
                    .map_or_else(|| "csv".to_string(), |format| format.value.clone()),
            }),
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

fn stage_name(stage: &Stage) -> &'static str {
    match stage {
        Stage::Filter { .. } => "filter",
        Stage::Select { .. } => "select",
        Stage::Drop { .. } => "drop",
        Stage::Rename { .. } => "rename",
        Stage::GroupBy { .. } => "group_by",
        Stage::Agg { .. } => "agg",
        Stage::Sort { .. } => "sort",
        Stage::Limit { .. } => "limit",
        Stage::Save(_) => "save",
        Stage::Unsupported { name, .. } => match name.value.as_str() {
            "mutate" => "mutate",
            "join" => "join",
            "union" => "union",
            "distinct" => "distinct",
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

        assert_eq!(plan.steps.len(), 4);
        assert_eq!(
            plan.steps.last(),
            Some(&ExecutionPlanStep::Stdout {
                format: "csv".to_string()
            })
        );
    }
}
