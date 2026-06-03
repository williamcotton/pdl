use pdl_core::{codes, Diagnostic, Span};
use pdl_syntax::{
    AggItem, Binding, Expr, LoadStage, Pipeline, PipelineStart, Program, SaveStage, SourceRef,
    Stage,
};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use crate::ir::{lower_program, ProgramIr};
use crate::registry::{accepts_arity, aggregate_function, format_info};
use crate::schema::{GroupingState, StageTrace};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Analysis {
    pub diagnostics: Vec<Diagnostic>,
    pub ir: Option<ProgramIr>,
    pub traces: Vec<StageTrace>,
}

#[derive(Clone, Debug)]
pub struct LoadRequest<'a> {
    pub load: &'a LoadStage,
    pub path: Option<PathBuf>,
}

pub fn analyze_program<F>(program: &Program, mut load_schema: F) -> Analysis
where
    F: FnMut(LoadRequest<'_>) -> Result<Vec<String>, Diagnostic>,
{
    let mut analyzer = Analyzer {
        diagnostics: Vec::new(),
        load_schema: &mut load_schema,
        binding_schemas: BTreeMap::new(),
        traces: Vec::new(),
        next_stage_id: 0,
    };
    analyzer.analyze(program);
    let has_error = analyzer
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == pdl_core::Severity::Error);
    Analysis {
        diagnostics: analyzer.diagnostics,
        ir: (!has_error).then(|| lower_program(program)),
        traces: analyzer.traces,
    }
}

