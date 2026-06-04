use pdl_core::Severity;

use crate::{
    parse, AggItem, BinaryOp, CompleteFillItem, Expr, FrameBound, JoinOn, MutateItem, NullsOrder,
    Pipeline, PipelineStart, SinkRef, SortDirection, SortItem, SourceRef, Spanned, Stage, UnaryOp,
    UnionOptionKind, WindowFrame, WindowSpec,
};

pub type FormatResult = Option<String>;

const MAX_INLINE_STAGE_LEN: usize = 88;

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
    for output in &parse.program.outputs {
        lines.push(format!("output {} =", output.name.value));
        lines.extend(format_pipeline(&output.pipeline, "  ", "  "));
        lines.push(String::new());
    }
    if let Some(main) = &parse.program.main {
        lines.extend(format_pipeline(main, "", "  "));
    } else if lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
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
        lines.extend(format_stage_lines(stage, pipe_indent));
    }
    lines
}

fn format_pipeline_start(start: &PipelineStart) -> String {
    match start {
        PipelineStart::Load(load) => {
            let mut text = match &load.source {
                SourceRef::Path(path) => format!("load {}", format_string_literal(&path.value)),
                SourceRef::Stdin(_) => "load stdin".to_string(),
            };
            if let Some(format) = &load.format {
                text.push_str(&format!(" format {}", format_string_literal(&format.value)));
            }
            text
        }
        PipelineStart::Binding(name) => name.value.clone(),
    }
}

fn format_stage_lines(stage: &Stage, pipe_indent: &str) -> Vec<String> {
    let inline = format_stage_inline(stage);
    match stage {
        Stage::Select { items, .. } if should_multiline_item_stage(&inline, items.len()) => {
            let items = items.iter().map(format_select_item).collect::<Vec<_>>();
            format_item_stage_lines("select", items, pipe_indent)
        }
        Stage::Drop { columns, .. } if should_multiline_item_stage(&inline, columns.len()) => {
            format_item_stage_lines("drop", format_column_items(columns), pipe_indent)
        }
        Stage::Rename { items, .. } if should_multiline_item_stage(&inline, items.len()) => {
            let items = items
                .iter()
                .map(|item| {
                    format!(
                        "{} = {}",
                        format_column_reference(&item.new.value),
                        format_column_reference(&item.old.value)
                    )
                })
                .collect::<Vec<_>>();
            format_item_stage_lines("rename", items, pipe_indent)
        }
        Stage::Mutate { items, .. } if should_multiline_mutate_stage(&inline, items) => {
            format_mutate_stage_lines(items, pipe_indent)
        }
        Stage::Agg { items, .. } if should_multiline_item_stage(&inline, items.len()) => {
            let items = items.iter().map(format_agg_item).collect::<Vec<_>>();
            format_item_stage_lines("agg", items, pipe_indent)
        }
        Stage::Sort { items, .. } if should_multiline_item_stage(&inline, items.len()) => {
            let items = items.iter().map(format_sort_item).collect::<Vec<_>>();
            format_item_stage_lines("sort", items, pipe_indent)
        }
        Stage::Distinct { columns, .. }
            if !columns.is_empty() && should_multiline_item_stage(&inline, columns.len()) =>
        {
            format_item_stage_lines("distinct", format_column_items(columns), pipe_indent)
        }
        Stage::PivotLonger { columns, .. }
            if should_multiline_item_stage(&inline, columns.len()) =>
        {
            format_pivot_longer_stage_lines(stage, pipe_indent)
        }
        Stage::Complete { keys, fills, .. }
            if should_multiline_item_stage(&inline, keys.len() + fills.len()) =>
        {
            format_complete_stage_lines(keys, fills, pipe_indent)
        }
        _ => vec![format!("{}| {}", pipe_indent, inline)],
    }
}

