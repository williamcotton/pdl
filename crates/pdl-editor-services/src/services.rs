use pdl_core::{Diagnostic, Severity, Span};
use pdl_semantics::registry::{AGGREGATE_FUNCTIONS, FORMATS, KEYWORDS, STAGES};
use pdl_semantics::{
    aggregate_function, analyze_program, format_info, stage_info, FormatInfo, FunctionInfo,
    StageInfo,
};
use pdl_syntax::{Expr, Pipeline, PipelineStart, Program, SaveStage, Stage};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

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
    Function,
    Stage,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EditorLocation {
    pub range: TextRange,
}

pub fn analyze_document(source: &str, _path: Option<&Path>) -> EditorDocument {
    let parse = pdl_syntax::parse(source);
    let mut diagnostics = parse.diagnostics.clone();
    let optimistic_schema = optimistic_columns(&parse.program);
    let analysis = analyze_program(&parse.program, |request| {
        let _ = request;
        Ok(optimistic_schema.clone())
    });
    diagnostics.extend(analysis.diagnostics);
    EditorDocument {
        diagnostics: diagnostics_for_editor(source, &diagnostics),
    }
}

pub fn diagnostics_for_editor(source: &str, diagnostics: &[Diagnostic]) -> Vec<EditorDiagnostic> {
    diagnostics
        .iter()
        .map(|diagnostic| EditorDiagnostic {
            range: range_for_span(source, diagnostic.span),
            severity: diagnostic.severity,
            code: diagnostic.code.to_string(),
            message: diagnostic.message.clone(),
        })
        .collect()
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
    } else if context.in_agg_function_context {
        completions.extend(AGGREGATE_FUNCTIONS.iter().map(function_completion));
    } else if context.in_sort_direction_context {
        completions.extend([
            keyword_completion("asc", "Sort ascending"),
            keyword_completion("desc", "Sort descending"),
            keyword_completion("nulls_first", "Place nulls before non-null values"),
            keyword_completion("nulls_last", "Place nulls after non-null values"),
        ]);
    } else if context.in_column_context {
        completions.extend(column_completions(&schema, context.inside_string));
    }

    if completions.is_empty() && context.inside_string {
        completions.extend(column_completions(&schema, true));
    }

    dedupe_completions(completions)
}

