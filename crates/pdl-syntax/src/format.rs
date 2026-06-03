use pdl_core::Severity;

use crate::{
    parse, AggItem, BinaryOp, Expr, MutateItem, NullsOrder, Pipeline, PipelineStart, SinkRef,
    SortDirection, SourceRef, Spanned, Stage, UnaryOp,
};

pub type FormatResult = Option<String>;

pub fn format_source(source: &str) -> FormatResult {
    // The current parser preserves trivia in the CST, but formatter rewriting is
    // still AST-shaped. Withhold edits for comments until comment attachment is
    // source-preserving.
    if source.contains("//") || source.contains("/*") {
        return None;
    }

    let parse = parse(source);
    if parse
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == Severity::Error)
    {
        return None;
    }

    let mut lines = Vec::new();
    for binding in &parse.program.bindings {
        lines.push(format!("let {} =", binding.name.value));
        lines.extend(format_pipeline(&binding.pipeline, "  ", "  "));
        lines.push(String::new());
    }
    if let Some(main) = &parse.program.main {
        lines.extend(format_pipeline(main, "", "  "));
    }

    Some(lines.join("\n"))
}

fn format_pipeline(pipeline: &Pipeline, first_indent: &str, pipe_indent: &str) -> Vec<String> {
    let mut lines = vec![format!(
        "{}{}",
        first_indent,
        format_pipeline_start(&pipeline.start)
    )];
    for stage in &pipeline.stages {
        lines.push(format!("{}| {}", pipe_indent, format_stage(stage)));
    }
    lines
}

fn format_pipeline_start(start: &PipelineStart) -> String {
    match start {
        PipelineStart::Load(load) => {
            let mut text = match &load.source {
                SourceRef::Path(path) => format!("load {}", quote(&path.value)),
                SourceRef::Stdin(_) => "load stdin".to_string(),
            };
            if let Some(format) = &load.format {
                text.push_str(&format!(" format {}", quote(&format.value)));
            }
            text
        }
        PipelineStart::Binding(name) => name.value.clone(),
    }
}

fn format_stage(stage: &Stage) -> String {
    match stage {
        Stage::Filter { expr, .. } => format!("filter {}", format_expr(expr)),
        Stage::Select { items, .. } => format!(
            "select {}",
            items
                .iter()
                .map(|item| {
                    let mut text = quote(&item.column.value);
                    if let Some(alias) = &item.alias {
                        text.push_str(&format!(" as {}", quote(&alias.value)));
                    }
                    text
                })
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Stage::Drop { columns, .. } => format!("drop {}", format_columns(columns)),
        Stage::Rename { items, .. } => format!(
            "rename {}",
            items
                .iter()
                .map(|item| format!("{} as {}", quote(&item.old.value), quote(&item.new.value)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Stage::Mutate { items, .. } => format!(
            "mutate {}",
            items
                .iter()
                .map(format_mutate_item)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Stage::GroupBy { columns, .. } => format!("group_by {}", format_columns(columns)),
        Stage::Agg { items, .. } => format!(
            "agg {}",
            items
                .iter()
                .map(format_agg_item)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Stage::Sort { items, .. } => format!(
            "sort {}",
            items
                .iter()
                .map(|item| {
                    let mut text = quote(&item.column.value);
                    if item.direction == SortDirection::Desc {
                        text.push_str(" desc");
                    }
                    if let Some(nulls) = item.nulls {
                        text.push_str(match nulls {
                            NullsOrder::First => " nulls_first",
                            NullsOrder::Last => " nulls_last",
                        });
                    }
                    text
                })
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Stage::Limit { n, .. } => format!("limit {n}"),
        Stage::Distinct { columns, .. } if columns.is_empty() => "distinct".to_string(),
        Stage::Distinct { columns, .. } => format!("distinct {}", format_columns(columns)),
        Stage::Save(save) => {
            let mut text = match &save.sink {
                SinkRef::Path(path) => format!("save {}", quote(&path.value)),
                SinkRef::Stdout(_) => "save stdout".to_string(),
            };
            if let Some(format) = &save.format {
                text.push_str(&format!(" format {}", quote(&format.value)));
            }
            text
        }
        Stage::Unsupported { name, .. } => name.value.clone(),
    }
}

fn format_columns(columns: &[Spanned<String>]) -> String {
    columns
        .iter()
        .map(|column| quote(&column.value))
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_mutate_item(item: &MutateItem) -> String {
    format!(
        "{} = {}",
        quote(&item.column.value),
        format_expr(&item.expr)
    )
}

fn format_agg_item(item: &AggItem) -> String {
    format!(
        "{}({}) as {}",
        item.function.value,
        item.args
            .iter()
            .map(format_expr)
            .collect::<Vec<_>>()
            .join(", "),
        quote(&item.alias.value)
    )
}

fn format_expr(expr: &Expr) -> String {
    match expr {
        Expr::Quoted(value) => quote(&value.value),
        Expr::Number(value) => format_number(value.value),
        Expr::Bool(value) => value.value.to_string(),
        Expr::Null(_) => "null".to_string(),
        Expr::Ident(value) => value.value.clone(),
        Expr::Call { name, args, .. } => format!(
            "{}({})",
            name.value,
            args.iter().map(format_expr).collect::<Vec<_>>().join(", ")
        ),
        Expr::Unary {
            op: UnaryOp::Not,
            expr,
            ..
        } => format!("not {}", format_expr(expr)),
        Expr::Unary {
            op: UnaryOp::Neg,
            expr,
            ..
        } => format!("-{}", format_expr(expr)),
        Expr::Binary {
            left, op, right, ..
        } => format!(
            "{} {} {}",
            format_expr(left),
            binary_op_text(*op),
            format_expr(right)
        ),
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

fn binary_op_text(op: BinaryOp) -> &'static str {
    match op {
        BinaryOp::Or => "or",
        BinaryOp::And => "and",
        BinaryOp::Eq => "==",
        BinaryOp::Ne => "!=",
        BinaryOp::Lt => "<",
        BinaryOp::Lte => "<=",
        BinaryOp::Gt => ">",
        BinaryOp::Gte => ">=",
        BinaryOp::Add => "+",
        BinaryOp::Sub => "-",
        BinaryOp::Mul => "*",
        BinaryOp::Div => "/",
        BinaryOp::Rem => "%",
    }
}

fn format_number(value: f64) -> String {
    if value.is_finite() && value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        let mut rendered = value.to_string();
        if rendered.contains('.') {
            while rendered.ends_with('0') {
                rendered.pop();
            }
            if rendered.ends_with('.') {
                rendered.push('0');
            }
        }
        rendered
    }
}
