use pdl_core::Span;
use pdl_data::{DataFormat, LogicalType, Table, Value};
use pdl_driver::{resolve_input_path, DriverIo, OsDriverIo};
use pdl_semantics::{aggregate_function, format_info, scalar_function, stage_info};
use pdl_syntax::{Expr, LoadStage, Pipeline, PipelineStart, Program, SaveStage, SourceRef, Stage};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::services::{
    byte_offset_for_position, contains, range_for_span, stage_name, unquoted_text_at_span,
    DocumentFacts, EditorHover, TextPosition,
};

const PREVIEW_ROW_LIMIT: usize = 5;
const SOURCE_ROW_DISPLAY_LIMIT: usize = 3;
const SOURCE_COLUMN_DISPLAY_LIMIT: usize = 6;
const SAMPLE_VALUE_LIMIT: usize = 4;
const SAMPLE_TEXT_LIMIT: usize = 32;

pub fn hover(source: &str, path: Option<&Path>, position: TextPosition) -> Option<EditorHover> {
    if let Some(path) = path {
        return hover_with_driver_io(source, path, &OsDriverIo, position);
    }

    hover_with_facts(source, None, position)
}

pub fn hover_with_driver_io(
    source: &str,
    path: &Path,
    io: &dyn DriverIo,
    position: TextPosition,
) -> Option<EditorHover> {
    hover_with_facts(source, Some((path, io)), position)
}

fn hover_with_facts(
    source: &str,
    driver: Option<(&Path, &dyn DriverIo)>,
    position: TextPosition,
) -> Option<EditorHover> {
    let offset = byte_offset_for_position(source, position);
    let parse = pdl_syntax::parse(source);
    let facts = DocumentFacts::new(&parse.program);
    let previews = driver
        .map(|(path, io)| PreviewFacts::new(&parse.program, path, io))
        .unwrap_or_default();

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

    hover_pipeline(source, &parse.program, &facts, &previews, offset)
}

fn hover_pipeline(
    source: &str,
    program: &Program,
    facts: &DocumentFacts,
    previews: &PreviewFacts,
    offset: usize,
) -> Option<EditorHover> {
    for binding in &program.bindings {
        if let Some(hover) = hover_for_pipeline(source, &binding.pipeline, facts, previews, offset)
        {
            return Some(hover);
        }
    }
    program
        .main
        .as_ref()
        .and_then(|pipeline| hover_for_pipeline(source, pipeline, facts, previews, offset))
}

fn hover_for_pipeline(
    source: &str,
    pipeline: &Pipeline,
    facts: &DocumentFacts,
    previews: &PreviewFacts,
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
        if let SourceRef::Path(path) = &load.source {
            if contains(path.span, offset) {
                return Some(EditorHover {
                    range: range_for_span(source, path.span),
                    markdown: source_hover_markdown(&path.value, previews.load_preview(load)),
                });
            }
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
        if let Some(hover) = hover_stage_detail(source, facts, previews, pipeline, stage, offset) {
            return Some(hover);
        }
    }

    None
}

