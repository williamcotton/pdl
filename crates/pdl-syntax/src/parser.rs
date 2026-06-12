use pdl_core::{codes, Diagnostic, Span};
use rowan::{Language, SyntaxKind as RowanSyntaxKind, SyntaxNode as RowanSyntaxNode};

use crate::cst::build_cst;
use crate::lexer::{lex_source, Token, TokenKind};

#[derive(Clone, Debug, PartialEq)]
pub struct ParseResult {
    pub syntax: SyntaxNode,
    pub program: Program,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum PdlLanguage {}

impl Language for PdlLanguage {
    type Kind = SyntaxKind;

    fn kind_from_raw(raw: RowanSyntaxKind) -> Self::Kind {
        SyntaxKind::from_raw(raw.0)
    }

    fn kind_to_raw(kind: Self::Kind) -> RowanSyntaxKind {
        RowanSyntaxKind(kind as u16)
    }
}

pub type SyntaxNode = RowanSyntaxNode<PdlLanguage>;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(u16)]
pub enum SyntaxKind {
    Root,
    Whitespace,
    LineComment,
    BlockComment,
    Ident,
    String,
    BacktickColumn,
    Number,
    Pipe,
    Comma,
    Equal,
    LParen,
    RParen,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Dollar,
    At,
    EqEq,
    NotEq,
    Lt,
    Lte,
    Gt,
    Gte,
    Bang,
    Eof,
    Error,
    ContextDecl,
    BindingDecl,
    PipelineExpr,
    LoadStageNode,
    BindingRefNode,
    StageNode,
    SaveStageNode,
    SelectItemNode,
    RenameItemNode,
    AggItemNode,
    SortItemNode,
    MutateItemNode,
    ExprNode,
    OutputDecl,
    Colon,
    Dot,
    LBracket,
    RBracket,
}

impl SyntaxKind {
    pub(crate) fn from_raw(raw: u16) -> Self {
        match raw {
            0 => SyntaxKind::Root,
            1 => SyntaxKind::Whitespace,
            2 => SyntaxKind::LineComment,
            3 => SyntaxKind::BlockComment,
            4 => SyntaxKind::Ident,
            5 => SyntaxKind::String,
            6 => SyntaxKind::BacktickColumn,
            7 => SyntaxKind::Number,
            8 => SyntaxKind::Pipe,
            9 => SyntaxKind::Comma,
            10 => SyntaxKind::Equal,
            11 => SyntaxKind::LParen,
            12 => SyntaxKind::RParen,
            13 => SyntaxKind::Plus,
            14 => SyntaxKind::Minus,
            15 => SyntaxKind::Star,
            16 => SyntaxKind::Slash,
            17 => SyntaxKind::Percent,
            18 => SyntaxKind::Dollar,
            19 => SyntaxKind::At,
            20 => SyntaxKind::EqEq,
            21 => SyntaxKind::NotEq,
            22 => SyntaxKind::Lt,
            23 => SyntaxKind::Lte,
            24 => SyntaxKind::Gt,
            25 => SyntaxKind::Gte,
            26 => SyntaxKind::Bang,
            27 => SyntaxKind::Eof,
            28 => SyntaxKind::Error,
            29 => SyntaxKind::ContextDecl,
            30 => SyntaxKind::BindingDecl,
            31 => SyntaxKind::PipelineExpr,
            32 => SyntaxKind::LoadStageNode,
            33 => SyntaxKind::BindingRefNode,
            34 => SyntaxKind::StageNode,
            35 => SyntaxKind::SaveStageNode,
            36 => SyntaxKind::SelectItemNode,
            37 => SyntaxKind::RenameItemNode,
            38 => SyntaxKind::AggItemNode,
            39 => SyntaxKind::SortItemNode,
            40 => SyntaxKind::MutateItemNode,
            41 => SyntaxKind::ExprNode,
            42 => SyntaxKind::OutputDecl,
            43 => SyntaxKind::Colon,
            44 => SyntaxKind::Dot,
            45 => SyntaxKind::LBracket,
            46 => SyntaxKind::RBracket,
            _ => SyntaxKind::Error,
        }
    }

