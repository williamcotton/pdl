use pdl_core::{codes, Diagnostic, Span};
use pdl_syntax::{
    AggItem, Binding, Expr, JoinKind, LoadStage, Pipeline, PipelineStart, Program, SaveStage,
    SourceRef, Stage, UnionOption, UnionOptionKind,
};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use crate::ir::{lower_program, ProgramIr};
use crate::registry::{
    accepts_arity, aggregate_function, format_info, scalar_function, window_function,
};
use crate::schema::{GroupingState, PipelineSchema, PipelineSchemaLabel, StageTrace};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Analysis {
    pub diagnostics: Vec<Diagnostic>,
    pub ir: Option<ProgramIr>,
    pub traces: Vec<StageTrace>,
    pub outputs: Vec<PipelineSchema>,
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
        binding_decls: BTreeMap::new(),
        binding_schemas: BTreeMap::new(),
        traces: Vec::new(),
        outputs: Vec::new(),
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
        outputs: analyzer.outputs,
    }
}

struct Analyzer<'a, F>
where
    F: FnMut(LoadRequest<'_>) -> Result<Vec<String>, Diagnostic>,
{
    diagnostics: Vec<Diagnostic>,
    load_schema: &'a mut F,
    binding_decls: BTreeMap<String, Binding>,
    binding_schemas: BTreeMap<String, Vec<String>>,
    traces: Vec<StageTrace>,
    outputs: Vec<PipelineSchema>,
    next_stage_id: usize,
}

impl<F> Analyzer<'_, F>
where
    F: FnMut(LoadRequest<'_>) -> Result<Vec<String>, Diagnostic>,
{
    fn analyze(&mut self, program: &Program) {
        self.check_top_level_names(program);
        for binding in &program.bindings {
            self.binding_decls
                .entry(binding.name.value.clone())
                .or_insert_with(|| binding.clone());
        }
        if !program.outputs.is_empty() && program.main.is_some() {
            let span = program
                .main
                .as_ref()
                .map_or_else(Span::zero, |pipeline| pipeline.span);
            self.diagnostics.push(Diagnostic::error(
                codes::E1503,
                "document cannot mix output declarations with a main pipeline",
                span,
            ));
        }
        for output in &program.outputs {
            if let Some(columns) = self.analyze_pipeline(&output.pipeline, &mut Vec::new()) {
                self.outputs.push(PipelineSchema {
                    label: PipelineSchemaLabel::Output(output.name.value.clone()),
                    span: output.pipeline.span,
                    columns,
                });
            }
        }
        if let Some(main) = &program.main {
            if let Some(columns) = self.analyze_pipeline(main, &mut Vec::new()) {
                self.outputs.push(PipelineSchema {
                    label: PipelineSchemaLabel::Main,
                    span: main.span,
                    columns,
                });
            }
        }
    }

    fn check_top_level_names(&mut self, program: &Program) {
        let mut seen = BTreeSet::new();
        for binding in &program.bindings {
            if !seen.insert(binding.name.value.clone()) {
                self.diagnostics.push(Diagnostic::error(
                    codes::E1001,
                    format!("duplicate binding `{}`", binding.name.value),
                    binding.name.span,
                ));
            }
        }
        let binding_names = seen.clone();
        let mut output_names = BTreeSet::new();
        for output in &program.outputs {
            if !output_names.insert(output.name.value.clone()) {
                self.diagnostics.push(Diagnostic::error(
                    codes::E1001,
                    format!("duplicate output `{}`", output.name.value),
                    output.name.span,
                ));
            }
            if binding_names.contains(&output.name.value) {
                self.diagnostics.push(Diagnostic::error(
                    codes::E1001,
                    format!(
                        "output `{}` conflicts with an existing binding",
                        output.name.value
                    ),
                    output.name.span,
                ));
            }
        }
    }

    fn analyze_binding(
        &mut self,
        name: &str,
        reference_span: Span,
        stack: &mut Vec<String>,
    ) -> Option<Vec<String>> {
        if let Some(schema) = self.binding_schemas.get(name) {
            return Some(schema.clone());
        }

        if let Some(index) = stack.iter().position(|active| active == name) {
            let mut path = stack[index..].to_vec();
            path.push(name.to_string());
            self.diagnostics.push(Diagnostic::error(
                codes::E1501,
                format!("binding dependency cycle: {}", path.join(" -> ")),
                reference_span,
            ));
            return None;
        }

        let Some(binding) = self.binding_decls.get(name).cloned() else {
            self.diagnostics.push(Diagnostic::error(
                codes::E1007,
                format!("unknown binding `{name}`"),
                reference_span,
            ));
            return None;
        };

        stack.push(name.to_string());
        let schema = self.analyze_pipeline(&binding.pipeline, stack);
        stack.pop();

        if let Some(schema) = &schema {
            self.binding_schemas
                .insert(binding.name.value.clone(), schema.clone());
            self.outputs.push(PipelineSchema {
                label: PipelineSchemaLabel::Binding(binding.name.value.clone()),
                span: binding.pipeline.span,
                columns: schema.clone(),
            });
        }
        schema
    }

    fn analyze_pipeline(
        &mut self,
        pipeline: &Pipeline,
        stack: &mut Vec<String>,
    ) -> Option<Vec<String>> {
        let mut schema = match &pipeline.start {
            PipelineStart::Load(load) => match (self.load_schema)(LoadRequest { load, path: None })
            {
                Ok(schema) => schema,
                Err(diagnostic) => {
                    self.diagnostics.push(diagnostic);
                    return None;
                }
            },
            PipelineStart::Binding(name) => self.analyze_binding(&name.value, name.span, stack)?,
        };

        let mut grouping: Option<Vec<String>> = None;
        for stage in &pipeline.stages {
            let input_schema = schema.clone();
            match stage {
                Stage::Filter { expr, .. } => {
                    self.analyze_row_expr(&schema, expr, ExprRole::PredicateRoot);
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
                Stage::Mutate { items, .. } => {
                    let mut seen = BTreeSet::new();
                    let mut output = schema.clone();
                    for item in items {
                        self.analyze_mutate_expr(&schema, &item.expr);
                        if !seen.insert(item.column.value.clone()) {
                            self.diagnostics.push(Diagnostic::error(
                                codes::E1207,
                                format!("duplicate output column `{}`", item.column.value),
                                item.column.span,
                            ));
                        }
                        if !output.iter().any(|column| column == &item.column.value) {
                            output.push(item.column.value.clone());
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
                Stage::Join {
                    source, on, kind, ..
                } => {
                    let right_schema = self.analyze_binding(&source.value, source.span, stack)?;
                    self.require_column(&schema, &on.left().value, on.left().span);
                    self.require_column(&right_schema, &on.right().value, on.right().span);
                    match joined_schema(&schema, &right_schema, &on.right().value, *kind) {
                        Ok(output) => schema = output,
                        Err(collision) => {
                            self.diagnostics.push(Diagnostic::error(
                                codes::E1207,
                                format!("output column collision `{collision}`"),
                                source.span,
                            ));
                        }
                    }
                    grouping = None;
                }
                Stage::Union {
                    source, options, ..
                } => {
                    let right_schema = self.analyze_binding(&source.value, source.span, stack)?;
                    let by_name = union_option_value(options, UnionOptionKind::ByName);
                    if !union_schema_compatible(&schema, &right_schema, by_name) {
                        self.diagnostics.push(Diagnostic::error(
                            codes::E1209,
                            format!(
                                "binding `{}` has an incompatible union schema",
                                source.value
                            ),
                            source.span,
                        ));
                    }
                    grouping = None;
                }
                Stage::Distinct { columns, .. } => {
                    for column in columns {
                        self.require_column(&schema, &column.value, column.span);
                    }
                    grouping = None;
                }
                Stage::PivotLonger {
                    columns,
                    names_to,
                    values_to,
                    ..
                } => {
                    if columns.is_empty() {
                        self.diagnostics.push(Diagnostic::error(
                            codes::E1203,
                            "pivot_longer requires at least one source column",
                            names_to.span,
                        ));
                    }
                    let mut seen = BTreeSet::new();
                    for column in columns {
                        self.require_column(&schema, &column.value, column.span);
                        if !seen.insert(column.value.clone()) {
                            self.diagnostics.push(Diagnostic::error(
                                codes::E1205,
                                format!("duplicate pivot_longer column `{}`", column.value),
                                column.span,
                            ));
                        }
                    }
                    let selected: BTreeSet<String> =
                        columns.iter().map(|column| column.value.clone()).collect();
                    let copied = schema
                        .iter()
                        .filter(|column| !selected.contains(*column))
                        .cloned()
                        .collect::<Vec<_>>();
                    if copied.iter().any(|column| column == &names_to.value) {
                        self.diagnostics.push(Diagnostic::error(
                            codes::E1207,
                            format!("pivot_longer names_to `{}` already exists", names_to.value),
                            names_to.span,
                        ));
                    }
                    if copied.iter().any(|column| column == &values_to.value) {
                        self.diagnostics.push(Diagnostic::error(
                            codes::E1207,
                            format!(
                                "pivot_longer values_to `{}` already exists",
                                values_to.value
                            ),
                            values_to.span,
                        ));
                    }
                    if names_to.value == values_to.value {
                        self.diagnostics.push(Diagnostic::error(
                            codes::E1207,
                            "pivot_longer names_to and values_to must be different columns",
                            values_to.span,
                        ));
                    }
                    schema = copied;
                    schema.push(names_to.value.clone());
                    schema.push(values_to.value.clone());
                    grouping = None;
                }
                Stage::Complete { keys, fills, .. } => {
                    if keys.is_empty() {
                        self.diagnostics.push(Diagnostic::error(
                            codes::E1203,
                            "complete requires at least one key column",
                            stage.span(),
                        ));
                    }
                    let mut key_names = BTreeSet::new();
                    for key in keys {
                        self.require_column(&schema, &key.value, key.span);
                        if !key_names.insert(key.value.clone()) {
                            self.diagnostics.push(Diagnostic::error(
                                codes::E1205,
                                format!("duplicate complete key `{}`", key.value),
                                key.span,
                            ));
                        }
                    }
                    let mut fill_names = BTreeSet::new();
                    for fill in fills {
                        self.require_column(&schema, &fill.column.value, fill.column.span);
                        self.analyze_mutate_expr(&schema, &fill.expr);
                        if key_names.contains(&fill.column.value) {
                            self.diagnostics.push(Diagnostic::error(
                                codes::E1207,
                                format!(
                                    "complete fill target `{}` cannot be a key column",
                                    fill.column.value
                                ),
                                fill.column.span,
                            ));
                        }
                        if !fill_names.insert(fill.column.value.clone()) {
                            self.diagnostics.push(Diagnostic::error(
                                codes::E1205,
                                format!("duplicate complete fill target `{}`", fill.column.value),
                                fill.column.span,
                            ));
                        }
                    }
                    grouping = None;
                }
                Stage::Save(save) => self.analyze_save(save),
                Stage::Unsupported { name, .. } => {
                    self.diagnostics.push(Diagnostic::error(
                        codes::E1211,
                        format!("stage `{}` is deferred in 0.25.0", name.value),
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
                    format!("format `{}` is not supported in 0.25.0", format.value),
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
            self.analyze_scalar_expr(arg, WindowContext::Disallowed);
            for column in aggregate_expr_column_refs(arg) {
                self.require_column(schema, &column.value, column.span);
            }
        }
    }

    fn analyze_row_expr(&mut self, schema: &[String], expr: &Expr, role: ExprRole) {
        self.analyze_scalar_expr(expr, WindowContext::Disallowed);
        for column in row_expr_column_refs(expr, schema, role) {
            self.require_column(schema, &column.value, column.span);
        }
    }

    fn analyze_mutate_expr(&mut self, schema: &[String], expr: &Expr) {
        self.analyze_scalar_expr(expr, WindowContext::Allowed);
        for column in row_expr_column_refs(expr, schema, ExprRole::Default) {
            self.require_column(schema, &column.value, column.span);
        }
    }

    fn analyze_scalar_expr(&mut self, expr: &Expr, window_context: WindowContext) {
        match expr {
            Expr::Call { name, args, span } => {
                match scalar_function(&name.value) {
                    Some(info) if !accepts_arity(*info, args.len()) => {
                        self.diagnostics.push(Diagnostic::error(
                            codes::E1402,
                            format!("function `{}` expects {}", name.value, info.expected_arity),
                            *span,
                        ));
                    }
                    Some(_) => {}
                    None if window_function(&name.value).is_some() => {
                        self.diagnostics.push(Diagnostic::error(
                            codes::E1226,
                            format!("window function `{}` requires `over (...)`", name.value),
                            name.span,
                        ));
                    }
                    None => {
                        self.diagnostics.push(Diagnostic::error(
                            codes::E1401,
                            format!("unknown function `{}`", name.value),
                            name.span,
                        ));
                    }
                }
                if name.value == "round"
                    && args.get(1).is_some_and(|arg| !is_round_digits_literal(arg))
                {
                    self.diagnostics.push(Diagnostic::error(
                        codes::E1206,
                        "round() digits must be an integer literal from 0 through 12",
                        args[1].span(),
                    ));
                }
                for arg in args {
                    self.analyze_scalar_expr(arg, WindowContext::Disallowed);
                }
            }
            Expr::Window {
                function,
                args,
                spec,
                span,
            } => self.analyze_window_expr(function, args, spec, *span, window_context),
            Expr::Unary { expr, .. } => self.analyze_scalar_expr(expr, window_context),
            Expr::Binary { left, right, .. } => {
                self.analyze_scalar_expr(left, window_context);
                self.analyze_scalar_expr(right, window_context);
            }
            Expr::Quoted(_) | Expr::Number(_) | Expr::Bool(_) | Expr::Null(_) | Expr::Ident(_) => {}
        }
    }

    fn analyze_window_expr(
        &mut self,
        function: &pdl_syntax::Spanned<String>,
        args: &[Expr],
        spec: &pdl_syntax::WindowSpec,
        span: Span,
        window_context: WindowContext,
    ) {
        if window_context == WindowContext::Disallowed {
            self.diagnostics.push(Diagnostic::error(
                codes::E1226,
                "window expressions are supported only in `mutate` assignments",
                span,
            ));
        }
        match window_function(&function.value) {
            Some(info) if !accepts_arity(*info, args.len()) => {
                self.diagnostics.push(Diagnostic::error(
                    codes::E1402,
                    format!(
                        "window function `{}` expects {}",
                        function.value, info.expected_arity
                    ),
                    span,
                ));
            }
            Some(_) => {}
            None => {
                self.diagnostics.push(Diagnostic::error(
                    codes::E1401,
                    format!("unknown window function `{}`", function.value),
                    function.span,
                ));
            }
        }

        if requires_order_by(&function.value) && spec.order_by.is_empty() {
            self.diagnostics.push(Diagnostic::error(
                codes::E1203,
                format!("window function `{}` requires `order_by`", function.value),
                function.span,
            ));
        }

        if matches!(function.value.as_str(), "lag" | "lead")
            && args
                .get(1)
                .is_some_and(|arg| !is_non_negative_integer_literal(arg))
        {
            self.diagnostics.push(Diagnostic::error(
                codes::E1206,
                "lag/lead offset must be a non-negative integer literal",
                args[1].span(),
            ));
        }

        for arg in args {
            if contains_window_expr(arg) {
                self.diagnostics.push(Diagnostic::error(
                    codes::E1226,
                    "nested window expressions are not supported",
                    arg.span(),
                ));
            }
            self.analyze_scalar_expr(arg, WindowContext::Disallowed);
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

fn joined_schema(
    left_schema: &[String],
    right_schema: &[String],
    right_key: &str,
    kind: JoinKind,
) -> Result<Vec<String>, String> {
    if matches!(kind, JoinKind::Semi | JoinKind::Anti) {
        return Ok(left_schema.to_vec());
    }

    let mut output = left_schema.to_vec();
    for column in right_schema {
        if column == right_key {
            continue;
        }
        let mut output_name = column.clone();
        if output.iter().any(|existing| existing == &output_name) {
            output_name.push_str("_right");
            if output.iter().any(|existing| existing == &output_name) {
                return Err(output_name);
            }
        }
        output.push(output_name);
    }
    Ok(output)
}

fn union_option_value(options: &[UnionOption], kind: UnionOptionKind) -> bool {
    options
        .iter()
        .find(|option| option.kind == kind)
        .is_some_and(|option| option.value.value)
}

fn union_schema_compatible(left_schema: &[String], right_schema: &[String], by_name: bool) -> bool {
    if by_name {
        let left: BTreeSet<&String> = left_schema.iter().collect();
        let right: BTreeSet<&String> = right_schema.iter().collect();
        left == right
    } else {
        left_schema.len() == right_schema.len()
    }
}

#[derive(Clone, Copy)]
enum ExprRole {
    PredicateRoot,
    Default,
    ComparisonLeft,
    ComparisonRight,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum WindowContext {
    Allowed,
    Disallowed,
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
        Expr::Call { name, .. } if name.value == "lit" => Vec::new(),
        Expr::Call { args, .. } => args
            .iter()
            .flat_map(|arg| row_expr_column_refs(arg, schema, ExprRole::Default))
            .collect(),
        Expr::Window { args, spec, .. } => window_expr_column_refs(args, spec, schema),
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
        Expr::Window { args, spec, .. } => window_expr_column_refs(args, spec, &[]),
        Expr::Unary { expr, .. } => aggregate_expr_column_refs(expr),
        Expr::Binary { left, right, .. } => {
            let mut refs = aggregate_expr_column_refs(left);
            refs.extend(aggregate_expr_column_refs(right));
            refs
        }
        Expr::Number(_) | Expr::Bool(_) | Expr::Null(_) | Expr::Ident(_) => Vec::new(),
    }
}

fn window_expr_column_refs(
    args: &[Expr],
    spec: &pdl_syntax::WindowSpec,
    schema: &[String],
) -> Vec<pdl_syntax::Spanned<String>> {
    let mut refs = Vec::new();
    for arg in args {
        refs.extend(row_expr_column_refs(arg, schema, ExprRole::Default));
    }
    refs.extend(spec.partition_by.iter().cloned());
    refs.extend(spec.order_by.iter().map(|item| item.column.clone()));
    refs
}

fn contains_window_expr(expr: &Expr) -> bool {
    match expr {
        Expr::Window { .. } => true,
        Expr::Call { args, .. } => args.iter().any(contains_window_expr),
        Expr::Unary { expr, .. } => contains_window_expr(expr),
        Expr::Binary { left, right, .. } => {
            contains_window_expr(left) || contains_window_expr(right)
        }
        Expr::Quoted(_) | Expr::Number(_) | Expr::Bool(_) | Expr::Null(_) | Expr::Ident(_) => false,
    }
}

fn is_non_negative_integer_literal(expr: &Expr) -> bool {
    matches!(expr, Expr::Number(value) if value.value >= 0.0 && value.value.fract() == 0.0)
}

fn is_round_digits_literal(expr: &Expr) -> bool {
    matches!(expr, Expr::Number(value) if (0.0..=12.0).contains(&value.value) && value.value.fract() == 0.0)
}

fn requires_order_by(function: &str) -> bool {
    matches!(
        function,
        "rank" | "dense_rank" | "percent_rank" | "cume_dist"
    )
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
        Stage::Mutate { .. } => "mutate",
        Stage::GroupBy { .. } => "group_by",
        Stage::Agg { .. } => "agg",
        Stage::Sort { .. } => "sort",
        Stage::Limit { .. } => "limit",
        Stage::Join { .. } => "join",
        Stage::Union { .. } => "union",
        Stage::Distinct { .. } => "distinct",
        Stage::PivotLonger { .. } => "pivot_longer",
        Stage::Complete { .. } => "complete",
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

    #[test]
    fn mutate_adds_columns_and_distinct_preserves_schema() {
        let parse = pdl_syntax::parse(
            r#"load "orders.csv"
  | mutate "net_amount" = "gross" - "discount", "region_channel" = concat(upper("region"), lit(":"), lower("channel"))
  | distinct "order_id""#,
        );

        let analysis = analyze_program(&parse.program, |_| {
            Ok(vec![
                "order_id".to_string(),
                "region".to_string(),
                "channel".to_string(),
                "gross".to_string(),
                "discount".to_string(),
            ])
        });

        assert!(
            analysis.diagnostics.is_empty(),
            "{:?}",
            analysis.diagnostics
        );
        assert!(analysis.ir.is_some());
        assert_eq!(
            analysis.traces[0].output_schema,
            Some(vec![
                "order_id".to_string(),
                "region".to_string(),
                "channel".to_string(),
                "gross".to_string(),
                "discount".to_string(),
                "net_amount".to_string(),
                "region_channel".to_string(),
            ])
        );
        assert_eq!(analysis.traces[1].stage_name, "distinct");
        assert_eq!(
            analysis.traces[1].output_schema,
            analysis.traces[0].output_schema
        );
    }

    #[test]
    fn mutate_assignments_are_parallel() {
        let parse = pdl_syntax::parse(
            r#"load "orders.csv"
  | mutate "net_amount" = "gross" - "discount", "is_large" = "net_amount" > 100"#,
        );

        let analysis = analyze_program(&parse.program, |_| {
            Ok(vec!["gross".to_string(), "discount".to_string()])
        });

        assert!(analysis.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E1005" && diagnostic.message == "unknown column `net_amount`"
        }));
        assert!(analysis.ir.is_none());
    }

    #[test]
    fn window_mutate_adds_columns_and_checks_referenced_columns() {
        let parse = pdl_syntax::parse(
            r#"load "orders.csv"
  | mutate "running_amount" = sum("amount") over (partition_by "customer_id" order_by "order_date" rows between unbounded_preceding and current_row), "rank" = dense_rank() over (partition_by "region" order_by "amount" desc)"#,
        );

        let analysis = analyze_program(&parse.program, |_| {
            Ok(vec![
                "order_id".to_string(),
                "customer_id".to_string(),
                "region".to_string(),
                "order_date".to_string(),
                "amount".to_string(),
            ])
        });

        assert!(
            analysis.diagnostics.is_empty(),
            "{:?}",
            analysis.diagnostics
        );
        assert!(analysis.ir.is_some());
        assert_eq!(
            analysis.traces[0].output_schema,
            Some(vec![
                "order_id".to_string(),
                "customer_id".to_string(),
                "region".to_string(),
                "order_date".to_string(),
                "amount".to_string(),
                "running_amount".to_string(),
                "rank".to_string(),
            ])
        );
    }

    #[test]
    fn window_context_errors_are_diagnostics() {
        let parse = pdl_syntax::parse(
            r#"load "orders.csv"
  | filter row_number() over (order_by "order_date") > 1
  | mutate "bad_rank" = rank() over (partition_by "customer_id")"#,
        );

        let analysis = analyze_program(&parse.program, |_| {
            Ok(vec![
                "customer_id".to_string(),
                "order_date".to_string(),
                "amount".to_string(),
            ])
        });

        assert!(analysis.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E1226"
                && diagnostic.message.contains("only in `mutate` assignments")
        }));
        assert!(analysis.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E1203" && diagnostic.message.contains("requires `order_by`")
        }));
        assert!(analysis.ir.is_none());
    }

    #[test]
    fn join_adds_right_non_key_columns_with_suffixes() {
        let parse = pdl_syntax::parse(
            r#"let customers =
  load "customers.csv"

load "sales.csv"
  | join customers on "customer_id" kind left"#,
        );

        let analysis = analyze_program(&parse.program, |request| match &request.load.source {
            SourceRef::Path(path) if path.value == "sales.csv" => Ok(vec![
                "customer_id".to_string(),
                "amount".to_string(),
                "segment".to_string(),
            ]),
            SourceRef::Path(path) if path.value == "customers.csv" => {
                Ok(vec!["customer_id".to_string(), "segment".to_string()])
            }
            _ => panic!("unexpected load request"),
        });

        assert!(
            analysis.diagnostics.is_empty(),
            "{:?}",
            analysis.diagnostics
        );
        let join_trace = analysis
            .traces
            .iter()
            .find(|trace| trace.stage_name == "join")
            .expect("join trace");
        assert_eq!(
            join_trace.output_schema,
            Some(vec![
                "customer_id".to_string(),
                "amount".to_string(),
                "segment".to_string(),
                "segment_right".to_string(),
            ])
        );
    }

    #[test]
    fn union_rejects_incompatible_schema() {
        let parse = pdl_syntax::parse(
            r#"let extra =
  load "extra.csv"

load "sales.csv"
  | union extra"#,
        );

        let analysis = analyze_program(&parse.program, |request| match &request.load.source {
            SourceRef::Path(path) if path.value == "sales.csv" => {
                Ok(vec!["order_id".to_string(), "amount".to_string()])
            }
            SourceRef::Path(path) if path.value == "extra.csv" => Ok(vec!["order_id".to_string()]),
            _ => panic!("unexpected load request"),
        });

        assert!(analysis.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E1209"
                && diagnostic.message == "binding `extra` has an incompatible union schema"
        }));
        assert!(analysis.ir.is_none());
    }

    #[test]
    fn unused_binding_is_not_loaded() {
        let parse = pdl_syntax::parse(
            r#"let unused =
  load "missing.csv"

load "sales.csv"
  | select "amount""#,
        );

        let analysis = analyze_program(&parse.program, |request| match &request.load.source {
            SourceRef::Path(path) if path.value == "sales.csv" => Ok(vec!["amount".to_string()]),
            SourceRef::Path(path) => panic!("unused binding loaded `{}`", path.value),
            SourceRef::Stdin(_) => panic!("unexpected stdin"),
        });

        assert!(
            analysis.diagnostics.is_empty(),
            "{:?}",
            analysis.diagnostics
        );
        assert!(analysis.ir.is_some());
    }

    #[test]
    fn binding_cycle_reports_cycle_path() {
        let parse = pdl_syntax::parse(
            r#"let a =
  b

let b =
  a

a"#,
        );

        let analysis = analyze_program(&parse.program, |_| panic!("no load expected"));

        assert!(analysis.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E1501" && diagnostic.message.contains("a -> b -> a")
        }));
        assert!(analysis.ir.is_none());
    }
}
