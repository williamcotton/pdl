use pdl_core::{codes, Diagnostic, Span};
use pdl_syntax::{
    AggItem, Binding, Expr, LoadStage, Pipeline, PipelineStart, Program, SaveStage, SourceRef,
    Stage,
};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Analysis {
    pub diagnostics: Vec<Diagnostic>,
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
    };
    analyzer.analyze(program);
    Analysis {
        diagnostics: analyzer.diagnostics,
    }
}

struct Analyzer<'a, F>
where
    F: FnMut(LoadRequest<'_>) -> Result<Vec<String>, Diagnostic>,
{
    diagnostics: Vec<Diagnostic>,
    load_schema: &'a mut F,
    binding_schemas: BTreeMap<String, Vec<String>>,
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
                        format!("stage `{}` is deferred in 0.3.0", name.value),
                        name.span,
                    ));
                }
            }
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
            if format.value != "csv" {
                self.diagnostics.push(Diagnostic::error(
                    codes::E1215,
                    format!("format `{}` is not supported in 0.3.0", format.value),
                    format.span,
                ));
            }
        }
    }

    fn analyze_aggregate_item(&mut self, schema: &[String], item: &AggItem) {
        let function = item.function.value.as_str();
        let expected_arity = match function {
            "count" => {
                if item.args.len() > 1 {
                    Some("zero or one argument")
                } else {
                    None
                }
            }
            "sum" | "mean" | "min" | "max" => {
                if item.args.len() != 1 {
                    Some("one argument")
                } else {
                    None
                }
            }
            _ => {
                self.diagnostics.push(Diagnostic::error(
                    codes::E1401,
                    format!("unknown aggregate function `{function}`"),
                    item.function.span,
                ));
                return;
            }
        };
        if let Some(expected) = expected_arity {
            self.diagnostics.push(Diagnostic::error(
                codes::E1402,
                format!("aggregate function `{function}` expects {expected}"),
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

#[allow(dead_code)]
fn load_source_path(load: &LoadStage) -> Option<&str> {
    match &load.source {
        SourceRef::Path(path) => Some(&path.value),
        SourceRef::Stdin(_) => None,
    }
}
