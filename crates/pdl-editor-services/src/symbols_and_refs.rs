// Binding/context navigation and rename helpers extracted from `services.rs`
// as part of the v0.42 split. See `services.rs` for the cross-module layout
// overview.

use pdl_core::Span;
use pdl_syntax::{
    decode_context_column_ref, ContextKind, Expr, Pipeline, PipelineStart, Program, Stage,
};

use crate::services::{contains, stage_name};
use crate::{range_for_span, DocumentSymbolKind, EditorDocumentSymbol};

pub(crate) fn context_name_at_offset(
    _source: &str,
    program: &Program,
    offset: usize,
) -> Option<(ContextKind, String)> {
    for context in &program.contexts {
        if contains(context.name.span, offset) {
            return Some((context.kind, context.name.value.clone()));
        }
    }
    context_reference_spans(program)
        .into_iter()
        .find(|(_, _, span)| contains(*span, offset))
        .map(|(kind, name, _)| (kind, name))
}

pub(crate) fn context_full_spans(
    _source: &str,
    program: &Program,
    kind: ContextKind,
    name: &str,
) -> Vec<Span> {
    let mut spans = Vec::new();
    spans.extend(
        program
            .contexts
            .iter()
            .filter(|context| context.kind == kind && context.name.value == name)
            .map(|context| context.name.span),
    );
    spans.extend(
        context_reference_spans(program)
            .into_iter()
            .filter(|(ref_kind, ref_name, _)| *ref_kind == kind && ref_name == name)
            .map(|(_, _, span)| span),
    );
    spans
}

pub(crate) fn context_name_spans(
    source: &str,
    program: &Program,
    kind: ContextKind,
    name: &str,
) -> Vec<Span> {
    context_full_spans(source, program, kind, name)
        .into_iter()
        .map(|span| context_reference_name_span(source, span))
        .collect()
}

fn context_reference_spans(program: &Program) -> Vec<(ContextKind, String, Span)> {
    let mut spans = Vec::new();
    for context in &program.contexts {
        collect_expr_context_references(&context.default, &mut spans);
    }
    for binding in &program.bindings {
        collect_pipeline_context_references(&binding.pipeline, &mut spans);
    }
    for output in &program.outputs {
        collect_pipeline_context_references(&output.pipeline, &mut spans);
    }
    if let Some(main) = &program.main {
        collect_pipeline_context_references(main, &mut spans);
    }
    spans
}

fn collect_pipeline_context_references(
    pipeline: &Pipeline,
    spans: &mut Vec<(ContextKind, String, Span)>,
) {
    for stage in &pipeline.stages {
        collect_stage_context_references(stage, spans);
    }
}

fn collect_stage_context_references(stage: &Stage, spans: &mut Vec<(ContextKind, String, Span)>) {
    match stage {
        Stage::Filter { expr, .. } => collect_expr_context_references(expr, spans),
        Stage::Select { items, .. } => {
            for item in items {
                push_context_column_reference(&item.column, spans);
                if let Some(alias) = &item.alias {
                    push_context_column_reference(alias, spans);
                }
            }
        }
        Stage::Drop { columns, .. }
        | Stage::GroupBy { columns, .. }
        | Stage::Distinct { columns, .. } => push_context_column_references(columns, spans),
        Stage::Rename { items, .. } => {
            for item in items {
                push_context_column_reference(&item.old, spans);
                push_context_column_reference(&item.new, spans);
            }
        }
        Stage::Mutate { items, .. } => {
            for item in items {
                push_context_column_reference(&item.column, spans);
                collect_expr_context_references(&item.expr, spans);
            }
        }
        Stage::Agg { items, .. } => {
            for item in items {
                push_context_column_reference(&item.alias, spans);
                for arg in &item.args {
                    collect_expr_context_references(arg, spans);
                }
            }
        }
        Stage::Sort { items, .. } => {
            for item in items {
                push_context_column_reference(&item.column, spans);
            }
        }
        Stage::Join { on, .. } => {
            for key in on.keys() {
                push_context_column_reference(&key.left, spans);
                push_context_column_reference(&key.right, spans);
            }
        }
        Stage::Union { .. } => {}
        Stage::PivotLonger {
            columns,
            names_to,
            values_to,
            ..
        } => {
            push_context_column_references(columns, spans);
            push_context_column_reference(names_to, spans);
            push_context_column_reference(values_to, spans);
        }
        Stage::Complete { keys, fills, .. } => {
            push_context_column_references(keys, spans);
            for fill in fills {
                push_context_column_reference(&fill.column, spans);
                collect_expr_context_references(&fill.expr, spans);
            }
        }
        Stage::Limit { .. } | Stage::Save(_) | Stage::Unsupported { .. } => {}
    }
}

fn collect_expr_context_references(expr: &Expr, spans: &mut Vec<(ContextKind, String, Span)>) {
    match expr {
        Expr::Context { kind, name, span } => {
            spans.push((*kind, name.value.clone(), *span));
        }
        Expr::Call { args, .. } => {
            for arg in args {
                collect_expr_context_references(arg, spans);
            }
        }
        Expr::Window { args, spec, .. } => {
            for arg in args {
                collect_expr_context_references(arg, spans);
            }
            push_context_column_references(&spec.partition_by, spans);
            for item in &spec.order_by {
                push_context_column_reference(&item.column, spans);
            }
        }
        Expr::Unary { expr, .. } => collect_expr_context_references(expr, spans),
        Expr::Binary { left, right, .. } => {
            collect_expr_context_references(left, spans);
            collect_expr_context_references(right, spans);
        }
        Expr::Quoted(_) | Expr::Number(_) | Expr::Bool(_) | Expr::Null(_) | Expr::Ident(_) => {}
    }
}

