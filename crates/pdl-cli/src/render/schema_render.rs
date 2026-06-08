// Schema-rendering JSON types extracted from `render.rs` as part of the v0.42
// split. See `render.rs` for the cross-module layout overview.

use pdl_core::Span;
use pdl_driver::PreparedProgram;
use pdl_semantics::PipelineSchemaLabel;
use serde::Serialize;

#[derive(Serialize)]
pub(crate) struct SchemaJson {
    pub(crate) columns: Vec<ColumnJson>,
}

#[derive(Serialize)]
pub(crate) struct NamedSchemaJson {
    pub(crate) name: String,
    pub(crate) schema: SchemaJson,
    pub(crate) span: Span,
}

impl SchemaJson {
    pub(crate) fn from_columns(columns: Vec<String>) -> Self {
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

pub(crate) fn output_schema_json(prepared: &PreparedProgram) -> Vec<NamedSchemaJson> {
    prepared
        .analysis
        .outputs
        .iter()
        .filter_map(|output| match &output.label {
            PipelineSchemaLabel::Output(name) => Some(NamedSchemaJson {
                name: name.clone(),
                schema: SchemaJson::from_columns(output.columns.clone()),
                span: output.span,
            }),
            PipelineSchemaLabel::Main | PipelineSchemaLabel::Binding(_) => None,
        })
        .collect()
}

#[derive(Serialize)]
pub(crate) struct ColumnJson {
    pub(crate) name: String,
    pub(crate) logical_type: &'static str,
    pub(crate) nullable: bool,
}
