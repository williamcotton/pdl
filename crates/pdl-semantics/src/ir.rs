use pdl_core::Span;
use pdl_syntax::{Pipeline, PipelineStart, Program, SinkRef, SourceRef, Stage};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProgramIr {
    pub bindings: Vec<BindingIr>,
    pub main: Option<PipelineIr>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BindingIr {
    pub name: String,
    pub span: Span,
    pub pipeline: PipelineIr,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PipelineIr {
    pub start: PipelineStartIr,
    pub stages: Vec<StageIr>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PipelineStartIr {
    Load {
        source: SourceIr,
        format: Option<String>,
        span: Span,
    },
    Binding {
        name: String,
        span: Span,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum SourceIr {
    Path(String),
    Stdin,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SinkIr {
    Path(String),
    Stdout,
}

#[derive(Clone, Debug, PartialEq)]
pub enum StageIr {
    Filter {
        span: Span,
    },
    Select {
        columns: Vec<String>,
        span: Span,
    },
    Drop {
        columns: Vec<String>,
        span: Span,
    },
    Rename {
        renames: Vec<(String, String)>,
        span: Span,
    },
    GroupBy {
        columns: Vec<String>,
        span: Span,
    },
    Agg {
        outputs: Vec<String>,
        span: Span,
    },
    Sort {
        columns: Vec<String>,
        span: Span,
    },
    Limit {
        n: usize,
        span: Span,
    },
    Save {
        sink: SinkIr,
        format: Option<String>,
        span: Span,
    },
    Unsupported {
        name: String,
        span: Span,
    },
}

pub fn lower_program(program: &Program) -> ProgramIr {
    ProgramIr {
        bindings: program
            .bindings
            .iter()
            .map(|binding| BindingIr {
                name: binding.name.value.clone(),
                span: binding.name.span,
                pipeline: lower_pipeline(&binding.pipeline),
            })
            .collect(),
        main: program.main.as_ref().map(lower_pipeline),
    }
}

fn lower_pipeline(pipeline: &Pipeline) -> PipelineIr {
    PipelineIr {
        start: lower_pipeline_start(&pipeline.start),
        stages: pipeline.stages.iter().map(lower_stage).collect(),
        span: pipeline.span,
    }
}

fn lower_pipeline_start(start: &PipelineStart) -> PipelineStartIr {
    match start {
        PipelineStart::Load(load) => PipelineStartIr::Load {
            source: match &load.source {
                SourceRef::Path(path) => SourceIr::Path(path.value.clone()),
                SourceRef::Stdin(_) => SourceIr::Stdin,
            },
            format: load.format.as_ref().map(|format| format.value.clone()),
            span: load.span,
        },
        PipelineStart::Binding(name) => PipelineStartIr::Binding {
            name: name.value.clone(),
            span: name.span,
        },
    }
}

fn lower_stage(stage: &Stage) -> StageIr {
    match stage {
        Stage::Filter { span, .. } => StageIr::Filter { span: *span },
        Stage::Select { items, span } => StageIr::Select {
            columns: items
                .iter()
                .map(|item| item.alias.as_ref().unwrap_or(&item.column).value.clone())
                .collect(),
            span: *span,
        },
        Stage::Drop { columns, span } => StageIr::Drop {
            columns: columns.iter().map(|column| column.value.clone()).collect(),
            span: *span,
        },
        Stage::Rename { items, span } => StageIr::Rename {
            renames: items
                .iter()
                .map(|item| (item.old.value.clone(), item.new.value.clone()))
                .collect(),
            span: *span,
        },
        Stage::GroupBy { columns, span } => StageIr::GroupBy {
            columns: columns.iter().map(|column| column.value.clone()).collect(),
            span: *span,
        },
        Stage::Agg { items, span } => StageIr::Agg {
            outputs: items.iter().map(|item| item.alias.value.clone()).collect(),
            span: *span,
        },
        Stage::Sort { items, span } => StageIr::Sort {
            columns: items.iter().map(|item| item.column.value.clone()).collect(),
            span: *span,
        },
        Stage::Limit { n, span } => StageIr::Limit { n: *n, span: *span },
        Stage::Save(save) => StageIr::Save {
            sink: match &save.sink {
                SinkRef::Path(path) => SinkIr::Path(path.value.clone()),
                SinkRef::Stdout(_) => SinkIr::Stdout,
            },
            format: save.format.as_ref().map(|format| format.value.clone()),
            span: save.span,
        },
        Stage::Unsupported { name, span } => StageIr::Unsupported {
            name: name.value.clone(),
            span: *span,
        },
    }
}
