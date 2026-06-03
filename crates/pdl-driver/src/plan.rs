use pdl_core::Span;
use pdl_data::DataFormat;
use pdl_syntax::{Pipeline, PipelineStart, Program, SinkRef, SourceRef, Stage};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::path::{resolve_input_path, resolve_output_path};
use crate::source::SourceOrigin;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DriverPlan {
    pub origin: SourceOrigin,
    pub source_path: PathBuf,
    pub base_dir: PathBuf,
    pub inputs: Vec<PlanInputSource>,
    pub sinks: Vec<PlanOutputSink>,
    pub dependencies: Vec<SourceDependency>,
    pub streams: Vec<StreamUse>,
}

impl DriverPlan {
    pub fn build(path: impl AsRef<Path>, origin: SourceOrigin, program: &Program) -> Self {
        let source_path = path.as_ref().to_path_buf();
        let base_dir = source_path
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
        let mut builder = DriverPlanBuilder {
            plan: DriverPlan {
                origin,
                source_path,
                base_dir,
                inputs: Vec::new(),
                sinks: Vec::new(),
                dependencies: Vec::new(),
                streams: Vec::new(),
            },
            seen_dependencies: BTreeSet::new(),
        };
        for binding in &program.bindings {
            builder.record_pipeline(
                PipelineLabel::Binding(binding.name.value.clone()),
                &binding.pipeline,
            );
        }
        if let Some(main) = &program.main {
            builder.record_pipeline(PipelineLabel::Main, main);
        }
        builder.plan
    }

    pub fn input_for_stage_span(&self, span: Span) -> Option<&PlanInputSource> {
        self.inputs.iter().find(|input| input.stage_span == span)
    }

    pub fn sink_for_stage_span(&self, span: Span) -> Option<&PlanOutputSink> {
        self.sinks.iter().find(|sink| sink.stage_span == span)
    }

    pub fn stdin_reads(&self) -> Vec<&StreamUse> {
        self.streams
            .iter()
            .filter(|usage| {
                usage.stream == StreamKind::Stdin && usage.direction == StreamDirection::Read
            })
            .collect()
    }

