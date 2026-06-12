use pdl_core::{codes, Diagnostic, Span};
use rowan::{GreenNode, GreenNodeBuilder, SyntaxKind as RowanSyntaxKind};

use crate::parser::SyntaxKind;

#[derive(Clone, Debug, PartialEq)]
pub enum TokenKind {
    Whitespace,
    LineComment,
    BlockComment,
    Ident(String),
    String(String),
    BacktickColumn(String),
    Number(String),
    Pipe,
    Comma,
    Colon,
    Dot,
    Equal,
    LBracket,
    RBracket,
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
    Error,
    Eof,
}

impl TokenKind {
    pub(crate) fn syntax_kind(&self) -> SyntaxKind {
        match self {
            TokenKind::Whitespace => SyntaxKind::Whitespace,
            TokenKind::LineComment => SyntaxKind::LineComment,
            TokenKind::BlockComment => SyntaxKind::BlockComment,
            TokenKind::Ident(_) => SyntaxKind::Ident,
            TokenKind::String(_) => SyntaxKind::String,
            TokenKind::BacktickColumn(_) => SyntaxKind::BacktickColumn,
            TokenKind::Number(_) => SyntaxKind::Number,
            TokenKind::Pipe => SyntaxKind::Pipe,
            TokenKind::Comma => SyntaxKind::Comma,
            TokenKind::Colon => SyntaxKind::Colon,
            TokenKind::Dot => SyntaxKind::Dot,
            TokenKind::Equal => SyntaxKind::Equal,
            TokenKind::LBracket => SyntaxKind::LBracket,
            TokenKind::RBracket => SyntaxKind::RBracket,
            TokenKind::LParen => SyntaxKind::LParen,
            TokenKind::RParen => SyntaxKind::RParen,
            TokenKind::Plus => SyntaxKind::Plus,
            TokenKind::Minus => SyntaxKind::Minus,
            TokenKind::Star => SyntaxKind::Star,
            TokenKind::Slash => SyntaxKind::Slash,
            TokenKind::Percent => SyntaxKind::Percent,
            TokenKind::Dollar => SyntaxKind::Dollar,
            TokenKind::At => SyntaxKind::At,
            TokenKind::EqEq => SyntaxKind::EqEq,
            TokenKind::NotEq => SyntaxKind::NotEq,
            TokenKind::Lt => SyntaxKind::Lt,
            TokenKind::Lte => SyntaxKind::Lte,
            TokenKind::Gt => SyntaxKind::Gt,
            TokenKind::Gte => SyntaxKind::Gte,
            TokenKind::Bang => SyntaxKind::Bang,
            TokenKind::Error => SyntaxKind::Error,
            TokenKind::Eof => SyntaxKind::Eof,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LexResult {
    pub tokens: Vec<Token>,
    pub(crate) parse_tokens: Vec<Token>,
    pub diagnostics: Vec<Diagnostic>,
    pub green: GreenNode,
}

pub fn lex_source(source: &str) -> LexResult {
    lex(source)
}

fn lex(source: &str) -> LexResult {
    let mut diagnostics = Vec::new();
    let mut tokens = Vec::new();
    let mut parse_tokens = Vec::new();
    let mut builder = GreenNodeBuilder::new();
    builder.start_node(RowanSyntaxKind(SyntaxKind::Root as u16));

    let mut pos = 0usize;
    while pos < source.len() {
        let rest = &source[pos..];
        let Some(ch) = rest.chars().next() else {
            break;
        };

        if ch.is_whitespace() {
            let end = scan_while(source, pos, char::is_whitespace);
            push_token(
                &mut tokens,
                &mut parse_tokens,
                &mut builder,
                TokenKind::Whitespace,
                Span::new(pos, end),
                &source[pos..end],
                false,
            );
            pos = end;
        } else if rest.starts_with("//") {
            let end = rest.find('\n').map_or(source.len(), |offset| pos + offset);
            push_token(
                &mut tokens,
                &mut parse_tokens,
                &mut builder,
                TokenKind::LineComment,
                Span::new(pos, end),
                &source[pos..end],
                false,
            );
            pos = end;
        } else if rest.starts_with("/*") {
            let (end, terminated) = scan_block_comment(source, pos);
            if !terminated {
                diagnostics.push(Diagnostic::error(
                    codes::E0003,
                    "unterminated block comment",
                    Span::new(pos, end),
                ));
            }
            push_token(
                &mut tokens,
                &mut parse_tokens,
                &mut builder,
                TokenKind::BlockComment,
                Span::new(pos, end),
                &source[pos..end],
                false,
            );
            pos = end;
        } else if ch == '"' {
            let (end, _terminated) = scan_quoted(source, pos);
            let span = Span::new(pos, end);
            let text = &source[pos..end];
            let (value, string_diagnostics) = parse_quoted_token(text, span);
            diagnostics.extend(string_diagnostics);
            push_token(
                &mut tokens,
                &mut parse_tokens,
                &mut builder,
                TokenKind::String(value),
                span,
                text,
                true,
            );
            pos = end;
        } else if ch == '`' {
            let (end, _terminated) = scan_backtick_column(source, pos);
            let span = Span::new(pos, end);
            let text = &source[pos..end];
            let (value, column_diagnostics) = parse_backtick_column(text, span);
            diagnostics.extend(column_diagnostics);
            push_token(
                &mut tokens,
                &mut parse_tokens,
                &mut builder,
                TokenKind::BacktickColumn(value),
                span,
                text,
                true,
            );
            pos = end;
        } else if ch.is_ascii_digit() {
            let end = scan_number(source, pos);
            push_token(
                &mut tokens,
                &mut parse_tokens,
                &mut builder,
                TokenKind::Number(source[pos..end].to_string()),
                Span::new(pos, end),
                &source[pos..end],
                true,
            );
            pos = end;
        } else if is_ident_start(ch) {
            let end = scan_identifier(source, pos);
            push_token(
                &mut tokens,
                &mut parse_tokens,
                &mut builder,
                TokenKind::Ident(source[pos..end].to_string()),
                Span::new(pos, end),
                &source[pos..end],
                true,
            );
            pos = end;
        } else if let Some((kind, end)) = scan_operator(source, pos) {
            push_token(
                &mut tokens,
                &mut parse_tokens,
                &mut builder,
                kind,
                Span::new(pos, end),
                &source[pos..end],
                true,
            );
            pos = end;
        } else {
            let end = pos + ch.len_utf8();
            let text = &source[pos..end];
            diagnostics.push(Diagnostic::error(
                codes::E0005,
                format!("invalid character `{text}`"),
                Span::new(pos, end),
            ));
            push_token(
                &mut tokens,
                &mut parse_tokens,
                &mut builder,
                TokenKind::Error,
                Span::new(pos, end),
                text,
                false,
            );
            pos = end;
        }
    }

    builder.finish_node();
    let eof = Token::new(TokenKind::Eof, source.len(), source.len(), "");
    tokens.push(eof.clone());
    parse_tokens.push(eof);
    LexResult {
        tokens,
        parse_tokens,
        diagnostics,
        green: builder.finish(),
    }
}

fn push_token(
    tokens: &mut Vec<Token>,
    parse_tokens: &mut Vec<Token>,
    builder: &mut GreenNodeBuilder<'_>,
    kind: TokenKind,
    span: Span,
    text: &str,
    parse_significant: bool,
) {
    let syntax = kind.syntax_kind();
    let token = Token::new(kind, span.start, span.end, text);
    if parse_significant {
        parse_tokens.push(token.clone());
    }
    tokens.push(token);
    builder.token(RowanSyntaxKind(syntax as u16), text);
}

fn scan_while(source: &str, start: usize, predicate: impl Fn(char) -> bool) -> usize {
    let mut pos = start;
    while pos < source.len() {
        let Some(ch) = source[pos..].chars().next() else {
            break;
        };
        if !predicate(ch) {
            break;
        }
        pos += ch.len_utf8();
    }
    pos
}

fn scan_block_comment(source: &str, start: usize) -> (usize, bool) {
    let mut pos = start;
    let mut depth = 0usize;
    while pos < source.len() {
        if source[pos..].starts_with("/*") {
            depth += 1;
            pos += 2;
        } else if source[pos..].starts_with("*/") {
            depth = depth.saturating_sub(1);
            pos += 2;
            if depth == 0 {
                return (pos, true);
            }
        } else if let Some(ch) = source[pos..].chars().next() {
            pos += ch.len_utf8();
        } else {
            break;
        }
    }
    (source.len(), false)
}

fn scan_quoted(source: &str, start: usize) -> (usize, bool) {
    let mut pos = start + 1;
    let mut escaped = false;
    while pos < source.len() {
        let Some(ch) = source[pos..].chars().next() else {
            break;
        };
        pos += ch.len_utf8();
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            return (pos, true);
        }
    }
    (source.len(), false)
}

fn scan_backtick_column(source: &str, start: usize) -> (usize, bool) {
    let mut pos = start + 1;
    let mut escaped = false;
    while pos < source.len() {
        let Some(ch) = source[pos..].chars().next() else {
            break;
        };
        pos += ch.len_utf8();
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '`' {
            return (pos, true);
        }
    }
    (source.len(), false)
}

fn scan_number(source: &str, start: usize) -> usize {
    let mut pos = scan_ascii_digits(source, start);
    if source[pos..].starts_with('.') {
        let after_dot = pos + 1;
        let after_fraction = scan_ascii_digits(source, after_dot);
        if after_fraction > after_dot {
            pos = after_fraction;
        }
    }
    if source[pos..].starts_with('e') || source[pos..].starts_with('E') {
        let mut exponent = pos + 1;
        if source[exponent..].starts_with('+') || source[exponent..].starts_with('-') {
            exponent += 1;
        }
        let after_exponent = scan_ascii_digits(source, exponent);
        if after_exponent > exponent {
            pos = after_exponent;
        }
    }
    pos
}

fn scan_ascii_digits(source: &str, start: usize) -> usize {
    scan_while(source, start, |ch| ch.is_ascii_digit())
}

fn scan_identifier(source: &str, start: usize) -> usize {
    scan_while(source, start, is_ident_char)
}

fn scan_operator(source: &str, start: usize) -> Option<(TokenKind, usize)> {
    let rest = &source[start..];
    for (text, kind) in [
        ("==", TokenKind::EqEq),
        ("!=", TokenKind::NotEq),
        ("<=", TokenKind::Lte),
        (">=", TokenKind::Gte),
    ] {
        if rest.starts_with(text) {
            return Some((kind, start + text.len()));
        }
    }

    let ch = rest.chars().next()?;
    let end = start + ch.len_utf8();
    let token = match ch {
        '|' => (TokenKind::Pipe, end),
        ',' => (TokenKind::Comma, end),
        ':' => (TokenKind::Colon, end),
        '.' => (TokenKind::Dot, end),
        '=' => (TokenKind::Equal, end),
        '[' => (TokenKind::LBracket, end),
        ']' => (TokenKind::RBracket, end),
        '(' => (TokenKind::LParen, end),
        ')' => (TokenKind::RParen, end),
        '+' => (TokenKind::Plus, end),
        '-' => (TokenKind::Minus, end),
        '*' => (TokenKind::Star, end),
        '/' => (TokenKind::Slash, end),
        '%' => (TokenKind::Percent, end),
        '$' => (TokenKind::Dollar, end),
        '@' => (TokenKind::At, end),
        '<' => (TokenKind::Lt, end),
        '>' => (TokenKind::Gt, end),
        '!' => (TokenKind::Bang, end),
        _ => return None,
    };
    Some(token)
}

fn is_ident_start(ch: char) -> bool {
    ch.is_ascii_alphabetic() || ch == '_'
}

fn is_ident_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

fn parse_quoted_token(text: &str, span: Span) -> (String, Vec<Diagnostic>) {
    let mut diagnostics = Vec::new();
    if !text.ends_with('"') || text.len() < 2 {
        diagnostics.push(Diagnostic::error(
            codes::E0002,
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
                codes::E0004,
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
                    codes::E0004,
                    "invalid unicode escape sequence",
                    Span::new(escape_start, span.end.min(escape_start + 8)),
                )),
            },
            _ => diagnostics.push(Diagnostic::error(
                codes::E0004,
                format!("invalid escape sequence `\\{escaped}`"),
                Span::new(escape_start, escape_start + 1 + escaped.len_utf8()),
            )),
        }
    }

