use logos::Logos;
use pdl_core::{Diagnostic, Span};
use rowan::{
    GreenNode, GreenNodeBuilder, Language, SyntaxKind as RowanSyntaxKind,
    SyntaxNode as RowanSyntaxNode,
};

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
    Error,
}

impl SyntaxKind {
    fn from_raw(raw: u16) -> Self {
        match raw {
            0 => SyntaxKind::Root,
            1 => SyntaxKind::Ident,
            2 => SyntaxKind::String,
            3 => SyntaxKind::Number,
            4 => SyntaxKind::Pipe,
            5 => SyntaxKind::Comma,
            6 => SyntaxKind::Equal,
            7 => SyntaxKind::LParen,
            8 => SyntaxKind::RParen,
            9 => SyntaxKind::Plus,
            10 => SyntaxKind::Minus,
            11 => SyntaxKind::Star,
            12 => SyntaxKind::Slash,
            13 => SyntaxKind::Percent,
            14 => SyntaxKind::EqEq,
            15 => SyntaxKind::NotEq,
            16 => SyntaxKind::Lt,
            17 => SyntaxKind::Lte,
            18 => SyntaxKind::Gt,
            19 => SyntaxKind::Gte,
            20 => SyntaxKind::Bang,
            _ => SyntaxKind::Error,
        }
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
            | Stage::GroupBy { span, .. }
            | Stage::Agg { span, .. }
            | Stage::Sort { span, .. }
            | Stage::Limit { span, .. }
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

#[derive(Clone, Debug, PartialEq)]
enum TokenKind {
    Ident(String),
    String(String),
    Number(String),
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
}

#[derive(Clone, Debug, PartialEq)]
struct Token {
    kind: TokenKind,
    span: Span,
}

pub fn parse(source: &str) -> ParseResult {
    let lexed = lex(source);
    let syntax = SyntaxNode::new_root(lexed.green);
    let mut parser = Parser::new(
        tokens_with_eof(lexed.tokens, source.len()),
        lexed.diagnostics,
    );
    parser.parse_program(syntax)
}

struct Lexed {
    tokens: Vec<Token>,
    diagnostics: Vec<Diagnostic>,
    green: GreenNode,
}

#[derive(Logos, Debug, PartialEq)]
#[logos(skip r"[ \t\n\f\r]+")]
#[logos(skip r"//[^\n]*")]
enum RawTokenKind {
    #[regex(r#""([^"\\]|\\.)*""#)]
    Quoted,
    #[regex(r"[0-9]+(\.[0-9]+)?([eE][+-]?[0-9]+)?")]
    Number,
    #[regex(r"[A-Za-z_][A-Za-z0-9_]*")]
    Ident,
    #[token("|")]
    Pipe,
    #[token(",")]
    Comma,
    #[token("==")]
    EqEq,
    #[token("!=")]
    NotEq,
    #[token("<=")]
    Lte,
    #[token(">=")]
    Gte,
    #[token("=")]
    Equal,
    #[token("(")]
    LParen,
    #[token(")")]
    RParen,
    #[token("+")]
    Plus,
    #[token("-")]
    Minus,
    #[token("*")]
    Star,
    #[token("/")]
    Slash,
    #[token("%")]
    Percent,
    #[token("<")]
    Lt,
    #[token(">")]
    Gt,
    #[token("!")]
    Bang,
}

fn lex(source: &str) -> Lexed {
    let (source_without_block_comments, mut diagnostics) = strip_block_comments(source);
    let mut lexer = RawTokenKind::lexer(&source_without_block_comments);
    let mut tokens = Vec::new();
    let mut builder = GreenNodeBuilder::new();
    builder.start_node(RowanSyntaxKind(SyntaxKind::Root as u16));

    while let Some(raw) = lexer.next() {
        let range = lexer.span();
        let span = Span::new(range.start, range.end);
        let text = &source[range.clone()];
        match raw {
            Ok(raw) => {
                let kind = match raw {
                    RawTokenKind::Quoted => {
                        let (value, string_diagnostics) = parse_quoted_token(text, span);
                        diagnostics.extend(string_diagnostics);
                        tokens.push(Token {
                            kind: TokenKind::String(value),
                            span,
                        });
                        SyntaxKind::String
                    }
                    RawTokenKind::Number => {
                        tokens.push(Token {
                            kind: TokenKind::Number(text.to_string()),
                            span,
                        });
                        SyntaxKind::Number
                    }
                    RawTokenKind::Ident => {
                        tokens.push(Token {
                            kind: TokenKind::Ident(text.to_string()),
                            span,
                        });
                        SyntaxKind::Ident
                    }
                    RawTokenKind::Pipe => {
                        push_simple(&mut tokens, TokenKind::Pipe, span, SyntaxKind::Pipe)
                    }
                    RawTokenKind::Comma => {
                        push_simple(&mut tokens, TokenKind::Comma, span, SyntaxKind::Comma)
                    }
                    RawTokenKind::Equal => {
                        push_simple(&mut tokens, TokenKind::Equal, span, SyntaxKind::Equal)
                    }
                    RawTokenKind::LParen => {
                        push_simple(&mut tokens, TokenKind::LParen, span, SyntaxKind::LParen)
                    }
                    RawTokenKind::RParen => {
                        push_simple(&mut tokens, TokenKind::RParen, span, SyntaxKind::RParen)
                    }
                    RawTokenKind::Plus => {
                        push_simple(&mut tokens, TokenKind::Plus, span, SyntaxKind::Plus)
                    }
                    RawTokenKind::Minus => {
                        push_simple(&mut tokens, TokenKind::Minus, span, SyntaxKind::Minus)
                    }
                    RawTokenKind::Star => {
                        push_simple(&mut tokens, TokenKind::Star, span, SyntaxKind::Star)
                    }
                    RawTokenKind::Slash => {
                        push_simple(&mut tokens, TokenKind::Slash, span, SyntaxKind::Slash)
                    }
                    RawTokenKind::Percent => {
                        push_simple(&mut tokens, TokenKind::Percent, span, SyntaxKind::Percent)
                    }
                    RawTokenKind::EqEq => {
                        push_simple(&mut tokens, TokenKind::EqEq, span, SyntaxKind::EqEq)
                    }
                    RawTokenKind::NotEq => {
                        push_simple(&mut tokens, TokenKind::NotEq, span, SyntaxKind::NotEq)
                    }
                    RawTokenKind::Lt => {
                        push_simple(&mut tokens, TokenKind::Lt, span, SyntaxKind::Lt)
                    }
                    RawTokenKind::Lte => {
                        push_simple(&mut tokens, TokenKind::Lte, span, SyntaxKind::Lte)
                    }
                    RawTokenKind::Gt => {
                        push_simple(&mut tokens, TokenKind::Gt, span, SyntaxKind::Gt)
                    }
                    RawTokenKind::Gte => {
                        push_simple(&mut tokens, TokenKind::Gte, span, SyntaxKind::Gte)
                    }
                    RawTokenKind::Bang => {
                        push_simple(&mut tokens, TokenKind::Bang, span, SyntaxKind::Bang)
                    }
                };
                builder.token(RowanSyntaxKind(kind as u16), text);
            }
            Err(()) => {
                let message = if text.starts_with('"') {
                    "unterminated quoted token".to_string()
                } else {
                    format!("invalid character `{text}`")
                };
                let code = if text.starts_with('"') {
                    "P0002"
                } else {
                    "P0005"
                };
                diagnostics.push(Diagnostic::error(code, message, span));
                builder.token(RowanSyntaxKind(SyntaxKind::Error as u16), text);
            }
        }
    }

    builder.finish_node();
    Lexed {
        tokens,
        diagnostics,
        green: builder.finish(),
    }
}

fn push_simple(
    tokens: &mut Vec<Token>,
    kind: TokenKind,
    span: Span,
    syntax: SyntaxKind,
) -> SyntaxKind {
    tokens.push(Token { kind, span });
    syntax
}

fn tokens_with_eof(mut tokens: Vec<Token>, source_len: usize) -> Vec<Token> {
    tokens.push(Token::new(TokenKind::Eof, source_len, source_len));
    tokens
}

fn strip_block_comments(source: &str) -> (String, Vec<Diagnostic>) {
    let mut stripped = String::with_capacity(source.len());
    let mut diagnostics = Vec::new();
    let mut pos = 0usize;

    while pos < source.len() {
        if source[pos..].starts_with("/*") {
            let start = pos;
            let mut depth = 0usize;
            while pos < source.len() {
                if source[pos..].starts_with("/*") {
                    depth += 1;
                    stripped.push_str("  ");
                    pos += 2;
                } else if source[pos..].starts_with("*/") {
                    depth = depth.saturating_sub(1);
                    stripped.push_str("  ");
                    pos += 2;
                    if depth == 0 {
                        break;
                    }
                } else if let Some(ch) = source[pos..].chars().next() {
                    if ch == '\n' {
                        stripped.push('\n');
                    } else {
                        for _ in 0..ch.len_utf8() {
                            stripped.push(' ');
                        }
                    }
                    pos += ch.len_utf8();
                }
            }

            if depth != 0 {
                diagnostics.push(Diagnostic::error(
                    "P0003",
                    "unterminated block comment",
                    Span::new(start, source.len()),
                ));
            }
        } else if let Some(ch) = source[pos..].chars().next() {
            stripped.push(ch);
            pos += ch.len_utf8();
        }
    }

    (stripped, diagnostics)
}

fn parse_quoted_token(text: &str, span: Span) -> (String, Vec<Diagnostic>) {
    let mut diagnostics = Vec::new();
    if !text.ends_with('"') || text.len() < 2 {
        diagnostics.push(Diagnostic::error(
            "P0002",
            "unterminated quoted token",
            span,
        ));
        return (text.trim_matches('"').to_string(), diagnostics);
    }

    let mut value = String::new();
    let mut chars = text[1..text.len() - 1].char_indices().peekable();
    while let Some((offset, ch)) = chars.next() {
        if ch != '\\' {
            value.push(ch);
            continue;
        }

        let escape_start = span.start + 1 + offset;
        let Some((_, escaped)) = chars.next() else {
            diagnostics.push(Diagnostic::error(
                "P0004",
                "invalid escape sequence",
                Span::new(escape_start, span.end),
            ));
            break;
        };
        match escaped {
            '"' => value.push('"'),
            '\\' => value.push('\\'),
            'n' => value.push('\n'),
            'r' => value.push('\r'),
            't' => value.push('\t'),
            'u' => match parse_unicode_escape(&mut chars) {
                Some(ch) => value.push(ch),
                None => diagnostics.push(Diagnostic::error(
                    "P0004",
                    "invalid unicode escape sequence",
                    Span::new(escape_start, span.end.min(escape_start + 8)),
                )),
            },
            _ => diagnostics.push(Diagnostic::error(
                "P0004",
                format!("invalid escape sequence `\\{escaped}`"),
                Span::new(escape_start, escape_start + 1 + escaped.len_utf8()),
            )),
        }
    }

    (value, diagnostics)
}

fn parse_unicode_escape(
    chars: &mut std::iter::Peekable<std::str::CharIndices<'_>>,
) -> Option<char> {
    if chars.next()?.1 != '{' {
        return None;
    }
    let mut digits = String::new();
    for (_, ch) in chars.by_ref() {
        if ch == '}' {
            let value = u32::from_str_radix(&digits, 16).ok()?;
            return char::from_u32(value);
        }
        if !ch.is_ascii_hexdigit() {
            return None;
        }
        digits.push(ch);
    }
    None
}

impl Token {
    fn new(kind: TokenKind, start: usize, end: usize) -> Self {
        Self {
            kind,
            span: Span::new(start, end),
        }
    }
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    diagnostics: Vec<Diagnostic>,
}

impl Parser {
    fn new(tokens: Vec<Token>, diagnostics: Vec<Diagnostic>) -> Self {
        Self {
            tokens,
            pos: 0,
            diagnostics,
        }
    }

    fn parse_program(&mut self, syntax: SyntaxNode) -> ParseResult {
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
                "P1502",
                "no runnable main pipeline",
                self.current().span,
            ));
            None
        } else {
            self.parse_pipeline()
        };

