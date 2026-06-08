// Document-wide schema inference, scope facts, and column-context helpers
// extracted from `services.rs` as part of the v0.42 split. See `services.rs`
// for the cross-module layout overview.

use pdl_syntax::{ContextKind, Expr, JoinKind, Pipeline, PipelineStart, Program, Stage};
use std::collections::{BTreeMap, BTreeSet};

use crate::services::{context_kind_detail, context_symbol_name};

#[derive(Clone, Debug)]
pub(crate) struct DocumentFacts {
    pub(crate) bindings: BTreeMap<String, BindingFact>,
    pub(crate) contexts: BTreeMap<String, ContextFact>,
}

#[derive(Clone, Debug)]
pub(crate) struct BindingFact {
    pub(crate) schema: Option<SchemaState>,
}

#[derive(Clone, Debug)]
pub(crate) struct ContextFact {
    pub(crate) kind: ContextKind,
    pub(crate) detail: String,
}

#[derive(Clone, Debug)]
pub(crate) struct SchemaState {
    pub(crate) columns: Vec<String>,
    pub(crate) grouping: Option<Vec<String>>,
}

impl DocumentFacts {
    pub(crate) fn new(program: &Program) -> Self {
        let mut facts = Self {
            bindings: BTreeMap::new(),
            contexts: BTreeMap::new(),
        };
        for context in &program.contexts {
            facts.contexts.insert(
                context.name.value.clone(),
                ContextFact {
                    kind: context.kind,
                    detail: format!(
                        "{} default `{}`",
                        context_kind_detail(context.kind),
                        format_context_default(&context.default)
                    ),
                },
            );
        }
        for binding in &program.bindings {
            let schema = facts.pipeline_schema(&binding.pipeline);
            facts
                .bindings
                .insert(binding.name.value.clone(), BindingFact { schema });
        }
        facts
    }

    pub(crate) fn schema_before_offset(
        &self,
        program: &Program,
        offset: usize,
    ) -> Option<Vec<String>> {
        for binding in &program.bindings {
            if crate::services::contains(binding.pipeline.span, offset) {
                return self.pipeline_schema_before_offset(&binding.pipeline, offset);
            }
        }
        for output in &program.outputs {
            if crate::services::contains(output.pipeline.span, offset) {
                return self.pipeline_schema_before_offset(&output.pipeline, offset);
            }
        }
        if let Some(main) = &program.main {
            return self.pipeline_schema_before_offset(main, offset);
        }
        None
    }

    pub(crate) fn pipeline_schema_before_offset(
        &self,
        pipeline: &Pipeline,
        offset: usize,
    ) -> Option<Vec<String>> {
        let mut schema = self.pipeline_start_schema(pipeline)?;
        for stage in &pipeline.stages {
            if offset <= stage.span().end {
                return Some(schema.columns);
            }
            apply_stage_to_schema(self, &mut schema, stage);
        }
        Some(schema.columns)
    }

    pub(crate) fn pipeline_schema(&self, pipeline: &Pipeline) -> Option<SchemaState> {
        let mut schema = self.pipeline_start_schema(pipeline)?;
        for stage in &pipeline.stages {
            apply_stage_to_schema(self, &mut schema, stage);
        }
        Some(schema)
    }

    pub(crate) fn pipeline_start_schema(&self, pipeline: &Pipeline) -> Option<SchemaState> {
        match &pipeline.start {
            PipelineStart::Load(_) => None,
            PipelineStart::Binding(name) => self
                .bindings
                .get(&name.value)
                .and_then(|binding| binding.schema.clone()),
        }
    }
}