fn hover_stage_detail(
    source: &str,
    facts: &DocumentFacts,
    previews: &PreviewFacts,
    pipeline: &Pipeline,
    stage: &Stage,
    offset: usize,
) -> Option<EditorHover> {
    let schema = facts
        .pipeline_schema_before_offset(pipeline, stage.span().start)
        .unwrap_or_default();
    let preview = previews.pipeline_preview_before_offset(pipeline, stage.span().start);
    for span in column_spans(stage) {
        if contains(span, offset) {
            let text = unquoted_text_at_span(source, span).unwrap_or_default();
            let markdown = if let Some(column) =
                preview.as_ref().and_then(|preview| preview.column(&text))
            {
                column_hover_markdown(&text, column)
            } else {
                let known = schema.iter().any(|column| column == &text);
                let detail = if known {
                    "Schema column from the current table. Sample data is not available for this column."
                } else {
                    "Column reference. The schema is unknown or this column has a diagnostic."
                };
                format!("**column `{}`**\n\n{detail}", escape_inline_code(&text))
            };
            return Some(EditorHover {
                range: range_for_span(source, span),
                markdown,
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

    for (name, span) in scalar_function_spans(stage) {
        if contains(span, offset) {
            if let Some(info) = scalar_function(&name) {
                return Some(info_hover(source, span, info.name, info.documentation));
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

fn source_hover_markdown(path: &str, preview: Option<&TablePreview>) -> String {
    let Some(preview) = preview else {
        return format!(
            "**source `{}`**\n\nCSV preview unavailable.",
            escape_inline_code(path)
        );
    };

    let mut markdown = format!(
        "**source `{}`**\n\nFormat: `csv`\n\nRows sampled: `{}`\n\n",
        escape_inline_code(path),
        preview.rows_sampled
    );
    markdown.push_str("| column | type | nullable | samples |\n");
    markdown.push_str("| --- | --- | --- | --- |\n");
    for column in preview.columns.iter().take(SOURCE_COLUMN_DISPLAY_LIMIT) {
        markdown.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            escape_table_cell(&column.name),
            logical_type_name(&column.logical_type),
            yes_no(column.nullable),
            escape_table_cell(&column.samples.join(", "))
        ));
    }
    if preview.columns.len() > SOURCE_COLUMN_DISPLAY_LIMIT {
        markdown.push_str(&format!(
            "\n{} more columns omitted.\n",
            preview.columns.len() - SOURCE_COLUMN_DISPLAY_LIMIT
        ));
    }
    if !preview.sample_rows.is_empty() {
        markdown.push_str("\nSample rows:\n\n");
        markdown.push_str(&sample_rows_markdown(preview));
    }
    markdown
}

fn column_hover_markdown(name: &str, column: &ColumnPreview) -> String {
    let mut markdown = format!(
        "**column `{}`**\n\nType: `{}`\n\nNullable: `{}`",
        escape_inline_code(name),
        logical_type_name(&column.logical_type),
        yes_no(column.nullable),
    );
    if !column.samples.is_empty() {
        markdown.push_str(&format!("\n\nSamples: {}", column.samples.join(", ")));
    }
    markdown
}

fn sample_rows_markdown(preview: &TablePreview) -> String {
    let columns: Vec<&ColumnPreview> = preview
        .columns
        .iter()
        .take(SOURCE_COLUMN_DISPLAY_LIMIT)
        .collect();
    let mut markdown = String::new();
    markdown.push('|');
    for column in &columns {
        markdown.push(' ');
        markdown.push_str(&escape_table_cell(&column.name));
        markdown.push_str(" |");
    }
    markdown.push('\n');
    markdown.push('|');
    for _ in &columns {
        markdown.push_str(" --- |");
    }
    markdown.push('\n');
    for row in preview.sample_rows.iter().take(SOURCE_ROW_DISPLAY_LIMIT) {
        markdown.push('|');
        for cell in row.iter().take(SOURCE_COLUMN_DISPLAY_LIMIT) {
            markdown.push(' ');
            markdown.push_str(&escape_table_cell(cell));
            markdown.push_str(" |");
        }
        markdown.push('\n');
    }
    markdown
}

#[derive(Clone, Debug, Default)]
struct PreviewFacts {
    load_previews: BTreeMap<usize, TablePreview>,
    binding_previews: BTreeMap<String, TablePreview>,
}

impl PreviewFacts {
    fn new(program: &Program, program_path: &Path, io: &dyn DriverIo) -> Self {
        let mut facts = Self::default();
        for binding in &program.bindings {
            if let Some(preview) =
                facts.pipeline_preview_with_io(&binding.pipeline, program_path, io)
            {
                facts
                    .binding_previews
                    .insert(binding.name.value.clone(), preview);
            }
        }
        if let Some(main) = &program.main {
            let _ = facts.pipeline_preview_with_io(main, program_path, io);
        }
        facts
    }

    fn load_preview(&self, load: &LoadStage) -> Option<&TablePreview> {
        self.load_previews.get(&load.span.start)
    }

    fn pipeline_preview_before_offset(
        &self,
        pipeline: &Pipeline,
        offset: usize,
    ) -> Option<TablePreview> {
        let mut preview = self.pipeline_start_preview(pipeline)?;
        for stage in &pipeline.stages {
            if offset <= stage.span().end {
                return Some(preview);
            }
            apply_stage_to_preview(&mut preview, stage);
        }
        Some(preview)
    }

    fn pipeline_preview_with_io(
        &mut self,
        pipeline: &Pipeline,
        program_path: &Path,
        io: &dyn DriverIo,
    ) -> Option<TablePreview> {
        let mut preview = match &pipeline.start {
            PipelineStart::Load(load) => self.load_preview_with_io(load, program_path, io)?,
            PipelineStart::Binding(name) => self.binding_previews.get(&name.value).cloned()?,
        };
        for stage in &pipeline.stages {
            apply_stage_to_preview(&mut preview, stage);
        }
        Some(preview)
    }

    fn load_preview_with_io(
        &mut self,
        load: &LoadStage,
        program_path: &Path,
        io: &dyn DriverIo,
    ) -> Option<TablePreview> {
        if let Some(preview) = self.load_previews.get(&load.span.start) {
            return Some(preview.clone());
        }
        let resolved = resolve_csv_load_path(load, program_path)?;
        let bytes = io.read_path_bytes(&resolved).ok()?;
        let table = pdl_data::read_table_from_bytes(&resolved, DataFormat::Csv, &bytes).ok()?;
        let preview = TablePreview::from_table(&table);
        self.load_previews.insert(load.span.start, preview.clone());
        Some(preview)
    }

    fn pipeline_start_preview(&self, pipeline: &Pipeline) -> Option<TablePreview> {
        match &pipeline.start {
            PipelineStart::Load(load) => self.load_preview(load).cloned(),
            PipelineStart::Binding(name) => self.binding_previews.get(&name.value).cloned(),
        }
    }
}

#[derive(Clone, Debug)]
struct TablePreview {
    columns: Vec<ColumnPreview>,
    rows_sampled: usize,
    sample_rows: Vec<Vec<String>>,
}

impl TablePreview {
    fn from_table(table: &Table) -> Self {
        let mut builders: Vec<ColumnPreviewBuilder> = table
            .columns
            .iter()
            .map(|column| ColumnPreviewBuilder::new(column.clone()))
            .collect();
        let mut sample_rows = Vec::new();
        for row in table.rows.iter().take(PREVIEW_ROW_LIMIT) {
            let mut sample_row = Vec::new();
            for (index, builder) in builders.iter_mut().enumerate() {
                let value = row.values.get(index).unwrap_or(&Value::Null);
                builder.observe(value);
                sample_row.push(sample_text(value));
            }
            sample_rows.push(sample_row);
        }
        Self {
            columns: builders
                .into_iter()
                .map(ColumnPreviewBuilder::finish)
                .collect(),
            rows_sampled: table.rows.len().min(PREVIEW_ROW_LIMIT),
            sample_rows,
        }
    }

    fn column(&self, name: &str) -> Option<&ColumnPreview> {
        self.columns.iter().find(|column| column.name == name)
    }
}

#[derive(Clone, Debug)]
struct ColumnPreview {
    name: String,
    logical_type: LogicalType,
    nullable: bool,
    samples: Vec<String>,
}

impl ColumnPreview {
    fn unknown(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            logical_type: LogicalType::Unknown,
            nullable: true,
            samples: Vec::new(),
        }
    }

    fn derived(name: impl Into<String>, logical_type: LogicalType) -> Self {
        Self {
            name: name.into(),
            logical_type,
            nullable: true,
            samples: Vec::new(),
        }
    }
}

#[derive(Clone, Debug)]
struct ColumnPreviewBuilder {
    name: String,
    saw_null: bool,
    saw_bool: bool,
    saw_number: bool,
    saw_string: bool,
    samples: Vec<String>,
}

impl ColumnPreviewBuilder {
    fn new(name: String) -> Self {
        Self {
            name,
            saw_null: false,
            saw_bool: false,
            saw_number: false,
            saw_string: false,
            samples: Vec::new(),
        }
    }

    fn observe(&mut self, value: &Value) {
        match value {
            Value::Null => self.saw_null = true,
            Value::Bool(_) => self.saw_bool = true,
            Value::Number(_) => self.saw_number = true,
            Value::String(_) => self.saw_string = true,
        }
        if !matches!(value, Value::Null) && self.samples.len() < SAMPLE_VALUE_LIMIT {
            let sample = sample_text(value);
            if !self.samples.iter().any(|existing| existing == &sample) {
                self.samples.push(sample);
            }
        }
    }

    fn finish(self) -> ColumnPreview {
        let logical_type = self.logical_type();
        ColumnPreview {
            name: self.name,
            logical_type,
            nullable: self.saw_null,
            samples: self.samples,
        }
    }

    fn logical_type(&self) -> LogicalType {
        match (
            self.saw_bool as u8 + self.saw_number as u8 + self.saw_string as u8,
            self.saw_bool,
            self.saw_number,
            self.saw_string,
            self.saw_null,
        ) {
            (0, _, _, _, true) => LogicalType::Null,
            (1, true, false, false, _) => LogicalType::Bool,
            (1, false, true, false, _) => LogicalType::Number,
            (1, false, false, true, _) => LogicalType::String,
            (0, _, _, _, false) => LogicalType::Unknown,
            _ => LogicalType::Unknown,
        }
    }
}

fn resolve_csv_load_path(load: &LoadStage, program_path: &Path) -> Option<PathBuf> {
    let SourceRef::Path(path) = &load.source else {
        return None;
    };
    let format = load
        .format
        .as_ref()
        .and_then(|format| DataFormat::from_name(&format.value))
        .or_else(|| DataFormat::infer_from_path(&path.value))?;
    if format != DataFormat::Csv {
        return None;
    }
    let base_dir = program_path
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    Some(if base_dir == Path::new(".") {
        resolve_input_path(program_path, &path.value)
    } else {
        base_dir.join(&path.value)
    })
}

fn apply_stage_to_preview(preview: &mut TablePreview, stage: &Stage) {
    match stage {
        Stage::Filter { .. }
        | Stage::Sort { .. }
        | Stage::GroupBy { .. }
        | Stage::Distinct { .. }
        | Stage::Save(_) => {}
        Stage::Limit { n, .. } => {
            preview.rows_sampled = preview.rows_sampled.min(*n);
            preview.sample_rows.truncate(*n);
        }
        Stage::Select { items, .. } => {
            preview.columns = items
                .iter()
                .map(|item| {
                    let output_name = item.alias.as_ref().unwrap_or(&item.column).value.clone();
                    preview
                        .column(&item.column.value)
                        .cloned()
                        .map(|mut column| {
                            column.name = output_name.clone();
                            column
                        })
                        .unwrap_or_else(|| ColumnPreview::unknown(output_name))
                })
                .collect();
            preview.sample_rows.clear();
        }
        Stage::Drop { columns, .. } => {
            preview
                .columns
                .retain(|column| !columns.iter().any(|drop| drop.value == column.name));
            preview.sample_rows.clear();
        }
        Stage::Rename { items, .. } => {
            for column in &mut preview.columns {
                if let Some(rename) = items.iter().find(|rename| rename.old.value == column.name) {
                    column.name = rename.new.value.clone();
                }
            }
            preview.sample_rows.clear();
        }
        Stage::Mutate { items, .. } => {
            for item in items {
                if !preview
                    .columns
                    .iter()
                    .any(|column| column.name == item.column.value)
                {
                    preview
                        .columns
                        .push(ColumnPreview::unknown(item.column.value.clone()));
                }
            }
            preview.sample_rows.clear();
        }
        Stage::Agg { items, .. } => {
            let mut output = Vec::new();
            for item in items {
                let logical_type = match item.function.value.as_str() {
                    "count" | "sum" | "mean" => LogicalType::Number,
                    "min" | "max" => item
                        .args
                        .first()
                        .and_then(quoted_expr_value)
                        .and_then(|name| preview.column(name))
                        .map_or(LogicalType::Unknown, |column| column.logical_type.clone()),
                    _ => LogicalType::Unknown,
                };
                output.push(ColumnPreview::derived(
                    item.alias.value.clone(),
                    logical_type,
                ));
            }
            preview.columns = output;
            preview.sample_rows.clear();
        }
        Stage::Unsupported { .. } => {}
    }
}

fn quoted_expr_value(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Quoted(value) => Some(&value.value),
        _ => None,
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
        Stage::Mutate { items, .. } => items
            .iter()
            .flat_map(|item| {
                expr_column_spans(&item.expr)
                    .into_iter()
                    .chain([item.column.span])
            })
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
        Stage::Distinct { columns, .. } => columns.iter().map(|column| column.span).collect(),
        Stage::Limit { .. } | Stage::Save(_) | Stage::Unsupported { .. } => Vec::new(),
    }
}

fn scalar_function_spans(stage: &Stage) -> Vec<(String, Span)> {
    match stage {
        Stage::Filter { expr, .. } => expr_function_spans(expr),
        Stage::Mutate { items, .. } => items
            .iter()
            .flat_map(|item| expr_function_spans(&item.expr))
            .collect(),
        Stage::Agg { items, .. } => items
            .iter()
            .flat_map(|item| item.args.iter().flat_map(expr_function_spans))
            .collect(),
        Stage::Select { .. }
        | Stage::Drop { .. }
        | Stage::Rename { .. }
        | Stage::GroupBy { .. }
        | Stage::Sort { .. }
        | Stage::Limit { .. }
        | Stage::Distinct { .. }
        | Stage::Save(_)
        | Stage::Unsupported { .. } => Vec::new(),
    }
}

fn expr_function_spans(expr: &Expr) -> Vec<(String, Span)> {
    match expr {
        Expr::Call { name, args, .. } => {
            let mut spans = vec![(name.value.clone(), name.span)];
            spans.extend(args.iter().flat_map(expr_function_spans));
            spans
        }
        Expr::Unary { expr, .. } => expr_function_spans(expr),
        Expr::Binary { left, right, .. } => {
            let mut spans = expr_function_spans(left);
            spans.extend(expr_function_spans(right));
            spans
        }
        Expr::Quoted(_) | Expr::Number(_) | Expr::Bool(_) | Expr::Null(_) | Expr::Ident(_) => {
            Vec::new()
        }
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

fn sample_text(value: &Value) -> String {
    truncate_text(&value.to_csv_cell())
}

fn truncate_text(text: &str) -> String {
    let mut chars = text.chars();
    let truncated: String = chars.by_ref().take(SAMPLE_TEXT_LIMIT).collect();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

fn logical_type_name(logical_type: &LogicalType) -> &'static str {
    match logical_type {
        LogicalType::String => "string",
        LogicalType::Bool => "bool",
        LogicalType::Int => "int",
        LogicalType::Number => "number",
        LogicalType::Decimal => "decimal",
        LogicalType::Date => "date",
        LogicalType::Time => "time",
        LogicalType::DateTime => "date-time",
        LogicalType::Duration => "duration",
        LogicalType::Binary => "binary",
        LogicalType::Null => "null",
        LogicalType::Unknown => "unknown",
    }
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

fn escape_inline_code(text: &str) -> String {
    text.replace('`', "\\`")
}

fn escape_table_cell(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('|', "\\|")
        .replace(['\n', '\r'], " ")
}

fn format_documentation(name: &str) -> &'static str {
    format_info(name).map_or("Format name.", |info| info.documentation)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdl_driver::InMemoryDriverIo;
    use std::fs;

    #[test]
    fn source_hover_uses_driver_csv_preview() {
        let source = r#"load "sales.csv" | group_by "region""#;
        let io = InMemoryDriverIo::default().with_file_bytes(
            "memory/sales.csv",
            "region,status,amount\nNorth,completed,120\nSouth,pending,75\nWest,completed,200\n",
        );
        let position = TextPosition {
            line: 0,
            character: 8,
        };

        let hover =
            hover_with_driver_io(source, Path::new("memory/main.pdl"), &io, position).unwrap();

        assert!(hover.markdown.contains("**source `sales.csv`**"));
        assert!(hover
            .markdown
            .contains("| region | string | no | North, South, West |"));
        assert!(hover
            .markdown
            .contains("| amount | number | no | 120, 75, 200 |"));
        assert!(hover.markdown.contains("Sample rows:"));
    }

    #[test]
    fn column_hover_uses_driver_csv_preview() {
        let source = r#"load "sales.csv"
  | group_by "region""#;
        let io = InMemoryDriverIo::default().with_file_bytes(
            "memory/sales.csv",
            "region,status,amount\nNorth,completed,120\nSouth,pending,75\nWest,completed,200\n",
        );
        let position = TextPosition {
            line: 1,
            character: 15,
        };

        let hover =
            hover_with_driver_io(source, Path::new("memory/main.pdl"), &io, position).unwrap();

        assert_eq!(hover.markdown, "**column `region`**\n\nType: `string`\n\nNullable: `no`\n\nSamples: North, South, West");
    }

    #[test]
    fn hover_with_path_uses_os_driver_io_for_native_lsp_route() {
        let root = std::env::temp_dir().join(format!("pdl-hover-native-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create temp dir");
        fs::write(
            root.join("sales.csv"),
            "region,status,amount\nNorth,completed,120\nSouth,pending,75\n",
        )
        .expect("write sales csv");
        let program_path = root.join("main.pdl");
        let source = r#"load "sales.csv"
  | group_by "region""#;

        let hover = hover(
            source,
            Some(&program_path),
            TextPosition {
                line: 1,
                character: 15,
            },
        )
        .unwrap();

        assert!(hover.markdown.contains("Type: `string`"));
        assert!(hover.markdown.contains("Samples: North, South"));

        let _ = fs::remove_dir_all(root);
    }
}
