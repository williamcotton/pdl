// `services.rs` is the editor-services public surface and the registry of
// shared editor types. Per the v0.42 split, related helpers live in three
// sibling modules:
//
// * [`crate::completion`] — `CompletionContext` plus the per-context
//   completion-item builders (`column_completions`, `binding_completions`,
//   `format_completions`, `function_completions`, etc.).
// * [`crate::scope_analysis`] — `DocumentFacts` and the schema-inference
//   helpers (`apply_stage_to_schema`, `optimistic_columns`,
//   `SchemaState`/`BindingFact`/`ContextFact`).
// * [`crate::symbols_and_refs`] — binding/context navigation, rename support,
//   and document-symbol construction.

use pdl_core::{Diagnostic, Severity, Span};
use pdl_semantics::registry::{
    AGGREGATE_FUNCTIONS, FORMATS, KEYWORDS, SCALAR_FUNCTIONS, STAGES, WINDOW_FUNCTIONS,
};
use pdl_semantics::{analyze_program, LoadRequest};
use pdl_syntax::{ContextKind, Expr, ParseResult, Pipeline, PipelineStart, Program, Stage};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::completion::{
    binding_completion, column_completions, context_completions, dedupe_completions,
    format_completion, function_completion, keyword_completion, stage_completion,
    window_frame_name_completions, CompletionContext,
};
use crate::diagnostics::diagnostics_for_editor;
use crate::scope_analysis::optimistic_columns;
pub(crate) use crate::scope_analysis::DocumentFacts;
use crate::symbols_and_refs::{
    binding_name_at_offset, binding_spans, context_name_spans, is_context_reference_span,
    is_valid_binding_name, pipeline_stage_symbols,
};
pub(crate) use crate::symbols_and_refs::{context_full_spans, context_name_at_offset};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TextPosition {
    pub line: u32,
    pub character: u32,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TextRange {
    pub start: TextPosition,
    pub end: TextPosition,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EditorDiagnostic {
    pub range: TextRange,
    pub severity: Severity,
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EditorDocument {
    pub diagnostics: Vec<EditorDiagnostic>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EditorCompletion {
    pub label: String,
    pub insert_text: String,
    pub detail: String,
    pub kind: CompletionKind,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum CompletionKind {
    Binding,
    Column,
    Context,
    Format,
    Function,
    Keyword,
    Stage,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EditorHover {
    pub range: TextRange,
    pub markdown: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EditorTextEdit {
    pub range: TextRange,
    pub new_text: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EditorSemanticToken {
    pub range: TextRange,
    pub token_type: SemanticTokenKind,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum SemanticTokenKind {
    Keyword,
    Function,
    Variable,
    String,
    Number,
    Operator,
    BindingDeclaration,
    BindingReference,
    ColumnDefinition,
    ColumnReference,
    ContextDeclaration,
    ContextReference,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EditorDocumentSymbol {
    pub name: String,
    pub detail: String,
    pub kind: DocumentSymbolKind,
    pub range: TextRange,
    pub selection_range: TextRange,
    pub children: Vec<EditorDocumentSymbol>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum DocumentSymbolKind {
    Binding,
    Context,
    Function,
    Stage,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EditorLocation {
    pub range: TextRange,
}

const JOIN_KINDS: &[&str] = &["inner", "left", "right", "full", "semi", "anti"];

pub fn analyze_document(source: &str, path: Option<&Path>) -> EditorDocument {
    if let Some(path) = path {
        return analyze_document_with_driver_io(source, path, &pdl_driver::OsDriverIo);
    }
    let parse = pdl_syntax::parse(source);
    let optimistic_schema = optimistic_columns(&parse.program);
    analyze_parsed_document(source, parse, |request| {
        let _ = request;
        Ok(optimistic_schema.clone())
    })
}

pub fn analyze_document_with_driver_io(
    source: &str,
    path: &Path,
    io: &dyn pdl_driver::DriverIo,
) -> EditorDocument {
    let prepared = pdl_driver::prepare_source_with_io(path, source, io);
    EditorDocument {
        diagnostics: diagnostics_for_editor(source, &prepared.diagnostics()),
    }
}

pub fn analyze_document_with_schemas<I, P>(source: &str, path: &Path, schemas: I) -> EditorDocument
where
    I: IntoIterator<Item = (P, Vec<String>)>,
    P: Into<PathBuf>,
{
    let mut io = pdl_driver::InMemoryDriverIo::default();
    for (path, columns) in schemas {
        io = io.with_schema(path, columns);
    }
    analyze_document_with_driver_io(source, path, &io)
}

pub fn analyze_document_with_load_schema<F>(source: &str, load_schema: F) -> EditorDocument
where
    F: FnMut(LoadRequest<'_>) -> Result<Vec<String>, Diagnostic>,
{
    let parse = pdl_syntax::parse(source);
    analyze_parsed_document(source, parse, load_schema)
}

fn analyze_parsed_document<F>(source: &str, parse: ParseResult, load_schema: F) -> EditorDocument
where
    F: FnMut(LoadRequest<'_>) -> Result<Vec<String>, Diagnostic>,
{
    let mut diagnostics = parse.diagnostics.clone();
    let analysis = analyze_program(&parse.program, load_schema);
    diagnostics.extend(analysis.diagnostics);
    EditorDocument {
        diagnostics: diagnostics_for_editor(source, &diagnostics),
    }
}

pub fn completions(
    source: &str,
    _path: Option<&Path>,
    position: TextPosition,
) -> Vec<EditorCompletion> {
    let offset = byte_offset_for_position(source, position);
    let parse = pdl_syntax::parse(source);
    let facts = DocumentFacts::new(&parse.program);
    let context = CompletionContext::new(source, offset, &parse.program);
    let schema = facts
        .schema_before_offset(&parse.program, offset)
        .unwrap_or_default();
    let mut completions = Vec::new();

    if context.in_format_context {
        completions.extend(
            FORMATS
                .iter()
                .filter(|info| info.load_supported || info.save_supported || info.stream_supported)
                .map(format_completion),
        );
    } else if context.in_join_or_union_source_context {
        completions.extend(
            facts
                .bindings
                .iter()
                .map(|(name, binding)| binding_completion(name, binding)),
        );
    } else if context.after_pipe {
        completions.extend(
            STAGES
                .iter()
                .filter(|info| info.implemented)
                .map(stage_completion),
        );
    } else if context.in_pipeline_start {
        completions.push(keyword_completion(
            "load",
            "Start a pipeline by loading a table",
        ));
        completions.extend(
            facts
                .bindings
                .iter()
                .map(|(name, binding)| binding_completion(name, binding)),
        );
    } else if context.in_join_kind_name_context {
        completions.extend(
            JOIN_KINDS
                .iter()
                .map(|kind| keyword_completion(kind, "Join kind")),
        );
    } else if context.in_join_kind_keyword_context {
        completions.push(keyword_completion("kind", "Select a join kind"));
    } else if let Some(kind) = context.context_reference_kind {
        completions.extend(context_completions(&facts, kind));
    } else if context.in_window_frame_name_context {
        completions.extend(window_frame_name_completions());
    } else if context.in_agg_function_context {
        completions.extend(AGGREGATE_FUNCTIONS.iter().map(function_completion));
    } else if context.in_scalar_function_context {
        completions.extend(SCALAR_FUNCTIONS.iter().map(function_completion));
        if context.in_mutate_context {
            completions.extend(WINDOW_FUNCTIONS.iter().map(function_completion));
        }
    } else if context.in_sort_direction_context {
        completions.extend([
            keyword_completion("asc", "Sort ascending"),
            keyword_completion("desc", "Sort descending"),
            keyword_completion("nulls_first", "Place nulls before non-null values"),
            keyword_completion("nulls_last", "Place nulls after non-null values"),
        ]);
    } else if context.in_column_context && !context.inside_string {
        completions.extend(column_completions(&schema, context.inside_string));
    }

    dedupe_completions(completions)
}

pub fn formatting_edit(source: &str) -> Option<EditorTextEdit> {
    let formatted = pdl_syntax::format_source(source)?;
    if formatted == source {
        return None;
    }
    Some(EditorTextEdit {
        range: TextRange {
            start: TextPosition {
                line: 0,
                character: 0,
            },
            end: position_for_byte_offset(source, source.len()),
        },
        new_text: formatted,
    })
}

pub fn semantic_tokens(source: &str) -> Vec<EditorSemanticToken> {
    let mut tokens = Vec::new();
    let parse = pdl_syntax::parse(source);
    let semantic_names = semantic_name_overrides(source, &parse.program);
    let mut pos = 0usize;

    while pos < source.len() {
        let rest = &source[pos..];
        let Some(ch) = rest.chars().next() else {
            break;
        };

        if ch.is_whitespace() {
            pos += ch.len_utf8();
        } else if rest.starts_with("//") {
            pos = skip_line_comment(source, pos);
        } else if rest.starts_with("/*") {
            pos = skip_block_comment(source, pos);
        } else if ch == '"' {
            let end = scan_string(source, pos);
            push_semantic_token(source, &mut tokens, pos, end, SemanticTokenKind::String);
            pos = end;
        } else if ch == '$' || ch == '@' {
            let end = scan_context_reference(source, pos);
            let token_type = semantic_names
                .get(&(pos, end))
                .copied()
                .unwrap_or(SemanticTokenKind::ContextReference);
            push_semantic_token(source, &mut tokens, pos, end, token_type);
            pos = end;
        } else if ch == '`' {
            let end = scan_backtick_column(source, pos);
            let token_type = semantic_names
                .get(&(pos, end))
                .copied()
                .unwrap_or(SemanticTokenKind::Variable);
            push_semantic_token(source, &mut tokens, pos, end, token_type);
            pos = end;
        } else if ch.is_ascii_digit() {
            let end = scan_number(source, pos);
            push_semantic_token(source, &mut tokens, pos, end, SemanticTokenKind::Number);
            pos = end;
        } else if is_ident_start(ch) {
            let end = scan_identifier(source, pos);
            let text = &source[pos..end];
            let token_type = if KEYWORDS.contains(&text) {
                SemanticTokenKind::Keyword
            } else if AGGREGATE_FUNCTIONS.iter().any(|info| info.name == text)
                || SCALAR_FUNCTIONS.iter().any(|info| info.name == text)
                || WINDOW_FUNCTIONS.iter().any(|info| info.name == text)
            {
                SemanticTokenKind::Function
            } else {
                SemanticTokenKind::Variable
            };
            let token_type = semantic_names
                .get(&(pos, end))
                .copied()
                .unwrap_or(token_type);
            push_semantic_token(source, &mut tokens, pos, end, token_type);
            pos = end;
        } else if let Some(end) = scan_operator(source, pos) {
            push_semantic_token(source, &mut tokens, pos, end, SemanticTokenKind::Operator);
            pos = end;
        } else {
            pos += ch.len_utf8();
        }
    }

    tokens
}

fn semantic_name_overrides(
    source: &str,
    program: &Program,
) -> BTreeMap<(usize, usize), SemanticTokenKind> {
    let mut names = BTreeMap::new();
    for context in &program.contexts {
        push_semantic_name(
            source,
            &mut names,
            context.name.span,
            SemanticTokenKind::ContextDeclaration,
        );
        collect_expr_semantic_names(source, &context.default, &mut names);
    }
    for binding in &program.bindings {
        push_semantic_name(
            source,
            &mut names,
            binding.name.span,
            SemanticTokenKind::BindingDeclaration,
        );
        collect_pipeline_semantic_names(source, &binding.pipeline, &mut names);
    }
    for output in &program.outputs {
        collect_pipeline_semantic_names(source, &output.pipeline, &mut names);
    }
    if let Some(main) = &program.main {
        collect_pipeline_semantic_names(source, main, &mut names);
    }
    names
}

fn collect_pipeline_semantic_names(
    source: &str,
    pipeline: &Pipeline,
    names: &mut BTreeMap<(usize, usize), SemanticTokenKind>,
) {
    if let PipelineStart::Binding(name) = &pipeline.start {
        push_semantic_name(
            source,
            names,
            name.span,
            SemanticTokenKind::BindingReference,
        );
    }
    for stage in &pipeline.stages {
        collect_stage_semantic_names(source, stage, names);
    }
}

fn collect_stage_semantic_names(
    source: &str,
    stage: &Stage,
    names: &mut BTreeMap<(usize, usize), SemanticTokenKind>,
) {
    match stage {
        Stage::Filter { expr, .. } => collect_expr_semantic_names(source, expr, names),
        Stage::Select { items, .. } => {
            for item in items {
                push_column_reference(source, names, item.column.span);
                if let Some(alias) = &item.alias {
                    push_column_definition(source, names, alias.span);
                }
            }
        }
        Stage::Drop { columns, .. } | Stage::GroupBy { columns, .. } => {
            push_column_references(source, names, columns);
        }
        Stage::Rename { items, .. } => {
            for item in items {
                push_column_reference(source, names, item.old.span);
                push_column_definition(source, names, item.new.span);
            }
        }
        Stage::Mutate { items, .. } => {
            for item in items {
                push_column_definition(source, names, item.column.span);
                collect_expr_semantic_names(source, &item.expr, names);
            }
        }
        Stage::Agg { items, .. } => {
            for item in items {
                push_column_definition(source, names, item.alias.span);
                for arg in &item.args {
                    collect_expr_semantic_names(source, arg, names);
                }
            }
        }
        Stage::Sort { items, .. } => {
            for item in items {
                push_column_reference(source, names, item.column.span);
            }
        }
        Stage::Join {
            source: binding,
            on,
            ..
        } => {
            push_semantic_name(
                source,
                names,
                binding.span,
                SemanticTokenKind::BindingReference,
            );
            for key in on.keys() {
                push_column_reference(source, names, key.left.span);
                if key.right.span != key.left.span {
                    push_column_reference(source, names, key.right.span);
                }
            }
        }
        Stage::Union {
            source: binding, ..
        } => {
            push_semantic_name(
                source,
                names,
                binding.span,
                SemanticTokenKind::BindingReference,
            );
        }
        Stage::Distinct { columns, .. } => push_column_references(source, names, columns),
        Stage::PivotLonger {
            columns,
            names_to,
            values_to,
            ..
        } => {
            push_column_references(source, names, columns);
            push_column_definition(source, names, names_to.span);
            push_column_definition(source, names, values_to.span);
        }
        Stage::Complete { keys, fills, .. } => {
            push_column_references(source, names, keys);
            for fill in fills {
                push_column_definition(source, names, fill.column.span);
                collect_expr_semantic_names(source, &fill.expr, names);
            }
        }
        Stage::Limit { .. } | Stage::Save(_) | Stage::Unsupported { .. } => {}
    }
}

fn collect_expr_semantic_names(
    source: &str,
    expr: &Expr,
    names: &mut BTreeMap<(usize, usize), SemanticTokenKind>,
) {
    match expr {
        Expr::Ident(value) => push_column_reference(source, names, value.span),
        Expr::Call { args, .. } => {
            for arg in args {
                collect_expr_semantic_names(source, arg, names);
            }
        }
        Expr::Window { args, spec, .. } => {
            for arg in args {
                collect_expr_semantic_names(source, arg, names);
            }
            push_column_references(source, names, &spec.partition_by);
            for item in &spec.order_by {
                push_column_reference(source, names, item.column.span);
            }
        }
        Expr::Unary { expr, .. } => collect_expr_semantic_names(source, expr, names),
        Expr::Binary { left, right, .. } => {
            collect_expr_semantic_names(source, left, names);
            collect_expr_semantic_names(source, right, names);
        }
        Expr::Context { span, .. } => {
            push_semantic_name(source, names, *span, SemanticTokenKind::ContextReference);
        }
        Expr::Quoted(_) | Expr::Number(_) | Expr::Bool(_) | Expr::Null(_) => {}
    }
}

fn push_column_references(
    source: &str,
    names: &mut BTreeMap<(usize, usize), SemanticTokenKind>,
    columns: &[pdl_syntax::Spanned<String>],
) {
    for column in columns {
        push_column_reference(source, names, column.span);
    }
}

fn push_column_reference(
    source: &str,
    names: &mut BTreeMap<(usize, usize), SemanticTokenKind>,
    span: Span,
) {
    let token_type = if is_context_reference_span(source, span) {
        SemanticTokenKind::ContextReference
    } else {
        SemanticTokenKind::ColumnReference
    };
    push_semantic_name(source, names, span, token_type);
}

fn push_column_definition(
    source: &str,
    names: &mut BTreeMap<(usize, usize), SemanticTokenKind>,
    span: Span,
) {
    let token_type = if is_context_reference_span(source, span) {
        SemanticTokenKind::ContextReference
    } else {
        SemanticTokenKind::ColumnDefinition
    };
    push_semantic_name(source, names, span, token_type);
}

fn push_semantic_name(
    source: &str,
    names: &mut BTreeMap<(usize, usize), SemanticTokenKind>,
    span: Span,
    token_type: SemanticTokenKind,
) {
    let Some(text) = source.get(span.start..span.end) else {
        return;
    };
    if text.is_empty() || text.starts_with('"') {
        return;
    }
    names.insert((span.start, span.end), token_type);
}

pub fn document_symbols(source: &str) -> Vec<EditorDocumentSymbol> {
    let parse = pdl_syntax::parse(source);
    let mut symbols = Vec::new();
    for context in &parse.program.contexts {
        symbols.push(EditorDocumentSymbol {
            name: context_symbol_name(context.kind, &context.name.value),
            detail: context_kind_detail(context.kind).to_string(),
            kind: DocumentSymbolKind::Context,
            range: range_for_span(source, context.span),
            selection_range: range_for_span(source, context.name.span),
            children: Vec::new(),
        });
    }
    for binding in &parse.program.bindings {
        symbols.push(EditorDocumentSymbol {
            name: binding.name.value.clone(),
            detail: "binding".to_string(),
            kind: DocumentSymbolKind::Binding,
            range: range_for_span(source, binding.name.span.join(binding.pipeline.span)),
            selection_range: range_for_span(source, binding.name.span),
            children: pipeline_stage_symbols(source, &binding.pipeline),
        });
    }
    for output in &parse.program.outputs {
        symbols.push(EditorDocumentSymbol {
            name: output.name.value.clone(),
            detail: "output".to_string(),
            kind: DocumentSymbolKind::Function,
            range: range_for_span(source, output.name.span.join(output.pipeline.span)),
            selection_range: range_for_span(source, output.name.span),
            children: pipeline_stage_symbols(source, &output.pipeline),
        });
    }
    if let Some(main) = &parse.program.main {
        symbols.push(EditorDocumentSymbol {
            name: "main".to_string(),
            detail: "pipeline".to_string(),
            kind: DocumentSymbolKind::Function,
            range: range_for_span(source, main.span),
            selection_range: range_for_span(source, main.span),
            children: pipeline_stage_symbols(source, main),
        });
    }
    symbols
}

pub fn binding_definition(source: &str, position: TextPosition) -> Option<EditorLocation> {
    let offset = byte_offset_for_position(source, position);
    let parse = pdl_syntax::parse(source);
    if let Some((kind, name)) = context_name_at_offset(source, &parse.program, offset) {
        return parse
            .program
            .contexts
            .iter()
            .find(|context| context.kind == kind && context.name.value == name)
            .map(|context| EditorLocation {
                range: range_for_span(source, context.name.span),
            });
    }
    let name = binding_name_at_offset(&parse.program, offset)?;
    parse
        .program
        .bindings
        .iter()
        .find(|binding| binding.name.value == name)
        .map(|binding| EditorLocation {
            range: range_for_span(source, binding.name.span),
        })
}

pub fn binding_references(source: &str, position: TextPosition) -> Vec<EditorLocation> {
    let offset = byte_offset_for_position(source, position);
    let parse = pdl_syntax::parse(source);
    if let Some((kind, name)) = context_name_at_offset(source, &parse.program, offset) {
        return context_full_spans(source, &parse.program, kind, &name)
            .into_iter()
            .map(|span| EditorLocation {
                range: range_for_span(source, span),
            })
            .collect();
    }
    let Some(name) = binding_name_at_offset(&parse.program, offset) else {
        return Vec::new();
    };
    binding_spans(&parse.program, &name)
        .into_iter()
        .map(|span| EditorLocation {
            range: range_for_span(source, span),
        })
        .collect()
}

pub fn rename_binding_edits(
    source: &str,
    position: TextPosition,
    new_name: &str,
) -> Vec<EditorTextEdit> {
    if !is_valid_binding_name(new_name) {
        return Vec::new();
    }
    let offset = byte_offset_for_position(source, position);
    let parse = pdl_syntax::parse(source);
    if let Some((kind, name)) = context_name_at_offset(source, &parse.program, offset) {
        return context_name_spans(source, &parse.program, kind, &name)
            .into_iter()
            .map(|span| EditorTextEdit {
                range: range_for_span(source, span),
                new_text: new_name.to_string(),
            })
            .collect();
    }
    let Some(name) = binding_name_at_offset(&parse.program, offset) else {
        return Vec::new();
    };
    binding_spans(&parse.program, &name)
        .into_iter()
        .map(|span| EditorTextEdit {
            range: range_for_span(source, span),
            new_text: new_name.to_string(),
        })
        .collect()
}

pub fn range_for_span(source: &str, span: Span) -> TextRange {
    TextRange {
        start: position_for_byte_offset(source, span.start),
        end: position_for_byte_offset(source, span.end),
    }
}

pub fn position_for_byte_offset(source: &str, byte_offset: usize) -> TextPosition {
    let mut line = 0u32;
    let mut character = 0u32;

    for (index, ch) in source.char_indices() {
        if index >= byte_offset {
            break;
        }

        if ch == '\n' {
            line += 1;
            character = 0;
        } else {
            character += ch.len_utf16() as u32;
        }
    }

    TextPosition { line, character }
}

pub fn byte_offset_for_position(source: &str, position: TextPosition) -> usize {
    let mut line = 0u32;
    let mut character = 0u32;

    for (index, ch) in source.char_indices() {
        if line == position.line && character >= position.character {
            return index;
        }
        if ch == '\n' {
            if line == position.line {
                return index;
            }
            line += 1;
            character = 0;
        } else {
            character += ch.len_utf16() as u32;
        }
    }

    source.len()
}

pub(crate) fn contains(span: Span, offset: usize) -> bool {
    span.start <= offset && offset <= span.end
}

fn push_semantic_token(
    source: &str,
    tokens: &mut Vec<EditorSemanticToken>,
    start: usize,
    end: usize,
    token_type: SemanticTokenKind,
) {
    if start == end {
        return;
    }
    tokens.push(EditorSemanticToken {
        range: range_for_span(source, Span::new(start, end)),
        token_type,
    });
}

fn skip_line_comment(source: &str, start: usize) -> usize {
    source[start..]
        .find('\n')
        .map_or(source.len(), |offset| start + offset)
}

fn skip_block_comment(source: &str, start: usize) -> usize {
    let mut pos = start + 2;
    let mut depth = 1usize;
    while pos < source.len() {
        if source[pos..].starts_with("/*") {
            depth += 1;
            pos += 2;
        } else if source[pos..].starts_with("*/") {
            depth -= 1;
            pos += 2;
            if depth == 0 {
                return pos;
            }
        } else if let Some(ch) = source[pos..].chars().next() {
            pos += ch.len_utf8();
        } else {
            break;
        }
    }
    source.len()
}

fn scan_string(source: &str, start: usize) -> usize {
    let mut pos = start + 1;
    let mut escaped = false;
    while pos < source.len() {
        let Some(ch) = source[pos..].chars().next() else {
            break;
        };
        pos += ch.len_utf8();
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            break;
        }
    }
    pos
}

fn scan_context_reference(source: &str, start: usize) -> usize {
    let mut pos = start + 1;
    while pos < source.len() {
        let Some(ch) = source[pos..].chars().next() else {
            break;
        };
        if !is_ident_char(ch) {
            break;
        }
        pos += ch.len_utf8();
    }
    pos
}

fn scan_backtick_column(source: &str, start: usize) -> usize {
    let mut pos = start + 1;
    let mut escaped = false;
    while pos < source.len() {
        let Some(ch) = source[pos..].chars().next() else {
            break;
        };
        pos += ch.len_utf8();
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '`' {
            break;
        }
    }
    pos
}

fn scan_number(source: &str, start: usize) -> usize {
    let mut pos = scan_ascii_digits(source, start);
    if source[pos..].starts_with('.') {
        let after_dot = pos + 1;
        let after_fraction = scan_ascii_digits(source, after_dot);
        if after_fraction > after_dot {
            pos = after_fraction;
        }
    }
    if source[pos..].starts_with('e') || source[pos..].starts_with('E') {
        let mut exponent = pos + 1;
        if source[exponent..].starts_with('+') || source[exponent..].starts_with('-') {
            exponent += 1;
        }
        let after_exponent = scan_ascii_digits(source, exponent);
        if after_exponent > exponent {
            pos = after_exponent;
        }
    }
    pos
}

fn scan_ascii_digits(source: &str, start: usize) -> usize {
    let mut pos = start;
    while pos < source.len() {
        let Some(ch) = source[pos..].chars().next() else {
            break;
        };
        if !ch.is_ascii_digit() {
            break;
        }
        pos += ch.len_utf8();
    }
    pos
}

fn scan_identifier(source: &str, start: usize) -> usize {
    let mut pos = start;
    while pos < source.len() {
        let Some(ch) = source[pos..].chars().next() else {
            break;
        };
        if !is_ident_char(ch) {
            break;
        }
        pos += ch.len_utf8();
    }
    pos
}

fn scan_operator(source: &str, start: usize) -> Option<usize> {
    let rest = &source[start..];
    if ["==", "!=", "<=", ">="]
        .iter()
        .any(|operator| rest.starts_with(operator))
    {
        return Some(start + 2);
    }
    let ch = rest.chars().next()?;
    matches!(
        ch,
        '|' | '=' | '+' | '-' | '*' | '/' | '%' | '<' | '>' | '!'
    )
    .then_some(start + ch.len_utf8())
}

pub(crate) fn stage_name(stage: &Stage) -> &'static str {
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
            "join" => "join",
            "union" => "union",
            _ => "unknown",
        },
    }
}

pub(crate) fn context_symbol_name(kind: ContextKind, name: &str) -> String {
    let prefix = match kind {
        ContextKind::Param => "$",
        ContextKind::State => "@",
    };
    format!("{prefix}{name}")
}

pub(crate) fn context_kind_detail(kind: ContextKind) -> &'static str {
    match kind {
        ContextKind::Param => "parameter",
        ContextKind::State => "state",
    }
}

pub(crate) fn unquoted_text_at_span(source: &str, span: Span) -> Option<String> {
    let text = source.get(span.start..span.end)?;
    if text.starts_with('`') && text.ends_with('`') && text.len() >= 2 {
        return Some(unescape_backtick_column(text));
    }
    if text.starts_with('"') && text.ends_with('"') && text.len() >= 2 {
        return Some(text.trim_matches('"').to_string());
    }
    Some(text.to_string())
}

fn unescape_backtick_column(text: &str) -> String {
    let mut value = String::new();
    let mut chars = text[1..text.len() - 1].chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(escaped) = chars.next() {
                value.push(escaped);
            }
        } else {
            value.push(ch);
        }
    }
    value
}

pub(crate) fn format_column_reference(value: &str) -> String {
    if is_simple_column_name(value) && !KEYWORDS.contains(&value) {
        return value.to_string();
    }
    let escaped = value.replace('\\', "\\\\").replace('`', "\\`");
    format!("`{escaped}`")
}

fn is_simple_column_name(value: &str) -> bool {
    let mut chars = value.chars();
    chars.next().is_some_and(is_ident_start) && chars.all(is_ident_char)
}

fn is_ident_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

fn is_ident_start(ch: char) -> bool {
    ch.is_ascii_alphabetic() || ch == '_'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_use_utf16_columns() {
        let source = "a\n😀x";

        assert_eq!(
            position_for_byte_offset(source, source.find('x').expect("x offset")),
            TextPosition {
                line: 1,
                character: 2
            }
        );
        assert_eq!(
            byte_offset_for_position(
                source,
                TextPosition {
                    line: 1,
                    character: 2
                }
            ),
            source.find('x').expect("x offset")
        );
    }

    #[test]
    fn diagnostics_map_byte_spans_to_utf16_ranges_after_non_ascii_text() {
        let source = "load \"😀.csv\"\n  | select missing";
        let start = source.find("missing").expect("diagnostic span");
        let diagnostics = diagnostics_for_editor(
            source,
            &[Diagnostic::error(
                pdl_core::codes::E1005,
                "unknown column `missing`",
                Span::new(start, start + "missing".len()),
            )],
        );

        assert_eq!(diagnostics[0].range.start.line, 1);
        assert_eq!(diagnostics[0].range.start.character, 11);
    }

    #[test]
    fn host_schema_diagnostics_report_unknown_filter_columns() {
        let source = r#"load "sales.csv"
  | filter sttus == "completed"
  | group_by region
  | agg total_revenue = sum(amount), avg_age = mean(customer_age), orders = count()
  | sort total_revenue desc
  | limit 3"#;

        let schemas = [(
            PathBuf::from("memory/sales.csv"),
            vec![
                "region".to_string(),
                "status".to_string(),
                "amount".to_string(),
                "customer_age".to_string(),
            ],
        )];
        let document = analyze_document_with_schemas(source, Path::new("memory/main.pdl"), schemas);

        let diagnostic = document
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code == "E1005")
            .expect("unknown column diagnostic");
        assert_eq!(diagnostic.message, "unknown column `sttus`");
        assert_eq!(diagnostic.range.start.line, 1);
        assert_eq!(diagnostic.range.start.character, 11);

        let corrected = source.replace("sttus", "status");
        let corrected_document = analyze_document_with_schemas(
            &corrected,
            Path::new("memory/main.pdl"),
            [(
                PathBuf::from("memory/sales.csv"),
                vec![
                    "region".to_string(),
                    "status".to_string(),
                    "amount".to_string(),
                    "customer_age".to_string(),
                ],
            )],
        );
        assert!(
            !corrected_document
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "E1005"),
            "{:?}",
            corrected_document.diagnostics
        );
    }

    #[test]
    fn formats_pipeline_style() {
        let source = r#"load "sales.csv"|filter status=="completed"|agg total = sum(amount)"#;

        assert_eq!(
            pdl_syntax::format_source(source).expect("formatted"),
            r#"load "sales.csv"
  | filter status == "completed"
  | agg total = sum(amount)"#
        );
    }

    #[test]
    fn provides_stage_completion_after_pipe() {
        let source = "load \"sales.csv\"\n  | f";

        let items = completions(
            source,
            None,
            TextPosition {
                line: 1,
                character: 5,
            },
        );

        assert!(items.iter().any(|item| item.label == "filter"));
    }

    #[test]
    fn provides_binding_completion_at_join_source() {
        let source = r#"let customers =
  load "customers.csv"

load "sales.csv"
  | join "#;

        let items = completions(source, None, position_for_byte_offset(source, source.len()));

        assert!(items
            .iter()
            .any(|item| { item.label == "customers" && item.kind == CompletionKind::Binding }));
    }

    #[test]
    fn provides_window_frame_name_completions_after_frame() {
        let source = r#"load "sales.csv"
  | mutate running_amount = sum(amount) over (partition_by region order_by amount frame )"#;
        let offset = source.find("frame ").expect("frame keyword") + "frame ".len();

        let items = completions(source, None, position_for_byte_offset(source, offset));

        for name in [
            "whole_partition",
            "running",
            "remaining",
            "trailing",
            "leading",
            "centered",
        ] {
            assert!(
                items.iter().any(|item| item.label == name),
                "missing frame name completion `{name}`: {items:?}"
            );
        }
    }

    #[test]
    fn provides_join_kind_completions_after_kind() {
        let source = r#"let customers =
  load "customers.csv"

load "sales.csv"
  | join customers on customer_id kind "#;

        let items = completions(source, None, position_for_byte_offset(source, source.len()));

        assert!(items.iter().any(|item| item.label == "left"));
        assert!(items.iter().any(|item| item.label == "anti"));
    }

    #[test]
    fn semantic_tokens_use_source_offsets() {
        let source = r#"load "sales.csv"
  | filter status == "completed"
  | group_by `group`
  | agg total_revenue = sum(amount), avg_age = mean(customer_age), orders = count()
  | sort total_revenue desc
  | limit 3"#;
        let tokens = semantic_tokens(source);

        for text in ["\"sales.csv\"", "\"completed\""] {
            let start = source.find(text).expect("sample string");
            let end = start + text.len();
            assert!(
                tokens.iter().any(|token| {
                    token.token_type == SemanticTokenKind::String
                        && token.range == range_for_span(source, Span::new(start, end))
                }),
                "missing intact semantic token for {text}"
            );
        }
        for text in ["status", "`group`"] {
            let start = source.find(text).expect("sample variable");
            let end = start + text.len();
            assert!(
                tokens.iter().any(|token| {
                    token.token_type == SemanticTokenKind::ColumnReference
                        && token.range == range_for_span(source, Span::new(start, end))
                }),
                "missing intact semantic token for {text}"
            );
        }
        let start = source.rfind("total_revenue").expect("sort key");
        let end = start + "total_revenue".len();
        assert!(
            tokens.iter().any(|token| {
                token.token_type == SemanticTokenKind::ColumnReference
                    && token.range == range_for_span(source, Span::new(start, end))
            }),
            "missing intact semantic token for total_revenue sort key"
        );
    }

    #[test]
    fn semantic_tokens_classify_bindings_and_columns_from_parsed_structure() {
        let source = r#"let cleaned =
  load "orders_raw.csv"
  | filter lower(trim(status)) == "completed"
  | mutate
      net_amount = gross_amount - coalesce(discount, 0),
      region_channel = concat(upper(trim(region)), ":", lower(trim(channel)))
  | distinct order_id

cleaned
  | group_by region_channel
  | agg orders = count(), revenue = sum(net_amount)
  | sort revenue desc"#;
        let tokens = semantic_tokens(source);

        assert_token_kind(
            source,
            &tokens,
            source.find("cleaned").expect("binding declaration"),
            "cleaned",
            SemanticTokenKind::BindingDeclaration,
        );
        assert_token_kind(
            source,
            &tokens,
            source.rfind("cleaned").expect("binding reference"),
            "cleaned",
            SemanticTokenKind::BindingReference,
        );

        for name in ["net_amount", "region_channel"] {
            assert_token_kind(
                source,
                &tokens,
                source.find(name).expect("column definition"),
                name,
                SemanticTokenKind::ColumnDefinition,
            );
        }
        for name in ["orders", "revenue"] {
            assert_token_kind(
                source,
                &tokens,
                source.find(&format!("{name} =")).expect("aggregate alias"),
                name,
                SemanticTokenKind::ColumnDefinition,
            );
        }

        for name in ["status", "gross_amount", "discount", "order_id"] {
            assert_token_kind(
                source,
                &tokens,
                source.find(name).expect("column reference"),
                name,
                SemanticTokenKind::ColumnReference,
            );
        }
        assert_token_kind(
            source,
            &tokens,
            source.find("trim(region)").expect("region reference") + "trim(".len(),
            "region",
            SemanticTokenKind::ColumnReference,
        );
        assert_token_kind(
            source,
            &tokens,
            source.find("trim(channel)").expect("channel reference") + "trim(".len(),
            "channel",
            SemanticTokenKind::ColumnReference,
        );
        for name in ["region_channel", "net_amount", "revenue"] {
            assert_token_kind(
                source,
                &tokens,
                source.rfind(name).expect("later column reference"),
                name,
                SemanticTokenKind::ColumnReference,
            );
        }
    }

    #[test]
    fn semantic_tokens_classify_stage_specific_column_positions() {
        let source = r#"let customers =
  load "customers.csv"

load "orders.csv"
  | join customers on (customer_id, id) kind left
  | mutate row_num = row_number() over (partition_by region order_by order_date desc frame running)
  | select final_region = region
  | rename customer_key = id
  | pivot_longer jan, feb names_to month values_to amount
  | complete final_region fill amount = coalesce(amount, 0)"#;
        let tokens = semantic_tokens(source);

        assert_token_kind(
            source,
            &tokens,
            source.find("customers on").expect("join binding source"),
            "customers",
            SemanticTokenKind::BindingReference,
        );

        for name in ["row_num", "final_region", "customer_key", "month", "amount"] {
            assert_token_kind(
                source,
                &tokens,
                source.find(name).expect("column definition"),
                name,
                SemanticTokenKind::ColumnDefinition,
            );
        }

        assert_token_kind(
            source,
            &tokens,
            source.find("customer_id").expect("join left key"),
            "customer_id",
            SemanticTokenKind::ColumnReference,
        );
        assert_token_kind(
            source,
            &tokens,
            source.rfind("= id").expect("rename source") + "= ".len(),
            "id",
            SemanticTokenKind::ColumnReference,
        );
        assert_token_kind(
            source,
            &tokens,
            source
                .find("partition_by region")
                .expect("window partition")
                + "partition_by ".len(),
            "region",
            SemanticTokenKind::ColumnReference,
        );
        assert_token_kind(
            source,
            &tokens,
            source.find("order_by order_date").expect("window order") + "order_by ".len(),
            "order_date",
            SemanticTokenKind::ColumnReference,
        );
        for name in ["jan", "feb"] {
            assert_token_kind(
                source,
                &tokens,
                source.find(name).expect("pivot source"),
                name,
                SemanticTokenKind::ColumnReference,
            );
        }
        assert_token_kind(
            source,
            &tokens,
            source.rfind("complete final_region").expect("complete key") + "complete ".len(),
            "final_region",
            SemanticTokenKind::ColumnReference,
        );
        assert_token_kind(
            source,
            &tokens,
            source.rfind("amount").expect("complete fill expression"),
            "amount",
            SemanticTokenKind::ColumnReference,
        );
    }

    #[test]
    fn context_bindings_have_editor_features() {
        let source = r#"param metric_column = "revenue"
state selected_zone = "Downtown"

load "trips.csv"
  | filter zone == @selected_zone
  | group_by $metric_column"#;

        let completions = completions(
            source,
            None,
            position_for_byte_offset(
                source,
                source.find("$metric").expect("context reference") + 1,
            ),
        );
        assert!(completions.iter().any(|item| {
            item.label == "$metric_column" && item.kind == CompletionKind::Context
        }));
        assert!(!completions
            .iter()
            .any(|item| item.label == "@selected_zone"));

        let tokens = semantic_tokens(source);
        assert_token_kind(
            source,
            &tokens,
            source.find("metric_column").expect("parameter declaration"),
            "metric_column",
            SemanticTokenKind::ContextDeclaration,
        );
        assert_token_kind(
            source,
            &tokens,
            source.find("@selected_zone").expect("state reference"),
            "@selected_zone",
            SemanticTokenKind::ContextReference,
        );
        assert_token_kind(
            source,
            &tokens,
            source.find("$metric_column").expect("parameter reference"),
            "$metric_column",
            SemanticTokenKind::ContextReference,
        );

        let symbols = document_symbols(source);
        assert!(symbols
            .iter()
            .any(|symbol| symbol.name == "$metric_column"
                && symbol.kind == DocumentSymbolKind::Context));

        let edits = rename_binding_edits(
            source,
            position_for_byte_offset(
                source,
                source.find("@selected_zone").expect("state ref") + 2,
            ),
            "active_zone",
        );
        assert_eq!(edits.len(), 2);
        assert!(edits.iter().all(|edit| edit.new_text == "active_zone"));
    }

    #[test]
    fn binding_document_symbol_selection_range_is_inside_full_range() {
        let source = r#"let cleaned =
  load "orders_raw.csv"
  | filter lower(trim(status)) == "completed"
  | mutate net_amount = gross_amount - coalesce(discount, 0), region_channel = concat(upper(trim(region)), ":", lower(trim(channel)))
  | distinct order_id

cleaned
  | group_by region_channel
  | agg orders = count(), revenue = sum(net_amount)
  | sort revenue desc"#;

        let symbols = document_symbols(source);
        let cleaned = symbols
            .iter()
            .find(|symbol| symbol.name == "cleaned")
            .expect("cleaned binding symbol");

        assert!(
            range_contains(cleaned.range, cleaned.selection_range),
            "binding range {:?} must contain selection range {:?}",
            cleaned.range,
            cleaned.selection_range
        );
    }

    fn range_contains(outer: TextRange, inner: TextRange) -> bool {
        position_lte(outer.start, inner.start) && position_lte(inner.end, outer.end)
    }

    fn position_lte(left: TextPosition, right: TextPosition) -> bool {
        left.line < right.line || (left.line == right.line && left.character <= right.character)
    }

    fn assert_token_kind(
        source: &str,
        tokens: &[EditorSemanticToken],
        start: usize,
        text: &str,
        token_type: SemanticTokenKind,
    ) {
        let span = Span::new(start, start + text.len());
        assert!(
            tokens.iter().any(|token| {
                token.token_type == token_type && token.range == range_for_span(source, span)
            }),
            "missing {token_type:?} semantic token for {text:?} at {span:?}"
        );
    }
}
