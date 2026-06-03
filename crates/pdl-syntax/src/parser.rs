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
    EqEq,
    NotEq,
    Lt,
    Lte,
    Gt,
    Gte,
    Bang,
    Eof,
    Error,
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
            6 => SyntaxKind::Number,
            7 => SyntaxKind::Pipe,
            8 => SyntaxKind::Comma,
            9 => SyntaxKind::Equal,
            10 => SyntaxKind::LParen,
            11 => SyntaxKind::RParen,
            12 => SyntaxKind::Plus,
            13 => SyntaxKind::Minus,
            14 => SyntaxKind::Star,
            15 => SyntaxKind::Slash,
            16 => SyntaxKind::Percent,
            17 => SyntaxKind::EqEq,
            18 => SyntaxKind::NotEq,
            19 => SyntaxKind::Lt,
            20 => SyntaxKind::Lte,
            21 => SyntaxKind::Gt,
            22 => SyntaxKind::Gte,
            23 => SyntaxKind::Bang,
            24 => SyntaxKind::Eof,
            25 => SyntaxKind::Error,
            26 => SyntaxKind::BindingDecl,
            27 => SyntaxKind::PipelineExpr,
            28 => SyntaxKind::LoadStageNode,
            29 => SyntaxKind::BindingRefNode,
            30 => SyntaxKind::StageNode,
            31 => SyntaxKind::SaveStageNode,
            32 => SyntaxKind::SelectItemNode,
            33 => SyntaxKind::RenameItemNode,
            34 => SyntaxKind::AggItemNode,
            35 => SyntaxKind::SortItemNode,
            36 => SyntaxKind::MutateItemNode,
            37 => SyntaxKind::ExprNode,
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
    pub bindings: Vec<Binding>,
    pub main: Option<Pipeline>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Binding {
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
}

impl JoinOn {
    pub fn span(&self) -> Span {
        match self {
            JoinOn::Same(column) => column.span,
            JoinOn::Pair { span, .. } => *span,
        }
    }

    pub fn left(&self) -> &Spanned<String> {
        match self {
            JoinOn::Same(column) => column,
            JoinOn::Pair { left, .. } => left,
        }
    }