fn format_stage_inline(stage: &Stage) -> String {
    match stage {
        Stage::Filter { expr, .. } => format!("filter {}", format_expr(expr)),
        Stage::Select { items, .. } => format!(
            "select {}",
            items
                .iter()
                .map(format_select_item)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Stage::Drop { columns, .. } => format!("drop {}", format_columns(columns)),
        Stage::Rename { items, .. } => format!(
            "rename {}",
            items
                .iter()
                .map(|item| {
                    format!(
                        "{} = {}",
                        format_column_reference(&item.new.value),
                        format_column_reference(&item.old.value)
                    )
                })
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
                .map(format_sort_item)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Stage::Limit { n, .. } => format!("limit {n}"),
        Stage::Join {
            source, on, kind, ..
        } => {
            let mut text = format!("join {} on {}", source.value, format_join_on(on));
            if *kind != crate::JoinKind::Inner {
                text.push_str(&format!(" kind {}", kind.as_str()));
            }
            text
        }
        Stage::Union {
            source, options, ..
        } => {
            let mut text = format!("union {}", source.value);
            for option in options {
                let name = match option.kind {
                    UnionOptionKind::ByName => "by_name",
                    UnionOptionKind::Distinct => "distinct",
                };
                text.push_str(&format!(" {name} {}", option.value.value));
            }
            text
        }
        Stage::Distinct { columns, .. } if columns.is_empty() => "distinct".to_string(),
        Stage::Distinct { columns, .. } => format!("distinct {}", format_columns(columns)),
        Stage::PivotLonger {
            columns,
            names_to,
            values_to,
            ..
        } => format!(
            "pivot_longer {} names_to {} values_to {}",
            format_columns(columns),
            format_column_reference(&names_to.value),
            format_column_reference(&values_to.value)
        ),
        Stage::Complete { keys, fills, .. } => {
            let mut text = format!("complete {}", format_columns(keys));
            if !fills.is_empty() {
                text.push_str(" fill ");
                text.push_str(
                    &fills
                        .iter()
                        .map(format_complete_fill_item)
                        .collect::<Vec<_>>()
                        .join(", "),
                );
            }
            text
        }
        Stage::Save(save) => {
            let mut text = match &save.sink {
                SinkRef::Path(path) => format!("save {}", format_string_literal(&path.value)),
                SinkRef::Stdout(_) => "save stdout".to_string(),
            };
            if let Some(format) = &save.format {
                text.push_str(&format!(" format {}", format_string_literal(&format.value)));
            }
            text
        }
        Stage::Unsupported { name, .. } => name.value.clone(),
    }
}

fn should_multiline_item_stage(inline: &str, item_count: usize) -> bool {
    item_count > 1 && inline.len() > MAX_INLINE_STAGE_LEN
}

fn should_multiline_mutate_stage(inline: &str, items: &[MutateItem]) -> bool {
    let has_window_assignment = items
        .iter()
        .any(|item| matches!(&item.expr, Expr::Window { .. }));
    has_window_assignment || should_multiline_item_stage(inline, items.len())
}

fn format_item_stage_lines(stage_name: &str, items: Vec<String>, pipe_indent: &str) -> Vec<String> {
    let item_indent = format!("{pipe_indent}    ");
    let last_index = items.len().saturating_sub(1);
    let mut lines = vec![format!("{pipe_indent}| {stage_name}")];
    for (index, item) in items.into_iter().enumerate() {
        let comma = if index == last_index { "" } else { "," };
        lines.push(format!("{item_indent}{item}{comma}"));
    }
    lines
}

fn format_column_items(columns: &[Spanned<String>]) -> Vec<String> {
    columns
        .iter()
        .map(|column| format_column_reference(&column.value))
        .collect()
}

fn format_select_item(item: &crate::SelectItem) -> String {
    if let Some(alias) = &item.alias {
        format!(
            "{} = {}",
            format_column_reference(&alias.value),
            format_column_reference(&item.column.value)
        )
    } else {
        format_column_reference(&item.column.value)
    }
}

fn format_sort_item(item: &SortItem) -> String {
    let mut text = format_column_reference(&item.column.value);
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
}

fn format_join_on(on: &JoinOn) -> String {
    match on {
        JoinOn::Same(column) => format_column_reference(&column.value),
        JoinOn::Pair { left, right, .. } => {
            format!(
                "({}, {})",
                format_column_reference(&left.value),
                format_column_reference(&right.value)
            )
        }
    }
}

fn format_columns(columns: &[Spanned<String>]) -> String {
    columns
        .iter()
        .map(|column| format_column_reference(&column.value))
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_mutate_item(item: &MutateItem) -> String {
    format!(
        "{} = {}",
        format_column_reference(&item.column.value),
        format_expr(&item.expr)
    )
}

fn format_complete_fill_item(item: &CompleteFillItem) -> String {
    format!(
        "{} = {}",
        format_column_reference(&item.column.value),
        format_expr(&item.expr)
    )
}

fn format_pivot_longer_stage_lines(stage: &Stage, pipe_indent: &str) -> Vec<String> {
    let Stage::PivotLonger {
        columns,
        names_to,
        values_to,
        ..
    } = stage
    else {
        return Vec::new();
    };
    let item_indent = format!("{pipe_indent}    ");
    let mut lines = vec![format!("{pipe_indent}| pivot_longer")];
    for column in columns {
        lines.push(format!(
            "{item_indent}{},",
            format_column_reference(&column.value)
        ));
    }
    lines.push(format!(
        "{item_indent}names_to {} values_to {}",
        format_column_reference(&names_to.value),
        format_column_reference(&values_to.value)
    ));
    lines
}

fn format_complete_stage_lines(
    keys: &[Spanned<String>],
    fills: &[CompleteFillItem],
    pipe_indent: &str,
) -> Vec<String> {
    let item_indent = format!("{pipe_indent}    ");
    let mut lines = vec![format!("{pipe_indent}| complete")];
    for key in keys {
        lines.push(format!(
            "{item_indent}{},",
            format_column_reference(&key.value)
        ));
    }
    if !fills.is_empty() {
        let last_index = fills.len().saturating_sub(1);
        lines.push(format!("{item_indent}fill"));
        for (index, item) in fills.iter().enumerate() {
            let comma = if index == last_index { "" } else { "," };
            lines.push(format!(
                "{item_indent}  {} = {}{}",
                format_column_reference(&item.column.value),
                format_expr(&item.expr),
                comma
            ));
        }
    }
    lines
}

fn format_mutate_stage_lines(items: &[MutateItem], pipe_indent: &str) -> Vec<String> {
    let item_indent = format!("{pipe_indent}    ");
    let expr_indent = format!("{pipe_indent}      ");
    let last_index = items.len().saturating_sub(1);
    let mut lines = vec![format!("{pipe_indent}| mutate")];

    for (index, item) in items.iter().enumerate() {
        let comma = if index == last_index { "" } else { "," };
        if matches!(&item.expr, Expr::Window { .. }) {
            lines.push(format!(
                "{}{} =",
                item_indent,
                format_column_reference(&item.column.value)
            ));
            let mut expr_lines = format_expr_lines(&item.expr, &expr_indent);
            append_suffix_to_last_line(&mut expr_lines, comma);
            lines.extend(expr_lines);
        } else {
            lines.push(format!(
                "{}{} = {}{}",
                item_indent,
                format_column_reference(&item.column.value),
                format_expr(&item.expr),
                comma
            ));
        }
    }

    lines
}

fn format_agg_item(item: &AggItem) -> String {
    format!(
        "{} = {}({})",
        format_column_reference(&item.alias.value),
        item.function.value,
        item.args
            .iter()
            .map(format_expr)
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn format_expr(expr: &Expr) -> String {
    match expr {
        Expr::Quoted(value) => format_string_literal(&value.value),
        Expr::Number(value) => format_number(value.value),
        Expr::Bool(value) => value.value.to_string(),
        Expr::Null(_) => "null".to_string(),
        Expr::Ident(value) => format_column_reference(&value.value),
        Expr::Call { name, args, .. } => format!(
            "{}({})",
            name.value,
            args.iter().map(format_expr).collect::<Vec<_>>().join(", ")
        ),
        Expr::Window {
            function,
            args,
            spec,
            ..
        } => format!(
            "{}({}) over ({})",
            function.value,
            args.iter().map(format_expr).collect::<Vec<_>>().join(", "),
            format_window_spec(spec)
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

fn format_expr_lines(expr: &Expr, indent: &str) -> Vec<String> {
    match expr {
        Expr::Window {
            function,
            args,
            spec,
            ..
        } => {
            let spec_lines = format_window_spec_lines(spec, &format!("{indent}  "));
            let args = args.iter().map(format_expr).collect::<Vec<_>>().join(", ");
            if spec_lines.is_empty() {
                return vec![format!("{indent}{}({args}) over ()", function.value)];
            }

            let mut lines = vec![format!("{indent}{}({args}) over (", function.value)];
            lines.extend(spec_lines);
            lines.push(format!("{indent})"));
            lines
        }
        _ => vec![format!("{indent}{}", format_expr(expr))],
    }
}

fn format_window_spec(spec: &WindowSpec) -> String {
    let mut parts = Vec::new();
    if !spec.partition_by.is_empty() {
        parts.push(format!(
            "partition_by {}",
            format_columns(&spec.partition_by)
        ));
    }
    if !spec.order_by.is_empty() {
        parts.push(format!(
            "order_by {}",
            spec.order_by
                .iter()
                .map(format_window_sort_item)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if let Some(frame) = &spec.frame {
        parts.push(format_window_frame(frame));
    }
    parts.join(" ")
}

fn format_window_spec_lines(spec: &WindowSpec, indent: &str) -> Vec<String> {
    let mut lines = Vec::new();
    if !spec.partition_by.is_empty() {
        lines.push(format!(
            "{indent}partition_by {}",
            format_columns(&spec.partition_by)
        ));
    }
    if !spec.order_by.is_empty() {
        lines.push(format!(
            "{indent}order_by {}",
            spec.order_by
                .iter()
                .map(format_window_sort_item)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if let Some(frame) = &spec.frame {
        lines.push(format!("{indent}{}", format_window_frame(frame)));
    }
    lines
}

fn format_window_sort_item(item: &SortItem) -> String {
    let mut text = format_column_reference(&item.column.value);
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
}

fn format_window_frame(frame: &WindowFrame) -> String {
    format!(
        "rows between {} and {}",
        format_frame_bound(&frame.start),
        format_frame_bound(&frame.end)
    )
}

fn format_frame_bound(bound: &FrameBound) -> String {
    match bound {
        FrameBound::UnboundedPreceding { .. } => "unbounded_preceding".to_string(),
        FrameBound::Preceding { rows, .. } => format!("{rows} preceding"),
        FrameBound::CurrentRow { .. } => "current_row".to_string(),
        FrameBound::Following { rows, .. } => format!("{rows} following"),
        FrameBound::UnboundedFollowing { .. } => "unbounded_following".to_string(),
    }
}

fn format_string_literal(value: &str) -> String {
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t");
    format!("\"{escaped}\"")
}

fn format_column_reference(value: &str) -> String {
    if is_simple_column_name(value) && !is_reserved_keyword(value) {
        return value.to_string();
    }
    let escaped = value.replace('\\', "\\\\").replace('`', "\\`");
    format!("`{escaped}`")
}

fn is_simple_column_name(value: &str) -> bool {
    let mut chars = value.chars();
    chars.next().is_some_and(is_ident_start) && chars.all(is_ident_char)
}

fn is_ident_start(ch: char) -> bool {
    ch.is_ascii_alphabetic() || ch == '_'
}

fn is_ident_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

fn is_reserved_keyword(value: &str) -> bool {
    matches!(
        value,
        "load"
            | "save"
            | "filter"
            | "select"
            | "drop"
            | "rename"
            | "mutate"
            | "group_by"
            | "agg"
            | "sort"
            | "limit"
            | "join"
            | "union"
            | "distinct"
            | "pivot_longer"
            | "complete"
            | "let"
            | "output"
            | "on"
            | "kind"
            | "by_name"
            | "names_to"
            | "values_to"
            | "fill"
            | "format"
            | "over"
            | "partition_by"
            | "order_by"
            | "rows"
            | "between"
            | "unbounded_preceding"
            | "current_row"
            | "unbounded_following"
            | "preceding"
            | "following"
            | "stdin"
            | "stdout"
            | "true"
            | "false"
            | "null"
            | "and"
            | "or"
            | "not"
            | "asc"
            | "desc"
    )
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

fn append_suffix_to_last_line(lines: &mut [String], suffix: &str) {
    if let Some(line) = lines.last_mut() {
        line.push_str(suffix);
    }
}

#[cfg(test)]
mod tests {
    use super::format_source;

    #[test]
    fn formats_window_heavy_pipeline_readably() {
        let source = r#"load "sales.csv"
  | filter status == "completed"
  | mutate customer_sale_number = row_number() over (partition_by customer_id order_by amount desc), customer_revenue = sum(amount) over (partition_by customer_id), region_revenue = sum(amount) over (partition_by region)
  | mutate region_revenue_rank = dense_rank() over (order_by region_revenue desc)
  | select region, customer_id, amount, customer_sale_number, customer_revenue, region_revenue_rank
  | sort region_revenue_rank, customer_id, amount desc"#;

        let expected = r#"load "sales.csv"
  | filter status == "completed"
  | mutate
      customer_sale_number =
        row_number() over (
          partition_by customer_id
          order_by amount desc
        ),
      customer_revenue =
        sum(amount) over (
          partition_by customer_id
        ),
      region_revenue =
        sum(amount) over (
          partition_by region
        )
  | mutate
      region_revenue_rank =
        dense_rank() over (
          order_by region_revenue desc
        )
  | select
      region,
      customer_id,
      amount,
      customer_sale_number,
      customer_revenue,
      region_revenue_rank
  | sort region_revenue_rank, customer_id, amount desc"#;

        assert_eq!(format_source(source).expect("formatted"), expected);
        assert_eq!(format_source(expected).expect("formatted"), expected);
    }

    #[test]
    fn keeps_short_item_stages_inline() {
        let source = r#"load "sales.csv"|select region, amount|sort amount desc"#;

        assert_eq!(
            format_source(source).expect("formatted"),
            r#"load "sales.csv"
  | select region, amount
  | sort amount desc"#
        );
    }
}
