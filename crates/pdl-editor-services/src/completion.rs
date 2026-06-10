// Completion-context inference and completion-item builders extracted from
// `services.rs` as part of the v0.42 split. See `services.rs` for the
// cross-module layout overview.

use pdl_semantics::{FormatInfo, FunctionInfo, StageInfo};
use pdl_syntax::{ContextKind, Expr, Pipeline, PipelineStart, Program, Stage};
use std::collections::BTreeSet;

use crate::scope_analysis::{BindingFact, DocumentFacts};
use crate::services::{contains, context_symbol_name, format_column_reference, stage_name};
use crate::{CompletionKind, EditorCompletion};

#[derive(Clone, Debug)]
pub(crate) struct CompletionContext {
    pub(crate) after_pipe: bool,
    pub(crate) in_pipeline_start: bool,
    pub(crate) in_format_context: bool,
    pub(crate) in_join_or_union_source_context: bool,
    pub(crate) in_join_kind_keyword_context: bool,
    pub(crate) in_join_kind_name_context: bool,
    pub(crate) in_agg_function_context: bool,
    pub(crate) in_scalar_function_context: bool,
    pub(crate) in_mutate_context: bool,
    pub(crate) in_window_frame_name_context: bool,
    pub(crate) in_sort_direction_context: bool,
    pub(crate) in_column_context: bool,
    pub(crate) context_reference_kind: Option<ContextKind>,
    pub(crate) inside_string: bool,
}

impl CompletionContext {
    pub(crate) fn new(source: &str, offset: usize, program: &Program) -> Self {
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
        let in_window_frame_name_context = !inside_string
            && preceding_word(source, word_start) == Some("frame")
            && offset_in_window_spec(program, offset);
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
            in_window_frame_name_context,
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

pub(crate) fn column_completions(columns: &[String], inside_string: bool) -> Vec<EditorCompletion> {
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

pub(crate) fn context_completions(
    facts: &DocumentFacts,
    kind: ContextKind,
) -> Vec<EditorCompletion> {
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

pub(crate) fn stage_completion(info: &StageInfo) -> EditorCompletion {
    EditorCompletion {
        label: info.name.to_string(),
        insert_text: info.name.to_string(),
        detail: info.documentation.to_string(),
        kind: CompletionKind::Stage,
    }
}

pub(crate) fn function_completion(info: &FunctionInfo) -> EditorCompletion {
    EditorCompletion {
        label: info.name.to_string(),
        insert_text: info.name.to_string(),
        detail: info.documentation.to_string(),
        kind: CompletionKind::Function,
    }
}

pub(crate) fn format_completion(info: &FormatInfo) -> EditorCompletion {
    EditorCompletion {
        label: info.name.to_string(),
        insert_text: info.name.to_string(),
        detail: info.documentation.to_string(),
        kind: CompletionKind::Format,
    }
}

pub(crate) fn binding_completion(name: &str, binding: &BindingFact) -> EditorCompletion {
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

/// The six v0.43.5 named window frames with the bound pair each name lowers
/// to, for completion details and hover text.
pub(crate) const WINDOW_FRAME_COMPLETIONS: [(&str, &str); 6] = [
    (
        "whole_partition",
        "Every row in the partition (unbounded_preceding..unbounded_following)",
    ),
    (
        "running",
        "Start of the partition through the current row (unbounded_preceding..current_row)",
    ),
    (
        "remaining",
        "Current row through the end of the partition (current_row..unbounded_following)",
    ),
    (
        "trailing",
        "Last N rows plus the current row (N preceding..current_row)",
    ),
    (
        "leading",
        "Current row plus the next N rows (current_row..N following)",
    ),
    (
        "centered",
        "N rows before through N rows after the current row (N preceding..N following)",
    ),
];

pub(crate) fn window_frame_name_completions() -> Vec<EditorCompletion> {
    WINDOW_FRAME_COMPLETIONS
        .iter()
        .map(|(name, detail)| keyword_completion(name, detail))
        .collect()
}

fn preceding_word(source: &str, word_start: usize) -> Option<&str> {
    let trimmed = source[..word_start].trim_end();
    let start = trimmed
        .char_indices()
        .rev()
        .take_while(|(_, ch)| is_ident_char(*ch))
        .last()
        .map(|(index, _)| index)?;
    Some(&trimmed[start..])
}

fn offset_in_window_spec(program: &Program, offset: usize) -> bool {
    let pipelines = program
        .bindings
        .iter()
        .map(|binding| &binding.pipeline)
        .chain(program.outputs.iter().map(|output| &output.pipeline))
        .chain(program.main.as_ref());
    for pipeline in pipelines {
        for stage in &pipeline.stages {
            let exprs: Vec<&Expr> = match stage {
                Stage::Filter { expr, .. } => vec![expr],
                Stage::Mutate { items, .. } => items.iter().map(|item| &item.expr).collect(),
                Stage::Agg { items, .. } => {
                    items.iter().flat_map(|item| item.args.iter()).collect()
                }
                Stage::Complete { fills, .. } => fills.iter().map(|fill| &fill.expr).collect(),
                _ => Vec::new(),
            };
            if exprs
                .into_iter()
                .any(|expr| expr_contains_window_spec_offset(expr, offset))
            {
                return true;
            }
        }
    }
    false
}

fn expr_contains_window_spec_offset(expr: &Expr, offset: usize) -> bool {
    match expr {
        Expr::Window { args, spec, .. } => {
            contains(spec.span, offset)
                || args
                    .iter()
                    .any(|arg| expr_contains_window_spec_offset(arg, offset))
        }
        Expr::Call { args, .. } => args
            .iter()
            .any(|arg| expr_contains_window_spec_offset(arg, offset)),
        Expr::Unary { expr, .. } => expr_contains_window_spec_offset(expr, offset),
        Expr::Binary { left, right, .. } => {
            expr_contains_window_spec_offset(left, offset)
                || expr_contains_window_spec_offset(right, offset)
        }
        Expr::Quoted(_)
        | Expr::Number(_)
        | Expr::Bool(_)
        | Expr::Null(_)
        | Expr::Ident(_)
        | Expr::Context { .. } => false,
    }
}

pub(crate) fn keyword_completion(label: &str, detail: &str) -> EditorCompletion {
    EditorCompletion {
        label: label.to_string(),
        insert_text: label.to_string(),
        detail: detail.to_string(),
        kind: CompletionKind::Keyword,
    }
}

pub(crate) fn dedupe_completions(items: Vec<EditorCompletion>) -> Vec<EditorCompletion> {
    let mut seen = BTreeSet::new();
    items
        .into_iter()
        .filter(|item| !item.label.is_empty() && seen.insert(item.label.clone()))
        .collect()
}

pub(crate) fn is_ident_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

fn is_ident_or_quote_char(ch: char) -> bool {
    is_ident_char(ch) || ch == '"' || ch.is_whitespace()
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