pub(crate) fn apply_stage_to_schema(
    facts: &DocumentFacts,
    schema: &mut SchemaState,
    stage: &Stage,
) {
    match stage {
        Stage::Filter { .. }
        | Stage::Sort { .. }
        | Stage::Limit { .. }
        | Stage::Distinct { .. }
        | Stage::Save(_) => {}
        Stage::Select { items, .. } => {
            schema.columns = items
                .iter()
                .map(|item| item.alias.as_ref().unwrap_or(&item.column).value.clone())
                .collect();
            schema.grouping = None;
        }
        Stage::Drop { columns, .. } => {
            schema
                .columns
                .retain(|column| !columns.iter().any(|drop| drop.value == *column));
            schema.grouping = None;
        }
        Stage::Rename { items, .. } => {
            for column in &mut schema.columns {
                if let Some(rename) = items.iter().find(|rename| rename.old.value == *column) {
                    *column = rename.new.value.clone();
                }
            }
            schema.grouping = None;
        }
        Stage::Mutate { items, .. } => {
            for item in items {
                if !schema
                    .columns
                    .iter()
                    .any(|column| column == &item.column.value)
                {
                    schema.columns.push(item.column.value.clone());
                }
            }
            schema.grouping = None;
        }
        Stage::GroupBy { columns, .. } => {
            schema.grouping = Some(columns.iter().map(|column| column.value.clone()).collect());
        }
        Stage::Agg { items, .. } => {
            let mut output = schema.grouping.take().unwrap_or_default();
            output.extend(items.iter().map(|item| item.alias.value.clone()));
            schema.columns = output;
        }
        Stage::Join {
            source, on, kind, ..
        } => {
            if let Some(right_schema) = facts
                .bindings
                .get(&source.value)
                .and_then(|binding| binding.schema.as_ref())
            {
                let keys = on
                    .keys()
                    .iter()
                    .map(|key| (key.left.value.clone(), key.right.value.clone()))
                    .collect::<Vec<_>>();
                schema.columns =
                    join_schema_for_editor(&schema.columns, &right_schema.columns, &keys, *kind);
            }
            schema.grouping = None;
        }
        Stage::Union { .. } => {
            schema.grouping = None;
        }
        Stage::PivotLonger {
            columns,
            names_to,
            values_to,
            ..
        } => {
            let selected = columns
                .iter()
                .map(|column| column.value.clone())
                .collect::<BTreeSet<_>>();
            schema
                .columns
                .retain(|column| !selected.iter().any(|selected| selected == column));
            schema.columns.push(names_to.value.clone());
            schema.columns.push(values_to.value.clone());
            schema.grouping = None;
        }
        Stage::Complete { .. } => {
            schema.grouping = None;
        }
        Stage::Unsupported { .. } => {}
    }
}

fn join_schema_for_editor(
    left_schema: &[String],
    right_schema: &[String],
    keys: &[(String, String)],
    kind: JoinKind,
) -> Vec<String> {
    if matches!(kind, JoinKind::Semi | JoinKind::Anti) {
        return left_schema.to_vec();
    }
    let right_keys = keys
        .iter()
        .map(|(_, right_key)| right_key)
        .collect::<BTreeSet<_>>();
    let mut output = left_schema.to_vec();
    for column in right_schema {
        if right_keys.contains(column) {
            continue;
        }
        let mut output_name = column.clone();
        if output.iter().any(|existing| existing == &output_name) {
            output_name.push_str("_right");
        }
        if !output.iter().any(|existing| existing == &output_name) {
            output.push(output_name);
        }
    }
    output
}

pub(crate) fn optimistic_columns(program: &Program) -> Vec<String> {
    let mut columns = BTreeSet::new();
    for binding in &program.bindings {
        collect_pipeline_columns(&binding.pipeline, &mut columns);
    }
    for output in &program.outputs {
        collect_pipeline_columns(&output.pipeline, &mut columns);
    }
    if let Some(main) = &program.main {
        collect_pipeline_columns(main, &mut columns);
    }
    columns.into_iter().collect()
}