pub fn hover(source: &str, _path: Option<&Path>, position: TextPosition) -> Option<EditorHover> {
    let offset = byte_offset_for_position(source, position);
    let parse = pdl_syntax::parse(source);
    let facts = DocumentFacts::new(&parse.program);

    for binding in parse.program.bindings.iter() {
        if contains(binding.name.span, offset) {
            let schema = facts.bindings.get(&binding.name.value).and_then(|binding| {
                binding
                    .schema
                    .as_ref()
                    .map(|schema| schema.columns.join(", "))
            });
            let schema = schema.unwrap_or_else(|| "unknown".to_string());
            return Some(EditorHover {
                range: range_for_span(source, binding.name.span),
                markdown: format!("**binding `{}`**\n\nSchema: `{schema}`", binding.name.value),
            });
        }
    }

    hover_pipeline(source, &parse.program, &facts, offset)
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
        } else if ch.is_ascii_digit() {
            let end = scan_number(source, pos);
            push_semantic_token(source, &mut tokens, pos, end, SemanticTokenKind::Number);
            pos = end;
        } else if is_ident_start(ch) {
            let end = scan_identifier(source, pos);
            let text = &source[pos..end];
            let token_type = if KEYWORDS.contains(&text) {
                SemanticTokenKind::Keyword
            } else if AGGREGATE_FUNCTIONS.iter().any(|info| info.name == text) {
                SemanticTokenKind::Function
            } else {
                SemanticTokenKind::Variable
            };
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

pub fn document_symbols(source: &str) -> Vec<EditorDocumentSymbol> {
    let parse = pdl_syntax::parse(source);
    let mut symbols = Vec::new();
    for binding in &parse.program.bindings {
        symbols.push(EditorDocumentSymbol {
            name: binding.name.value.clone(),
            detail: "binding".to_string(),
            kind: DocumentSymbolKind::Binding,
            range: range_for_span(source, binding.pipeline.span),
            selection_range: range_for_span(source, binding.name.span),
            children: pipeline_stage_symbols(source, &binding.pipeline),
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
    in_agg_function_context: bool,
    in_sort_direction_context: bool,
    in_column_context: bool,
    inside_string: bool,
}

impl CompletionContext {
    fn new(source: &str, offset: usize, program: &Program) -> Self {
        let line_start = source[..offset].rfind('\n').map_or(0, |index| index + 1);
        let line_prefix = &source[line_start..offset];
        let after_pipe = line_prefix
            .trim_start()
            .strip_prefix('|')
            .is_some_and(|rest| rest.trim().chars().all(is_ident_char));
        let word = current_word(source, offset).2;
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
        let in_agg_function_context = stage.as_deref() == Some("agg")
            && !inside_string
            && after_keyword.is_some_and(|suffix| !suffix.contains('(') || suffix.ends_with(','));
        let in_sort_direction_context = stage.as_deref() == Some("sort")
            && !inside_string
            && line_prefix.contains('"')
            && word.chars().all(is_ident_char);
        let in_column_context = matches!(
            stage.as_deref(),
            Some("filter" | "select" | "drop" | "rename" | "group_by" | "agg" | "sort")
        );

        Self {
            after_pipe,
            in_pipeline_start,
            in_format_context,
            in_agg_function_context,
            in_sort_direction_context,
            in_column_context,
            inside_string,
        }
    }
}

fn optimistic_columns(program: &Program) -> Vec<String> {
    let mut columns = BTreeSet::new();
    for binding in &program.bindings {
        collect_pipeline_columns(&binding.pipeline, &mut columns);
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
            Stage::Limit { .. } | Stage::Save(_) | Stage::Unsupported { .. } => {}
        }
    }
}

fn collect_expr_columns(expr: &Expr, columns: &mut BTreeSet<String>) {
    match expr {
        Expr::Quoted(value) => {
            columns.insert(value.value.clone());
        }
        Expr::Ident(value) => {
            columns.insert(value.value.clone());
        }
        Expr::Call { args, .. } => {
            for arg in args {
                collect_expr_columns(arg, columns);
            }
        }
        Expr::Unary { expr, .. } => collect_expr_columns(expr, columns),
        Expr::Binary { left, right, .. } => {
            collect_expr_columns(left, columns);
            collect_expr_columns(right, columns);
        }
        Expr::Number(_) | Expr::Bool(_) | Expr::Null(_) => {}
    }
}

#[derive(Clone, Debug)]
struct DocumentFacts {
    bindings: BTreeMap<String, BindingFact>,
}

#[derive(Clone, Debug)]
struct BindingFact {
    schema: Option<SchemaState>,
}

#[derive(Clone, Debug)]
struct SchemaState {
    columns: Vec<String>,
    grouping: Option<Vec<String>>,
}

impl DocumentFacts {
    fn new(program: &Program) -> Self {
        let mut facts = Self {
            bindings: BTreeMap::new(),
        };
        for binding in &program.bindings {
            let schema = facts.pipeline_schema(&binding.pipeline);
            facts
                .bindings
                .insert(binding.name.value.clone(), BindingFact { schema });
        }
        facts
    }

    fn schema_before_offset(&self, program: &Program, offset: usize) -> Option<Vec<String>> {
        for binding in &program.bindings {
            if contains(binding.pipeline.span, offset) {
                return self.pipeline_schema_before_offset(&binding.pipeline, offset);
            }
        }
        if let Some(main) = &program.main {
            return self.pipeline_schema_before_offset(main, offset);
        }
        None
    }

    fn pipeline_schema_before_offset(
        &self,
        pipeline: &Pipeline,
        offset: usize,
    ) -> Option<Vec<String>> {
        let mut schema = self.pipeline_start_schema(pipeline)?;
        for stage in &pipeline.stages {
            if offset <= stage.span().end {
                return Some(schema.columns);
            }
            apply_stage_to_schema(&mut schema, stage);
        }
        Some(schema.columns)
    }

    fn pipeline_schema(&self, pipeline: &Pipeline) -> Option<SchemaState> {
        let mut schema = self.pipeline_start_schema(pipeline)?;
        for stage in &pipeline.stages {
            apply_stage_to_schema(&mut schema, stage);
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

fn apply_stage_to_schema(schema: &mut SchemaState, stage: &Stage) {
    match stage {
        Stage::Filter { .. } | Stage::Sort { .. } | Stage::Limit { .. } | Stage::Save(_) => {}
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
        Stage::GroupBy { columns, .. } => {
            schema.grouping = Some(columns.iter().map(|column| column.value.clone()).collect());
        }
        Stage::Agg { items, .. } => {
            let mut output = schema.grouping.take().unwrap_or_default();
            output.extend(items.iter().map(|item| item.alias.value.clone()));
            schema.columns = output;
        }
        Stage::Unsupported { .. } => {}
    }
}

fn hover_pipeline(
    source: &str,
    program: &Program,
    facts: &DocumentFacts,
    offset: usize,
) -> Option<EditorHover> {
    for binding in &program.bindings {
        if let Some(hover) = hover_for_pipeline(source, &binding.pipeline, facts, offset) {
            return Some(hover);
        }
    }
    program
        .main
        .as_ref()
        .and_then(|pipeline| hover_for_pipeline(source, pipeline, facts, offset))
}

fn hover_for_pipeline(
    source: &str,
    pipeline: &Pipeline,
    facts: &DocumentFacts,
    offset: usize,
) -> Option<EditorHover> {
    if let PipelineStart::Binding(name) = &pipeline.start {
        if contains(name.span, offset) {
            return facts.bindings.get(&name.value).map(|binding| {
                let schema = binding
                    .schema
                    .as_ref()
                    .map_or_else(|| "unknown".to_string(), |schema| schema.columns.join(", "));
                EditorHover {
                    range: range_for_span(source, name.span),
                    markdown: format!("**binding `{}`**\n\nSchema: `{schema}`", name.value),
                }
            });
        }
    }

    if let PipelineStart::Load(load) = &pipeline.start {
        let load_span = Span::new(load.span.start, load.span.start + "load".len());
        if contains(load_span, offset) {
            return Some(info_hover(
                source,
                load_span,
                "load",
                stage_info("load")?.documentation,
            ));
        }
        if let Some(format) = &load.format {
            if contains(format.span, offset) {
                return Some(info_hover(
                    source,
                    format.span,
                    &format.value,
                    format_documentation(&format.value),
                ));
            }
        }
    }

    for stage in &pipeline.stages {
        let stage_name = stage_name(stage);
        let keyword_span = Span::new(stage.span().start, stage.span().start + stage_name.len());
        if contains(keyword_span, offset) {
            return stage_info(stage_name)
                .map(|info| info_hover(source, keyword_span, info.name, info.documentation));
        }
        if let Some(hover) = hover_stage_detail(source, facts, pipeline, stage, offset) {
            return Some(hover);
        }
    }

    None
}

fn hover_stage_detail(
    source: &str,
    facts: &DocumentFacts,
    pipeline: &Pipeline,
    stage: &Stage,
    offset: usize,
) -> Option<EditorHover> {
    let schema = facts
        .pipeline_schema_before_offset(pipeline, stage.span().start)
        .unwrap_or_default();
    for span in column_spans(stage) {
        if contains(span, offset) {
            let text = unquoted_text_at_span(source, span).unwrap_or_default();
            let known = schema.iter().any(|column| column == &text);
            let detail = if known {
                "Schema column from the current table."
            } else {
                "Column reference. The schema is unknown or this column has a diagnostic."
            };
            return Some(EditorHover {
                range: range_for_span(source, span),
                markdown: format!("**column `{text}`**\n\n{detail}"),
            });
        }
    }

    if let Stage::Agg { items, .. } = stage {
        for item in items {
            if contains(item.function.span, offset) {
                if let Some(info) = aggregate_function(&item.function.value) {
                    return Some(info_hover(
                        source,
                        item.function.span,
                        info.name,
                        info.documentation,
                    ));
                }
            }
        }
    }

    if let Stage::Save(save) = stage {
        if let Some(hover) = hover_save_format(source, save, offset) {
            return Some(hover);
        }
    }

    None
}

fn hover_save_format(source: &str, save: &SaveStage, offset: usize) -> Option<EditorHover> {
    let format = save.format.as_ref()?;
    contains(format.span, offset).then(|| {
        info_hover(
            source,
            format.span,
            &format.value,
            format_documentation(&format.value),
        )
    })
}

fn info_hover(source: &str, span: Span, name: &str, documentation: &str) -> EditorHover {
    EditorHover {
        range: range_for_span(source, span),
        markdown: format!("**{name}**\n\n{documentation}"),
    }
}

fn column_spans(stage: &Stage) -> Vec<Span> {
    match stage {
        Stage::Filter { expr, .. } => expr_column_spans(expr),
        Stage::Select { items, .. } => items
            .iter()
            .flat_map(|item| {
                item.alias
                    .iter()
                    .map(|alias| alias.span)
                    .chain([item.column.span])
            })
            .collect(),
        Stage::Drop { columns, .. } | Stage::GroupBy { columns, .. } => {
            columns.iter().map(|column| column.span).collect()
        }
        Stage::Rename { items, .. } => items
            .iter()
            .flat_map(|item| [item.old.span, item.new.span])
            .collect(),
        Stage::Agg { items, .. } => items
            .iter()
            .flat_map(|item| {
                item.args
                    .iter()
                    .flat_map(expr_column_spans)
                    .chain([item.alias.span])
            })
            .collect(),
        Stage::Sort { items, .. } => items.iter().map(|item| item.column.span).collect(),
        Stage::Limit { .. } | Stage::Save(_) | Stage::Unsupported { .. } => Vec::new(),
    }
}

fn expr_column_spans(expr: &Expr) -> Vec<Span> {
    match expr {
        Expr::Quoted(value) => vec![value.span],
        Expr::Call { args, .. } => args.iter().flat_map(expr_column_spans).collect(),
        Expr::Unary { expr, .. } => expr_column_spans(expr),
        Expr::Binary { left, right, .. } => {
            let mut spans = expr_column_spans(left);
            spans.extend(expr_column_spans(right));
            spans
        }
        Expr::Number(_) | Expr::Bool(_) | Expr::Null(_) | Expr::Ident(_) => Vec::new(),
    }
}

fn quote(value: &str) -> String {
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t");
    format!("\"{escaped}\"")
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

fn binding_name_at_offset(program: &Program, offset: usize) -> Option<String> {
    for binding in &program.bindings {
        if contains(binding.name.span, offset) {
            return Some(binding.name.value.clone());
        }
        if let Some(name) = pipeline_start_binding_at_offset(&binding.pipeline, offset) {
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
        _ => None,
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
    }
    if let Some(main) = &program.main {
        if let PipelineStart::Binding(start) = &main.start {
            if start.value == name {
                spans.push(start.span);
            }
        }
    }
    spans
}

fn contains(span: Span, offset: usize) -> bool {
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

fn column_completions(columns: &[String], inside_string: bool) -> Vec<EditorCompletion> {
    columns
        .iter()
        .map(|column| EditorCompletion {
            label: column.clone(),
            insert_text: if inside_string {
                column.clone()
            } else {
                quote(column)
            },
            detail: "column".to_string(),
            kind: CompletionKind::Column,
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

fn dedupe_completions(items: Vec<EditorCompletion>) -> Vec<EditorCompletion> {
    let mut seen = BTreeSet::new();
    items
        .into_iter()
        .filter(|item| !item.label.is_empty() && seen.insert(item.label.clone()))
        .collect()
}

fn unquoted_text_at_span(source: &str, span: Span) -> Option<String> {
    let text = source.get(span.start..span.end)?;
    Some(text.trim_matches('"').to_string())
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

fn format_documentation(name: &str) -> &'static str {
    format_info(name).map_or("Format name.", |info| info.documentation)
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
        let source = "load \"😀.csv\"\n  | select \"missing\"";
        let start = source.find("\"missing\"").expect("diagnostic span");
        let diagnostics = diagnostics_for_editor(
            source,
            &[Diagnostic::error(
                pdl_core::codes::E1005,
                "unknown column `missing`",
                Span::new(start, start + "\"missing\"".len()),
            )],
        );

        assert_eq!(diagnostics[0].range.start.line, 1);
        assert_eq!(diagnostics[0].range.start.character, 11);
    }

    #[test]
    fn formats_pipeline_style() {
        let source =
            r#"load "sales.csv"|filter "status"=="completed"|agg sum("amount") as "total""#;

        assert_eq!(
            pdl_syntax::format_source(source).expect("formatted"),
            r#"load "sales.csv"
  | filter "status" == "completed"
  | agg sum("amount") as "total""#
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
    fn semantic_string_tokens_use_source_offsets() {
        let source = r#"load "sales.csv"
  | filter "status" == "completed"
  | group_by "region"
  | agg sum("amount") as "total_revenue", mean("customer_age") as "avg_age", count() as "orders"
  | sort "total_revenue" desc
  | limit 3"#;
        let tokens = semantic_tokens(source);

        for text in [
            "\"status\"",
            "\"region\"",
            "\"customer_age\"",
            "\"avg_age\"",
            "\"orders\"",
        ] {
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
    }
}
