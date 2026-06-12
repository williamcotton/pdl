pub use crate::parser::{
    AggItem, BinaryOp, Binding, CompleteFillItem, ContextDecl, ContextKind, ControlArg,
    ControlInitializer, ControlKind, ControlLiteral, ControlValue, Expr, JoinKey, JoinKind, JoinOn,
    LoadStage, MutateItem, NullsOrder, OutputDecl, Pipeline, PipelineStart, Program, RenameItem,
    SaveStage, SelectItem, SinkRef, SortDirection, SortItem, SourceRef, Spanned, Stage, UnaryOp,
    UnionOption, UnionOptionKind, WindowFrame, WindowFrameKind, WindowSpec, WINDOW_FRAME_NAMES,
};

use pdl_core::Span;

use crate::cst::SyntaxToken;
use crate::parser::{SyntaxKind, SyntaxNode};

pub trait AstNode: Sized {
    fn can_cast(kind: SyntaxKind) -> bool;
    fn cast(syntax: SyntaxNode) -> Option<Self>;
    fn syntax(&self) -> &SyntaxNode;
}

macro_rules! ast_node {
    ($(#[$meta:meta])* $name:ident = $kind:ident) => {
        $(#[$meta])*
        #[derive(Clone, Debug, Eq, PartialEq)]
        pub struct $name {
            syntax: SyntaxNode,
        }

        impl $name {
            pub fn cast(node: SyntaxNode) -> Option<Self> {
                if node.kind() == SyntaxKind::$kind {
                    Some(Self { syntax: node })
                } else {
                    None
                }
            }

            pub fn syntax(&self) -> &SyntaxNode {
                &self.syntax
            }
        }

        impl AstNode for $name {
            fn can_cast(kind: SyntaxKind) -> bool {
                kind == SyntaxKind::$kind
            }

            fn cast(syntax: SyntaxNode) -> Option<Self> {
                Self::can_cast(syntax.kind()).then(|| Self { syntax })
            }

            fn syntax(&self) -> &SyntaxNode {
                &self.syntax
            }
        }
    };
}

fn child_nodes<T>(node: &SyntaxNode, cast: fn(SyntaxNode) -> Option<T>) -> Vec<T> {
    node.children().filter_map(cast).collect()
}

fn first_token(node: &SyntaxNode, kind: SyntaxKind) -> Option<SyntaxToken> {
    node.children_with_tokens()
        .filter_map(|element| element.into_token())
        .find(|token| token.kind() == kind)
}

fn first_ident(node: &SyntaxNode) -> Option<SyntaxToken> {
    first_token(node, SyntaxKind::Ident)
}

fn token_span(token: SyntaxToken) -> Span {
    let range = token.text_range();
    Span::new(
        u32::from(range.start()) as usize,
        u32::from(range.end()) as usize,
    )
}

ast_node!(
    /// The tree root for a PDL file.
    Root = Root
);

impl Root {
    pub fn context_decls(&self) -> Vec<ContextDeclNode> {
        child_nodes(&self.syntax, ContextDeclNode::cast)
    }

    pub fn binding_decls(&self) -> Vec<BindingDecl> {
        child_nodes(&self.syntax, BindingDecl::cast)
    }

    pub fn output_decls(&self) -> Vec<OutputDeclNode> {
        child_nodes(&self.syntax, OutputDeclNode::cast)
    }

    pub fn main_pipeline(&self) -> Option<PipelineExpr> {
        self.syntax.children().filter_map(PipelineExpr::cast).last()
    }

    pub fn pipelines(&self) -> Vec<PipelineExpr> {
        child_nodes(&self.syntax, PipelineExpr::cast)
    }

    pub fn token_count(&self) -> usize {
        self.syntax.children_with_tokens().count()
    }
}

ast_node!(
    /// A top-level `param` or `state` declaration.
    ContextDeclNode = ContextDecl
);

impl ContextDeclNode {
    pub fn name(&self) -> Option<String> {
        self.name_token().map(|token| token.text().to_string())
    }

    pub fn name_span(&self) -> Option<Span> {
        self.name_token().map(token_span)
    }

    fn name_token(&self) -> Option<SyntaxToken> {
        self.syntax
            .children_with_tokens()
            .filter_map(|element| element.into_token())
            .filter(|token| token.kind() == SyntaxKind::Ident)
            .find(|token| token.text() != "param" && token.text() != "state")
    }
}

ast_node!(
    /// A top-level `let name = pipeline` declaration.
    BindingDecl = BindingDecl
);

impl BindingDecl {
    pub fn name(&self) -> Option<String> {
        self.name_token().map(|token| token.text().to_string())
    }

    pub fn name_span(&self) -> Option<Span> {
        self.name_token().map(token_span)
    }

    pub fn pipeline(&self) -> Option<PipelineExpr> {
        self.syntax.children().find_map(PipelineExpr::cast)
    }

    fn name_token(&self) -> Option<SyntaxToken> {
        self.syntax
            .children_with_tokens()
            .filter_map(|element| element.into_token())
            .filter(|token| token.kind() == SyntaxKind::Ident)
            .find(|token| token.text() != "let")
    }
}

ast_node!(
    /// A top-level `output name = pipeline` declaration.
    OutputDeclNode = OutputDecl
);

impl OutputDeclNode {
    pub fn name(&self) -> Option<String> {
        self.name_token().map(|token| token.text().to_string())
    }

    pub fn name_span(&self) -> Option<Span> {
        self.name_token().map(token_span)
    }

    pub fn pipeline(&self) -> Option<PipelineExpr> {
        self.syntax.children().find_map(PipelineExpr::cast)
    }

    fn name_token(&self) -> Option<SyntaxToken> {
        self.syntax
            .children_with_tokens()
            .filter_map(|element| element.into_token())
            .filter(|token| token.kind() == SyntaxKind::Ident)
            .find(|token| token.text() != "output")
    }
}

ast_node!(
    /// A source-shaped pipeline expression.
    PipelineExpr = PipelineExpr
);

impl PipelineExpr {
    pub fn load_stage(&self) -> Option<LoadStageNode> {
        self.syntax.children().find_map(LoadStageNode::cast)
    }

    pub fn binding_ref(&self) -> Option<BindingRefNode> {
        self.syntax.children().find_map(BindingRefNode::cast)
    }

    pub fn stages(&self) -> Vec<StageNode> {
        child_nodes(&self.syntax, StageNode::cast)
    }

    pub fn save_stages(&self) -> Vec<SaveStageNode> {
        child_nodes(&self.syntax, SaveStageNode::cast)
    }
}

ast_node!(
    /// A `load` pipeline start.
    LoadStageNode = LoadStageNode
);

impl LoadStageNode {
    pub fn source_token(&self) -> Option<SyntaxToken> {
        self.syntax
            .children_with_tokens()
            .filter_map(|element| element.into_token())
            .find(|token| {
                matches!(
                    token.kind(),
                    SyntaxKind::String | SyntaxKind::Ident | SyntaxKind::Minus
                ) && !matches!(token.text(), "load" | "format")
            })
    }

    pub fn source_span(&self) -> Option<Span> {
        self.source_token().map(token_span)
    }
}

ast_node!(
    /// A bare binding reference at pipeline start.
    BindingRefNode = BindingRefNode
);

impl BindingRefNode {
    pub fn name(&self) -> Option<String> {
        first_ident(&self.syntax).map(|token| token.text().to_string())
    }

    pub fn name_span(&self) -> Option<Span> {
        first_ident(&self.syntax).map(token_span)
    }
}

ast_node!(
    /// A transform stage after a pipe.
    StageNode = StageNode
);

impl StageNode {
    pub fn name(&self) -> Option<String> {
        first_ident(&self.syntax).map(|token| token.text().to_string())
    }

    pub fn name_span(&self) -> Option<Span> {
        first_ident(&self.syntax).map(token_span)
    }

    pub fn exprs(&self) -> Vec<ExprNode> {
        child_nodes(&self.syntax, ExprNode::cast)
    }

    pub fn select_items(&self) -> Vec<SelectItemNode> {
        child_nodes(&self.syntax, SelectItemNode::cast)
    }

    pub fn rename_items(&self) -> Vec<RenameItemNode> {
        child_nodes(&self.syntax, RenameItemNode::cast)
    }

    pub fn mutate_items(&self) -> Vec<MutateItemNode> {
        child_nodes(&self.syntax, MutateItemNode::cast)
    }

    pub fn agg_items(&self) -> Vec<AggItemNode> {
        child_nodes(&self.syntax, AggItemNode::cast)
    }

    pub fn sort_items(&self) -> Vec<SortItemNode> {
        child_nodes(&self.syntax, SortItemNode::cast)
    }
}

ast_node!(
    /// A `save` stage after a pipe.
    SaveStageNode = SaveStageNode
);

impl SaveStageNode {
    pub fn sink_token(&self) -> Option<SyntaxToken> {
        self.syntax
            .children_with_tokens()
            .filter_map(|element| element.into_token())
            .find(|token| {
                matches!(
                    token.kind(),
                    SyntaxKind::String | SyntaxKind::Ident | SyntaxKind::Minus
                ) && !matches!(token.text(), "save" | "format")
            })
    }

    pub fn sink_span(&self) -> Option<Span> {
        self.sink_token().map(token_span)
    }
}

ast_node!(
    /// One item inside `select`.
    SelectItemNode = SelectItemNode
);

ast_node!(
    /// One item inside `rename`.
    RenameItemNode = RenameItemNode
);

ast_node!(
    /// One assignment inside `mutate`.
    MutateItemNode = MutateItemNode
);

ast_node!(
    /// One item inside `agg`.
    AggItemNode = AggItemNode
);

impl AggItemNode {
    pub fn function_name(&self) -> Option<String> {
        first_ident(&self.syntax).map(|token| token.text().to_string())
    }

    pub fn args(&self) -> Vec<ExprNode> {
        child_nodes(&self.syntax, ExprNode::cast)
    }
}

ast_node!(
    /// One item inside `sort`.
    SortItemNode = SortItemNode
);

ast_node!(
    /// A value expression node.
    ExprNode = ExprNode
);

impl ExprNode {
    pub fn child_exprs(&self) -> Vec<ExprNode> {
        child_nodes(&self.syntax, ExprNode::cast)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_view_wraps_cst_without_losing_trivia() {
        let parse = crate::parse("load \"sales.csv\" // comment");
        let root = Root::cast(parse.syntax).expect("root cst view");

        assert_eq!(root.syntax().kind(), SyntaxKind::Root);
        assert!(root.token_count() >= 4);
    }

    #[test]
    fn pipeline_views_walk_composite_cst_nodes() {
        let parse = crate::parse(
            r#"let clean =
  load "sales.csv"
  | filter amount > 0

clean
  | select region"#,
        );
        let root = Root::cast(parse.syntax).expect("root cst view");
        let binding = root.binding_decls().into_iter().next().expect("binding");
        let main = root.main_pipeline().expect("main pipeline");

        assert_eq!(binding.name().as_deref(), Some("clean"));
        assert!(binding
            .pipeline()
            .and_then(|pipeline| pipeline.load_stage())
            .is_some());
        assert_eq!(
            main.binding_ref().and_then(|binding| binding.name()),
            Some("clean".to_string())
        );
        assert_eq!(main.stages()[0].name().as_deref(), Some("select"));
    }
}
