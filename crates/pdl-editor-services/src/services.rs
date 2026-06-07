use pdl_core::{Diagnostic, Severity, Span};
use pdl_semantics::registry::{
    AGGREGATE_FUNCTIONS, FORMATS, KEYWORDS, SCALAR_FUNCTIONS, STAGES, WINDOW_FUNCTIONS,
};
use pdl_semantics::{analyze_program, FormatInfo, FunctionInfo, LoadRequest, StageInfo};
use pdl_syntax::{
    decode_context_column_ref, ContextKind, Expr, JoinKind, ParseResult, Pipeline, PipelineStart,
    Program, Stage,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::diagnostics::diagnostics_for_editor;

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

#[derive(Clone, Debug)]
struct CompletionContext {
    after_pipe: bool,
    in_pipeline_start: bool,
    in_format_context: bool,
    in_join_or_union_source_context: bool,
    in_join_kind_keyword_context: bool,
    in_join_kind_name_context: bool,
    in_agg_function_context: bool,
    in_scalar_function_context: bool,
    in_mutate_context: bool,
    in_sort_direction_context: bool,
    in_column_context: bool,
    context_reference_kind: Option<ContextKind>,
    inside_string: bool,
}

impl CompletionContext {
    fn new(source: &str, offset: usize, program: &Program) -> Self {
        let line_start = source[..offset].rfind('\n').map_or(0, |index| index + 1);
        let line_prefix = &source[line_start..offset];
        let (word_start, _, word) = current_word(source, offset);
        let after_pipe = line_prefix
            .trim_start()
            .strip_prefix('|')
            .is_some_and(|rest| rest.trim().chars().all(is_ident_char));
        let context_reference_kind = context_reference_kind_before_word(source, word_start);
        let in_pipeline_start = source[..offset].trim().is_empty()
            || line_prefix.trim().is_empty()
            || source[..offset].trim_end().ends_with('=');
        let lower_prefix = line_prefix.to_ascii_lowercase();
        let in_format_context = lower_prefix
            .rsplit_once("format")
            .is_some_and(|(_, suffix)| suffix.trim().chars().all(is_ident_or_quote_char));
        let stage =
            stage_name_for_offset(program, offset).or_else(|| stage_name_from_line(line_prefix));
        let inside_string = inside_string_on_line(line_prefix);
        let after_keyword = stage.as_ref().and_then(|stage| {
            lower_prefix
                .rsplit_once(stage)
                .map(|(_, suffix)| suffix.trim())
        });
        let in_join_or_union_source_context = matches!(stage.as_deref(), Some("join" | "union"))
            && !inside_string
            && after_keyword.is_some_and(|suffix| {
                !suffix.contains(" on ")
                    && !suffix.contains(" by_name")
                    && !suffix.contains(" distinct")
                    && suffix.chars().all(is_ident_char)
            });
        let in_join_kind_name_context = stage.as_deref() == Some("join")
            && !inside_string
            && lower_prefix
                .rsplit_once("kind")
                .is_some_and(|(_, suffix)| suffix.trim().chars().all(is_ident_char));
        let in_join_kind_keyword_context = stage.as_deref() == Some("join")
            && !inside_string
            && !in_join_kind_name_context
            && after_keyword
                .is_some_and(|suffix| suffix.contains(" on ") && !suffix.contains(" kind"));
        let in_agg_function_context = stage.as_deref() == Some("agg")
            && !inside_string
            && after_keyword.is_some_and(|suffix| !suffix.contains('(') || suffix.ends_with(','));
        let in_mutate_context = stage.as_deref() == Some("mutate");
        let in_scalar_function_context =
            matches!(stage.as_deref(), Some("filter" | "mutate" | "complete"))
                && !inside_string
                && word.chars().all(is_ident_char);
        let current_sort_item = after_keyword
            .map(|suffix| suffix.rsplit(',').next().unwrap_or("").trim_start())
            .unwrap_or("");
        let in_sort_direction_context = stage.as_deref() == Some("sort")
            && !inside_string
            && !current_sort_item.is_empty()
            && (current_sort_item.split_whitespace().count() > 1
                || current_sort_item
                    .chars()
                    .next_back()
                    .is_some_and(char::is_whitespace))
            && word.chars().all(is_ident_char);
        let in_column_context = matches!(
            stage.as_deref(),
            Some(
                "filter"
                    | "select"
                    | "drop"
                    | "rename"
                    | "mutate"
                    | "group_by"
                    | "agg"
                    | "sort"
                    | "join"
                    | "distinct"
                    | "pivot_longer"
                    | "complete"
            )
        );

        Self {
            after_pipe,
            in_pipeline_start,
            in_format_context,
            in_join_or_union_source_context,
            in_join_kind_keyword_context,
            in_join_kind_name_context,
            in_agg_function_context,
            in_scalar_function_context,
            in_mutate_context,
            in_sort_direction_context,
            in_column_context,
            context_reference_kind: if inside_string {
                None
            } else {
                context_reference_kind
            },
            inside_string,
        }
    }
}

fn optimistic_columns(program: &Program) -> Vec<String> {
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
            if contains(binding.pipeline.span, offset) {
                return self.pipeline_schema_before_offset(&binding.pipeline, offset);
            }
        }
        for output in &program.outputs {
            if contains(output.pipeline.span, offset) {
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

    fn pipeline_schema(&self, pipeline: &Pipeline) -> Option<SchemaState> {
        let mut schema = self.pipeline_start_schema(pipeline)?;
        for stage in &pipeline.stages {
            apply_stage_to_schema(self, &mut schema, stage);
        }
        Some(schema)
    }

    fn pipeline_start_schema(&self, pipeline: &Pipeline) -> Option<SchemaState> {
        match &pipeline.start {
            PipelineStart::Load(_) => None,
            PipelineStart::Binding(name) => self
                .bindings
                .get(&name.value)
                .and_then(|binding| binding.schema.clone()),
        }
    }
}

fn apply_stage_to_schema(facts: &DocumentFacts, schema: &mut SchemaState, stage: &Stage) {
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

fn pipeline_stage_symbols(source: &str, pipeline: &Pipeline) -> Vec<EditorDocumentSymbol> {
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

fn context_name_spans(source: &str, program: &Program, kind: ContextKind, name: &str) -> Vec<Span> {
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

fn is_context_reference_span(source: &str, span: Span) -> bool {
    source
        .get(span.start..span.end)
        .is_some_and(|text| text.starts_with('$') || text.starts_with('@'))
}

fn binding_name_at_offset(program: &Program, offset: usize) -> Option<String> {
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

fn binding_spans(program: &Program, name: &str) -> Vec<Span> {
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

fn current_word(source: &str, offset: usize) -> (usize, usize, &str) {
    let mut start = offset;
    while start > 0 {
        let Some(ch) = source[..start].chars().next_back() else {
            break;
        };
        if !is_ident_char(ch) {
            break;
        }
        start -= ch.len_utf8();
    }
    let mut end = offset;
    while end < source.len() {
        let Some(ch) = source[end..].chars().next() else {
            break;
        };
        if !is_ident_char(ch) {
            break;
        }
        end += ch.len_utf8();
    }
    (start, end, &source[start..end])
}

fn context_reference_kind_before_word(source: &str, word_start: usize) -> Option<ContextKind> {
    match source[..word_start].chars().next_back()? {
        '$' => Some(ContextKind::Param),
        '@' => Some(ContextKind::State),
        _ => None,
    }
}

fn inside_string_on_line(line_prefix: &str) -> bool {
    let mut escaped = false;
    let mut quote_count = 0usize;
    for ch in line_prefix.chars() {
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            quote_count += 1;
        }
    }
    quote_count % 2 == 1
}

fn stage_name_for_offset(program: &Program, offset: usize) -> Option<String> {
    program
        .bindings
        .iter()
        .find_map(|binding| stage_name_for_pipeline_offset(&binding.pipeline, offset))
        .or_else(|| {
            program
                .outputs
                .iter()
                .find_map(|output| stage_name_for_pipeline_offset(&output.pipeline, offset))
        })
        .or_else(|| {
            program
                .main
                .as_ref()
                .and_then(|pipeline| stage_name_for_pipeline_offset(pipeline, offset))
        })
}

fn stage_name_for_pipeline_offset(pipeline: &Pipeline, offset: usize) -> Option<String> {
    if let PipelineStart::Load(load) = &pipeline.start {
        if contains(load.span, offset) {
            return Some("load".to_string());
        }
    }
    pipeline
        .stages
        .iter()
        .find(|stage| contains(stage.span(), offset))
        .map(|stage| stage_name(stage).to_string())
}

fn stage_name_from_line(line_prefix: &str) -> Option<String> {
    let trimmed = line_prefix.trim_start().strip_prefix('|')?.trim_start();
    let name: String = trimmed
        .chars()
        .take_while(|ch| is_ident_char(*ch))
        .collect();
    (!name.is_empty()).then_some(name)
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

fn column_completions(columns: &[String], inside_string: bool) -> Vec<EditorCompletion> {
    columns
        .iter()
        .map(|column| EditorCompletion {
            label: column.clone(),
            insert_text: if inside_string {
                column.clone()
            } else {
                format_column_reference(column)
            },
            detail: "column".to_string(),
            kind: CompletionKind::Column,
        })
        .collect()
}

fn context_completions(facts: &DocumentFacts, kind: ContextKind) -> Vec<EditorCompletion> {
    facts
        .contexts
        .iter()
        .filter(|(_, context)| context.kind == kind)
        .map(|(name, context)| EditorCompletion {
            label: context_symbol_name(kind, name),
            insert_text: name.clone(),
            detail: context.detail.clone(),
            kind: CompletionKind::Context,
        })
        .collect()
}

fn stage_completion(info: &StageInfo) -> EditorCompletion {
    EditorCompletion {
        label: info.name.to_string(),
        insert_text: info.name.to_string(),
        detail: info.documentation.to_string(),
        kind: CompletionKind::Stage,
    }
}

fn function_completion(info: &FunctionInfo) -> EditorCompletion {
    EditorCompletion {
        label: info.name.to_string(),
        insert_text: info.name.to_string(),
        detail: info.documentation.to_string(),
        kind: CompletionKind::Function,
    }
}

fn format_completion(info: &FormatInfo) -> EditorCompletion {
    EditorCompletion {
        label: info.name.to_string(),
        insert_text: info.name.to_string(),
        detail: info.documentation.to_string(),
        kind: CompletionKind::Format,
    }
}

fn binding_completion(name: &str, binding: &BindingFact) -> EditorCompletion {
    let detail = binding.schema.as_ref().map_or_else(
        || "binding".to_string(),
        |schema| format!("binding: {}", schema.columns.join(", ")),
    );
    EditorCompletion {
        label: name.to_string(),
        insert_text: name.to_string(),
        detail,
        kind: CompletionKind::Binding,
    }
}

fn keyword_completion(label: &str, detail: &str) -> EditorCompletion {
    EditorCompletion {
        label: label.to_string(),
        insert_text: label.to_string(),
        detail: detail.to_string(),
        kind: CompletionKind::Keyword,
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

fn dedupe_completions(items: Vec<EditorCompletion>) -> Vec<EditorCompletion> {
    let mut seen = BTreeSet::new();
    items
        .into_iter()
        .filter(|item| !item.label.is_empty() && seen.insert(item.label.clone()))
        .collect()
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

fn format_column_reference(value: &str) -> String {
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

fn is_ident_or_quote_char(ch: char) -> bool {
    is_ident_char(ch) || ch == '"' || ch.is_whitespace()
}

fn is_valid_binding_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(is_ident_start)
        && chars.all(is_ident_char)
        && !KEYWORDS.contains(&name)
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
  | mutate row_num = row_number() over (partition_by region order_by order_date desc rows between unbounded_preceding and current_row)
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