        ParseResult {
            syntax,
            program: Program { bindings, main },
            diagnostics: std::mem::take(&mut self.diagnostics),
        }
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
                "P0007",
                "expected pipeline start",
                self.current().span,
            ));
            return None;
        };

        let mut stages = Vec::new();
        while self.consume_pipe() {
            if self.at_eof() {
                self.diagnostics.push(Diagnostic::error(
                    "P0006",
                    "missing stage after pipe",
                    self.previous_span(),
                ));
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
                    "P1202",
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
            "group_by" => self.parse_group_by(name.span),
            "agg" => self.parse_agg(name.span),
            "sort" => self.parse_sort(name.span),
            "limit" => self.parse_limit(name.span),
            "save" => self.parse_save(name.span).map(Stage::Save),
            "mutate" | "join" | "union" | "distinct" => {
                let span = self.consume_until_stage_boundary(name.span);
                Some(Stage::Unsupported { name, span })
            }
            _ => {
                self.diagnostics.push(Diagnostic::error(
                    "P1201",
                    format!("unknown stage `{}`", name.value),
                    name.span,
                ));
                let span = self.consume_until_stage_boundary(name.span);
                Some(Stage::Unsupported { name, span })
            }
        }
    }

    fn parse_filter(&mut self, name_span: Span) -> Option<Stage> {
        let expr = self.parse_expr(0)?;
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
                    "P1203",
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
                self.diagnostics.push(Diagnostic::error(
                    "P1213",
                    "aggregate items require `as`",
                    close_span,
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
        loop {
            let column = self.expect_column_name()?;
            let direction = if self.consume_ident("desc") {
                SortDirection::Desc
            } else {
                self.consume_ident("asc");
                SortDirection::Asc
            };
            let nulls = if self.consume_ident("nulls_first") {
                Some(NullsOrder::First)
            } else if self.consume_ident("nulls_last") {
                Some(NullsOrder::Last)
            } else {
                None
            };
            items.push(SortItem {
                column,
                direction,
                nulls,
            });
            if !self.consume_comma() {
                break;
            }
        }
        let end = items
            .last()
            .map_or(name_span.end, |item| item.column.span.end);
        Some(Stage::Sort {
            items,
            span: Span::new(name_span.start, end),
        })
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
                        "P1206",
                        "limit requires a non-negative integer",
                        token.span,
                    ));
                    None
                }
            },
            _ => {
                self.diagnostics.push(Diagnostic::error(
                    "P1203",
                    "limit requires a row count",
                    token.span,
                ));
                None
            }
        }
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

    fn parse_source_ref(&mut self) -> Option<SourceRef> {
        let token = self.advance().clone();
        match token.kind {
            TokenKind::String(value) => Some(SourceRef::Path(Spanned::new(value, token.span))),
            TokenKind::Ident(value) if value == "stdin" => Some(SourceRef::Stdin(token.span)),
            TokenKind::Minus => Some(SourceRef::Stdin(token.span)),
            _ => {
                self.diagnostics.push(Diagnostic::error(
                    "P1203",
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
                    "P1203",
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
                    "P1203",
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
                        "P1206",
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
                    "P0008",
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
                "P1002",
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
                    "P0009",
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
                    "P0008",
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
                    "P0001",
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
                .push(Diagnostic::error("P0001", "expected `=`", token.span));
        }
    }

    fn expect_lparen(&mut self) {
        if !self.consume_lparen() {
            self.diagnostics.push(Diagnostic::error(
                "P0001",
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
                .push(Diagnostic::error("P0001", "expected `)`", token.span));
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

    fn at_expr_boundary(&self) -> bool {
        matches!(
            self.current().kind,
            TokenKind::Comma | TokenKind::Pipe | TokenKind::RParen | TokenKind::Eof
        )
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
        assert_eq!(result.diagnostics[0].code, "P1201");
    }
}
