// AST-to-JSON converters and text helpers extracted from `render.rs` as part
// of the v0.42 split. See `render.rs` for the cross-module layout overview.

use pdl_core::Span;
use pdl_syntax::{
    AggItem, BinaryOp, Binding, CompleteFillItem, ContextDecl, ContextKind, Expr, FrameBound,
    JoinOn, MutateItem, OutputDecl, Pipeline, PipelineStart, Program, SaveStage, SinkRef,
    SortDirection, SourceRef, Stage, UnaryOp, UnionOptionKind, WindowFrame, WindowSpec,
};
use serde::Serialize;

use crate::render::span_json::{spanned_json, SpannedJson};

#[derive(Serialize)]
pub(crate) struct ProgramJson {
    pub(crate) contexts: Vec<ContextDeclJson>,
    pub(crate) bindings: Vec<BindingJson>,
    pub(crate) outputs: Vec<OutputJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) main: Option<PipelineJson>,
}

#[derive(Serialize)]
pub(crate) struct ContextDeclJson {
    pub(crate) context_kind: &'static str,
    pub(crate) name: SpannedJson<String>,
    pub(crate) default: ExprJson,
    pub(crate) span: Span,
}

#[derive(Serialize)]
pub(crate) struct BindingJson {
    pub(crate) name: SpannedJson<String>,
    pub(crate) pipeline: PipelineJson,
}

#[derive(Serialize)]
pub(crate) struct OutputJson {
    pub(crate) name: SpannedJson<String>,
    pub(crate) pipeline: PipelineJson,
}