    pub fn stdout_writes(&self) -> Vec<&StreamUse> {
        self.streams
            .iter()
            .filter(|usage| {
                usage.stream == StreamKind::Stdout && usage.direction == StreamDirection::Write
            })
            .collect()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlanInputSource {
    pub pipeline: PipelineLabel,
    pub source: SourceDescriptor,
    pub format: FormatDecision,
    pub span: Span,
    pub stage_span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlanOutputSink {
    pub pipeline: PipelineLabel,
    pub sink: SinkDescriptor,
    pub format: FormatDecision,
    pub span: Span,
    pub stage_span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceDependency {
    pub logical_path: String,
    pub resolved_path: PathBuf,
    pub inferred_format: Option<DataFormat>,
    pub span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PipelineLabel {
    Main,
    Binding(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SourceDescriptor {
    Path {
        logical_path: String,
        resolved_path: PathBuf,
    },
    Stdin,
}

impl SourceDescriptor {
    pub fn display_name(&self) -> String {
        match self {
            SourceDescriptor::Path { logical_path, .. } => logical_path.clone(),
            SourceDescriptor::Stdin => "stdin".to_string(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SinkDescriptor {
    Path {
        logical_path: String,
        resolved_path: PathBuf,
    },
    Stdout,
}

impl SinkDescriptor {
    pub fn display_name(&self) -> String {
        match self {
            SinkDescriptor::Path { logical_path, .. } => logical_path.clone(),
            SinkDescriptor::Stdout => "stdout".to_string(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FormatDecision {
    pub explicit: Option<String>,
    pub inferred_from_path: Option<DataFormat>,
    pub sniffing: SniffingDecision,
}

impl FormatDecision {
    pub fn effective_name(&self) -> String {
        self.explicit
            .clone()
            .or_else(|| {
                self.inferred_from_path
                    .map(|format| format.canonical_name().to_string())
            })
            .unwrap_or_else(|| "unresolved".to_string())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SniffingDecision {
    NotNeeded,
    Deferred { reason: SniffingReason },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SniffingReason {
    StdinWithoutExplicitFormat,
    StdoutWithoutExplicitFormat,
    PathWithoutKnownExtension,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StreamUse {
    pub pipeline: PipelineLabel,
    pub stream: StreamKind,
    pub direction: StreamDirection,
    pub explicit_format: Option<String>,
    pub span: Span,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StreamKind {
    Stdin,
    Stdout,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StreamDirection {
    Read,
    Write,
}

struct DriverPlanBuilder {
    plan: DriverPlan,
    seen_dependencies: BTreeSet<PathBuf>,
}

impl DriverPlanBuilder {
    fn record_pipeline(&mut self, label: PipelineLabel, pipeline: &Pipeline) {
        if let PipelineStart::Load(load) = &pipeline.start {
            self.record_load(label.clone(), load);
        }
        for stage in &pipeline.stages {
            if let Stage::Save(save) = stage {
                self.record_save(label.clone(), save);
            }
        }
    }

    fn record_load(&mut self, label: PipelineLabel, load: &pdl_syntax::LoadStage) {
        match &load.source {
            SourceRef::Path(path) => {
                let resolved = resolve_input_path(&self.plan.source_path, &path.value);
                let inferred = DataFormat::infer_from_path(&path.value);
                let source = SourceDescriptor::Path {
                    logical_path: path.value.clone(),
                    resolved_path: resolved.clone(),
                };
                self.plan.inputs.push(PlanInputSource {
                    pipeline: label,
                    source,
                    format: format_decision(
                        load.format.as_ref().map(|format| format.value.clone()),
                        inferred,
                        SniffingReason::PathWithoutKnownExtension,
                    ),
                    span: path.span,
                    stage_span: load.span,
                });
                if self.seen_dependencies.insert(resolved.clone()) {
                    self.plan.dependencies.push(SourceDependency {
                        logical_path: path.value.clone(),
                        resolved_path: resolved,
                        inferred_format: inferred,
                        span: path.span,
                    });
                }
            }
            SourceRef::Stdin(span) => {
                let explicit_format = load.format.as_ref().map(|format| format.value.clone());
                self.plan.inputs.push(PlanInputSource {
                    pipeline: label.clone(),
                    source: SourceDescriptor::Stdin,
                    format: format_decision(
                        explicit_format.clone(),
                        None,
                        SniffingReason::StdinWithoutExplicitFormat,
                    ),
                    span: *span,
                    stage_span: load.span,
                });
                self.plan.streams.push(StreamUse {
                    pipeline: label,
                    stream: StreamKind::Stdin,
                    direction: StreamDirection::Read,
                    explicit_format,
                    span: *span,
                });
            }
        }
    }

    fn record_save(&mut self, label: PipelineLabel, save: &pdl_syntax::SaveStage) {
        match &save.sink {
            SinkRef::Path(path) => {
                let inferred = DataFormat::infer_from_path(&path.value);
                self.plan.sinks.push(PlanOutputSink {
                    pipeline: label,
                    sink: SinkDescriptor::Path {
                        logical_path: path.value.clone(),
                        resolved_path: resolve_output_path(&path.value),
                    },
                    format: format_decision(
                        save.format.as_ref().map(|format| format.value.clone()),
                        inferred,
                        SniffingReason::PathWithoutKnownExtension,
                    ),
                    span: path.span,
                    stage_span: save.span,
                });
            }
            SinkRef::Stdout(span) => {
                let explicit_format = save.format.as_ref().map(|format| format.value.clone());
                self.plan.sinks.push(PlanOutputSink {
                    pipeline: label.clone(),
                    sink: SinkDescriptor::Stdout,
                    format: format_decision(
                        explicit_format.clone(),
                        None,
                        SniffingReason::StdoutWithoutExplicitFormat,
                    ),
                    span: *span,
                    stage_span: save.span,
                });
                self.plan.streams.push(StreamUse {
                    pipeline: label,
                    stream: StreamKind::Stdout,
                    direction: StreamDirection::Write,
                    explicit_format,
                    span: *span,
                });
            }
        }
    }
}

fn format_decision(
    explicit: Option<String>,
    inferred_from_path: Option<DataFormat>,
    unresolved_reason: SniffingReason,
) -> FormatDecision {
    let sniffing = if explicit.is_some() || inferred_from_path.is_some() {
        SniffingDecision::NotNeeded
    } else {
        SniffingDecision::Deferred {
            reason: unresolved_reason,
        }
    };
    FormatDecision {
        explicit,
        inferred_from_path,
        sniffing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdl_syntax::parse;

    #[test]
    fn plan_records_dependencies_streams_and_deferred_sniffing_without_io() {
        let parse = parse(
            r#"let via_stdin =
  load stdin
  | save stdout format "arrow-stream"

load "sales.csv"
  | save "out.data""#,
        );

        let plan = DriverPlan::build(
            "memory/main.pdl",
            SourceOrigin::path("memory/main.pdl"),
            &parse.program,
        );

        assert_eq!(plan.dependencies.len(), 1);
        assert_eq!(plan.dependencies[0].logical_path, "sales.csv");
        assert_eq!(plan.dependencies[0].inferred_format, Some(DataFormat::Csv));
        assert_eq!(plan.stdin_reads().len(), 1);
        assert_eq!(plan.stdout_writes().len(), 1);
        assert_eq!(
            plan.inputs[0].format.sniffing,
            SniffingDecision::Deferred {
                reason: SniffingReason::StdinWithoutExplicitFormat
            }
        );
        assert_eq!(
            plan.sinks
                .iter()
                .find(|sink| matches!(sink.sink, SinkDescriptor::Path { .. }))
                .expect("path sink")
                .format
                .sniffing,
            SniffingDecision::Deferred {
                reason: SniffingReason::PathWithoutKnownExtension
            }
        );
    }
}