    pub fn right(&self) -> &Spanned<String> {
        match self {
            JoinOn::Same(column) => column,
            JoinOn::Pair { right, .. } => right,
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
pub enum Expr {
    Quoted(Spanned<String>),
    Number(Spanned<f64>),
    Bool(Spanned<bool>),
    Null(Span),
    Ident(Spanned<String>),
    Call {
        name: Spanned<String>,
        args: Vec<Expr>,
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
            Expr::Call { span, .. } | Expr::Unary { span, .. } | Expr::Binary { span, .. } => *span,
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
        let mut bindings = Vec::new();
        while self.at_ident("let") {
            if let Some(binding) = self.parse_binding() {
                bindings.push(binding);
            } else {
                self.recover_to_pipe_or_eof();
            }
        }

        let main = if self.at_eof() {
            self.diagnostics.push(Diagnostic::error(
                codes::E1502,
                "no runnable main pipeline",
                self.current().span,
            ));
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
            Program { bindings, main },
            std::mem::take(&mut self.diagnostics),
        )
    }

    fn parse_binding(&mut self) -> Option<Binding> {
        self.expect_ident("let")?;
        let name = self.expect_binding_name()?;
        self.expect_equal();
        let pipeline = self.parse_pipeline()?;
        Some(Binding { name, pipeline })
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
            return Some(JoinOn::Pair {
                left,
                right,
                span: Span::new(start, end),
            });
        }

        self.expect_column_name().map(JoinOn::Same)
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
            let column = self.expect_column_name()?;
            let alias = if self.consume_ident("as") {
                Some(self.expect_column_name()?)
            } else {
                None
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
            let old = self.expect_column_name()?;
            if !self.consume_ident("as") {
                self.diagnostics.push(Diagnostic::error(
                    codes::E1203,
                    "rename items require `as`",
                    self.current().span,
                ));
            }
            let new = self.expect_column_name()?;
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
            if !self.consume_ident("as") {
                let diagnostic_span = if self.at_ident_followed_by_column_name() {
                    self.advance().span
                } else {
                    close_span
                };
                self.diagnostics.push(Diagnostic::error(
                    codes::E1213,
                    "aggregate items require `as`",
                    diagnostic_span,
                ));
            }
            let alias = self.expect_column_name()?;
            let span = function.span.join(alias.span);
            items.push(AggItem {
                function,
                args,
                alias,
                span,
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
            if !self.at_sort_item_boundary() {
                self.diagnostics.push(Diagnostic::error(
                    codes::E1214,
                    "malformed sort item",
                    self.current().span,
                ));
                while !self.at_sort_item_boundary() {
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
                if self.consume_lparen() {
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
                    Some(Expr::Call { name, args, span })
                } else {
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

    fn expect_column_name(&mut self) -> Option<Spanned<String>> {
        let token = self.advance().clone();
        match token.kind {
            TokenKind::String(value) => Some(Spanned::new(value, token.span)),
            _ => {
                self.diagnostics.push(Diagnostic::error(
                    codes::E0009,
                    "expected quoted column name",
                    token.span,
                ));
                None
            }
        }
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

    fn at_ident_followed_by_column_name(&self) -> bool {
        matches!(self.current().kind, TokenKind::Ident(_))
            && self
                .tokens
                .get(self.pos + 1)
                .is_some_and(|token| matches!(token.kind, TokenKind::String(_)))
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

    fn at_sort_item_boundary(&self) -> bool {
        matches!(
            self.current().kind,
            TokenKind::Comma | TokenKind::Pipe | TokenKind::Eof
        )
    }

    fn at_recoverable_stage_start(&self) -> bool {
        matches!(&self.current().kind, TokenKind::Ident(value) if is_recoverable_stage_name(value))
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

    fn current_is_on_same_line_after(&self, span: Span) -> bool {
        !self.source[span.end..self.current().span.start].contains('\n')
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
            | "let"
            | "as"
            | "on"
            | "kind"
            | "by_name"
            | "format"
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_top_regions_shape() {
        let result = parse(
            r#"load "sales.csv"
  | filter "status" == "completed"
  | group_by "region"
  | agg sum("amount") as "total"
  | sort "total" desc
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
  | filter "status" == "completed"
  | group_by "region"
  | agg sum("amount") as "total_revenue"
  | sort "total_revenue" des"#;
        let result = parse(source);

        assert_eq!(result.diagnostics.len(), 1, "{:?}", result.diagnostics);
        assert_eq!(result.diagnostics[0].code, "E1210");
        assert_eq!(
            result.diagnostics[0].span.start,
            source.find("des").expect("direction offset")
        );
    }

    #[test]
    fn reports_missing_filter_operator_and_recovers_to_next_stage() {
        let source = r#"load "sales.csv"
  | filter "status" "completed"
  | sort "status" desc"#;
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
        let source = r#"load "sales.csv" | filter "staus" = "completed""#;
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
  filter "staus" == "completed"
  | group_by "region"
  | agg count() as "orders""#;
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
  | mutate "net_amount" = "gross_amount" - "discount", "label" = concat(upper("region"), lit(":"), lower("channel"))
  | distinct "order_id""#,
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
    fn parses_join_and_union_stages() {
        let result = parse(
            r#"let customers =
  load "customers.csv"

load "sales.csv"
  | join customers on ("customer_id", "id") kind left
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
    fn invalid_join_kind_uses_join_kind_diagnostic() {
        let result = parse(r#"load "sales.csv" | join customers on "id" kind outer"#);

        assert_eq!(result.diagnostics.len(), 1, "{:?}", result.diagnostics);
        assert_eq!(result.diagnostics[0].code, "E1223");
    }

    #[test]
    fn reports_mistyped_aggregate_as_without_extra_alias_error() {
        let source = r#"load "sales.csv" | agg sum("amount") a "total_revenue""#;
        let result = parse(source);

        assert_eq!(result.diagnostics.len(), 1, "{:?}", result.diagnostics);
        assert_eq!(result.diagnostics[0].code, "E1213");
        assert_eq!(
            result.diagnostics[0].span.start,
            source.find(" a ").expect("mistyped as offset") + 1
        );
        let main = result.program.main.expect("main pipeline");
        let Stage::Agg { items, .. } = &main.stages[0] else {
            panic!("agg stage");
        };
        assert_eq!(items[0].alias.value, "total_revenue");
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