fn collect_pipeline_columns(pipeline: &Pipeline, columns: &mut BTreeSet<String>) {
    for stage in &pipeline.stages {
        match stage {
            Stage::Filter { expr, .. } => collect_expr_columns(expr, columns),
            Stage::Select { items, .. } => {
                for item in items {
                    columns.insert(item.column.value.clone());
                    if let Some(alias) = &item.alias {
                        columns.insert(alias.value.clone());
                    }
                }
            }
            Stage::Drop {
                columns: dropped, ..
            }
            | Stage::GroupBy {
                columns: dropped, ..
            } => {
                for column in dropped {
                    columns.insert(column.value.clone());
                }
            }
            Stage::Rename { items, .. } => {
                for item in items {
                    columns.insert(item.old.value.clone());
                    columns.insert(item.new.value.clone());
                }
            }
            Stage::Mutate { items, .. } => {
                for item in items {
                    columns.insert(item.column.value.clone());
                    collect_expr_columns(&item.expr, columns);
                }
            }
            Stage::Agg { items, .. } => {
                for item in items {
                    for arg in &item.args {
                        collect_expr_columns(arg, columns);
                    }
                    columns.insert(item.alias.value.clone());
                }
            }
            Stage::Sort { items, .. } => {
                for item in items {
                    columns.insert(item.column.value.clone());
                }
            }
            Stage::Join { on, .. } => {
                for key in on.keys() {
                    columns.insert(key.left.value);
                    columns.insert(key.right.value);
                }
            }
            Stage::Union { .. } => {}
            Stage::Distinct { columns: keys, .. } => {
                for column in keys {
                    columns.insert(column.value.clone());
                }
            }
            Stage::PivotLonger {
                columns: keys,
                names_to,
                values_to,
                ..
            } => {
                for column in keys {
                    columns.insert(column.value.clone());
                }
                columns.insert(names_to.value.clone());
                columns.insert(values_to.value.clone());
            }
            Stage::Complete { keys, fills, .. } => {
                for key in keys {
                    columns.insert(key.value.clone());
                }
                for fill in fills {
                    columns.insert(fill.column.value.clone());
                    collect_expr_columns(&fill.expr, columns);
                }
            }
            Stage::Limit { .. } | Stage::Save(_) | Stage::Unsupported { .. } => {}
        }
    }
}

fn collect_expr_columns(expr: &Expr, columns: &mut BTreeSet<String>) {
    match expr {
        Expr::Quoted(_) => {}
        Expr::Ident(value) => {
            columns.insert(value.value.clone());
        }
        Expr::Call { args, .. } => {
            for arg in args {
                collect_expr_columns(arg, columns);
            }
        }
        Expr::Window { args, spec, .. } => {
            for arg in args {
                collect_expr_columns(arg, columns);
            }
            for column in &spec.partition_by {
                columns.insert(column.value.clone());
            }
            for item in &spec.order_by {
                columns.insert(item.column.value.clone());
            }
        }
        Expr::Unary { expr, .. } => collect_expr_columns(expr, columns),
        Expr::Binary { left, right, .. } => {
            collect_expr_columns(left, columns);
            collect_expr_columns(right, columns);
        }
        Expr::Number(_) | Expr::Bool(_) | Expr::Null(_) | Expr::Context { .. } => {}
    }
}

fn format_context_default(expr: &Expr) -> String {
    match expr {
        Expr::Quoted(value) => format!("\"{}\"", value.value.replace('"', "\\\"")),
        Expr::Number(value) => value.value.to_string(),
        Expr::Bool(value) => value.value.to_string(),
        Expr::Null(_) => "null".to_string(),
        Expr::Ident(value) => value.value.clone(),
        Expr::Context { kind, name, .. } => context_symbol_name(*kind, &name.value),
        Expr::Call { name, .. } => format!("{}(...)", name.value),
        Expr::Window { function, .. } => format!("{}(...) over (...)", function.value),
        Expr::Unary { .. } | Expr::Binary { .. } => "expression".to_string(),
    }
}
