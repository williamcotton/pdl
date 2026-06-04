use pdl_core::Span;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GroupingState {
    pub columns: Vec<String>,
}

impl GroupingState {
    pub fn none() -> Self {
        Self {
            columns: Vec::new(),
        }
    }

    pub fn from_columns(columns: Vec<String>) -> Self {
        Self { columns }
    }

    pub fn is_active(&self) -> bool {
        !self.columns.is_empty()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StageTrace {
    pub stage_id: usize,
    pub stage_name: String,
    pub span: Span,
    pub input_schema: Option<Vec<String>>,
    pub output_schema: Option<Vec<String>>,
    pub grouping: GroupingState,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PipelineSchema {
    pub label: PipelineSchemaLabel,
    pub span: Span,
    pub columns: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PipelineSchemaLabel {
    Main,
    Binding(String),
    Output(String),
}
