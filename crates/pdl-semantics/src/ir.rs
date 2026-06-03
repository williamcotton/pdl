use pdl_core::Span;
use pdl_syntax::{
    AggItem, BinaryOp, Expr, Pipeline, PipelineStart, Program, SinkRef, SortItem, SourceRef, Stage,
    UnaryOp,
};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProgramIr {
    pub bindings: Vec<BindingIr>,
    pub main: Option<PipelineIr>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BindingIr {
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
    Call {
        name: String,
        args: Vec<ExprIr>,
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
            | ExprIr::Call { span, .. }
            | ExprIr::Unary { span, .. }
            | ExprIr::Binary { span, .. } => *span,
        }
    }
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
        bindings: program
            .bindings
            .iter()
            .map(|binding| BindingIr {
                name: binding.name.value.clone(),
                span: binding.name.span,
                pipeline: lower_pipeline(&binding.pipeline),
            })
            .collect(),
        main: program.main.as_ref().map(lower_pipeline),
    }
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
        Expr::Call { name, args, span } => ExprIr::Call {
            name: name.value.clone(),
            args: args.iter().map(lower_expr).collect(),
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
