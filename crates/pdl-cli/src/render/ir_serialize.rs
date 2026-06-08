// IR-to-JSON converters and text helpers extracted from `render.rs` as part of
// the v0.42 split. See `render.rs` for the cross-module layout overview.

use pdl_core::Span;
use pdl_semantics::{
    BinaryOpIr, CompleteFillItemIr, ContextDeclIr, ContextKindIr, ExprIr, FrameBoundIr, JoinKindIr,
    PipelineStartIr, ProgramIr, SinkIr, SortDirectionIr, StageIr, UnaryOpIr, WindowFrameIr,
    WindowSpecIr,
};
use serde::Serialize;

#[derive(Serialize)]
pub(crate) struct ProgramIrJson {
    pub(crate) contexts: Vec<ContextDeclIrJson>,
    pub(crate) bindings: Vec<BindingIrJson>,
    pub(crate) outputs: Vec<OutputIrJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) main: Option<PipelineIrJson>,
}

#[derive(Serialize)]
pub(crate) struct ContextDeclIrJson {
    pub(crate) context_kind: &'static str,
    pub(crate) name: String,
    pub(crate) span: Span,
    pub(crate) default: ExprIrJson,
}

#[derive(Serialize)]
pub(crate) struct BindingIrJson {
    pub(crate) name: String,
    pub(crate) span: Span,
    pub(crate) pipeline: PipelineIrJson,
}

#[derive(Serialize)]
pub(crate) struct OutputIrJson {
    pub(crate) name: String,
    pub(crate) span: Span,
    pub(crate) pipeline: PipelineIrJson,
}