struct Analyzer<'a, F>
where
    F: FnMut(LoadRequest<'_>) -> Result<Vec<String>, Diagnostic>,
{
    diagnostics: Vec<Diagnostic>,
    load_schema: &'a mut F,
    binding_schemas: BTreeMap<String, Vec<String>>,
    traces: Vec<StageTrace>,
    next_stage_id: usize,
}

impl<F> Analyzer<'_, F>
where
    F: FnMut(LoadRequest<'_>) -> Result<Vec<String>, Diagnostic>,
{
    fn analyze(&mut self, program: &Program) {
        self.check_duplicate_bindings(&program.bindings);
        for binding in &program.bindings {
            if let Some(schema) = self.analyze_pipeline(&binding.pipeline) {
                self.binding_schemas
                    .insert(binding.name.value.clone(), schema);
            }
        }
        if let Some(main) = &program.main {
            self.analyze_pipeline(main);
        }
    }

    fn check_duplicate_bindings(&mut self, bindings: &[Binding]) {
        let mut seen = BTreeSet::new();
        for binding in bindings {
            if !seen.insert(binding.name.value.clone()) {
                self.diagnostics.push(Diagnostic::error(
                    codes::E1001,
                    format!("duplicate binding `{}`", binding.name.value),
                    binding.name.span,
                ));
            }
        }
    }

    fn analyze_pipeline(&mut self, pipeline: &Pipeline) -> Option<Vec<String>> {
        let mut schema = match &pipeline.start {
            PipelineStart::Load(load) => match (self.load_schema)(LoadRequest { load, path: None })
            {
                Ok(schema) => schema,
                Err(diagnostic) => {
                    self.diagnostics.push(diagnostic);
                    return None;
                }
            },
            PipelineStart::Binding(name) => match self.binding_schemas.get(&name.value) {
                Some(schema) => schema.clone(),
                None => {
                    self.diagnostics.push(Diagnostic::error(
                        codes::E1007,
                        format!("unknown binding `{}`", name.value),
                        name.span,
                    ));
                    return None;
                }
            },
        };

        let mut grouping: Option<Vec<String>> = None;
        for stage in &pipeline.stages {
            let input_schema = schema.clone();
            match stage {
                Stage::Filter { expr, .. } => {
                    for column in row_expr_column_refs(expr, &schema, ExprRole::PredicateRoot) {
                        self.require_column(&schema, &column.value, column.span);
                    }
                }
                Stage::Select { items, .. } => {
                    let mut output = Vec::new();
                    let mut seen = BTreeSet::new();
                    for item in items {
                        self.require_column(&schema, &item.column.value, item.column.span);
                        let output_name = item.alias.as_ref().unwrap_or(&item.column);
                        if !seen.insert(output_name.value.clone()) {
                            self.diagnostics.push(Diagnostic::error(
                                codes::E1207,
                                format!("duplicate output column `{}`", output_name.value),
                                output_name.span,
                            ));
                        }
                        output.push(output_name.value.clone());
                    }
                    schema = output;
                    grouping = None;
                }
                Stage::Drop { columns, .. } => {
                    for column in columns {
                        self.require_column(&schema, &column.value, column.span);
                    }
                    schema.retain(|column| !columns.iter().any(|drop| drop.value == *column));
                    grouping = None;
                }
                Stage::Rename { items, .. } => {
                    for item in items {
                        self.require_column(&schema, &item.old.value, item.old.span);
                    }
                    let mut output = schema.clone();
                    for item in items {
                        if schema.iter().any(|column| column == &item.new.value)
                            && item.old.value != item.new.value
                        {
                            self.diagnostics.push(Diagnostic::error(
                                codes::E1207,
                                format!("rename target `{}` already exists", item.new.value),
                                item.new.span,
                            ));
                        }
                        for column in &mut output {
                            if *column == item.old.value {
                                *column = item.new.value.clone();
                            }
                        }
                    }
                    schema = output;
                    grouping = None;
                }
                Stage::GroupBy { columns, .. } => {
                    for column in columns {
                        self.require_column(&schema, &column.value, column.span);
                    }
                    grouping = Some(columns.iter().map(|column| column.value.clone()).collect());
                }
                Stage::Agg { items, .. } => {
                    let keys = grouping.take().unwrap_or_default();
                    let mut output = keys;
                    let mut seen: BTreeSet<String> = output.iter().cloned().collect();
                    for item in items {
                        self.analyze_aggregate_item(&schema, item);
                        if !seen.insert(item.alias.value.clone()) {
                            self.diagnostics.push(Diagnostic::error(
                                codes::E1207,
                                format!("duplicate output column `{}`", item.alias.value),
                                item.alias.span,
                            ));
                        }
                        output.push(item.alias.value.clone());
                    }
                    schema = output;
                }
                Stage::Sort { items, .. } => {
                    for item in items {
                        self.require_column(&schema, &item.column.value, item.column.span);
                    }
                }
                Stage::Limit { .. } => {}
                Stage::Save(save) => self.analyze_save(save),
                Stage::Unsupported { name, .. } => {
                    self.diagnostics.push(Diagnostic::error(
                        codes::E1211,
                        format!("stage `{}` is deferred in 0.5.0", name.value),
                        name.span,
                    ));
                }
            }
            self.push_trace(stage, input_schema, schema.clone(), grouping.clone());
        }

        if let Some(keys) = grouping {
            if !keys.is_empty() {
                self.diagnostics.push(Diagnostic::warning(
                    codes::W2001,
                    "pipeline ended with active grouping state and no `agg`",
                    pipeline.span,
                ));
            }
        }

        Some(schema)
    }

    fn analyze_save(&mut self, save: &SaveStage) {
        if let Some(format) = &save.format {
            if !format_info(&format.value).is_some_and(|info| info.save_supported) {
                self.diagnostics.push(Diagnostic::error(
                    codes::E1215,
                    format!("format `{}` is not supported in 0.5.0", format.value),
                    format.span,
                ));
            }
        }
    }

    fn analyze_aggregate_item(&mut self, schema: &[String], item: &AggItem) {
        let function = item.function.value.as_str();
        let Some(info) = aggregate_function(function) else {
            self.diagnostics.push(Diagnostic::error(
                codes::E1401,
                format!("unknown aggregate function `{function}`"),
                item.function.span,
            ));
            return;
        };
        if !accepts_arity(*info, item.args.len()) {
            self.diagnostics.push(Diagnostic::error(
                codes::E1402,
                format!(
                    "aggregate function `{function}` expects {}",
                    info.expected_arity
                ),
                item.span,
            ));
        }
        for arg in &item.args {
            for column in aggregate_expr_column_refs(arg) {
                self.require_column(schema, &column.value, column.span);
            }
        }
    }

    fn require_column(&mut self, schema: &[String], name: &str, span: Span) {
        if !schema.iter().any(|column| column == name) {
            self.diagnostics.push(Diagnostic::error(
                codes::E1005,
                format!("unknown column `{name}`"),
                span,
            ));
        }
    }

    fn push_trace(
        &mut self,
        stage: &Stage,
        input_schema: Vec<String>,
        output_schema: Vec<String>,
        grouping: Option<Vec<String>>,
    ) {
        let stage_id = self.next_stage_id;
        self.next_stage_id += 1;
        self.traces.push(StageTrace {
            stage_id,
            stage_name: stage_name(stage).to_string(),
            span: stage.span(),
            input_schema: Some(input_schema),
            output_schema: Some(output_schema),
            grouping: grouping
                .map(GroupingState::from_columns)
                .unwrap_or_else(GroupingState::none),
        });
    }
}

#[derive(Clone, Copy)]
enum ExprRole {
    PredicateRoot,
    Default,
    ComparisonLeft,
    ComparisonRight,
}

fn row_expr_column_refs(
    expr: &Expr,
    schema: &[String],
    role: ExprRole,
) -> Vec<pdl_syntax::Spanned<String>> {
    match expr {
        Expr::Quoted(value) => match role {
            ExprRole::ComparisonLeft => vec![value.clone()],
            ExprRole::Default | ExprRole::PredicateRoot
                if schema.iter().any(|column| column == &value.value) =>
            {
                vec![value.clone()]
            }
            _ => Vec::new(),
        },
        Expr::Call { name, args, .. } if name.value == "col" => args
            .first()
            .and_then(|arg| match arg {
                Expr::Quoted(value) => Some(vec![value.clone()]),
                _ => None,
            })
            .unwrap_or_default(),
        Expr::Call { args, .. } => args
            .iter()
            .flat_map(|arg| row_expr_column_refs(arg, schema, ExprRole::Default))
            .collect(),
        Expr::Unary { expr, .. } => row_expr_column_refs(expr, schema, ExprRole::Default),
        Expr::Binary {
            left, op, right, ..
        } if is_comparison_op(*op) => {
            let mut refs = row_expr_column_refs(left, schema, ExprRole::ComparisonLeft);
            refs.extend(row_expr_column_refs(
                right,
                schema,
                ExprRole::ComparisonRight,
            ));
            refs
        }
        Expr::Binary { left, right, .. } => {
            let mut refs = row_expr_column_refs(left, schema, ExprRole::Default);
            refs.extend(row_expr_column_refs(right, schema, ExprRole::Default));
            refs
        }
        Expr::Number(_) | Expr::Bool(_) | Expr::Null(_) | Expr::Ident(_) => Vec::new(),
    }
}

fn aggregate_expr_column_refs(expr: &Expr) -> Vec<pdl_syntax::Spanned<String>> {
    match expr {
        Expr::Quoted(value) => vec![value.clone()],
        Expr::Call { name, .. } if name.value == "lit" => Vec::new(),
        Expr::Call { name, args, .. } if name.value == "col" => args
            .first()
            .and_then(|arg| match arg {
                Expr::Quoted(value) => Some(vec![value.clone()]),
                _ => None,
            })
            .unwrap_or_default(),
        Expr::Call { args, .. } => args.iter().flat_map(aggregate_expr_column_refs).collect(),
        Expr::Unary { expr, .. } => aggregate_expr_column_refs(expr),
        Expr::Binary { left, right, .. } => {
            let mut refs = aggregate_expr_column_refs(left);
            refs.extend(aggregate_expr_column_refs(right));
            refs
        }
        Expr::Number(_) | Expr::Bool(_) | Expr::Null(_) | Expr::Ident(_) => Vec::new(),
    }
}

fn is_comparison_op(op: pdl_syntax::BinaryOp) -> bool {
    matches!(
        op,
        pdl_syntax::BinaryOp::Eq
            | pdl_syntax::BinaryOp::Ne
            | pdl_syntax::BinaryOp::Lt
            | pdl_syntax::BinaryOp::Lte
            | pdl_syntax::BinaryOp::Gt
            | pdl_syntax::BinaryOp::Gte
    )
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

#[allow(dead_code)]
fn load_source_path(load: &LoadStage) -> Option<&str> {
    match &load.source {
        SourceRef::Path(path) => Some(&path.value),
        SourceRef::Stdin(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analysis_builds_ir_and_stage_traces() {
        let parse = pdl_syntax::parse(
            r#"load "sales.csv"
  | filter "amount" > 0
  | select "region""#,
        );

        let analysis = analyze_program(&parse.program, |_| {
            Ok(vec!["amount".to_string(), "region".to_string()])
        });

        assert!(
            analysis.diagnostics.is_empty(),
            "{:?}",
            analysis.diagnostics
        );
        assert!(analysis.ir.is_some());
        assert_eq!(analysis.traces.len(), 2);
        assert_eq!(analysis.traces[0].stage_name, "filter");
        assert_eq!(
            analysis.traces[1].output_schema,
            Some(vec!["region".to_string()])
        );
    }
}
