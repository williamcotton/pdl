use pdl_core::Span;
use pdl_syntax::{
    AggItem, BinaryOp, CompleteFillItem, ContextKind, ControlKind, Expr, Pipeline, PipelineStart,
    Program, SinkRef, SortItem, SourceRef, Stage, UnaryOp, UnionOptionKind, WindowFrame,
    WindowFrameKind, WindowSpec,
};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProgramIr {
    pub contexts: Vec<ContextDeclIr>,
    pub bindings: Vec<BindingIr>,
    pub outputs: Vec<OutputIr>,
    pub main: Option<PipelineIr>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ContextDeclIr {
    pub kind: ContextKindIr,
    pub name: String,
    pub span: Span,
    pub default: ExprIr,
    pub control: Option<ControlInitializerIr>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContextKindIr {
    Param,
    State,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ControlInitializerIr {
    pub kind: ControlKindIr,
    pub span: Span,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControlKindIr {
    Text,
    Textarea,
    Number,
    Range,
    Checkbox,
    Select,
    Radio,
    Date,
    Time,
    Datetime,
    Color,
}

impl ControlKindIr {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Text => "input_text",
            Self::Textarea => "input_textarea",
            Self::Number => "input_number",
            Self::Range => "input_range",
            Self::Checkbox => "input_checkbox",
            Self::Select => "input_select",
            Self::Radio => "input_radio",
            Self::Date => "input_date",
            Self::Time => "input_time",
            Self::Datetime => "input_datetime",
            Self::Color => "input_color",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct BindingIr {
    pub name: String,
    pub span: Span,
    pub pipeline: PipelineIr,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OutputIr {
    pub name: String,
    pub span: Span,
    pub pipeline: PipelineIr,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PipelineIr {
    pub start: PipelineStartIr,
    pub stages: Vec<StageIr>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PipelineStartIr {
    Load {
        source: SourceIr,
        format: Option<String>,
        span: Span,
    },
    Binding {
        name: String,
        span: Span,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum SourceIr {
    Path(String),
    Stdin,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SinkIr {
    Path(String),
    Stdout,
}

#[derive(Clone, Debug, PartialEq)]
pub enum StageIr {
    Filter {
        expr: ExprIr,
        span: Span,
    },
    Select {
        items: Vec<SelectItemIr>,
        span: Span,
    },
    Drop {
        columns: Vec<String>,
        span: Span,
    },
    Rename {
        items: Vec<RenameItemIr>,
        span: Span,
    },
    Mutate {
        items: Vec<MutateItemIr>,
        span: Span,
    },
    GroupBy {
        columns: Vec<String>,
        span: Span,
    },
    Agg {
        items: Vec<AggItemIr>,
        span: Span,
    },
    Sort {
        items: Vec<SortItemIr>,
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
        keys: Vec<JoinKeyIr>,
        kind: JoinKindIr,
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
        fills: Vec<CompleteFillItemIr>,
        span: Span,
    },
    Save {
        sink: SinkIr,
        format: Option<String>,
        span: Span,
    },
    Unsupported {
        name: String,
        span: Span,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct SelectItemIr {
    pub source: String,
    pub output: String,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RenameItemIr {
    pub old: String,
    pub new: String,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MutateItemIr {
    pub column: String,
    pub expr: ExprIr,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CompleteFillItemIr {
    pub column: String,
    pub expr: ExprIr,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AggItemIr {
    pub function: String,
    pub args: Vec<ExprIr>,
    pub alias: String,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SortItemIr {
    pub column: String,
    pub direction: SortDirectionIr,
    pub nulls: Option<NullsOrderIr>,
    pub span: Span,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SortDirectionIr {
    Asc,
    Desc,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NullsOrderIr {
    First,
    Last,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JoinKindIr {
    Inner,
    Left,
    Right,
    Full,
    Semi,
    Anti,
}

#[derive(Clone, Debug, PartialEq)]
pub struct JoinKeyIr {
    pub left: String,
    pub right: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ExprIr {
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
        kind: ContextKindIr,
        name: String,
        span: Span,
    },
    Call {
        name: String,
        args: Vec<ExprIr>,
        span: Span,
    },
    Window {
        function: String,
        args: Vec<ExprIr>,
        spec: WindowSpecIr,
        span: Span,
    },
    Unary {
        op: UnaryOpIr,
        expr: Box<ExprIr>,
        span: Span,
    },
    Binary {
        left: Box<ExprIr>,
        op: BinaryOpIr,
        right: Box<ExprIr>,
        span: Span,
    },
}

impl ExprIr {
    pub fn span(&self) -> Span {
        match self {
            ExprIr::Quoted { span, .. }
            | ExprIr::Number { span, .. }
            | ExprIr::Bool { span, .. }
            | ExprIr::Null { span }
            | ExprIr::Ident { span, .. }
            | ExprIr::Context { span, .. }
            | ExprIr::Call { span, .. }
            | ExprIr::Window { span, .. }
            | ExprIr::Unary { span, .. }
            | ExprIr::Binary { span, .. } => *span,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct WindowSpecIr {
    pub partition_by: Vec<String>,
    pub order_by: Vec<SortItemIr>,
    pub frame: Option<WindowFrameIr>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WindowFrameIr {
    pub start: FrameBoundIr,
    pub end: FrameBoundIr,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum FrameBoundIr {
    UnboundedPreceding { span: Span },
    Preceding { rows: usize, span: Span },
    CurrentRow { span: Span },
    Following { rows: usize, span: Span },
    UnboundedFollowing { span: Span },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnaryOpIr {
    Not,
    Neg,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BinaryOpIr {
    Or,
    And,
    Eq,
    Ne,
    Lt,
    Lte,
    Gt,
    Gte,
    Add,
    Sub,
    Mul,
    Div,
    Rem,
}

pub fn lower_program(program: &Program) -> ProgramIr {
    ProgramIr {
        contexts: program
            .contexts
            .iter()
            .map(|context| ContextDeclIr {
                kind: lower_context_kind(context.kind),
                name: context.name.value.clone(),
                span: context.name.span,
                default: lower_expr(&context.default),
                control: context
                    .control
                    .as_ref()
                    .map(|control| ControlInitializerIr {
                        kind: lower_control_kind(control.kind),
                        span: control.span,
                    }),
            })
            .collect(),
        bindings: program
            .bindings
            .iter()
            .map(|binding| BindingIr {
                name: binding.name.value.clone(),
                span: binding.name.span,
                pipeline: lower_pipeline(&binding.pipeline),
            })
            .collect(),
        outputs: program
            .outputs
            .iter()
            .map(|output| OutputIr {
                name: output.name.value.clone(),
                span: output.name.span,
                pipeline: lower_pipeline(&output.pipeline),
            })
            .collect(),
        main: program.main.as_ref().map(lower_pipeline),
    }
}

fn lower_context_kind(kind: ContextKind) -> ContextKindIr {
    match kind {
        ContextKind::Param => ContextKindIr::Param,
        ContextKind::State => ContextKindIr::State,
    }
}

fn lower_control_kind(kind: ControlKind) -> ControlKindIr {
    match kind {
        ControlKind::Text => ControlKindIr::Text,
        ControlKind::Textarea => ControlKindIr::Textarea,
        ControlKind::Number => ControlKindIr::Number,
        ControlKind::Range => ControlKindIr::Range,
        ControlKind::Checkbox => ControlKindIr::Checkbox,
        ControlKind::Select => ControlKindIr::Select,
        ControlKind::Radio => ControlKindIr::Radio,
        ControlKind::Date => ControlKindIr::Date,
        ControlKind::Time => ControlKindIr::Time,
        ControlKind::Datetime => ControlKindIr::Datetime,
        ControlKind::Color => ControlKindIr::Color,
    }
}

pub fn decode_context_column_ref_ir(value: &str) -> Option<(ContextKindIr, &str)> {
    let (kind, name) = pdl_syntax::decode_context_column_ref(value)?;
    Some((lower_context_kind(kind), name))
}

fn lower_pipeline(pipeline: &Pipeline) -> PipelineIr {
    PipelineIr {
        start: lower_pipeline_start(&pipeline.start),
        stages: pipeline.stages.iter().map(lower_stage).collect(),
        span: pipeline.span,
    }
}

fn lower_pipeline_start(start: &PipelineStart) -> PipelineStartIr {
    match start {
        PipelineStart::Load(load) => PipelineStartIr::Load {
            source: match &load.source {
                SourceRef::Path(path) => SourceIr::Path(path.value.clone()),
                SourceRef::Stdin(_) => SourceIr::Stdin,
            },
            format: load.format.as_ref().map(|format| format.value.clone()),
            span: load.span,
        },
        PipelineStart::Binding(name) => PipelineStartIr::Binding {
            name: name.value.clone(),
            span: name.span,
        },
    }
}

fn lower_stage(stage: &Stage) -> StageIr {
    match stage {
        Stage::Filter { expr, span } => StageIr::Filter {
            expr: lower_expr(expr),
            span: *span,
        },
        Stage::Select { items, span } => StageIr::Select {
            items: items
                .iter()
                .map(|item| SelectItemIr {
                    source: item.column.value.clone(),
                    output: item.alias.as_ref().unwrap_or(&item.column).value.clone(),
                    span: item
                        .alias
                        .as_ref()
                        .map_or(item.column.span, |alias| item.column.span.join(alias.span)),
                })
                .collect(),
            span: *span,
        },
        Stage::Drop { columns, span } => StageIr::Drop {
            columns: columns.iter().map(|column| column.value.clone()).collect(),
            span: *span,
        },
        Stage::Rename { items, span } => StageIr::Rename {
            items: items
                .iter()
                .map(|item| RenameItemIr {
                    old: item.old.value.clone(),
                    new: item.new.value.clone(),
                    span: item.old.span.join(item.new.span),
                })
                .collect(),
            span: *span,
        },
        Stage::Mutate { items, span } => StageIr::Mutate {
            items: items
                .iter()
                .map(|item| MutateItemIr {
                    column: item.column.value.clone(),
                    expr: lower_expr(&item.expr),
                    span: item.span,
                })
                .collect(),
            span: *span,
        },
        Stage::GroupBy { columns, span } => StageIr::GroupBy {
            columns: columns.iter().map(|column| column.value.clone()).collect(),
            span: *span,
        },
        Stage::Agg { items, span } => StageIr::Agg {
            items: items.iter().map(lower_agg_item).collect(),
            span: *span,
        },
        Stage::Sort { items, span } => StageIr::Sort {
            items: items.iter().map(lower_sort_item).collect(),
            span: *span,
        },
        Stage::Limit { n, span } => StageIr::Limit { n: *n, span: *span },
        Stage::Join {
            source,
            on,
            kind,
            span,
            ..
        } => StageIr::Join {
            source: source.value.clone(),
            source_span: source.span,
            left_key: on.left().value.clone(),
            right_key: on.right().value.clone(),
            keys: on
                .keys()
                .iter()
                .map(|key| JoinKeyIr {
                    left: key.left.value.clone(),
                    right: key.right.value.clone(),
                })
                .collect(),
            kind: match kind {
                pdl_syntax::JoinKind::Inner => JoinKindIr::Inner,
                pdl_syntax::JoinKind::Left => JoinKindIr::Left,
                pdl_syntax::JoinKind::Right => JoinKindIr::Right,
                pdl_syntax::JoinKind::Full => JoinKindIr::Full,
                pdl_syntax::JoinKind::Semi => JoinKindIr::Semi,
                pdl_syntax::JoinKind::Anti => JoinKindIr::Anti,
            },
            span: *span,
        },
        Stage::Union {
            source,
            options,
            span,
        } => StageIr::Union {
            source: source.value.clone(),
            source_span: source.span,
            by_name: options
                .iter()
                .find(|option| option.kind == UnionOptionKind::ByName)
                .is_some_and(|option| option.value.value),
            distinct: options
                .iter()
                .find(|option| option.kind == UnionOptionKind::Distinct)
                .is_some_and(|option| option.value.value),
            span: *span,
        },
        Stage::Distinct { columns, span } => StageIr::Distinct {
            columns: columns.iter().map(|column| column.value.clone()).collect(),
            span: *span,
        },
        Stage::PivotLonger {
            columns,
            names_to,
            values_to,
            span,
        } => StageIr::PivotLonger {
            columns: columns.iter().map(|column| column.value.clone()).collect(),
            names_to: names_to.value.clone(),
            values_to: values_to.value.clone(),
            span: *span,
        },
        Stage::Complete { keys, fills, span } => StageIr::Complete {
            keys: keys.iter().map(|key| key.value.clone()).collect(),
            fills: fills.iter().map(lower_complete_fill_item).collect(),
            span: *span,
        },
        Stage::Save(save) => StageIr::Save {
            sink: match &save.sink {
                SinkRef::Path(path) => SinkIr::Path(path.value.clone()),
                SinkRef::Stdout(_) => SinkIr::Stdout,
            },
            format: save.format.as_ref().map(|format| format.value.clone()),
            span: save.span,
        },
        Stage::Unsupported { name, span } => StageIr::Unsupported {
            name: name.value.clone(),
            span: *span,
        },
    }
}

fn lower_complete_fill_item(item: &CompleteFillItem) -> CompleteFillItemIr {
    CompleteFillItemIr {
        column: item.column.value.clone(),
        expr: lower_expr(&item.expr),
        span: item.span,
    }
}

fn lower_agg_item(item: &AggItem) -> AggItemIr {
    AggItemIr {
        function: item.function.value.clone(),
        args: item.args.iter().map(lower_expr).collect(),
        alias: item.alias.value.clone(),
        span: item.span,
    }
}

fn lower_sort_item(item: &SortItem) -> SortItemIr {
    SortItemIr {
        column: item.column.value.clone(),
        direction: match item.direction {
            pdl_syntax::SortDirection::Asc => SortDirectionIr::Asc,
            pdl_syntax::SortDirection::Desc => SortDirectionIr::Desc,
        },
        nulls: item.nulls.map(|nulls| match nulls {
            pdl_syntax::NullsOrder::First => NullsOrderIr::First,
            pdl_syntax::NullsOrder::Last => NullsOrderIr::Last,
        }),
        span: item.column.span,
    }
}

fn lower_expr(expr: &Expr) -> ExprIr {
    match expr {
        Expr::Quoted(value) => ExprIr::Quoted {
            value: value.value.clone(),
            span: value.span,
        },
        Expr::Number(value) => ExprIr::Number {
            value: value.value,
            span: value.span,
        },
        Expr::Bool(value) => ExprIr::Bool {
            value: value.value,
            span: value.span,
        },
        Expr::Null(span) => ExprIr::Null { span: *span },
        Expr::Ident(value) => ExprIr::Ident {
            value: value.value.clone(),
            span: value.span,
        },
        Expr::Context { kind, name, span } => ExprIr::Context {
            kind: lower_context_kind(*kind),
            name: name.value.clone(),
            span: *span,
        },
        Expr::Call { name, args, span } => ExprIr::Call {
            name: name.value.clone(),
            args: args.iter().map(lower_expr).collect(),
            span: *span,
        },
        Expr::Window {
            function,
            args,
            spec,
            span,
        } => ExprIr::Window {
            function: function.value.clone(),
            args: args.iter().map(lower_expr).collect(),
            spec: lower_window_spec(spec),
            span: *span,
        },
        Expr::Unary { op, expr, span } => ExprIr::Unary {
            op: lower_unary_op(*op),
            expr: Box::new(lower_expr(expr)),
            span: *span,
        },
        Expr::Binary {
            left,
            op,
            right,
            span,
        } => ExprIr::Binary {
            left: Box::new(lower_expr(left)),
            op: lower_binary_op(*op),
            right: Box::new(lower_expr(right)),
            span: *span,
        },
    }
}

fn lower_window_spec(spec: &WindowSpec) -> WindowSpecIr {
    WindowSpecIr {
        partition_by: spec
            .partition_by
            .iter()
            .map(|column| column.value.clone())
            .collect(),
        order_by: spec.order_by.iter().map(lower_sort_item).collect(),
        frame: spec.frame.as_ref().map(lower_window_frame),
        span: spec.span,
    }
}

/// Desugars the v0.43.5 named-frame surface into the stable bound-pair IR.
/// The synthesized bounds carry the surface `frame` clause span so
/// diagnostics and editor navigation point at real source.
fn lower_window_frame(frame: &WindowFrame) -> WindowFrameIr {
    let span = frame.span;
    let (start, end) = match frame.kind {
        WindowFrameKind::WholePartition => (
            FrameBoundIr::UnboundedPreceding { span },
            FrameBoundIr::UnboundedFollowing { span },
        ),
        WindowFrameKind::Running => (
            FrameBoundIr::UnboundedPreceding { span },
            FrameBoundIr::CurrentRow { span },
        ),
        WindowFrameKind::Remaining => (
            FrameBoundIr::CurrentRow { span },
            FrameBoundIr::UnboundedFollowing { span },
        ),
        WindowFrameKind::Trailing { rows } => (
            FrameBoundIr::Preceding { rows, span },
            FrameBoundIr::CurrentRow { span },
        ),
        WindowFrameKind::Leading { rows } => (
            FrameBoundIr::CurrentRow { span },
            FrameBoundIr::Following { rows, span },
        ),
        WindowFrameKind::Centered { rows } => (
            FrameBoundIr::Preceding { rows, span },
            FrameBoundIr::Following { rows, span },
        ),
    };
    WindowFrameIr { start, end, span }
}

fn lower_unary_op(op: UnaryOp) -> UnaryOpIr {
    match op {
        UnaryOp::Not => UnaryOpIr::Not,
        UnaryOp::Neg => UnaryOpIr::Neg,
    }
}

fn lower_binary_op(op: BinaryOp) -> BinaryOpIr {
    match op {
        BinaryOp::Or => BinaryOpIr::Or,
        BinaryOp::And => BinaryOpIr::And,
        BinaryOp::Eq => BinaryOpIr::Eq,
        BinaryOp::Ne => BinaryOpIr::Ne,
        BinaryOp::Lt => BinaryOpIr::Lt,
        BinaryOp::Lte => BinaryOpIr::Lte,
        BinaryOp::Gt => BinaryOpIr::Gt,
        BinaryOp::Gte => BinaryOpIr::Gte,
        BinaryOp::Add => BinaryOpIr::Add,
        BinaryOp::Sub => BinaryOpIr::Sub,
        BinaryOp::Mul => BinaryOpIr::Mul,
        BinaryOp::Div => BinaryOpIr::Div,
        BinaryOp::Rem => BinaryOpIr::Rem,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lowered_window_frame(clause: &str) -> (WindowFrameIr, Span) {
        let source = format!(
            "load \"orders.csv\"\n  | mutate value = sum(amount) over (partition_by region order_by amount {clause})"
        );
        let parse = pdl_syntax::parse(&source);
        assert!(parse.diagnostics.is_empty(), "{:?}", parse.diagnostics);
        let ir = lower_program(&parse.program);
        let main = ir.main.expect("main pipeline");
        let StageIr::Mutate { items, .. } = &main.stages[0] else {
            panic!("mutate stage");
        };
        let ExprIr::Window { spec, .. } = &items[0].expr else {
            panic!("window expression");
        };
        let frame = spec.frame.clone().expect("window frame");
        let start = source.find(clause).expect("frame clause offset");
        (frame, Span::new(start, start + clause.len()))
    }

    /// Each `frame <name> [N]` surface form must produce a `WindowFrameIr`
    /// bit-identical to the reference bound pair from the v0.43.5 plan, with
    /// every synthesized bound carrying the surface `frame` clause span.
    #[test]
    fn window_frame_named_ir_stability() {
        type FrameCase = (&'static str, fn(Span) -> WindowFrameIr);
        let cases: Vec<FrameCase> = vec![
            ("frame whole_partition", |span| WindowFrameIr {
                start: FrameBoundIr::UnboundedPreceding { span },
                end: FrameBoundIr::UnboundedFollowing { span },
                span,
            }),
            ("frame running", |span| WindowFrameIr {
                start: FrameBoundIr::UnboundedPreceding { span },
                end: FrameBoundIr::CurrentRow { span },
                span,
            }),
            ("frame remaining", |span| WindowFrameIr {
                start: FrameBoundIr::CurrentRow { span },
                end: FrameBoundIr::UnboundedFollowing { span },
                span,
            }),
            ("frame trailing 3", |span| WindowFrameIr {
                start: FrameBoundIr::Preceding { rows: 3, span },
                end: FrameBoundIr::CurrentRow { span },
                span,
            }),
            ("frame leading 2", |span| WindowFrameIr {
                start: FrameBoundIr::CurrentRow { span },
                end: FrameBoundIr::Following { rows: 2, span },
                span,
            }),
            ("frame centered 1", |span| WindowFrameIr {
                start: FrameBoundIr::Preceding { rows: 1, span },
                end: FrameBoundIr::Following { rows: 1, span },
                span,
            }),
            ("frame trailing 0", |span| WindowFrameIr {
                start: FrameBoundIr::Preceding { rows: 0, span },
                end: FrameBoundIr::CurrentRow { span },
                span,
            }),
        ];
        for (clause, expected) in cases {
            let (frame, span) = lowered_window_frame(clause);
            assert_eq!(frame, expected(span), "clause `{clause}`");
        }
    }
}