    pub fn is_trivia(self) -> bool {
        matches!(
            self,
            SyntaxKind::Whitespace | SyntaxKind::LineComment | SyntaxKind::BlockComment
        )
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Program {
    pub contexts: Vec<ContextDecl>,
    pub bindings: Vec<Binding>,
    pub outputs: Vec<OutputDecl>,
    pub main: Option<Pipeline>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ContextDecl {
    pub kind: ContextKind,
    pub name: Spanned<String>,
    pub default: Expr,
    pub control: Option<ControlInitializer>,
    pub span: Span,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContextKind {
    Param,
    State,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ControlInitializer {
    pub kind: ControlKind,
    pub name: Spanned<String>,
    pub args: Vec<ControlArg>,
    pub span: Span,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControlKind {
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

impl ControlKind {
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "input_text" => Some(Self::Text),
            "input_textarea" => Some(Self::Textarea),
            "input_number" => Some(Self::Number),
            "input_range" => Some(Self::Range),
            "input_checkbox" => Some(Self::Checkbox),
            "input_select" => Some(Self::Select),
            "input_radio" => Some(Self::Radio),
            "input_date" => Some(Self::Date),
            "input_time" => Some(Self::Time),
            "input_datetime" => Some(Self::Datetime),
            "input_color" => Some(Self::Color),
            _ => None,
        }
    }

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
pub struct ControlArg {
    pub name: Spanned<String>,
    pub value: ControlValue,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ControlValue {
    Literal(ControlLiteral),
    Array {
        values: Vec<ControlLiteral>,
        span: Span,
    },
    BindingColumn {
        binding: Spanned<String>,
        column: Spanned<String>,
        span: Span,
    },
}

impl ControlValue {
    pub fn span(&self) -> Span {
        match self {
            ControlValue::Literal(value) => value.span(),
            ControlValue::Array { span, .. } | ControlValue::BindingColumn { span, .. } => *span,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ControlLiteral {
    Quoted(Spanned<String>),
    Number(Spanned<f64>),
    Bool(Spanned<bool>),
    Null(Span),
}

impl ControlLiteral {
    pub fn span(&self) -> Span {
        match self {
            ControlLiteral::Quoted(value) => value.span,
            ControlLiteral::Number(value) => value.span,
            ControlLiteral::Bool(value) => value.span,
            ControlLiteral::Null(span) => *span,
        }
    }

    pub fn to_expr(&self) -> Expr {
        match self {
            ControlLiteral::Quoted(value) => Expr::Quoted(value.clone()),
            ControlLiteral::Number(value) => Expr::Number(value.clone()),
            ControlLiteral::Bool(value) => Expr::Bool(value.clone()),
            ControlLiteral::Null(span) => Expr::Null(*span),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Binding {
    pub name: Spanned<String>,
    pub pipeline: Pipeline,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OutputDecl {
    pub name: Spanned<String>,
    pub pipeline: Pipeline,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Pipeline {
    pub start: PipelineStart,
    pub stages: Vec<Stage>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PipelineStart {
    Load(LoadStage),
    Binding(Spanned<String>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct LoadStage {
    pub source: SourceRef,
    pub format: Option<Spanned<String>>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SourceRef {
    Path(Spanned<String>),
    Stdin(Span),
}

#[derive(Clone, Debug, PartialEq)]
pub enum SinkRef {
    Path(Spanned<String>),
    Stdout(Span),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Stage {
    Filter {
        expr: Expr,
        span: Span,
    },
    Select {
        items: Vec<SelectItem>,
        span: Span,
    },
    Drop {
        columns: Vec<Spanned<String>>,
        span: Span,
    },
    Rename {
        items: Vec<RenameItem>,
        span: Span,
    },
    Mutate {
        items: Vec<MutateItem>,
        span: Span,
    },
    GroupBy {
        columns: Vec<Spanned<String>>,
        span: Span,
    },
    Agg {
        items: Vec<AggItem>,
        span: Span,
    },
    Sort {
        items: Vec<SortItem>,
        span: Span,
    },
    Limit {
        n: usize,
        span: Span,
    },
    Join {
        source: Spanned<String>,
        on: JoinOn,
        kind: JoinKind,
        kind_span: Option<Span>,
        span: Span,
    },
    Union {
        source: Spanned<String>,
        options: Vec<UnionOption>,
        span: Span,
    },
    Distinct {
        columns: Vec<Spanned<String>>,
        span: Span,
    },
    PivotLonger {
        columns: Vec<Spanned<String>>,
        names_to: Spanned<String>,
        values_to: Spanned<String>,
        span: Span,
    },
    Complete {
        keys: Vec<Spanned<String>>,
        fills: Vec<CompleteFillItem>,
        span: Span,
    },
    Save(SaveStage),
    Unsupported {
        name: Spanned<String>,
        span: Span,
    },
}

impl Stage {
    pub fn span(&self) -> Span {
        match self {
            Stage::Filter { span, .. }
            | Stage::Select { span, .. }
            | Stage::Drop { span, .. }
            | Stage::Rename { span, .. }
            | Stage::Mutate { span, .. }
            | Stage::GroupBy { span, .. }
            | Stage::Agg { span, .. }
            | Stage::Sort { span, .. }
            | Stage::Limit { span, .. }
            | Stage::Join { span, .. }
            | Stage::Union { span, .. }
            | Stage::Distinct { span, .. }
            | Stage::PivotLonger { span, .. }
            | Stage::Complete { span, .. }
            | Stage::Unsupported { span, .. } => *span,
            Stage::Save(save) => save.span,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SaveStage {
    pub sink: SinkRef,
    pub format: Option<Spanned<String>>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SelectItem {
    pub column: Spanned<String>,
    pub alias: Option<Spanned<String>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RenameItem {
    pub old: Spanned<String>,
    pub new: Spanned<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MutateItem {
    pub column: Spanned<String>,
    pub expr: Expr,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CompleteFillItem {
    pub column: Spanned<String>,
    pub expr: Expr,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AggItem {
    pub function: Spanned<String>,
    pub args: Vec<Expr>,
    pub alias: Spanned<String>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SortItem {
    pub column: Spanned<String>,
    pub direction: SortDirection,
    pub nulls: Option<NullsOrder>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum JoinOn {
    Same(Spanned<String>),
    Pair {
        left: Spanned<String>,
        right: Spanned<String>,
        span: Span,
    },
    Composite {
        keys: Vec<JoinKey>,
        span: Span,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct JoinKey {
    pub left: Spanned<String>,
    pub right: Spanned<String>,
    pub span: Span,
}

impl JoinOn {
    pub fn span(&self) -> Span {
        match self {
            JoinOn::Same(column) => column.span,
            JoinOn::Pair { span, .. } => *span,
            JoinOn::Composite { span, .. } => *span,
        }
    }

    pub fn left(&self) -> &Spanned<String> {
        match self {
            JoinOn::Same(column) => column,
            JoinOn::Pair { left, .. } => left,
            JoinOn::Composite { keys, .. } => &keys[0].left,
        }
    }

    pub fn right(&self) -> &Spanned<String> {
        match self {
            JoinOn::Same(column) => column,
            JoinOn::Pair { right, .. } => right,
            JoinOn::Composite { keys, .. } => &keys[0].right,
        }
    }

    pub fn keys(&self) -> Vec<JoinKey> {
        match self {
            JoinOn::Same(column) => vec![JoinKey {
                left: column.clone(),
                right: column.clone(),
                span: column.span,
            }],
            JoinOn::Pair { left, right, span } => vec![JoinKey {
                left: left.clone(),
                right: right.clone(),
                span: *span,
            }],
            JoinOn::Composite { keys, .. } => keys.clone(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JoinKind {
    Inner,
    Left,
    Right,
    Full,
    Semi,
    Anti,
}

impl JoinKind {
    pub fn as_str(self) -> &'static str {
        match self {
            JoinKind::Inner => "inner",
            JoinKind::Left => "left",
            JoinKind::Right => "right",
            JoinKind::Full => "full",
            JoinKind::Semi => "semi",
            JoinKind::Anti => "anti",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct UnionOption {
    pub kind: UnionOptionKind,
    pub value: Spanned<bool>,
    pub span: Span,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnionOptionKind {
    ByName,
    Distinct,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SortDirection {
    Asc,
    Desc,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NullsOrder {
    First,
    Last,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WindowSpec {
    pub partition_by: Vec<Spanned<String>>,
    pub order_by: Vec<SortItem>,
    pub frame: Option<WindowFrame>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WindowFrame {
    pub kind: WindowFrameKind,
    pub span: Span,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindowFrameKind {
    WholePartition,
    Running,
    Remaining,
    Trailing { rows: usize },
    Leading { rows: usize },
    Centered { rows: usize },
}

impl WindowFrameKind {
    pub fn name(self) -> &'static str {
        match self {
            WindowFrameKind::WholePartition => "whole_partition",
            WindowFrameKind::Running => "running",
            WindowFrameKind::Remaining => "remaining",
            WindowFrameKind::Trailing { .. } => "trailing",
            WindowFrameKind::Leading { .. } => "leading",
            WindowFrameKind::Centered { .. } => "centered",
        }
    }

    pub fn rows(self) -> Option<usize> {
        match self {
            WindowFrameKind::WholePartition
            | WindowFrameKind::Running
            | WindowFrameKind::Remaining => None,
            WindowFrameKind::Trailing { rows }
            | WindowFrameKind::Leading { rows }
            | WindowFrameKind::Centered { rows } => Some(rows),
        }
    }
}

pub const WINDOW_FRAME_NAMES: [&str; 6] = [
    "whole_partition",
    "running",
    "remaining",
    "trailing",
    "leading",
    "centered",
];

#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Quoted(Spanned<String>),
    Number(Spanned<f64>),
    Bool(Spanned<bool>),
    Null(Span),
    Ident(Spanned<String>),
    Context {
        kind: ContextKind,
        name: Spanned<String>,
        span: Span,
    },
    Call {
        name: Spanned<String>,
        args: Vec<Expr>,
        span: Span,
    },
    Window {
        function: Spanned<String>,
        args: Vec<Expr>,
        spec: WindowSpec,
        span: Span,
    },
    Unary {
        op: UnaryOp,
        expr: Box<Expr>,
        span: Span,
    },
    Binary {
        left: Box<Expr>,
        op: BinaryOp,
        right: Box<Expr>,
        span: Span,
    },
}

impl Expr {
    pub fn span(&self) -> Span {
        match self {
            Expr::Quoted(value) => value.span,
            Expr::Number(value) => value.span,
            Expr::Bool(value) => value.span,
            Expr::Null(span) => *span,
            Expr::Ident(value) => value.span,
            Expr::Context { span, .. } => *span,
            Expr::Call { span, .. }
            | Expr::Window { span, .. }
            | Expr::Unary { span, .. }
            | Expr::Binary { span, .. } => *span,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnaryOp {
    Not,
    Neg,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BinaryOp {
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

#[derive(Clone, Debug, PartialEq)]
pub struct Spanned<T> {
    pub value: T,
    pub span: Span,
}

impl<T> Spanned<T> {
    pub fn new(value: T, span: Span) -> Self {
        Self { value, span }
    }
}

const PARAM_COLUMN_PREFIX: &str = "\0param:";
const STATE_COLUMN_PREFIX: &str = "\0state:";

pub fn encode_context_column_ref(kind: ContextKind, name: &str) -> String {
    match kind {
        ContextKind::Param => format!("{PARAM_COLUMN_PREFIX}{name}"),
        ContextKind::State => format!("{STATE_COLUMN_PREFIX}{name}"),
    }
}

pub fn decode_context_column_ref(value: &str) -> Option<(ContextKind, &str)> {
    value
        .strip_prefix(PARAM_COLUMN_PREFIX)
        .map(|name| (ContextKind::Param, name))
        .or_else(|| {
            value
                .strip_prefix(STATE_COLUMN_PREFIX)
                .map(|name| (ContextKind::State, name))
        })
}

pub fn parse(source: &str) -> ParseResult {
    let lexed = lex_source(source);
    let mut parser = Parser::new(source, lexed.parse_tokens, lexed.diagnostics);
    let (program, diagnostics) = parser.parse_program();
    let syntax = SyntaxNode::new_root(build_cst(&lexed.tokens, &program, source.len()));
    ParseResult {
        syntax,
        program,
        diagnostics,
    }
}

struct Parser<'a> {
    source: &'a str,
    tokens: Vec<Token>,
    pos: usize,
    diagnostics: Vec<Diagnostic>,
}

impl<'a> Parser<'a> {
    fn new(source: &'a str, tokens: Vec<Token>, diagnostics: Vec<Diagnostic>) -> Self {
        Self {
            source,
            tokens,
            pos: 0,
            diagnostics,
        }
    }

    fn parse_program(&mut self) -> (Program, Vec<Diagnostic>) {
        let mut contexts = Vec::new();
        let mut bindings = Vec::new();
        let mut outputs = Vec::new();
        while self.at_ident("param") || self.at_ident("state") {
            if let Some(context) = self.parse_context_decl() {
                contexts.push(context);
            } else {
                self.recover_to_pipe_or_eof();
            }
        }
        while self.at_ident("let") {
            if let Some(binding) = self.parse_binding() {
                bindings.push(binding);
            } else {
                self.recover_to_pipe_or_eof();
            }
        }
        while self.at_ident("output") {
            if let Some(output) = self.parse_output_decl() {
                outputs.push(output);
            } else {
                self.recover_to_pipe_or_eof();
            }
        }

        let main = if self.at_eof() {
            if outputs.is_empty() {
                self.diagnostics.push(Diagnostic::error(
                    codes::E1502,
                    "no runnable main pipeline",
                    self.current().span,
                ));
            }
            None
        } else {
            self.parse_pipeline()
        };
        if main.is_some() && !self.at_eof() {
            self.diagnostics.push(Diagnostic::error(
                codes::E0021,
                "trailing tokens after pipeline",
                self.current().span,
            ));
            self.recover_to_eof();
        }

        (
            Program {
                contexts,
                bindings,
                outputs,
                main,
            },
            std::mem::take(&mut self.diagnostics),
        )
    }

    fn parse_context_decl(&mut self) -> Option<ContextDecl> {
        let keyword = self.advance().clone();
        let kind = match &keyword.kind {
            TokenKind::Ident(value) if value == "param" => ContextKind::Param,
            TokenKind::Ident(value) if value == "state" => ContextKind::State,
            _ => {
                self.diagnostics.push(Diagnostic::error(
                    codes::E0001,
                    "expected `param` or `state`",
                    keyword.span,
                ));
                return None;
            }
        };
        let name = self.expect_context_name(kind)?;
        self.expect_equal();
        let (default, control) = if self.at_control_initializer_start() {
            let control = self.parse_control_initializer()?;
            (control_default_expr(&control), Some(control))
        } else {
            (self.parse_expr(0)?, None)
        };
        let span = keyword.span.join(default.span());
        Some(ContextDecl {
            kind,
            name,
            default,
            control,
            span,
        })
    }

    fn parse_control_initializer(&mut self) -> Option<ControlInitializer> {
        let name = self.expect_identifier("control initializer")?;
        let kind = ControlKind::from_name(&name.value)?;
        Some(self.parse_control_initializer_after_name(kind, name))
    }

    fn parse_control_initializer_after_name(
        &mut self,
        kind: ControlKind,
        name: Spanned<String>,
    ) -> ControlInitializer {
        self.expect_lparen();
        let mut args = Vec::new();
        if !self.at_rparen() {
            loop {
                let arg_name = match self.expect_identifier("control argument name") {
                    Some(name) => name,
                    None => {
                        self.recover_to_control_arg_boundary();
                        if self.consume_comma() {
                            continue;
                        }
                        break;
                    }
                };
                if !self.consume_colon() {
                    self.diagnostics.push(Diagnostic::error(
                        codes::E2007,
                        "control arguments require `:`",
                        self.current().span,
                    ));
                }
                let value = self.parse_control_value();
                let span = arg_name.span.join(value.span());
                args.push(ControlArg {
                    name: arg_name,
                    value,
                    span,
                });
                if self.consume_comma() {
                    if self.at_rparen() {
                        break;
                    }
                    continue;
                }
                if !self.at_rparen() {
                    self.diagnostics.push(Diagnostic::error(
                        codes::E2007,
                        "expected `,` between control arguments",
                        self.current().span,
                    ));
                    self.recover_to_control_arg_boundary();
                    if self.consume_comma() {
                        continue;
                    }
                }
                break;
            }
        }
        let close = self.expect_rparen();
        let end = close.map_or_else(|| self.previous_span().end, |token| token.span.end);
        let start = name.span.start;
        ControlInitializer {
            kind,
            name,
            args,
            span: Span::new(start, end),
        }
    }

    fn parse_control_value(&mut self) -> ControlValue {
        if self.consume_lbracket() {
            let start = self.previous_span().start;
            let mut values = Vec::new();
            if !self.at_rbracket() {
                loop {
                    values.push(self.parse_control_literal());
                    if self.consume_comma() {
                        if self.at_rbracket() {
                            break;
                        }
                        continue;
                    }
                    break;
                }
            }
            let close = self.expect_rbracket();
            let end = close.map_or_else(|| self.previous_span().end, |token| token.span.end);
            return ControlValue::Array {
                values,
                span: Span::new(start, end),
            };
        }

        if self.at_ident_followed_by_dot() {
            let binding = self
                .expect_identifier("choice source binding")
                .unwrap_or_else(|| Spanned::new(String::new(), self.current().span));
            self.expect_dot();
            let column = self
                .expect_identifier("choice source column")
                .unwrap_or_else(|| Spanned::new(String::new(), self.current().span));
            let span = binding.span.join(column.span);
            return ControlValue::BindingColumn {
                binding,
                column,
                span,
            };
        }

        ControlValue::Literal(self.parse_control_literal())
    }

    fn parse_control_literal(&mut self) -> ControlLiteral {
        let token = self.advance().clone();
        match token.kind {
            TokenKind::String(value) => ControlLiteral::Quoted(Spanned::new(value, token.span)),
            TokenKind::Number(raw) => match raw.parse::<f64>() {
                Ok(value) => ControlLiteral::Number(Spanned::new(value, token.span)),
                Err(_) => {
                    self.diagnostics.push(Diagnostic::error(
                        codes::E1206,
                        "invalid number literal",
                        token.span,
                    ));
                    ControlLiteral::Null(token.span)
                }
            },
            TokenKind::Minus => {
                let number = self.advance().clone();
                match number.kind {
                    TokenKind::Number(raw) => match raw.parse::<f64>() {
                        Ok(value) => ControlLiteral::Number(Spanned::new(
                            -value,
                            token.span.join(number.span),
                        )),
                        Err(_) => {
                            self.diagnostics.push(Diagnostic::error(
                                codes::E1206,
                                "invalid number literal",
                                number.span,
                            ));
                            ControlLiteral::Null(token.span.join(number.span))
                        }
                    },
                    _ => {
                        self.diagnostics.push(Diagnostic::error(
                            codes::E2007,
                            "expected number after `-` in control literal",
                            number.span,
                        ));
                        ControlLiteral::Null(token.span.join(number.span))
                    }
                }
            }
            TokenKind::Ident(value) if value == "true" => {
                ControlLiteral::Bool(Spanned::new(true, token.span))
            }
            TokenKind::Ident(value) if value == "false" => {
                ControlLiteral::Bool(Spanned::new(false, token.span))
            }
            TokenKind::Ident(value) if value == "null" => ControlLiteral::Null(token.span),
            _ => {
                self.diagnostics.push(Diagnostic::error(
                    codes::E2007,
                    "expected control literal",
                    token.span,
                ));
                ControlLiteral::Null(token.span)
            }
        }
    }

    fn parse_binding(&mut self) -> Option<Binding> {
        self.expect_ident("let")?;
        let name = self.expect_binding_name()?;
        self.expect_equal();
        let pipeline = self.parse_pipeline()?;
        Some(Binding { name, pipeline })
    }

    fn parse_output_decl(&mut self) -> Option<OutputDecl> {
        self.expect_ident("output")?;
        let name = self.expect_output_name()?;
        self.expect_equal();
        let pipeline = self.parse_pipeline()?;
        Some(OutputDecl { name, pipeline })
    }

    fn parse_pipeline(&mut self) -> Option<Pipeline> {
        let start_span = self.current().span;
        let start = if self.at_ident("load") {
            PipelineStart::Load(self.parse_load_stage()?)
        } else if let Some(name) = self.consume_ident_value() {
            PipelineStart::Binding(name)
        } else {
            self.diagnostics.push(Diagnostic::error(
                codes::E0007,
                "expected pipeline start",
                self.current().span,
            ));
            return None;
        };

        let mut stages = Vec::new();
        loop {
            if self.consume_pipe() {
                if self.at_eof() {
                    self.diagnostics.push(Diagnostic::error(
                        codes::E0006,
                        "missing stage after pipe",
                        self.previous_span(),
                    ));
                    break;
                }
            } else if self.at_recoverable_stage_start() {
                self.diagnostics.push(Diagnostic::error(
                    codes::E0001,
                    "expected `|` before stage",
                    self.current().span,
                ));
            } else {
                break;
            }
            if let Some(stage) = self.parse_stage() {
                stages.push(stage);
            } else {
                self.recover_to_pipe_or_eof();
            }
        }

        let end = stages
            .last()
            .map_or_else(|| self.previous_span().end, |stage| stage.span().end);
        Some(Pipeline {
            start,
            stages,
            span: Span::new(start_span.start, end),
        })
    }

    fn parse_load_stage(&mut self) -> Option<LoadStage> {
        let start = self.expect_ident("load")?.span.start;
        let source = self.parse_source_ref()?;
        let format = self.parse_format_clause();
        let end = format
            .as_ref()
            .map_or_else(|| source_span(&source).end, |format| format.span.end);
        Some(LoadStage {
            source,
            format,
            span: Span::new(start, end),
        })
    }

    fn parse_stage(&mut self) -> Option<Stage> {
        let name = self.consume_ident_value()?;
        match name.value.as_str() {
            "load" => {
                self.diagnostics.push(Diagnostic::error(
                    codes::E1202,
                    "`load` is valid only as a pipeline start",
                    name.span,
                ));
                self.recover_to_pipe_or_eof();
                Some(Stage::Unsupported {
                    span: name.span,
                    name,
                })
            }
            "filter" => self.parse_filter(name.span),
            "select" => self.parse_select(name.span),
            "drop" => self.parse_drop(name.span),
            "rename" => self.parse_rename(name.span),
            "mutate" => self.parse_mutate(name.span),
            "group_by" => self.parse_group_by(name.span),
            "agg" => self.parse_agg(name.span),
            "sort" => self.parse_sort(name.span),
            "limit" => self.parse_limit(name.span),
            "join" => self.parse_join(name.span),
            "union" => self.parse_union(name.span),
            "distinct" => self.parse_distinct(name.span),
            "pivot_longer" => self.parse_pivot_longer(name.span),
            "complete" => self.parse_complete(name.span),
            "save" => self.parse_save(name.span).map(Stage::Save),
            _ => {
                self.diagnostics.push(Diagnostic::error(
                    codes::E1201,
                    format!("unknown stage `{}`", name.value),
                    name.span,
                ));
                let span = self.consume_until_stage_boundary(name.span);
                Some(Stage::Unsupported { name, span })
            }
        }
    }

    fn parse_join(&mut self, name_span: Span) -> Option<Stage> {
        let source = self.expect_identifier("join source")?;
        if !self.consume_ident("on") {
            self.diagnostics.push(Diagnostic::error(
                codes::E1203,
                "join requires `on`",
                self.current().span,
            ));
        }
        let on = self.parse_join_on()?;
        let mut end = on.span().end;
        let mut kind = JoinKind::Inner;
        let mut kind_span = None;
        if self.consume_ident("kind") {
            let (parsed, span) = self.parse_join_kind();
            kind = parsed;
            kind_span = Some(span);
            end = span.end;
        }
        if !self.at_stage_boundary() {
            self.diagnostics.push(Diagnostic::error(
                codes::E1204,
                "unknown join option",
                self.current().span,
            ));
            end = self.consume_until_stage_boundary(name_span).end;
        }
        Some(Stage::Join {
            source,
            on,
            kind,
            kind_span,
            span: Span::new(name_span.start, end),
        })
    }

    fn parse_join_on(&mut self) -> Option<JoinOn> {
        let first = self.parse_join_key()?;
        if !self.consume_comma() {
            if first.left == first.right {
                return Some(JoinOn::Same(first.left));
            }
            return Some(JoinOn::Pair {
                left: first.left,
                right: first.right,
                span: first.span,
            });
        }

        let start = first.span.start;
        let mut keys = vec![first];
        let key = self.parse_join_key()?;
        let mut end = key.span.end;
        keys.push(key);
        while self.consume_comma() {
            let key = self.parse_join_key()?;
            end = key.span.end;
            keys.push(key);
        }
        Some(JoinOn::Composite {
            keys,
            span: Span::new(start, end),
        })
    }

    fn parse_join_key(&mut self) -> Option<JoinKey> {
        if self.consume_lparen() {
            let start = self.previous_span().start;
            let left = self.expect_column_name()?;
            if !self.consume_comma() {
                self.diagnostics.push(Diagnostic::error(
                    codes::E0001,
                    "expected `,`",
                    self.current().span,
                ));
            }
            let right = self.expect_column_name()?;
            let close = self.expect_rparen();
            let end = close.map_or(right.span.end, |token| token.span.end);
            return Some(JoinKey {
                left,
                right,
                span: Span::new(start, end),
            });
        }

        self.expect_column_name().map(|column| JoinKey {
            left: column.clone(),
            right: column.clone(),
            span: column.span,
        })
    }

    fn parse_join_kind(&mut self) -> (JoinKind, Span) {
        let token = self.advance().clone();
        match token.kind {
            TokenKind::Ident(value) => {
                let kind = match value.as_str() {
                    "inner" => Some(JoinKind::Inner),
                    "left" => Some(JoinKind::Left),
                    "right" => Some(JoinKind::Right),
                    "full" => Some(JoinKind::Full),
                    "semi" => Some(JoinKind::Semi),
                    "anti" => Some(JoinKind::Anti),
                    _ => None,
                };
                match kind {
                    Some(kind) => (kind, token.span),
                    None => {
                        self.diagnostics.push(Diagnostic::error(
                            codes::E1223,
                            format!("invalid join kind `{value}`"),
                            token.span,
                        ));
                        (JoinKind::Inner, token.span)
                    }
                }
            }
            _ => {
                self.diagnostics.push(Diagnostic::error(
                    codes::E1223,
                    "invalid join kind",
                    token.span,
                ));
                (JoinKind::Inner, token.span)
            }
        }
    }

    fn parse_union(&mut self, name_span: Span) -> Option<Stage> {
        let source = self.expect_identifier("union source")?;
        let mut options = Vec::new();
        while !self.at_stage_boundary() {
            let option = self.expect_identifier("union option")?;
            let kind = match option.value.as_str() {
                "by_name" => UnionOptionKind::ByName,
                "distinct" => UnionOptionKind::Distinct,
                _ => {
                    self.diagnostics.push(Diagnostic::error(
                        codes::E1204,
                        format!("unknown union option `{}`", option.value),
                        option.span,
                    ));
                    if !self.at_stage_boundary() {
                        let _ = self.advance();
                    }
                    continue;
                }
            };
            if options
                .iter()
                .any(|existing: &UnionOption| existing.kind == kind)
            {
                self.diagnostics.push(Diagnostic::error(
                    codes::E1205,
                    format!("duplicate union option `{}`", option.value),
                    option.span,
                ));
            }
            let value = self.parse_bool_literal(&option.value)?;
            options.push(UnionOption {
                kind,
                span: option.span.join(value.span),
                value,
            });
        }

        let end = options
            .last()
            .map_or(source.span.end, |option| option.span.end);
        Some(Stage::Union {
            source,
            options,
            span: Span::new(name_span.start, end),
        })
    }

    fn parse_filter(&mut self, name_span: Span) -> Option<Stage> {
        let mut expr = self.parse_expr(0)?;
        if self.current().kind == TokenKind::Equal
            && self.current_is_on_same_line_after(expr.span())
        {
            let operator_span = self.advance().span;
            self.diagnostics.push(Diagnostic::error(
                codes::E0001,
                "expected operator in filter expression",
                operator_span,
            ));
            if let Some(rhs) = self.parse_expr(0) {
                let span = expr.span().join(rhs.span());
                expr = Expr::Binary {
                    left: Box::new(expr),
                    op: BinaryOp::Eq,
                    right: Box::new(rhs),
                    span,
                };
            }
        }
        if !self.at_stage_boundary() && self.current_is_on_same_line_after(expr.span()) {
            self.diagnostics.push(Diagnostic::error(
                codes::E0001,
                "expected operator in filter expression",
                self.current().span,
            ));
            self.recover_to_pipe_or_eof();
        }
        let span = name_span.join(expr.span());
        Some(Stage::Filter { expr, span })
    }

    fn parse_select(&mut self, name_span: Span) -> Option<Stage> {
        let mut items = Vec::new();
        loop {
            let first = self.expect_column_name()?;
            let (column, alias) = if self.consume_equal() {
                let source = self.expect_column_name()?;
                (source, Some(first))
            } else if self.consume_ident("as") {
                self.legacy_as_diagnostic(self.previous_span());
                let output = self.expect_column_name()?;
                (first, Some(output))
            } else {
                (first, None)
            };
            items.push(SelectItem { column, alias });
            if !self.consume_comma() {
                break;
            }
        }
        let end = items
            .last()
            .and_then(|item| item.alias.as_ref().or(Some(&item.column)))
            .map_or(name_span.end, |value| value.span.end);
        Some(Stage::Select {
            items,
            span: Span::new(name_span.start, end),
        })
    }

    fn parse_drop(&mut self, name_span: Span) -> Option<Stage> {
        let columns = self.parse_column_list()?;
        let end = columns
            .last()
            .map_or(name_span.end, |column| column.span.end);
        Some(Stage::Drop {
            columns,
            span: Span::new(name_span.start, end),
        })
    }

    fn parse_rename(&mut self, name_span: Span) -> Option<Stage> {
        let mut items = Vec::new();
        loop {
            let first = self.expect_column_name()?;
            let (old, new) = if self.consume_equal() {
                let old = self.expect_column_name()?;
                (old, first)
            } else if self.consume_ident("as") {
                self.legacy_as_diagnostic(self.previous_span());
                let new = self.expect_column_name()?;
                (first, new)
            } else {
                self.diagnostics.push(
                    Diagnostic::error(
                        codes::E0015,
                        "rename items require `=`",
                        self.current().span,
                    )
                    .with_help("write rename items as `new_name = old_name`"),
                );
                let old = self.expect_column_name()?;
                (old, first)
            };
            items.push(RenameItem { old, new });
            if !self.consume_comma() {
                break;
            }
        }
        let end = items.last().map_or(name_span.end, |item| item.new.span.end);
        Some(Stage::Rename {
            items,
            span: Span::new(name_span.start, end),
        })
    }

    fn parse_mutate(&mut self, name_span: Span) -> Option<Stage> {
        let mut items = Vec::new();
        loop {
            let column = self.expect_column_name()?;
            if !self.consume_equal() {
                self.diagnostics.push(Diagnostic::error(
                    codes::E1203,
                    "mutate assignments require `=`",
                    self.current().span,
                ));
            }
            let expr = self.parse_expr(0)?;
            let span = column.span.join(expr.span());
            items.push(MutateItem { column, expr, span });
            if !self.consume_comma() {
                break;
            }
        }
        let end = items.last().map_or(name_span.end, |item| item.span.end);
        Some(Stage::Mutate {
            items,
            span: Span::new(name_span.start, end),
        })
    }

    fn parse_group_by(&mut self, name_span: Span) -> Option<Stage> {
        let columns = self.parse_column_list()?;
        let end = columns
            .last()
            .map_or(name_span.end, |column| column.span.end);
        Some(Stage::GroupBy {
            columns,
            span: Span::new(name_span.start, end),
        })
    }

    fn parse_agg(&mut self, name_span: Span) -> Option<Stage> {
        let mut items = Vec::new();
        loop {
            let (alias, function, args, call_span) = if self.at_ident_followed_by_lparen() {
                let (function, args, call_span) = self.parse_agg_call()?;
                if self.consume_ident("as") {
                    self.legacy_as_diagnostic(self.previous_span());
                } else {
                    let diagnostic_span = if self.at_ident_followed_by_column_name() {
                        self.advance().span
                    } else {
                        call_span
                    };
                    self.diagnostics.push(
                        Diagnostic::error(
                            codes::E1417,
                            "aggregate items require assignment",
                            diagnostic_span,
                        )
                        .with_help("write aggregate items as `output_name = aggregate_call(...)`"),
                    );
                }
                let alias = self.expect_column_name()?;
                (alias, function, args, call_span)
            } else {
                let alias = self.expect_column_name()?;
                if !self.consume_equal() {
                    self.diagnostics.push(
                        Diagnostic::error(
                            codes::E1417,
                            "aggregate items require assignment",
                            self.current().span,
                        )
                        .with_help("write aggregate items as `output_name = aggregate_call(...)`"),
                    );
                }
                let (function, args, call_span) = self.parse_agg_call()?;
                (alias, function, args, call_span)
            };
            let span = function.span.join(alias.span);
            items.push(AggItem {
                function,
                args,
                alias,
                span: span.join(call_span),
            });
            if !self.consume_comma() {
                break;
            }
        }
        let end = items.last().map_or(name_span.end, |item| item.span.end);
        Some(Stage::Agg {
            items,
            span: Span::new(name_span.start, end),
        })
    }

    fn parse_sort(&mut self, name_span: Span) -> Option<Stage> {
        let mut items = Vec::new();
        let stage_end = loop {
            let column = self.expect_column_name()?;
            let mut item_end = column.span.end;
            let direction = self.parse_sort_direction(&mut item_end);
            let nulls = self.parse_sort_nulls(&mut item_end);
            if !self.at_sort_item_boundary_after(item_end) {
                self.diagnostics.push(Diagnostic::error(
                    codes::E1214,
                    "malformed sort item",
                    self.current().span,
                ));
                while !self.at_sort_item_boundary_after(item_end) {
                    item_end = self.advance().span.end;
                }
            }
            items.push(SortItem {
                column,
                direction,
                nulls,
            });
            if !self.consume_comma() {
                break item_end;
            }
        };
        Some(Stage::Sort {
            items,
            span: Span::new(name_span.start, stage_end),
        })
    }

    fn parse_sort_direction(&mut self, item_end: &mut usize) -> SortDirection {
        if self.at_pipeline_boundary_after(*item_end) {
            return SortDirection::Asc;
        }

        let token = self.current().clone();
        match token.kind {
            TokenKind::Ident(value) if value == "desc" => {
                self.advance();
                *item_end = token.span.end;
                SortDirection::Desc
            }
            TokenKind::Ident(value) if value == "asc" => {
                self.advance();
                *item_end = token.span.end;
                SortDirection::Asc
            }
            TokenKind::Ident(value) if value.starts_with("nulls") => SortDirection::Asc,
            TokenKind::Ident(value) => {
                self.advance();
                *item_end = token.span.end;
                self.diagnostics.push(Diagnostic::error(
                    codes::E1210,
                    format!("invalid sort direction `{value}`; expected `asc` or `desc`"),
                    token.span,
                ));
                SortDirection::Asc
            }
            _ => SortDirection::Asc,
        }
    }

    fn parse_sort_nulls(&mut self, item_end: &mut usize) -> Option<NullsOrder> {
        if self.at_pipeline_boundary_after(*item_end) {
            return None;
        }

        let token = self.current().clone();
        match token.kind {
            TokenKind::Ident(value) if value == "nulls_first" => {
                self.advance();
                *item_end = token.span.end;
                Some(NullsOrder::First)
            }
            TokenKind::Ident(value) if value == "nulls_last" => {
                self.advance();
                *item_end = token.span.end;
                Some(NullsOrder::Last)
            }
            TokenKind::Ident(value) if value.starts_with("nulls") => {
                self.advance();
                *item_end = token.span.end;
                self.diagnostics.push(Diagnostic::error(
                    codes::E1210,
                    format!(
                        "invalid sort null order `{value}`; expected `nulls_first` or `nulls_last`"
                    ),
                    token.span,
                ));
                None
            }
            _ => None,
        }
    }

    fn parse_limit(&mut self, name_span: Span) -> Option<Stage> {
        let token = self.advance().clone();
        match token.kind {
            TokenKind::Number(raw) => match raw.parse::<usize>() {
                Ok(n) => Some(Stage::Limit {
                    n,
                    span: name_span.join(token.span),
                }),
                Err(_) => {
                    self.diagnostics.push(Diagnostic::error(
                        codes::E1206,
                        "limit requires a non-negative integer",
                        token.span,
                    ));
                    None
                }
            },
            _ => {
                self.diagnostics.push(Diagnostic::error(
                    codes::E1203,
                    "limit requires a row count",
                    token.span,
                ));
                None
            }
        }
    }

    fn parse_distinct(&mut self, name_span: Span) -> Option<Stage> {
        if self.at_stage_boundary() {
            return Some(Stage::Distinct {
                columns: Vec::new(),
                span: name_span,
            });
        }

        let columns = self.parse_column_list()?;
        let end = columns
            .last()
            .map_or(name_span.end, |column| column.span.end);
        Some(Stage::Distinct {
            columns,
            span: Span::new(name_span.start, end),
        })
    }

    fn parse_pivot_longer(&mut self, name_span: Span) -> Option<Stage> {
        let mut columns = Vec::new();
        while !self.at_stage_boundary() && !self.at_ident("names_to") {
            columns.push(self.expect_column_name()?);
            if !self.consume_comma() {
                break;
            }
        }
        if !self.consume_ident("names_to") {
            self.diagnostics.push(Diagnostic::error(
                codes::E1203,
                "pivot_longer requires `names_to`",
                self.current().span,
            ));
        }
        let names_to = self.expect_column_name()?;
        if !self.consume_ident("values_to") {
            self.diagnostics.push(Diagnostic::error(
                codes::E1203,
                "pivot_longer requires `values_to`",
                self.current().span,
            ));
        }
        let values_to = self.expect_column_name()?;
        let mut end = values_to.span.end;
        if !self.at_stage_boundary() {
            self.diagnostics.push(Diagnostic::error(
                codes::E1204,
                "unknown pivot_longer option",
                self.current().span,
            ));
            end = self.consume_until_stage_boundary(name_span).end;
        }
        Some(Stage::PivotLonger {
            columns,
            names_to,
            values_to,
            span: Span::new(name_span.start, end),
        })
    }

    fn parse_complete(&mut self, name_span: Span) -> Option<Stage> {
        let mut keys = Vec::new();
        while !self.at_stage_boundary() && !self.at_ident("fill") {
            keys.push(self.expect_column_name()?);
            if !self.consume_comma() {
                break;
            }
        }
        let mut fills = Vec::new();
        if self.consume_ident("fill") {
            loop {
                let column = self.expect_column_name()?;
                if !self.consume_equal() {
                    self.diagnostics.push(Diagnostic::error(
                        codes::E1203,
                        "complete fill assignments require `=`",
                        self.current().span,
                    ));
                }
                let expr = self.parse_expr(0)?;
                let span = column.span.join(expr.span());
                fills.push(CompleteFillItem { column, expr, span });
                if !self.consume_comma() {
                    break;
                }
            }
        }
        let mut end = fills.last().map_or_else(
            || keys.last().map_or(name_span.end, |key| key.span.end),
            |fill| fill.span.end,
        );
        if !self.at_stage_boundary() {
            self.diagnostics.push(Diagnostic::error(
                codes::E1204,
                "unknown complete option",
                self.current().span,
            ));
            end = self.consume_until_stage_boundary(name_span).end;
        }
        Some(Stage::Complete {
            keys,
            fills,
            span: Span::new(name_span.start, end),
        })
    }

    fn parse_save(&mut self, name_span: Span) -> Option<SaveStage> {
        let sink = self.parse_sink_ref()?;
        let format = self.parse_format_clause();
        let end = format
            .as_ref()
            .map_or_else(|| sink_span(&sink).end, |format| format.span.end);
        Some(SaveStage {
            sink,
            format,
            span: Span::new(name_span.start, end),
        })
    }

    fn parse_column_list(&mut self) -> Option<Vec<Spanned<String>>> {
        let mut columns = Vec::new();
        loop {
            columns.push(self.expect_column_name()?);
            if !self.consume_comma() {
                break;
            }
        }
        Some(columns)
    }

    fn parse_bool_literal(&mut self, option: &str) -> Option<Spanned<bool>> {
        let token = self.advance().clone();
        match token.kind {
            TokenKind::Ident(value) if value == "true" => Some(Spanned::new(true, token.span)),
            TokenKind::Ident(value) if value == "false" => Some(Spanned::new(false, token.span)),
            _ => {
                self.diagnostics.push(Diagnostic::error(
                    codes::E1206,
                    format!("union option `{option}` requires `true` or `false`"),
                    token.span,
                ));
                Some(Spanned::new(false, token.span))
            }
        }
    }

    fn parse_source_ref(&mut self) -> Option<SourceRef> {
        let token = self.advance().clone();
        match token.kind {
            TokenKind::String(value) => Some(SourceRef::Path(Spanned::new(value, token.span))),
            TokenKind::Ident(value) if value == "stdin" => Some(SourceRef::Stdin(token.span)),
            TokenKind::Minus => Some(SourceRef::Stdin(token.span)),
            _ => {
                self.diagnostics.push(Diagnostic::error(
                    codes::E1203,
                    "load requires a path or stdin",
                    token.span,
                ));
                None
            }
        }
    }

    fn parse_sink_ref(&mut self) -> Option<SinkRef> {
        let token = self.advance().clone();
        match token.kind {
            TokenKind::String(value) => Some(SinkRef::Path(Spanned::new(value, token.span))),
            TokenKind::Ident(value) if value == "stdout" => Some(SinkRef::Stdout(token.span)),
            TokenKind::Minus => Some(SinkRef::Stdout(token.span)),
            _ => {
                self.diagnostics.push(Diagnostic::error(
                    codes::E1203,
                    "save requires a path or stdout",
                    token.span,
                ));
                None
            }
        }
    }

    fn parse_format_clause(&mut self) -> Option<Spanned<String>> {
        if !self.consume_ident("format") {
            return None;
        }

        let token = self.advance().clone();
        match token.kind {
            TokenKind::String(value) | TokenKind::Ident(value) => {
                Some(Spanned::new(value, token.span))
            }
            _ => {
                self.diagnostics.push(Diagnostic::error(
                    codes::E1203,
                    "format requires a format name",
                    token.span,
                ));
                None
            }
        }
    }

    fn parse_window_spec(&mut self, over_span: Span) -> Option<WindowSpec> {
        let start = if self.consume_lparen() {
            self.previous_span().start
        } else {
            self.diagnostics.push(Diagnostic::error(
                codes::E1203,
                "window expression requires `(` after `over`",
                self.current().span,
            ));
            over_span.end
        };

        let mut partition_by = Vec::new();
        let mut order_by = Vec::new();
        let mut frame = None;
        let mut seen_partition = false;
        let mut seen_order = false;
        let mut seen_frame = false;

        while !self.at_rparen() && !self.at_stage_boundary() {
            if self.consume_ident("partition_by") {
                let option_span = self.previous_span();
                if seen_partition {
                    self.diagnostics.push(Diagnostic::error(
                        codes::E1205,
                        "duplicate window option `partition_by`",
                        option_span,
                    ));
                }
                seen_partition = true;
                partition_by = self.parse_window_partition_columns()?;
            } else if self.consume_ident("order_by") {
                let option_span = self.previous_span();
                if seen_order {
                    self.diagnostics.push(Diagnostic::error(
                        codes::E1205,
                        "duplicate window option `order_by`",
                        option_span,
                    ));
                }
                seen_order = true;
                order_by = self.parse_window_order_items()?;
            } else if self.consume_ident("frame") {
                let frame_span = self.previous_span();
                if seen_frame {
                    self.diagnostics.push(Diagnostic::error(
                        codes::E1205,
                        "duplicate window option `frame`",
                        frame_span,
                    ));
                }
                seen_frame = true;
                frame = self.parse_window_frame(frame_span);
            } else {
                self.diagnostics.push(Diagnostic::error(
                    codes::E1204,
                    "unknown window option",
                    self.current().span,
                ));
                self.advance();
            }
        }

        let close = self.expect_rparen();
        let end = close.map_or_else(|| self.previous_span().end, |token| token.span.end);
        Some(WindowSpec {
            partition_by,
            order_by,
            frame,
            span: Span::new(start, end),
        })
    }

    fn parse_window_partition_columns(&mut self) -> Option<Vec<Spanned<String>>> {
        let mut columns = Vec::new();
        loop {
            columns.push(self.expect_column_name()?);
            if !self.consume_comma() {
                break;
            }
        }
        Some(columns)
    }

    fn parse_window_order_items(&mut self) -> Option<Vec<SortItem>> {
        let mut items = Vec::new();
        loop {
            items.push(self.parse_window_order_item()?);
            if !self.consume_comma() {
                break;
            }
        }
        Some(items)
    }

    fn parse_window_order_item(&mut self) -> Option<SortItem> {
        let column = self.expect_column_name()?;
        let mut item_end = column.span.end;
        let direction = self.parse_window_sort_direction(&mut item_end);
        let nulls = self.parse_window_sort_nulls(&mut item_end);
        if !self.at_window_sort_item_boundary() {
            self.diagnostics.push(Diagnostic::error(
                codes::E1214,
                "malformed window order item",
                self.current().span,
            ));
            while !self.at_window_sort_item_boundary() {
                item_end = self.advance().span.end;
            }
        }
        let _ = item_end;
        Some(SortItem {
            column,
            direction,
            nulls,
        })
    }

    fn parse_window_sort_direction(&mut self, item_end: &mut usize) -> SortDirection {
        let token = self.current().clone();
        match token.kind {
            TokenKind::Ident(value) if value == "desc" => {
                self.advance();
                *item_end = token.span.end;
                SortDirection::Desc
            }
            TokenKind::Ident(value) if value == "asc" => {
                self.advance();
                *item_end = token.span.end;
                SortDirection::Asc
            }
            TokenKind::Ident(value) if value.starts_with("nulls") || value == "frame" => {
                SortDirection::Asc
            }
            _ => SortDirection::Asc,
        }
    }

    fn parse_window_sort_nulls(&mut self, item_end: &mut usize) -> Option<NullsOrder> {
        let token = self.current().clone();
        match token.kind {
            TokenKind::Ident(value) if value == "nulls_first" => {
                self.advance();
                *item_end = token.span.end;
                Some(NullsOrder::First)
            }
            TokenKind::Ident(value) if value == "nulls_last" => {
                self.advance();
                *item_end = token.span.end;
                Some(NullsOrder::Last)
            }
            TokenKind::Ident(value) if value.starts_with("nulls") => {
                self.advance();
                *item_end = token.span.end;
                self.diagnostics.push(Diagnostic::error(
                    codes::E1210,
                    format!(
                        "invalid window null order `{value}`; expected `nulls_first` or `nulls_last`"
                    ),
                    token.span,
                ));
                None
            }
            _ => None,
        }
    }

    fn parse_window_frame(&mut self, frame_span: Span) -> Option<WindowFrame> {
        let token = self.current().clone();
        let TokenKind::Ident(name) = token.kind else {
            self.diagnostics.push(Diagnostic::error(
                codes::E1230,
                format!(
                    "window frame requires a frame name; expected one of {}",
                    window_frame_name_list()
                ),
                token.span,
            ));
            return None;
        };
        if !WINDOW_FRAME_NAMES.contains(&name.as_str()) {
            let mut message = format!(
                "unknown window frame name `{name}`; expected one of {}",
                window_frame_name_list()
            );
            if let Some(suggestion) = closest_window_frame_name(&name) {
                message.push_str(&format!("; did you mean `{suggestion}`?"));
            }
            self.diagnostics
                .push(Diagnostic::error(codes::E1230, message, token.span));
            return None;
        }
        self.advance();
        let mut end_span = token.span;
        let rows = self.parse_window_frame_rows(&name, token.span, &mut end_span);
        let kind = match name.as_str() {
            "whole_partition" => WindowFrameKind::WholePartition,
            "running" => WindowFrameKind::Running,
            "remaining" => WindowFrameKind::Remaining,
            "trailing" => WindowFrameKind::Trailing { rows: rows? },
            "leading" => WindowFrameKind::Leading { rows: rows? },
            _ => WindowFrameKind::Centered { rows: rows? },
        };
        Some(WindowFrame {
            kind,
            span: frame_span.join(end_span),
        })
    }

    /// Parses the optional integer argument after a frame name and enforces
    /// arity: `trailing` / `leading` / `centered` require it (`E1231`),
    /// `whole_partition` / `running` / `remaining` reject it (`E1232`).
    fn parse_window_frame_rows(
        &mut self,
        name: &str,
        name_span: Span,
        end_span: &mut Span,
    ) -> Option<usize> {
        let takes_rows = matches!(name, "trailing" | "leading" | "centered");
        let token = self.current().clone();
        let TokenKind::Number(raw) = token.kind else {
            if takes_rows {
                self.diagnostics.push(Diagnostic::error(
                    codes::E1231,
                    format!(
                        "window frame `{name}` requires an integer row count (`frame {name} N`)"
                    ),
                    name_span,
                ));
                return None;
            }
            return Some(0);
        };
        if !takes_rows {
            self.diagnostics.push(Diagnostic::error(
                codes::E1232,
                format!("window frame `{name}` does not take an argument"),
                token.span,
            ));
            self.advance();
            *end_span = token.span;
            return Some(0);
        }
        self.advance();
        *end_span = token.span;
        match raw.parse::<usize>() {
            Ok(rows) => Some(rows),
            Err(_) => {
                self.diagnostics.push(Diagnostic::error(
                    codes::E1206,
                    "window frame row count requires a non-negative integer",
                    token.span,
                ));
                Some(0)
            }
        }
    }

    fn parse_expr(&mut self, min_bp: u8) -> Option<Expr> {
        let mut lhs = self.parse_prefix()?;

        loop {
            if self.at_expr_boundary() {
                break;
            }

            let Some((op, left_bp, right_bp)) = self.current_binary_op() else {
                break;
            };
            if left_bp < min_bp {
                break;
            }
            self.advance();
            let rhs = self.parse_expr(right_bp)?;
            let span = lhs.span().join(rhs.span());
            lhs = Expr::Binary {
                left: Box::new(lhs),
                op,
                right: Box::new(rhs),
                span,
            };
        }

        Some(lhs)
    }

    fn parse_prefix(&mut self) -> Option<Expr> {
        let token = self.advance().clone();
        match token.kind {
            TokenKind::String(value) => Some(Expr::Quoted(Spanned::new(value, token.span))),
            TokenKind::BacktickColumn(value) => Some(Expr::Ident(Spanned::new(value, token.span))),
            TokenKind::Number(raw) => match raw.parse::<f64>() {
                Ok(value) => Some(Expr::Number(Spanned::new(value, token.span))),
                Err(_) => {
                    self.diagnostics.push(Diagnostic::error(
                        codes::E1206,
                        "invalid number literal",
                        token.span,
                    ));
                    None
                }
            },
            TokenKind::Ident(value) if value == "true" => {
                Some(Expr::Bool(Spanned::new(true, token.span)))
            }
            TokenKind::Ident(value) if value == "false" => {
                Some(Expr::Bool(Spanned::new(false, token.span)))
            }
            TokenKind::Ident(value) if value == "null" => Some(Expr::Null(token.span)),
            TokenKind::Dollar => self.parse_context_expr(ContextKind::Param, token.span),
            TokenKind::At => self.parse_context_expr(ContextKind::State, token.span),
            TokenKind::Ident(value) if value == "not" => {
                let expr = self.parse_expr(11)?;
                let span = token.span.join(expr.span());
                Some(Expr::Unary {
                    op: UnaryOp::Not,
                    expr: Box::new(expr),
                    span,
                })
            }
            TokenKind::Bang => {
                let expr = self.parse_expr(11)?;
                let span = token.span.join(expr.span());
                Some(Expr::Unary {
                    op: UnaryOp::Not,
                    expr: Box::new(expr),
                    span,
                })
            }
            TokenKind::Minus => {
                let expr = self.parse_expr(11)?;
                let span = token.span.join(expr.span());
                Some(Expr::Unary {
                    op: UnaryOp::Neg,
                    expr: Box::new(expr),
                    span,
                })
            }
            TokenKind::Ident(value) => {
                let name = Spanned::new(value, token.span);
                if let Some(kind) = ControlKind::from_name(&name.value)
                    .filter(|_| self.current().kind == TokenKind::LParen)
                {
                    let control = self.parse_control_initializer_after_name(kind, name);
                    self.diagnostics.push(Diagnostic::error(
                        codes::E2006,
                        "control initializers are valid only as top-level `param` defaults",
                        control.span,
                    ));
                    Some(Expr::Null(control.span))
                } else if self.consume_lparen() {
                    let mut args = Vec::new();
                    if !self.at_rparen() {
                        loop {
                            args.push(self.parse_expr(0)?);
                            if !self.consume_comma() {
                                break;
                            }
                        }
                    }
                    let close = self.expect_rparen().map_or(name.span, |token| token.span);
                    let span = name.span.join(close);
                    if self.consume_ident("over") {
                        let spec = self.parse_window_spec(self.previous_span())?;
                        let span = name.span.join(spec.span);
                        Some(Expr::Window {
                            function: name,
                            args,
                            spec,
                            span,
                        })
                    } else {
                        Some(Expr::Call { name, args, span })
                    }
                } else {
                    if is_reserved_keyword(&name.value) {
                        self.diagnostics.push(
                            Diagnostic::error(
                                codes::E0009,
                                format!(
                                    "reserved keyword `{}` must be escaped with backticks",
                                    name.value
                                ),
                                name.span,
                            )
                            .with_help("use backticks for keyword column names"),
                        );
                    }
                    Some(Expr::Ident(name))
                }
            }
            TokenKind::LParen => {
                let expr = self.parse_expr(0)?;
                self.expect_rparen();
                Some(expr)
            }
            _ => {
                self.diagnostics.push(Diagnostic::error(
                    codes::E0008,
                    "expected expression",
                    token.span,
                ));
                None
            }
        }
    }

    fn current_binary_op(&self) -> Option<(BinaryOp, u8, u8)> {
        match &self.current().kind {
            TokenKind::Ident(value) if value == "or" => Some((BinaryOp::Or, 1, 2)),
            TokenKind::Ident(value) if value == "and" => Some((BinaryOp::And, 3, 4)),
            TokenKind::EqEq => Some((BinaryOp::Eq, 5, 6)),
            TokenKind::NotEq => Some((BinaryOp::Ne, 5, 6)),
            TokenKind::Lt => Some((BinaryOp::Lt, 7, 8)),
            TokenKind::Lte => Some((BinaryOp::Lte, 7, 8)),
            TokenKind::Gt => Some((BinaryOp::Gt, 7, 8)),
            TokenKind::Gte => Some((BinaryOp::Gte, 7, 8)),
            TokenKind::Plus => Some((BinaryOp::Add, 9, 10)),
            TokenKind::Minus => Some((BinaryOp::Sub, 9, 10)),
            TokenKind::Star => Some((BinaryOp::Mul, 11, 12)),
            TokenKind::Slash => Some((BinaryOp::Div, 11, 12)),
            TokenKind::Percent => Some((BinaryOp::Rem, 11, 12)),
            _ => None,
        }
    }

    fn expect_binding_name(&mut self) -> Option<Spanned<String>> {
        let name = self.expect_identifier("binding name")?;
        if is_reserved_keyword(&name.value) {
            self.diagnostics.push(Diagnostic::error(
                codes::E1002,
                format!(
                    "reserved keyword `{}` cannot be used as a binding",
                    name.value
                ),
                name.span,
            ));
        }
        Some(name)
    }

    fn expect_output_name(&mut self) -> Option<Spanned<String>> {
        let name = self.expect_identifier("output name")?;
        if is_reserved_keyword(&name.value) {
            self.diagnostics.push(Diagnostic::error(
                codes::E1002,
                format!(
                    "reserved keyword `{}` cannot be used as an output",
                    name.value
                ),
                name.span,
            ));
        }
        Some(name)
    }

    fn expect_context_name(&mut self, kind: ContextKind) -> Option<Spanned<String>> {
        let label = match kind {
            ContextKind::Param => "parameter name",
            ContextKind::State => "state name",
        };
        let name = self.expect_identifier(label)?;
        if is_reserved_keyword(&name.value) {
            self.diagnostics.push(Diagnostic::error(
                codes::E1002,
                format!(
                    "reserved keyword `{}` cannot be used as a context name",
                    name.value
                ),
                name.span,
            ));
        }
        Some(name)
    }

    fn parse_context_expr(&mut self, kind: ContextKind, sigil_span: Span) -> Option<Expr> {
        let label = match kind {
            ContextKind::Param => "parameter reference",
            ContextKind::State => "state reference",
        };
        let name = self.expect_identifier(label)?;
        let span = sigil_span.join(name.span);
        Some(Expr::Context { kind, name, span })
    }

    fn expect_column_name(&mut self) -> Option<Spanned<String>> {
        let token = self.advance().clone();
        match token.kind {
            TokenKind::Ident(value) => {
                if is_reserved_keyword(&value) {
                    self.diagnostics.push(
                        Diagnostic::error(
                            codes::E0009,
                            format!("reserved keyword `{value}` must be escaped with backticks"),
                            token.span,
                        )
                        .with_help("use backticks for keyword column names"),
                    );
                }
                Some(Spanned::new(value, token.span))
            }
            TokenKind::BacktickColumn(value) => Some(Spanned::new(value, token.span)),
            TokenKind::String(value) => {
                self.quoted_column_diagnostic(&value, token.span);
                Some(Spanned::new(value, token.span))
            }
            TokenKind::Dollar => {
                let name = self.expect_identifier("parameter column reference")?;
                let span = token.span.join(name.span);
                Some(Spanned::new(
                    encode_context_column_ref(ContextKind::Param, &name.value),
                    span,
                ))
            }
            TokenKind::At => {
                let name = self.expect_identifier("state column reference")?;
                let span = token.span.join(name.span);
                Some(Spanned::new(
                    encode_context_column_ref(ContextKind::State, &name.value),
                    span,
                ))
            }
            _ => {
                self.diagnostics.push(
                    Diagnostic::error(codes::E0009, "expected column name", token.span)
                        .with_help("use a bare identifier or backticks for a column reference"),
                );
                None
            }
        }
    }

    fn parse_agg_call(&mut self) -> Option<(Spanned<String>, Vec<Expr>, Span)> {
        let function = self.expect_identifier("aggregate function")?;
        self.expect_lparen();
        let mut args = Vec::new();
        if !self.at_rparen() {
            loop {
                args.push(self.parse_expr(0)?);
                if !self.consume_comma() {
                    break;
                }
            }
        }
        let close_span = self
            .expect_rparen()
            .map_or(function.span, |token| token.span);
        let span = function.span.join(close_span);
        Some((function, args, span))
    }

    fn expect_identifier(&mut self, label: &str) -> Option<Spanned<String>> {
        let token = self.advance().clone();
        match token.kind {
            TokenKind::Ident(value) => Some(Spanned::new(value, token.span)),
            _ => {
                self.diagnostics.push(Diagnostic::error(
                    codes::E0008,
                    format!("expected {label}"),
                    token.span,
                ));
                None
            }
        }
    }

    fn legacy_as_diagnostic(&mut self, span: Span) {
        self.diagnostics.push(
            Diagnostic::error(
                codes::E0027,
                "legacy `as` alias syntax is not valid in v0.26 syntax",
                span,
            )
            .with_help("write aliases as `new_name = expression`"),
        );
    }

    fn quoted_column_diagnostic(&mut self, value: &str, span: Span) {
        self.diagnostics.push(
            Diagnostic::error(
                codes::E0026,
                "double-quoted tokens are string literals, not column references",
                span,
            )
            .with_help(format!(
                "write this column reference as `{}`",
                format_column_reference(value)
            )),
        );
    }

    fn expect_ident(&mut self, expected: &str) -> Option<Token> {
        let token = self.advance().clone();
        match &token.kind {
            TokenKind::Ident(value) if value == expected => Some(token),
            _ => {
                self.diagnostics.push(Diagnostic::error(
                    codes::E0001,
                    format!("expected `{expected}`"),
                    token.span,
                ));
                None
            }
        }
    }

    fn expect_equal(&mut self) {
        let token = self.advance().clone();
        if token.kind != TokenKind::Equal {
            self.diagnostics
                .push(Diagnostic::error(codes::E0001, "expected `=`", token.span));
        }
    }

    fn expect_lparen(&mut self) {
        if !self.consume_lparen() {
            self.diagnostics.push(Diagnostic::error(
                codes::E0001,
                "expected `(`",
                self.current().span,
            ));
        }
    }

    fn expect_rparen(&mut self) -> Option<Token> {
        let token = self.advance().clone();
        if token.kind == TokenKind::RParen {
            Some(token)
        } else {
            self.diagnostics
                .push(Diagnostic::error(codes::E0001, "expected `)`", token.span));
            None
        }
    }

    fn expect_rbracket(&mut self) -> Option<Token> {
        let token = self.advance().clone();
        if token.kind == TokenKind::RBracket {
            Some(token)
        } else {
            self.diagnostics
                .push(Diagnostic::error(codes::E0001, "expected `]`", token.span));
            None
        }
    }

    fn expect_dot(&mut self) {
        let token = self.advance().clone();
        if token.kind != TokenKind::Dot {
            self.diagnostics
                .push(Diagnostic::error(codes::E0001, "expected `.`", token.span));
        }
    }

    fn consume_ident_value(&mut self) -> Option<Spanned<String>> {
        let token = self.current().clone();
        if let TokenKind::Ident(value) = token.kind {
            self.pos += 1;
            Some(Spanned::new(value, token.span))
        } else {
            None
        }
    }

    fn consume_ident(&mut self, expected: &str) -> bool {
        if self.at_ident(expected) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn consume_lparen(&mut self) -> bool {
        if self.current().kind == TokenKind::LParen {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn consume_lbracket(&mut self) -> bool {
        if self.current().kind == TokenKind::LBracket {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn consume_colon(&mut self) -> bool {
        if self.current().kind == TokenKind::Colon {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn consume_equal(&mut self) -> bool {
        if self.current().kind == TokenKind::Equal {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn consume_pipe(&mut self) -> bool {
        if self.current().kind == TokenKind::Pipe {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn consume_comma(&mut self) -> bool {
        if self.current().kind == TokenKind::Comma {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn at_ident(&self, expected: &str) -> bool {
        matches!(&self.current().kind, TokenKind::Ident(value) if value == expected)
    }

    fn at_eof(&self) -> bool {
        self.current().kind == TokenKind::Eof
    }

    fn at_rparen(&self) -> bool {
        self.current().kind == TokenKind::RParen
    }

    fn at_rbracket(&self) -> bool {
        self.current().kind == TokenKind::RBracket
    }

    fn at_control_initializer_start(&self) -> bool {
        matches!(&self.current().kind, TokenKind::Ident(value) if ControlKind::from_name(value).is_some())
            && self
                .tokens
                .get(self.pos + 1)
                .is_some_and(|token| token.kind == TokenKind::LParen)
    }

    fn at_ident_followed_by_dot(&self) -> bool {
        matches!(self.current().kind, TokenKind::Ident(_))
            && self
                .tokens
                .get(self.pos + 1)
                .is_some_and(|token| token.kind == TokenKind::Dot)
    }

    fn at_ident_followed_by_column_name(&self) -> bool {
        matches!(self.current().kind, TokenKind::Ident(_))
            && self.tokens.get(self.pos + 1).is_some_and(|token| {
                matches!(
                    token.kind,
                    TokenKind::Ident(_)
                        | TokenKind::BacktickColumn(_)
                        | TokenKind::String(_)
                        | TokenKind::Dollar
                        | TokenKind::At
                )
            })
    }

    fn at_ident_followed_by_lparen(&self) -> bool {
        matches!(self.current().kind, TokenKind::Ident(_))
            && self
                .tokens
                .get(self.pos + 1)
                .is_some_and(|token| token.kind == TokenKind::LParen)
    }

    fn at_expr_boundary(&self) -> bool {
        matches!(
            self.current().kind,
            TokenKind::Comma | TokenKind::Pipe | TokenKind::RParen | TokenKind::Eof
        )
    }

    fn at_stage_boundary(&self) -> bool {
        matches!(self.current().kind, TokenKind::Pipe | TokenKind::Eof)
    }

    fn at_sort_item_boundary_after(&self, item_end: usize) -> bool {
        matches!(
            self.current().kind,
            TokenKind::Comma | TokenKind::Pipe | TokenKind::Eof
        ) || self.at_pipeline_boundary_after(item_end)
    }

    fn at_window_sort_item_boundary(&self) -> bool {
        matches!(
            self.current().kind,
            TokenKind::Comma | TokenKind::RParen | TokenKind::Pipe | TokenKind::Eof
        ) || self.at_ident("frame")
    }

    fn at_recoverable_stage_start(&self) -> bool {
        matches!(&self.current().kind, TokenKind::Ident(value) if is_recoverable_stage_name(value))
    }

    fn at_pipeline_boundary_after(&self, end: usize) -> bool {
        matches!(self.current().kind, TokenKind::Ident(_))
            && !self.current_is_on_same_line_after_end(end)
    }

    fn current(&self) -> &Token {
        &self.tokens[self.pos]
    }

    fn advance(&mut self) -> &Token {
        let pos = self.pos;
        if !self.at_eof() {
            self.pos += 1;
        }
        &self.tokens[pos]
    }

    fn previous_span(&self) -> Span {
        self.tokens
            .get(self.pos.saturating_sub(1))
            .map_or(Span::zero(), |token| token.span)
    }

    fn recover_to_pipe_or_eof(&mut self) {
        while !matches!(self.current().kind, TokenKind::Pipe | TokenKind::Eof) {
            self.pos += 1;
        }
    }

    fn recover_to_eof(&mut self) {
        while !self.at_eof() {
            self.pos += 1;
        }
    }

    fn recover_to_control_arg_boundary(&mut self) {
        while !matches!(
            self.current().kind,
            TokenKind::Comma | TokenKind::RParen | TokenKind::Eof
        ) {
            self.pos += 1;
        }
    }

    fn current_is_on_same_line_after(&self, span: Span) -> bool {
        self.current_is_on_same_line_after_end(span.end)
    }

    fn current_is_on_same_line_after_end(&self, end: usize) -> bool {
        !self.source[end..self.current().span.start].contains('\n')
    }

    fn consume_until_stage_boundary(&mut self, start: Span) -> Span {
        let mut end = start.end;
        while !matches!(self.current().kind, TokenKind::Pipe | TokenKind::Eof) {
            end = self.advance().span.end;
        }
        Span::new(start.start, end)
    }
}

fn source_span(source: &SourceRef) -> Span {
    match source {
        SourceRef::Path(value) => value.span,
        SourceRef::Stdin(span) => *span,
    }
}

fn sink_span(sink: &SinkRef) -> Span {
    match sink {
        SinkRef::Path(value) => value.span,
        SinkRef::Stdout(span) => *span,
    }
}

fn control_default_expr(control: &ControlInitializer) -> Expr {
    control
        .args
        .iter()
        .find(|arg| arg.name.value == "default")
        .and_then(|arg| match &arg.value {
            ControlValue::Literal(value) => Some(value.to_expr()),
            _ => None,
        })
        .unwrap_or(Expr::Null(control.span))
}

fn is_recoverable_stage_name(value: &str) -> bool {
    matches!(
        value,
        "filter"
            | "select"
            | "drop"
            | "rename"
            | "mutate"
            | "group_by"
            | "agg"
            | "sort"
            | "limit"
            | "save"
            | "join"
            | "union"
            | "distinct"
            | "pivot_longer"
            | "complete"
    )
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
            | "param"
            | "state"
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
            | "frame"
            | "whole_partition"
            | "running"
            | "remaining"
            | "trailing"
            | "leading"
            | "centered"
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

fn format_column_reference(value: &str) -> String {
    if is_simple_column_name(value) && !is_reserved_keyword(value) {
        return value.to_string();
    }
    let escaped = value.replace('\\', "\\\\").replace('`', "\\`");
    format!("`{escaped}`")
}

fn window_frame_name_list() -> String {
    WINDOW_FRAME_NAMES
        .iter()
        .map(|name| format!("`{name}`"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn closest_window_frame_name(name: &str) -> Option<&'static str> {
    WINDOW_FRAME_NAMES
        .iter()
        .map(|candidate| (levenshtein_distance(name, candidate), *candidate))
        .min()
        .filter(|(distance, _)| *distance <= 3)
        .map(|(_, candidate)| candidate)
}

fn levenshtein_distance(left: &str, right: &str) -> usize {
    let left: Vec<char> = left.chars().collect();
    let right: Vec<char> = right.chars().collect();
    let mut previous: Vec<usize> = (0..=right.len()).collect();
    let mut current = vec![0; right.len() + 1];
    for (row, left_ch) in left.iter().enumerate() {
        current[0] = row + 1;
        for (column, right_ch) in right.iter().enumerate() {
            let substitution = previous[column] + usize::from(left_ch != right_ch);
            current[column + 1] = substitution
                .min(previous[column + 1] + 1)
                .min(current[column] + 1);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[right.len()]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_top_regions_shape() {
        let result = parse(
            r#"load "sales.csv"
  | filter status == "completed"
  | group_by region
  | agg total = sum(amount)
  | sort total desc
  | limit 5
  | save "out.csv""#,
        );

        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        let main = result.program.main.expect("main pipeline");
        assert_eq!(main.stages.len(), 6);
    }

    #[test]
    fn reports_unknown_stage() {
        let result = parse(r#"load "sales.csv" | nope "x""#);
        assert_eq!(result.diagnostics[0].code, "E1201");
    }

    #[test]
    fn reports_invalid_sort_direction() {
        let source = r#"load "sales.csv"
  | filter status == "completed"
  | group_by region
  | agg total_revenue = sum(amount)
  | sort total_revenue des"#;
        let result = parse(source);

        assert_eq!(result.diagnostics.len(), 1, "{:?}", result.diagnostics);
        assert_eq!(result.diagnostics[0].code, "E1210");
        assert_eq!(
            result.diagnostics[0].span.start,
            source.find("des").expect("direction offset")
        );
    }

    #[test]
    fn parses_binding_ending_in_sort_before_main_binding_reference() {
        let result = parse(
            r#"let cleaned =
  load "orders_raw.csv"
    | sort order_date

cleaned
  | group_by region
  | agg orders = count()"#,
        );

        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert_eq!(result.program.bindings.len(), 1);
        let binding = &result.program.bindings[0];
        let Stage::Sort { items, .. } = binding.pipeline.stages.last().expect("binding sort stage")
        else {
            panic!("sort stage");
        };
        assert_eq!(items[0].column.value, "order_date");
        assert_eq!(items[0].direction, SortDirection::Asc);

        let main = result.program.main.expect("main pipeline");
        assert!(matches!(
            main.start,
            PipelineStart::Binding(Spanned { ref value, .. }) if value == "cleaned"
        ));
        assert_eq!(main.stages.len(), 2);
    }

    #[test]
    fn reports_missing_filter_operator_and_recovers_to_next_stage() {
        let source = r#"load "sales.csv"
  | filter status "completed"
  | sort status desc"#;
        let result = parse(source);

        assert_eq!(result.diagnostics.len(), 1, "{:?}", result.diagnostics);
        assert_eq!(result.diagnostics[0].code, "E0001");
        assert_eq!(
            result.diagnostics[0].span.start,
            source.find("\"completed\"").expect("literal offset")
        );
        let main = result.program.main.expect("main pipeline");
        assert_eq!(main.stages.len(), 2);
    }

    #[test]
    fn reports_single_equal_filter_operator_and_recovers_comparison() {
        let source = r#"load "sales.csv" | filter staus = "completed""#;
        let result = parse(source);

        assert_eq!(result.diagnostics.len(), 1, "{:?}", result.diagnostics);
        assert_eq!(result.diagnostics[0].code, "E0001");
        assert_eq!(
            result.diagnostics[0].span.start,
            source.find('=').expect("operator offset")
        );
        let main = result.program.main.expect("main pipeline");
        let Stage::Filter { expr, .. } = &main.stages[0] else {
            panic!("filter stage");
        };
        assert!(matches!(
            expr,
            Expr::Binary {
                op: BinaryOp::Eq,
                ..
            }
        ));
    }

    #[test]
    fn reports_missing_pipe_before_stage_and_recovers() {
        let source = r#"load "sales.csv"
  filter staus == "completed"
  | group_by region
  | agg orders = count()"#;
        let result = parse(source);

        assert_eq!(result.diagnostics.len(), 1, "{:?}", result.diagnostics);
        assert_eq!(result.diagnostics[0].code, "E0001");
        assert_eq!(
            result.diagnostics[0].span.start,
            source.find("filter").expect("filter offset")
        );
        let main = result.program.main.expect("main pipeline");
        assert_eq!(main.stages.len(), 3);
        assert!(matches!(main.stages[0], Stage::Filter { .. }));
    }

    #[test]
    fn parses_mutate_and_distinct_stages() {
        let result = parse(
            r#"load "orders.csv"
  | mutate net_amount = gross_amount - discount, label = concat(upper(region), ":", lower(channel))
  | distinct order_id"#,
        );

        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        let main = result.program.main.expect("main pipeline");
        let Stage::Mutate { items, .. } = &main.stages[0] else {
            panic!("mutate stage");
        };
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].column.value, "net_amount");
        assert!(matches!(items[0].expr, Expr::Binary { .. }));
        let Stage::Distinct { columns, .. } = &main.stages[1] else {
            panic!("distinct stage");
        };
        assert_eq!(columns[0].value, "order_id");
    }

    #[test]
    fn parses_window_expressions() {
        let result = parse(
            r#"load "orders.csv"
  | mutate running_amount = sum(amount) over (partition_by customer_id order_by order_date asc frame running), rank = dense_rank() over (partition_by region order_by amount desc nulls_last)"#,
        );

        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        let main = result.program.main.expect("main pipeline");
        let Stage::Mutate { items, .. } = &main.stages[0] else {
            panic!("mutate stage");
        };
        let Expr::Window { function, spec, .. } = &items[0].expr else {
            panic!("window expression");
        };
        assert_eq!(function.value, "sum");
        assert_eq!(spec.partition_by[0].value, "customer_id");
        assert_eq!(spec.order_by[0].column.value, "order_date");
        assert_eq!(
            spec.frame.as_ref().map(|frame| frame.kind),
            Some(WindowFrameKind::Running)
        );
    }

    fn parse_window_frame_kind(frame: &str) -> WindowFrameKind {
        let source = format!(
            r#"load "orders.csv"
  | mutate value = sum(amount) over (partition_by region order_by amount {frame})"#
        );
        let result = parse(&source);
        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        let main = result.program.main.expect("main pipeline");
        let Stage::Mutate { items, .. } = &main.stages[0] else {
            panic!("mutate stage");
        };
        let Expr::Window { spec, .. } = &items[0].expr else {
            panic!("window expression");
        };
        spec.frame.as_ref().expect("window frame").kind
    }

    fn parse_window_frame_diagnostics(frame: &str) -> Vec<Diagnostic> {
        let source = format!(
            r#"load "orders.csv"
  | mutate value = sum(amount) over (partition_by region order_by amount {frame})"#
        );
        parse(&source).diagnostics
    }

    #[test]
    fn parses_window_frame_named_forms() {
        assert_eq!(
            parse_window_frame_kind("frame whole_partition"),
            WindowFrameKind::WholePartition
        );
        assert_eq!(
            parse_window_frame_kind("frame running"),
            WindowFrameKind::Running
        );
        assert_eq!(
            parse_window_frame_kind("frame remaining"),
            WindowFrameKind::Remaining
        );
        assert_eq!(
            parse_window_frame_kind("frame trailing 3"),
            WindowFrameKind::Trailing { rows: 3 }
        );
        assert_eq!(
            parse_window_frame_kind("frame leading 2"),
            WindowFrameKind::Leading { rows: 2 }
        );
        assert_eq!(
            parse_window_frame_kind("frame centered 1"),
            WindowFrameKind::Centered { rows: 1 }
        );
        assert_eq!(
            parse_window_frame_kind("frame trailing 0"),
            WindowFrameKind::Trailing { rows: 0 }
        );
    }

    #[test]
    fn window_frame_named_unknown_name_suggests_closest() {
        let diagnostics = parse_window_frame_diagnostics("frame trialing 3");
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == codes::E1230
                    && diagnostic.message.contains("did you mean `trailing`?")),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn window_frame_named_missing_argument_is_rejected() {
        let diagnostics = parse_window_frame_diagnostics("frame trailing");
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == codes::E1231),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn window_frame_named_unexpected_argument_is_rejected() {
        let diagnostics = parse_window_frame_diagnostics("frame running 3");
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == codes::E1232),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn window_frame_named_legacy_syntax_is_a_parse_error() {
        // Composed at runtime so the repository-wide legacy-syntax grep
        // guard does not match this deliberate negative fixture.
        let legacy = ["rows", "between unbounded_preceding and current_row"].join(" ");
        let diagnostics = parse_window_frame_diagnostics(&legacy);
        assert!(!diagnostics.is_empty());
    }

    #[test]
    fn parses_join_and_union_stages() {
        let result = parse(
            r#"let customers =
  load "customers.csv"

load "sales.csv"
  | join customers on (customer_id, id) kind left
  | union customers by_name true distinct false"#,
        );

        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        let main = result.program.main.expect("main pipeline");
        let Stage::Join {
            source, on, kind, ..
        } = &main.stages[0]
        else {
            panic!("join stage");
        };
        assert_eq!(source.value, "customers");
        assert_eq!(on.left().value, "customer_id");
        assert_eq!(on.right().value, "id");
        assert_eq!(*kind, JoinKind::Left);
        let Stage::Union {
            source, options, ..
        } = &main.stages[1]
        else {
            panic!("union stage");
        };
        assert_eq!(source.value, "customers");
        assert_eq!(options.len(), 2);
        assert_eq!(options[0].kind, UnionOptionKind::ByName);
        assert!(options[0].value.value);
        assert_eq!(options[1].kind, UnionOptionKind::Distinct);
        assert!(!options[1].value.value);
    }

    #[test]
    fn parses_composite_join_keys() {
        let result = parse(
            r#"load "sales.csv"
  | join customers on customer_id, order_date
  | join products on (sku, product_sku), (region, market)"#,
        );

        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        let main = result.program.main.expect("main pipeline");
        let Stage::Join { on, .. } = &main.stages[0] else {
            panic!("same-name composite join stage");
        };
        let JoinOn::Composite { keys, .. } = on else {
            panic!("composite join");
        };
        assert_eq!(keys[0].left.value, "customer_id");
        assert_eq!(keys[0].right.value, "customer_id");
        assert_eq!(keys[1].left.value, "order_date");
        assert_eq!(keys[1].right.value, "order_date");

        let Stage::Join { on, .. } = &main.stages[1] else {
            panic!("paired composite join stage");
        };
        let JoinOn::Composite { keys, .. } = on else {
            panic!("composite join");
        };
        assert_eq!(keys[0].left.value, "sku");
        assert_eq!(keys[0].right.value, "product_sku");
        assert_eq!(keys[1].left.value, "region");
        assert_eq!(keys[1].right.value, "market");
    }

    #[test]
    fn invalid_join_kind_uses_join_kind_diagnostic() {
        let result = parse(r#"load "sales.csv" | join customers on id kind outer"#);

        assert_eq!(result.diagnostics.len(), 1, "{:?}", result.diagnostics);
        assert_eq!(result.diagnostics[0].code, "E1223");
    }

    #[test]
    fn reports_missing_aggregate_assignment_without_extra_alias_error() {
        let source = r#"load "sales.csv" | agg total_revenue sum(amount)"#;
        let result = parse(source);

        assert_eq!(result.diagnostics.len(), 1, "{:?}", result.diagnostics);
        assert_eq!(result.diagnostics[0].code, "E1417");
        assert_eq!(
            result.diagnostics[0].span.start,
            source.find("sum").expect("aggregate function offset")
        );
        let main = result.program.main.expect("main pipeline");
        let Stage::Agg { items, .. } = &main.stages[0] else {
            panic!("agg stage");
        };
        assert_eq!(items[0].alias.value, "total_revenue");
    }

    #[test]
    fn reports_legacy_aggregate_as_and_recovers_alias() {
        let source = r#"load "sales.csv" | agg sum(amount) as total_revenue"#;
        let result = parse(source);

        assert_eq!(result.diagnostics.len(), 1, "{:?}", result.diagnostics);
        assert_eq!(result.diagnostics[0].code, "E0027");
        let main = result.program.main.expect("main pipeline");
        let Stage::Agg { items, .. } = &main.stages[0] else {
            panic!("agg stage");
        };
        assert_eq!(items[0].alias.value, "total_revenue");
    }

    #[test]
    fn parses_as_as_column_name_when_not_alias_syntax() {
        let result = parse(r#"load "sales.csv" | select as | agg as = count()"#);

        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        let main = result.program.main.expect("main pipeline");
        let Stage::Select { items, .. } = &main.stages[0] else {
            panic!("select stage");
        };
        assert_eq!(items[0].column.value, "as");
        let Stage::Agg { items, .. } = &main.stages[1] else {
            panic!("agg stage");
        };
        assert_eq!(items[0].alias.value, "as");
    }

    #[test]
    fn parses_context_declarations_and_references() {
        let source = r#"param metric_column = "revenue"
state selected_zone = "Downtown"

load "trips.csv"
  | filter zone == @selected_zone
  | group_by $metric_column"#;
        let result = parse(source);

        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert_eq!(result.program.contexts.len(), 2);
        assert_eq!(result.program.contexts[0].kind, ContextKind::Param);
        assert_eq!(result.program.contexts[0].name.value, "metric_column");
        let main = result.program.main.expect("main pipeline");
        let Stage::GroupBy { columns, .. } = &main.stages[1] else {
            panic!("expected group_by stage");
        };
        assert_eq!(
            decode_context_column_ref(&columns[0].value),
            Some((ContextKind::Param, "metric_column"))
        );
    }

    #[test]
    fn parses_param_control_initializers() {
        let source = r#"param min_commits = input_range(label: "Min Commits", min: 0, max: 500, default: 50, step: 10)
param active_author = input_select(label: "Author", choices: ["all", "Jane"], choicesFrom: author_totals.author_name, default: "all")

let author_totals =
  load "authors.csv"
    | select author_name

author_totals
  | filter author_name == $active_author"#;
        let result = parse(source);

        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert_eq!(result.program.contexts.len(), 2);
        let control = result.program.contexts[0]
            .control
            .as_ref()
            .expect("range control");
        assert_eq!(control.kind, ControlKind::Range);
        assert_eq!(control.args.len(), 5);
        assert!(matches!(
            result.program.contexts[0].default,
            Expr::Number(Spanned { value: 50.0, .. })
        ));
        let select = result.program.contexts[1]
            .control
            .as_ref()
            .expect("select control");
        assert!(select.args.iter().any(|arg| matches!(
            &arg.value,
            ControlValue::BindingColumn { binding, column, .. }
                if binding.value == "author_totals" && column.value == "author_name"
        )));
    }

    #[test]
    fn rejects_control_initializers_in_row_expressions() {
        let result = parse(
            r#"load "authors.csv"
  | filter input_text(label: "Search", default: "") == author_name"#,
        );

        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == codes::E2006),
            "{:?}",
            result.diagnostics
        );
    }

    #[test]
    fn reports_trailing_tokens_after_pipeline() {
        let source = r#"load "sales.csv" "extra""#;
        let result = parse(source);

        assert_eq!(result.diagnostics.len(), 1, "{:?}", result.diagnostics);
        assert_eq!(result.diagnostics[0].code, "E0021");
        assert_eq!(
            result.diagnostics[0].span.start,
            source.find("\"extra\"").expect("trailing offset")
        );
    }
}