#[derive(Serialize)]
pub(crate) struct PipelineJson {
    pub(crate) start: PipelineStartJson,
    pub(crate) stages: Vec<StageJson>,
    pub(crate) span: Span,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum PipelineStartJson {
    Load {
        source: SourceJson,
        #[serde(skip_serializing_if = "Option::is_none")]
        format: Option<SpannedJson<String>>,
        span: Span,
    },
    Binding {
        name: SpannedJson<String>,
    },
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum SourceJson {
    Path { path: SpannedJson<String> },
    Stdin { span: Span },
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum SinkJsonAst {
    Path { path: SpannedJson<String> },
    Stdout { span: Span },
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum StageJson {
    Filter {
        expr: ExprJson,
        span: Span,
    },
    Select {
        items: Vec<SelectItemJson>,
        span: Span,
    },
    Drop {
        columns: Vec<SpannedJson<String>>,
        span: Span,
    },
    Rename {
        items: Vec<RenameItemJson>,
        span: Span,
    },
    Mutate {
        items: Vec<MutateItemJson>,
        span: Span,
    },
    GroupBy {
        columns: Vec<SpannedJson<String>>,
        span: Span,
    },
    Agg {
        items: Vec<AggItemJson>,
        span: Span,
    },
    Sort {
        items: Vec<SortItemJson>,
        span: Span,
    },
    Limit {
        n: usize,
        span: Span,
    },
    Join {
        source: SpannedJson<String>,
        on: JoinOnJson,
        join_kind: &'static str,
        #[serde(skip_serializing_if = "Option::is_none")]
        kind_span: Option<Span>,
        span: Span,
    },
    Union {
        source: SpannedJson<String>,
        options: Vec<UnionOptionJson>,
        span: Span,
    },
    Distinct {
        columns: Vec<SpannedJson<String>>,
        span: Span,
    },
    PivotLonger {
        columns: Vec<SpannedJson<String>>,
        names_to: SpannedJson<String>,
        values_to: SpannedJson<String>,
        span: Span,
    },
    Complete {
        keys: Vec<SpannedJson<String>>,
        fills: Vec<CompleteFillItemJson>,
        span: Span,
    },
    Save {
        sink: SinkJsonAst,
        #[serde(skip_serializing_if = "Option::is_none")]
        format: Option<SpannedJson<String>>,
        span: Span,
    },
    Unsupported {
        name: SpannedJson<String>,
        span: Span,
    },
}

#[derive(Serialize)]
pub(crate) struct SelectItemJson {
    pub(crate) column: SpannedJson<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) alias: Option<SpannedJson<String>>,
}

#[derive(Serialize)]
pub(crate) struct RenameItemJson {
    pub(crate) old: SpannedJson<String>,
    pub(crate) new: SpannedJson<String>,
}

#[derive(Serialize)]
pub(crate) struct MutateItemJson {
    pub(crate) column: SpannedJson<String>,
    pub(crate) expr: ExprJson,
    pub(crate) span: Span,
}

#[derive(Serialize)]
pub(crate) struct CompleteFillItemJson {
    pub(crate) column: SpannedJson<String>,
    pub(crate) expr: ExprJson,
    pub(crate) span: Span,
}

#[derive(Serialize)]
pub(crate) struct AggItemJson {
    pub(crate) function: SpannedJson<String>,
    pub(crate) args: Vec<ExprJson>,
    pub(crate) alias: SpannedJson<String>,
    pub(crate) span: Span,
}

#[derive(Serialize)]
pub(crate) struct SortItemJson {
    pub(crate) column: SpannedJson<String>,
    pub(crate) direction: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) nulls: Option<&'static str>,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum JoinOnJson {
    Same {
        column: SpannedJson<String>,
    },
    Pair {
        left: SpannedJson<String>,
        right: SpannedJson<String>,
        span: Span,
    },
    Composite {
        keys: Vec<JoinKeyJson>,
        span: Span,
    },
}

#[derive(Serialize)]
pub(crate) struct JoinKeyJson {
    pub(crate) left: SpannedJson<String>,
    pub(crate) right: SpannedJson<String>,
    pub(crate) span: Span,
}

#[derive(Serialize)]
pub(crate) struct UnionOptionJson {
    pub(crate) option: &'static str,
    pub(crate) value: SpannedJson<bool>,
    pub(crate) span: Span,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum ExprJson {
    Quoted {
        value: SpannedJson<String>,
    },
    Number {
        value: SpannedJson<f64>,
    },
    Bool {
        value: SpannedJson<bool>,
    },
    Null {
        span: Span,
    },
    Ident {
        value: SpannedJson<String>,
    },
    Context {
        context_kind: &'static str,
        name: SpannedJson<String>,
        span: Span,
    },
    Call {
        name: SpannedJson<String>,
        args: Vec<ExprJson>,
        span: Span,
    },
    Window {
        function: SpannedJson<String>,
        args: Vec<ExprJson>,
        spec: WindowSpecJson,
        span: Span,
    },
    Unary {
        op: &'static str,
        expr: Box<ExprJson>,
        span: Span,
    },
    Binary {
        left: Box<ExprJson>,
        op: &'static str,
        right: Box<ExprJson>,
        span: Span,
    },
}

#[derive(Serialize)]
pub(crate) struct WindowSpecJson {
    pub(crate) partition_by: Vec<SpannedJson<String>>,
    pub(crate) order_by: Vec<SortItemJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) frame: Option<WindowFrameJson>,
    pub(crate) span: Span,
}

#[derive(Serialize)]
pub(crate) struct WindowFrameJson {
    pub(crate) start: FrameBoundJson,
    pub(crate) end: FrameBoundJson,
    pub(crate) span: Span,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum FrameBoundJson {
    UnboundedPreceding { span: Span },
    Preceding { rows: usize, span: Span },
    CurrentRow { span: Span },
    Following { rows: usize, span: Span },
    UnboundedFollowing { span: Span },
}

pub(crate) fn program_json(program: &Program) -> ProgramJson {
    ProgramJson {
        contexts: program.contexts.iter().map(context_decl_json).collect(),
        bindings: program.bindings.iter().map(binding_json).collect(),
        outputs: program.outputs.iter().map(output_json).collect(),
        main: program.main.as_ref().map(pipeline_json),
    }
}

fn context_decl_json(context: &ContextDecl) -> ContextDeclJson {
    ContextDeclJson {
        context_kind: context_kind_text(context.kind),
        name: spanned_json(&context.name),
        default: expr_json(&context.default),
        span: context.span,
    }
}

fn binding_json(binding: &Binding) -> BindingJson {
    BindingJson {
        name: spanned_json(&binding.name),
        pipeline: pipeline_json(&binding.pipeline),
    }
}

fn output_json(output: &OutputDecl) -> OutputJson {
    OutputJson {
        name: spanned_json(&output.name),
        pipeline: pipeline_json(&output.pipeline),
    }
}

fn pipeline_json(pipeline: &Pipeline) -> PipelineJson {
    PipelineJson {
        start: pipeline_start_json(&pipeline.start),
        stages: pipeline.stages.iter().map(stage_json).collect(),
        span: pipeline.span,
    }
}

fn pipeline_start_json(start: &PipelineStart) -> PipelineStartJson {
    match start {
        PipelineStart::Load(load) => PipelineStartJson::Load {
            source: source_json(&load.source),
            format: load.format.as_ref().map(spanned_json),
            span: load.span,
        },
        PipelineStart::Binding(name) => PipelineStartJson::Binding {
            name: spanned_json(name),
        },
    }
}

fn source_json(source: &SourceRef) -> SourceJson {
    match source {
        SourceRef::Path(path) => SourceJson::Path {
            path: spanned_json(path),
        },
        SourceRef::Stdin(span) => SourceJson::Stdin { span: *span },
    }
}

fn sink_ast_json(sink: &SinkRef) -> SinkJsonAst {
    match sink {
        SinkRef::Path(path) => SinkJsonAst::Path {
            path: spanned_json(path),
        },
        SinkRef::Stdout(span) => SinkJsonAst::Stdout { span: *span },
    }
}

fn stage_json(stage: &Stage) -> StageJson {
    match stage {
        Stage::Filter { expr, span } => StageJson::Filter {
            expr: expr_json(expr),
            span: *span,
        },
        Stage::Select { items, span } => StageJson::Select {
            items: items
                .iter()
                .map(|item| SelectItemJson {
                    column: spanned_json(&item.column),
                    alias: item.alias.as_ref().map(spanned_json),
                })
                .collect(),
            span: *span,
        },
        Stage::Drop { columns, span } => StageJson::Drop {
            columns: columns.iter().map(spanned_json).collect(),
            span: *span,
        },
        Stage::Rename { items, span } => StageJson::Rename {
            items: items
                .iter()
                .map(|item| RenameItemJson {
                    old: spanned_json(&item.old),
                    new: spanned_json(&item.new),
                })
                .collect(),
            span: *span,
        },
        Stage::Mutate { items, span } => StageJson::Mutate {
            items: items.iter().map(mutate_item_json).collect(),
            span: *span,
        },
        Stage::GroupBy { columns, span } => StageJson::GroupBy {
            columns: columns.iter().map(spanned_json).collect(),
            span: *span,
        },
        Stage::Agg { items, span } => StageJson::Agg {
            items: items.iter().map(agg_item_json).collect(),
            span: *span,
        },
        Stage::Sort { items, span } => StageJson::Sort {
            items: items
                .iter()
                .map(|item| SortItemJson {
                    column: spanned_json(&item.column),
                    direction: sort_direction_text(item.direction),
                    nulls: item.nulls.map(|nulls| match nulls {
                        pdl_syntax::NullsOrder::First => "first",
                        pdl_syntax::NullsOrder::Last => "last",
                    }),
                })
                .collect(),
            span: *span,
        },
        Stage::Limit { n, span } => StageJson::Limit { n: *n, span: *span },
        Stage::Join {
            source,
            on,
            kind,
            kind_span,
            span,
        } => StageJson::Join {
            source: spanned_json(source),
            on: join_on_json(on),
            join_kind: kind.as_str(),
            kind_span: *kind_span,
            span: *span,
        },
        Stage::Union {
            source,
            options,
            span,
        } => StageJson::Union {
            source: spanned_json(source),
            options: options
                .iter()
                .map(|option| UnionOptionJson {
                    option: match option.kind {
                        UnionOptionKind::ByName => "by_name",
                        UnionOptionKind::Distinct => "distinct",
                    },
                    value: spanned_json(&option.value),
                    span: option.span,
                })
                .collect(),
            span: *span,
        },
        Stage::Distinct { columns, span } => StageJson::Distinct {
            columns: columns.iter().map(spanned_json).collect(),
            span: *span,
        },
        Stage::PivotLonger {
            columns,
            names_to,
            values_to,
            span,
        } => StageJson::PivotLonger {
            columns: columns.iter().map(spanned_json).collect(),
            names_to: spanned_json(names_to),
            values_to: spanned_json(values_to),
            span: *span,
        },
        Stage::Complete { keys, fills, span } => StageJson::Complete {
            keys: keys.iter().map(spanned_json).collect(),
            fills: fills.iter().map(complete_fill_item_json).collect(),
            span: *span,
        },
        Stage::Save(save) => save_stage_json(save),
        Stage::Unsupported { name, span } => StageJson::Unsupported {
            name: spanned_json(name),
            span: *span,
        },
    }
}

fn save_stage_json(save: &SaveStage) -> StageJson {
    StageJson::Save {
        sink: sink_ast_json(&save.sink),
        format: save.format.as_ref().map(spanned_json),
        span: save.span,
    }
}

fn mutate_item_json(item: &MutateItem) -> MutateItemJson {
    MutateItemJson {
        column: spanned_json(&item.column),
        expr: expr_json(&item.expr),
        span: item.span,
    }
}

fn complete_fill_item_json(item: &CompleteFillItem) -> CompleteFillItemJson {
    CompleteFillItemJson {
        column: spanned_json(&item.column),
        expr: expr_json(&item.expr),
        span: item.span,
    }
}

fn agg_item_json(item: &AggItem) -> AggItemJson {
    AggItemJson {
        function: spanned_json(&item.function),
        args: item.args.iter().map(expr_json).collect(),
        alias: spanned_json(&item.alias),
        span: item.span,
    }
}

fn join_on_json(on: &JoinOn) -> JoinOnJson {
    match on {
        JoinOn::Same(column) => JoinOnJson::Same {
            column: spanned_json(column),
        },
        JoinOn::Pair { left, right, span } => JoinOnJson::Pair {
            left: spanned_json(left),
            right: spanned_json(right),
            span: *span,
        },
        JoinOn::Composite { keys, span } => JoinOnJson::Composite {
            keys: keys
                .iter()
                .map(|key| JoinKeyJson {
                    left: spanned_json(&key.left),
                    right: spanned_json(&key.right),
                    span: key.span,
                })
                .collect(),
            span: *span,
        },
    }
}

fn expr_json(expr: &Expr) -> ExprJson {
    match expr {
        Expr::Quoted(value) => ExprJson::Quoted {
            value: spanned_json(value),
        },
        Expr::Number(value) => ExprJson::Number {
            value: spanned_json(value),
        },
        Expr::Bool(value) => ExprJson::Bool {
            value: spanned_json(value),
        },
        Expr::Null(span) => ExprJson::Null { span: *span },
        Expr::Ident(value) => ExprJson::Ident {
            value: spanned_json(value),
        },
        Expr::Context { kind, name, span } => ExprJson::Context {
            context_kind: context_kind_text(*kind),
            name: spanned_json(name),
            span: *span,
        },
        Expr::Call { name, args, span } => ExprJson::Call {
            name: spanned_json(name),
            args: args.iter().map(expr_json).collect(),
            span: *span,
        },
        Expr::Window {
            function,
            args,
            spec,
            span,
        } => ExprJson::Window {
            function: spanned_json(function),
            args: args.iter().map(expr_json).collect(),
            spec: window_spec_json(spec),
            span: *span,
        },
        Expr::Unary { op, expr, span } => ExprJson::Unary {
            op: unary_op_text(*op),
            expr: Box::new(expr_json(expr)),
            span: *span,
        },
        Expr::Binary {
            left,
            op,
            right,
            span,
        } => ExprJson::Binary {
            left: Box::new(expr_json(left)),
            op: binary_op_text(*op),
            right: Box::new(expr_json(right)),
            span: *span,
        },
    }
}

fn window_spec_json(spec: &WindowSpec) -> WindowSpecJson {
    WindowSpecJson {
        partition_by: spec.partition_by.iter().map(spanned_json).collect(),
        order_by: spec
            .order_by
            .iter()
            .map(|item| SortItemJson {
                column: spanned_json(&item.column),
                direction: sort_direction_text(item.direction),
                nulls: item.nulls.map(|nulls| match nulls {
                    pdl_syntax::NullsOrder::First => "first",
                    pdl_syntax::NullsOrder::Last => "last",
                }),
            })
            .collect(),
        frame: spec.frame.as_ref().map(window_frame_json),
        span: spec.span,
    }
}

fn window_frame_json(frame: &WindowFrame) -> WindowFrameJson {
    WindowFrameJson {
        start: frame_bound_json(&frame.start),
        end: frame_bound_json(&frame.end),
        span: frame.span,
    }
}

fn frame_bound_json(bound: &FrameBound) -> FrameBoundJson {
    match bound {
        FrameBound::UnboundedPreceding { span } => {
            FrameBoundJson::UnboundedPreceding { span: *span }
        }
        FrameBound::Preceding { rows, span } => FrameBoundJson::Preceding {
            rows: *rows,
            span: *span,
        },
        FrameBound::CurrentRow { span } => FrameBoundJson::CurrentRow { span: *span },
        FrameBound::Following { rows, span } => FrameBoundJson::Following {
            rows: *rows,
            span: *span,
        },
        FrameBound::UnboundedFollowing { span } => {
            FrameBoundJson::UnboundedFollowing { span: *span }
        }
    }
}

fn sort_direction_text(direction: SortDirection) -> &'static str {
    match direction {
        SortDirection::Asc => "asc",
        SortDirection::Desc => "desc",
    }
}

fn unary_op_text(op: UnaryOp) -> &'static str {
    match op {
        UnaryOp::Not => "not",
        UnaryOp::Neg => "neg",
    }
}

fn binary_op_text(op: BinaryOp) -> &'static str {
    match op {
        BinaryOp::Or => "or",
        BinaryOp::And => "and",
        BinaryOp::Eq => "eq",
        BinaryOp::Ne => "ne",
        BinaryOp::Lt => "lt",
        BinaryOp::Lte => "lte",
        BinaryOp::Gt => "gt",
        BinaryOp::Gte => "gte",
        BinaryOp::Add => "add",
        BinaryOp::Sub => "sub",
        BinaryOp::Mul => "mul",
        BinaryOp::Div => "div",
        BinaryOp::Rem => "rem",
    }
}

fn context_kind_text(kind: ContextKind) -> &'static str {
    match kind {
        ContextKind::Param => "param",
        ContextKind::State => "state",
    }
}