fn push_context_column_references(
    columns: &[pdl_syntax::Spanned<String>],
    spans: &mut Vec<(ContextKind, String, Span)>,
) {
    for column in columns {
        push_context_column_reference(column, spans);
    }
}

fn push_context_column_reference(
    column: &pdl_syntax::Spanned<String>,
    spans: &mut Vec<(ContextKind, String, Span)>,
) {
    if let Some((kind, name)) = decode_context_column_ref(&column.value) {
        spans.push((kind, name.to_string(), column.span));
    }
}

fn context_reference_name_span(source: &str, span: Span) -> Span {
    if is_context_reference_span(source, span) {
        Span::new(span.start + 1, span.end)
    } else {
        span
    }
}

pub(crate) fn is_context_reference_span(source: &str, span: Span) -> bool {
    source
        .get(span.start..span.end)
        .is_some_and(|text| text.starts_with('$') || text.starts_with('@'))
}

pub(crate) fn binding_name_at_offset(program: &Program, offset: usize) -> Option<String> {
    for binding in &program.bindings {
        if contains(binding.name.span, offset) {
            return Some(binding.name.value.clone());
        }
        if let Some(name) = pipeline_start_binding_at_offset(&binding.pipeline, offset) {
            return Some(name);
        }
    }
    for output in &program.outputs {
        if let Some(name) = pipeline_start_binding_at_offset(&output.pipeline, offset) {
            return Some(name);
        }
    }
    program
        .main
        .as_ref()
        .and_then(|pipeline| pipeline_start_binding_at_offset(pipeline, offset))
}

fn pipeline_start_binding_at_offset(pipeline: &Pipeline, offset: usize) -> Option<String> {
    match &pipeline.start {
        PipelineStart::Binding(name) if contains(name.span, offset) => Some(name.value.clone()),
        _ => pipeline.stages.iter().find_map(|stage| match stage {
            Stage::Join { source, .. } | Stage::Union { source, .. }
                if contains(source.span, offset) =>
            {
                Some(source.value.clone())
            }
            _ => None,
        }),
    }
}

pub(crate) fn binding_spans(program: &Program, name: &str) -> Vec<Span> {
    let mut spans = Vec::new();
    for binding in &program.bindings {
        if binding.name.value == name {
            spans.push(binding.name.span);
        }
        if let PipelineStart::Binding(start) = &binding.pipeline.start {
            if start.value == name {
                spans.push(start.span);
            }
        }
        spans.extend(
            binding
                .pipeline
                .stages
                .iter()
                .filter_map(|stage| match stage {
                    Stage::Join { source, .. } | Stage::Union { source, .. }
                        if source.value == name =>
                    {
                        Some(source.span)
                    }
                    _ => None,
                }),
        );
    }
    if let Some(main) = &program.main {
        if let PipelineStart::Binding(start) = &main.start {
            if start.value == name {
                spans.push(start.span);
            }
        }
        spans.extend(main.stages.iter().filter_map(|stage| match stage {
            Stage::Join { source, .. } | Stage::Union { source, .. } if source.value == name => {
                Some(source.span)
            }
            _ => None,
        }));
    }
    for output in &program.outputs {
        if let PipelineStart::Binding(start) = &output.pipeline.start {
            if start.value == name {
                spans.push(start.span);
            }
        }
        spans.extend(
            output
                .pipeline
                .stages
                .iter()
                .filter_map(|stage| match stage {
                    Stage::Join { source, .. } | Stage::Union { source, .. }
                        if source.value == name =>
                    {
                        Some(source.span)
                    }
                    _ => None,
                }),
        );
    }
    spans
}

pub(crate) fn is_valid_binding_name(name: &str) -> bool {
    use pdl_semantics::registry::KEYWORDS;
    let mut chars = name.chars();
    chars.next().is_some_and(is_ident_start)
        && chars.all(is_ident_char)
        && !KEYWORDS.contains(&name)
}

fn is_ident_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

fn is_ident_start(ch: char) -> bool {
    ch.is_ascii_alphabetic() || ch == '_'
}

pub(crate) fn pipeline_stage_symbols(
    source: &str,
    pipeline: &Pipeline,
) -> Vec<EditorDocumentSymbol> {
    let mut symbols = Vec::new();
    if let PipelineStart::Load(load) = &pipeline.start {
        symbols.push(EditorDocumentSymbol {
            name: "load".to_string(),
            detail: "stage".to_string(),
            kind: DocumentSymbolKind::Stage,
            range: range_for_span(source, load.span),
            selection_range: range_for_span(
                source,
                Span::new(load.span.start, load.span.start + 4),
            ),
            children: Vec::new(),
        });
    }
    symbols.extend(pipeline.stages.iter().map(|stage| {
        let name = stage_name(stage);
        EditorDocumentSymbol {
            name: name.to_string(),
            detail: "stage".to_string(),
            kind: DocumentSymbolKind::Stage,
            range: range_for_span(source, stage.span()),
            selection_range: range_for_span(
                source,
                Span::new(stage.span().start, stage.span().start + name.len()),
            ),
            children: Vec::new(),
        }
    }));
    symbols
}