#[derive(Serialize)]
pub(crate) struct PipelineIrJson {
    pub(crate) start: PipelineStartIrJson,
    pub(crate) stages: Vec<StageIrJson>,
    pub(crate) span: Span,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum PipelineStartIrJson {
    Load {
        source: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        format: Option<String>,
        span: Span,
    },
    Binding {
        name: String,
        span: Span,
    },
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum StageIrJson {
    Filter {
        expr: ExprIrJson,
        span: Span,
    },
    Select {
        items: Vec<SelectItemIrJson>,
        span: Span,
    },
    Drop {
        columns: Vec<String>,
        span: Span,
    },
    Rename {
        items: Vec<RenameItemIrJson>,
        span: Span,
    },
    Mutate {
        items: Vec<MutateItemIrJson>,
        span: Span,
    },
    GroupBy {
        columns: Vec<String>,
        span: Span,
    },
    Agg {
        items: Vec<AggItemIrJson>,
        span: Span,
    },
    Sort {
        items: Vec<SortItemIrJson>,
        span: Span,
    },
    Limit {
        n: usize,
        span: Span,
    },
    Join {
        source: String,
        source_span: Span,
        left_key: String,
        right_key: String,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        keys: Vec<JoinKeyIrJson>,
        join_kind: &'static str,
        span: Span,
    },
    Union {
        source: String,
        source_span: Span,
        by_name: bool,
        distinct: bool,
        span: Span,
    },
    Distinct {
        columns: Vec<String>,
        span: Span,
    },
    PivotLonger {
        columns: Vec<String>,
        names_to: String,
        values_to: String,
        span: Span,
    },
    Complete {
        keys: Vec<String>,
        fills: Vec<CompleteFillItemIrJson>,
        span: Span,
    },
    Save {
        sink: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        format: Option<String>,
        span: Span,
    },
    Unsupported {
        name: String,
        span: Span,
    },
}

#[derive(Serialize)]
pub(crate) struct SelectItemIrJson {
    pub(crate) source: String,
    pub(crate) output: String,
    pub(crate) span: Span,
}

#[derive(Serialize)]
pub(crate) struct RenameItemIrJson {
    pub(crate) old: String,
    pub(crate) new: String,
    pub(crate) span: Span,
}

#[derive(Serialize)]
pub(crate) struct MutateItemIrJson {
    pub(crate) column: String,
    pub(crate) expr: ExprIrJson,
    pub(crate) span: Span,
}

#[derive(Serialize)]
pub(crate) struct CompleteFillItemIrJson {
    pub(crate) column: String,
    pub(crate) expr: ExprIrJson,
    pub(crate) span: Span,
}

#[derive(Serialize)]
pub(crate) struct AggItemIrJson {
    pub(crate) function: String,
    pub(crate) args: Vec<ExprIrJson>,
    pub(crate) alias: String,
    pub(crate) span: Span,
}

#[derive(Serialize)]
pub(crate) struct SortItemIrJson {
    pub(crate) column: String,
    pub(crate) direction: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) nulls: Option<&'static str>,
    pub(crate) span: Span,
}

#[derive(Serialize)]
pub(crate) struct JoinKeyIrJson {
    pub(crate) left: String,
    pub(crate) right: String,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum ExprIrJson {
    Quoted {
        value: String,
        span: Span,
    },
    Number {
        value: f64,
        span: Span,
    },
    Bool {
        value: bool,
        span: Span,
    },
    Null {
        span: Span,
    },
    Ident {
        value: String,
        span: Span,
    },
    Context {
        context_kind: &'static str,
        name: String,
        span: Span,
    },
    Call {
        name: String,
        args: Vec<ExprIrJson>,
        span: Span,
    },
    Window {
        function: String,
        args: Vec<ExprIrJson>,
        spec: WindowSpecIrJson,
        span: Span,
    },
    Unary {
        op: &'static str,
        expr: Box<ExprIrJson>,
        span: Span,
    },
    Binary {
        left: Box<ExprIrJson>,
        op: &'static str,
        right: Box<ExprIrJson>,
        span: Span,
    },
}

#[derive(Serialize)]
pub(crate) struct WindowSpecIrJson {
    pub(crate) partition_by: Vec<String>,
    pub(crate) order_by: Vec<SortItemIrJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) frame: Option<WindowFrameIrJson>,
    pub(crate) span: Span,
}

#[derive(Serialize)]
pub(crate) struct WindowFrameIrJson {
    pub(crate) start: FrameBoundIrJson,
    pub(crate) end: FrameBoundIrJson,
    pub(crate) span: Span,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum FrameBoundIrJson {
    UnboundedPreceding { span: Span },
    Preceding { rows: usize, span: Span },
    CurrentRow { span: Span },
    Following { rows: usize, span: Span },
    UnboundedFollowing { span: Span },
}

pub(crate) fn program_ir_json(ir: &ProgramIr) -> ProgramIrJson {
    ProgramIrJson {
        contexts: ir.contexts.iter().map(context_decl_ir_json).collect(),
        bindings: ir
            .bindings
            .iter()
            .map(|binding| BindingIrJson {
                name: binding.name.clone(),
                span: binding.span,
                pipeline: PipelineIrJson {
                    start: pipeline_start_ir_json(&binding.pipeline.start),
                    stages: binding.pipeline.stages.iter().map(stage_ir_json).collect(),
                    span: binding.pipeline.span,
                },
            })
            .collect(),
        outputs: ir
            .outputs
            .iter()
            .map(|output| OutputIrJson {
                name: output.name.clone(),
                span: output.span,
                pipeline: PipelineIrJson {
                    start: pipeline_start_ir_json(&output.pipeline.start),
                    stages: output.pipeline.stages.iter().map(stage_ir_json).collect(),
                    span: output.pipeline.span,
                },
            })
            .collect(),
        main: ir.main.as_ref().map(|pipeline| PipelineIrJson {
            start: pipeline_start_ir_json(&pipeline.start),
            stages: pipeline.stages.iter().map(stage_ir_json).collect(),
            span: pipeline.span,
        }),
    }
}

fn context_decl_ir_json(context: &ContextDeclIr) -> ContextDeclIrJson {
    ContextDeclIrJson {
        context_kind: context_kind_ir_text(context.kind),
        name: context.name.clone(),
        span: context.span,
        default: expr_ir_json(&context.default),
    }
}

fn pipeline_start_ir_json(start: &PipelineStartIr) -> PipelineStartIrJson {
    match start {
        PipelineStartIr::Load {
            source,
            format,
            span,
        } => PipelineStartIrJson::Load {
            source: match source {
                pdl_semantics::SourceIr::Path(path) => path.clone(),
                pdl_semantics::SourceIr::Stdin => "stdin".to_string(),
            },
            format: format.clone(),
            span: *span,
        },
        PipelineStartIr::Binding { name, span } => PipelineStartIrJson::Binding {
            name: name.clone(),
            span: *span,
        },
    }
}

fn stage_ir_json(stage: &StageIr) -> StageIrJson {
    match stage {
        StageIr::Filter { expr, span } => StageIrJson::Filter {
            expr: expr_ir_json(expr),
            span: *span,
        },
        StageIr::Select { items, span } => StageIrJson::Select {
            items: items
                .iter()
                .map(|item| SelectItemIrJson {
                    source: item.source.clone(),
                    output: item.output.clone(),
                    span: item.span,
                })
                .collect(),
            span: *span,
        },
        StageIr::Drop { columns, span } => StageIrJson::Drop {
            columns: columns.clone(),
            span: *span,
        },
        StageIr::Rename { items, span } => StageIrJson::Rename {
            items: items
                .iter()
                .map(|item| RenameItemIrJson {
                    old: item.old.clone(),
                    new: item.new.clone(),
                    span: item.span,
                })
                .collect(),
            span: *span,
        },
        StageIr::Mutate { items, span } => StageIrJson::Mutate {
            items: items
                .iter()
                .map(|item| MutateItemIrJson {
                    column: item.column.clone(),
                    expr: expr_ir_json(&item.expr),
                    span: item.span,
                })
                .collect(),
            span: *span,
        },
        StageIr::GroupBy { columns, span } => StageIrJson::GroupBy {
            columns: columns.clone(),
            span: *span,
        },
        StageIr::Agg { items, span } => StageIrJson::Agg {
            items: items
                .iter()
                .map(|item| AggItemIrJson {
                    function: item.function.clone(),
                    args: item.args.iter().map(expr_ir_json).collect(),
                    alias: item.alias.clone(),
                    span: item.span,
                })
                .collect(),
            span: *span,
        },
        StageIr::Sort { items, span } => StageIrJson::Sort {
            items: items
                .iter()
                .map(|item| SortItemIrJson {
                    column: item.column.clone(),
                    direction: match item.direction {
                        SortDirectionIr::Asc => "asc",
                        SortDirectionIr::Desc => "desc",
                    },
                    nulls: item.nulls.map(|nulls| match nulls {
                        pdl_semantics::NullsOrderIr::First => "first",
                        pdl_semantics::NullsOrderIr::Last => "last",
                    }),
                    span: item.span,
                })
                .collect(),
            span: *span,
        },
        StageIr::Limit { n, span } => StageIrJson::Limit { n: *n, span: *span },
        StageIr::Join {
            source,
            source_span,
            left_key,
            right_key,
            keys,
            kind,
            span,
        } => StageIrJson::Join {
            source: source.clone(),
            source_span: *source_span,
            left_key: left_key.clone(),
            right_key: right_key.clone(),
            keys: if keys.len() > 1 {
                keys.iter()
                    .map(|key| JoinKeyIrJson {
                        left: key.left.clone(),
                        right: key.right.clone(),
                    })
                    .collect()
            } else {
                Vec::new()
            },
            join_kind: join_kind_ir_text(*kind),
            span: *span,
        },
        StageIr::Union {
            source,
            source_span,
            by_name,
            distinct,
            span,
        } => StageIrJson::Union {
            source: source.clone(),
            source_span: *source_span,
            by_name: *by_name,
            distinct: *distinct,
            span: *span,
        },
        StageIr::Distinct { columns, span } => StageIrJson::Distinct {
            columns: columns.clone(),
            span: *span,
        },
        StageIr::PivotLonger {
            columns,
            names_to,
            values_to,
            span,
        } => StageIrJson::PivotLonger {
            columns: columns.clone(),
            names_to: names_to.clone(),
            values_to: values_to.clone(),
            span: *span,
        },
        StageIr::Complete { keys, fills, span } => StageIrJson::Complete {
            keys: keys.clone(),
            fills: fills.iter().map(complete_fill_item_ir_json).collect(),
            span: *span,
        },
        StageIr::Save { sink, format, span } => StageIrJson::Save {
            sink: match sink {
                SinkIr::Path(path) => path.clone(),
                SinkIr::Stdout => "stdout".to_string(),
            },
            format: format.clone(),
            span: *span,
        },
        StageIr::Unsupported { name, span } => StageIrJson::Unsupported {
            name: name.clone(),
            span: *span,
        },
    }
}

fn complete_fill_item_ir_json(item: &CompleteFillItemIr) -> CompleteFillItemIrJson {
    CompleteFillItemIrJson {
        column: item.column.clone(),
        expr: expr_ir_json(&item.expr),
        span: item.span,
    }
}

fn expr_ir_json(expr: &ExprIr) -> ExprIrJson {
    match expr {
        ExprIr::Quoted { value, span } => ExprIrJson::Quoted {
            value: value.clone(),
            span: *span,
        },
        ExprIr::Number { value, span } => ExprIrJson::Number {
            value: *value,
            span: *span,
        },
        ExprIr::Bool { value, span } => ExprIrJson::Bool {
            value: *value,
            span: *span,
        },
        ExprIr::Null { span } => ExprIrJson::Null { span: *span },
        ExprIr::Ident { value, span } => ExprIrJson::Ident {
            value: value.clone(),
            span: *span,
        },
        ExprIr::Context { kind, name, span } => ExprIrJson::Context {
            context_kind: context_kind_ir_text(*kind),
            name: name.clone(),
            span: *span,
        },
        ExprIr::Call { name, args, span } => ExprIrJson::Call {
            name: name.clone(),
            args: args.iter().map(expr_ir_json).collect(),
            span: *span,
        },
        ExprIr::Window {
            function,
            args,
            spec,
            span,
        } => ExprIrJson::Window {
            function: function.clone(),
            args: args.iter().map(expr_ir_json).collect(),
            spec: window_spec_ir_json(spec),
            span: *span,
        },
        ExprIr::Unary { op, expr, span } => ExprIrJson::Unary {
            op: unary_op_ir_text(*op),
            expr: Box::new(expr_ir_json(expr)),
            span: *span,
        },
        ExprIr::Binary {
            left,
            op,
            right,
            span,
        } => ExprIrJson::Binary {
            left: Box::new(expr_ir_json(left)),
            op: binary_op_ir_text(*op),
            right: Box::new(expr_ir_json(right)),
            span: *span,
        },
    }
}

fn window_spec_ir_json(spec: &WindowSpecIr) -> WindowSpecIrJson {
    WindowSpecIrJson {
        partition_by: spec.partition_by.clone(),
        order_by: spec
            .order_by
            .iter()
            .map(|item| SortItemIrJson {
                column: item.column.clone(),
                direction: match item.direction {
                    SortDirectionIr::Asc => "asc",
                    SortDirectionIr::Desc => "desc",
                },
                nulls: item.nulls.map(|nulls| match nulls {
                    pdl_semantics::NullsOrderIr::First => "first",
                    pdl_semantics::NullsOrderIr::Last => "last",
                }),
                span: item.span,
            })
            .collect(),
        frame: spec.frame.as_ref().map(window_frame_ir_json),
        span: spec.span,
    }
}

fn window_frame_ir_json(frame: &WindowFrameIr) -> WindowFrameIrJson {
    WindowFrameIrJson {
        start: frame_bound_ir_json(&frame.start),
        end: frame_bound_ir_json(&frame.end),
        span: frame.span,
    }
}

fn frame_bound_ir_json(bound: &FrameBoundIr) -> FrameBoundIrJson {
    match bound {
        FrameBoundIr::UnboundedPreceding { span } => {
            FrameBoundIrJson::UnboundedPreceding { span: *span }
        }
        FrameBoundIr::Preceding { rows, span } => FrameBoundIrJson::Preceding {
            rows: *rows,
            span: *span,
        },
        FrameBoundIr::CurrentRow { span } => FrameBoundIrJson::CurrentRow { span: *span },
        FrameBoundIr::Following { rows, span } => FrameBoundIrJson::Following {
            rows: *rows,
            span: *span,
        },
        FrameBoundIr::UnboundedFollowing { span } => {
            FrameBoundIrJson::UnboundedFollowing { span: *span }
        }
    }
}

fn join_kind_ir_text(kind: JoinKindIr) -> &'static str {
    match kind {
        JoinKindIr::Inner => "inner",
        JoinKindIr::Left => "left",
        JoinKindIr::Right => "right",
        JoinKindIr::Full => "full",
        JoinKindIr::Semi => "semi",
        JoinKindIr::Anti => "anti",
    }
}

fn unary_op_ir_text(op: UnaryOpIr) -> &'static str {
    match op {
        UnaryOpIr::Not => "not",
        UnaryOpIr::Neg => "neg",
    }
}

fn binary_op_ir_text(op: BinaryOpIr) -> &'static str {
    match op {
        BinaryOpIr::Or => "or",
        BinaryOpIr::And => "and",
        BinaryOpIr::Eq => "eq",
        BinaryOpIr::Ne => "ne",
        BinaryOpIr::Lt => "lt",
        BinaryOpIr::Lte => "lte",
        BinaryOpIr::Gt => "gt",
        BinaryOpIr::Gte => "gte",
        BinaryOpIr::Add => "add",
        BinaryOpIr::Sub => "sub",
        BinaryOpIr::Mul => "mul",
        BinaryOpIr::Div => "div",
        BinaryOpIr::Rem => "rem",
    }
}

fn context_kind_ir_text(kind: ContextKindIr) -> &'static str {
    match kind {
        ContextKindIr::Param => "param",
        ContextKindIr::State => "state",
    }
}
