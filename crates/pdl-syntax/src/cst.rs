use pdl_core::Span;
use rowan::{
    GreenNode, GreenNodeBuilder, SyntaxKind as RowanSyntaxKind, SyntaxToken as RowanSyntaxToken,
};

use crate::lexer::{Token, TokenKind};
use crate::parser::{
    AggItem, Binding, Expr, PdlLanguage, Pipeline, PipelineStart, Program, SinkRef, SourceRef,
    Stage, SyntaxKind,
};

pub type SyntaxToken = RowanSyntaxToken<PdlLanguage>;

pub(crate) fn build_cst(tokens: &[Token], program: &Program, source_len: usize) -> GreenNode {
    let mut builder = CstBuilder {
        tokens,
        index: 0,
        builder: GreenNodeBuilder::new(),
    };
    builder.start_node(SyntaxKind::Root);
    for binding in &program.bindings {
        builder.binding(binding);
    }
    if let Some(main) = &program.main {
        builder.pipeline(main);
    }
    builder.emit_until(source_len, true);
    builder.finish_node();
    builder.builder.finish()
}

struct CstBuilder<'a> {
    tokens: &'a [Token],
    index: usize,
    builder: GreenNodeBuilder<'static>,
}

impl CstBuilder<'_> {
    fn start_node(&mut self, kind: SyntaxKind) {
        self.builder.start_node(RowanSyntaxKind(kind as u16));
    }

    fn finish_node(&mut self) {
        self.builder.finish_node();
    }

    fn token(&mut self, token: &Token) {
        self.builder.token(
            RowanSyntaxKind(token.kind.syntax_kind() as u16),
            &token.text,
        );
    }

    fn emit_until(&mut self, end: usize, include_eof: bool) {
        while let Some(token) = self.tokens.get(self.index) {
            if matches!(token.kind, TokenKind::Eof) {
                if include_eof {
                    self.token(token);
                    self.index += 1;
                }
                break;
            }
            if token.span.start >= end {
                break;
            }
            self.token(token);
            self.index += 1;
        }
    }

    fn node(&mut self, kind: SyntaxKind, span: Span, children: impl FnOnce(&mut Self)) {
        self.emit_until(span.start, false);
        self.start_node(kind);
        children(self);
        self.emit_until(span.end, false);
        self.finish_node();
    }

    fn binding(&mut self, binding: &Binding) {
        let span = Span::new(
            self.find_keyword_before("let", binding.name.span)
                .map_or(binding.name.span.start, |span| span.start),
            binding.pipeline.span.end,
        );
        self.node(SyntaxKind::BindingDecl, span, |builder| {
            builder.pipeline(&binding.pipeline);
        });
    }

    fn pipeline(&mut self, pipeline: &Pipeline) {
        self.node(SyntaxKind::PipelineExpr, pipeline.span, |builder| {
            match &pipeline.start {
                PipelineStart::Load(load) => {
                    builder.node(SyntaxKind::LoadStageNode, load.span, |_| {});
                }
                PipelineStart::Binding(name) => {
                    builder.node(SyntaxKind::BindingRefNode, name.span, |_| {});
                }
            }
            for stage in &pipeline.stages {
                builder.stage(stage);
            }
        });
    }

    fn stage(&mut self, stage: &Stage) {
        match stage {
            Stage::Save(save) => self.node(SyntaxKind::SaveStageNode, save.span, |_| {}),
            _ => self.node(SyntaxKind::StageNode, stage.span(), |builder| match stage {
                Stage::Filter { expr, .. } => builder.expr(expr),
                Stage::Select { items, .. } => {
                    for item in items {
                        let end = item
                            .alias
                            .as_ref()
                            .map_or(item.column.span.end, |alias| alias.span.end);
                        builder.node(
                            SyntaxKind::SelectItemNode,
                            Span::new(item.column.span.start, end),
                            |_| {},
                        );
                    }
                }
                Stage::Rename { items, .. } => {
                    for item in items {
                        builder.node(
                            SyntaxKind::RenameItemNode,
                            Span::new(item.old.span.start, item.new.span.end),
                            |_| {},
                        );
                    }
                }
                Stage::Agg { items, .. } => {
                    for item in items {
                        builder.agg_item(item);
                    }
                }
                Stage::Sort { items, .. } => {
                    for item in items {
                        builder.node(SyntaxKind::SortItemNode, item.column.span, |_| {});
                    }
                }
                Stage::Drop { .. }
                | Stage::GroupBy { .. }
                | Stage::Limit { .. }
                | Stage::Save(_)
                | Stage::Unsupported { .. } => {}
            }),
        }
    }

    fn agg_item(&mut self, item: &AggItem) {
        self.node(SyntaxKind::AggItemNode, item.span, |builder| {
            for arg in &item.args {
                builder.expr(arg);
            }
        });
    }

    fn expr(&mut self, expr: &Expr) {
        self.node(SyntaxKind::ExprNode, expr.span(), |builder| match expr {
            Expr::Call { args, .. } => {
                for arg in args {
                    builder.expr(arg);
                }
            }
            Expr::Unary { expr, .. } => builder.expr(expr),
            Expr::Binary { left, right, .. } => {
                builder.expr(left);
                builder.expr(right);
            }
            Expr::Quoted(_) | Expr::Number(_) | Expr::Bool(_) | Expr::Null(_) | Expr::Ident(_) => {}
        });
    }

    fn find_keyword_before(&self, keyword: &str, before: Span) -> Option<Span> {
        self.tokens
            .iter()
            .rev()
            .filter(|token| token.span.end <= before.start)
            .find_map(|token| match &token.kind {
                TokenKind::Ident(value) if value == keyword => Some(token.span),
                _ => None,
            })
    }
}

pub fn source_span(source: &SourceRef) -> Span {
    match source {
        SourceRef::Path(value) => value.span,
        SourceRef::Stdin(span) => *span,
    }
}

pub fn sink_span(sink: &SinkRef) -> Span {
    match sink {
        SinkRef::Path(value) => value.span,
        SinkRef::Stdout(span) => *span,
    }
}