    (value, diagnostics)
}

fn parse_backtick_column(text: &str, span: Span) -> (String, Vec<Diagnostic>) {
    let mut diagnostics = Vec::new();
    if !text.ends_with('`') || text.len() < 2 {
        diagnostics.push(Diagnostic::error(
            codes::E0002,
            "unterminated backtick column reference",
            span,
        ));
        return (text.trim_matches('`').to_string(), diagnostics);
    }

    let mut value = String::new();
    let mut chars = text[1..text.len() - 1].char_indices();
    while let Some((offset, ch)) = chars.next() {
        if ch != '\\' {
            value.push(ch);
            continue;
        }

        let escape_start = span.start + 1 + offset;
        let Some((_, escaped)) = chars.next() else {
            diagnostics.push(Diagnostic::error(
                codes::E0004,
                "invalid escape sequence",
                Span::new(escape_start, span.end),
            ));
            break;
        };
        match escaped {
            '`' => value.push('`'),
            '\\' => value.push('\\'),
            _ => diagnostics.push(Diagnostic::error(
                codes::E0004,
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
    fn new(kind: TokenKind, start: usize, end: usize, text: impl Into<String>) -> Self {
        Self {
            kind,
            span: Span::new(start, end),
            text: text.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexer_preserves_trivia_and_eof() {
        let result = lex_source("load /* nested /* ok */ done */ \"sales.csv\" // tail");
        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        assert!(matches!(result.tokens[1].kind, TokenKind::Whitespace));
        assert!(result.tokens.iter().any(|token| {
            matches!(token.kind, TokenKind::BlockComment) && token.text.contains("nested")
        }));
        assert!(matches!(
            result.tokens.last().map(|token| &token.kind),
            Some(TokenKind::Eof)
        ));
        assert!(result.parse_tokens.iter().all(|token| !matches!(
            token.kind,
            TokenKind::Whitespace | TokenKind::LineComment | TokenKind::BlockComment
        )));
    }

    #[test]
    fn lexer_uses_byte_spans_for_non_ascii_text() {
        let result = lex_source("load \"é.csv\"");
        let path = result
            .tokens
            .iter()
            .find(|token| matches!(token.kind, TokenKind::String(_)))
            .expect("string token");

        assert_eq!(path.span.start, 5);
        assert_eq!(path.span.end, 13);
    }
}
