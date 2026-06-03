use dashmap::DashMap;
use pdl_editor_services::{
    CompletionKind, DocumentSymbolKind, EditorCompletion, EditorDiagnostic, EditorDocumentSymbol,
    EditorLocation, EditorSemanticToken, EditorTextEdit, SemanticTokenKind, TextPosition,
    TextRange,
};
use std::collections::HashMap;
use std::path::PathBuf;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::{
    CompletionItem, CompletionItemKind, CompletionOptions, CompletionParams, CompletionResponse,
    Diagnostic as LspDiagnostic, DiagnosticSeverity, DidChangeTextDocumentParams,
    DidCloseTextDocumentParams, DidOpenTextDocumentParams, DidSaveTextDocumentParams,
    DocumentFormattingParams, DocumentSymbol, DocumentSymbolParams, DocumentSymbolResponse,
    GotoDefinitionParams, GotoDefinitionResponse, Hover, HoverContents, HoverParams,
    HoverProviderCapability, InitializeParams, InitializeResult, Location, MarkupContent,
    MarkupKind, MessageType, NumberOrString, OneOf, Position, Range, ReferenceParams, RenameParams,
    SemanticToken, SemanticTokenType, SemanticTokens, SemanticTokensFullOptions,
    SemanticTokensLegend, SemanticTokensOptions, SemanticTokensParams, SemanticTokensResult,
    SemanticTokensServerCapabilities, ServerCapabilities, SymbolKind, TextDocumentSyncCapability,
    TextDocumentSyncKind, TextEdit, Url, WorkspaceEdit,
};
use tower_lsp::{async_trait, Client, LanguageServer, LspService, Server};

#[derive(Clone, Debug)]
struct DocumentState {
    source: String,
    path: Option<PathBuf>,
}

pub struct Backend {
    client: Client,
    documents: DashMap<Url, DocumentState>,
}

impl Backend {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            documents: DashMap::new(),
        }
    }

    async fn update_document(&self, uri: Url, source: String) {
        let path = uri.to_file_path().ok();
        self.documents.insert(
            uri.clone(),
            DocumentState {
                source: source.clone(),
                path: path.clone(),
            },
        );
        self.publish_document_diagnostics(uri, &source, path.as_deref())
            .await;
    }

    async fn publish_document_diagnostics(
        &self,
        uri: Url,
        source: &str,
        path: Option<&std::path::Path>,
    ) {
        let document = pdl_editor_services::analyze_document(source, path);
        let diagnostics = document
            .diagnostics
            .into_iter()
            .map(lsp_diagnostic)
            .collect();
        self.client
            .publish_diagnostics(uri, diagnostics, None)
            .await;
    }

    fn document_state(&self, uri: &Url) -> Option<DocumentState> {
        self.documents.get(uri).map(|entry| entry.clone())
    }
}

#[async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, _params: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec![
                        "|".to_string(),
                        "\"".to_string(),
                        " ".to_string(),
                    ]),
                    ..CompletionOptions::default()
                }),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                document_formatting_provider: Some(OneOf::Left(true)),
                semantic_tokens_provider: Some(
                    SemanticTokensServerCapabilities::SemanticTokensOptions(
                        SemanticTokensOptions {
                            legend: semantic_tokens_legend(),
                            range: Some(false),
                            full: Some(SemanticTokensFullOptions::Bool(true)),
                            ..SemanticTokensOptions::default()
                        },
                    ),
                ),
                document_symbol_provider: Some(OneOf::Left(true)),
                definition_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                rename_provider: Some(OneOf::Left(true)),
                ..ServerCapabilities::default()
            },
            server_info: Some(tower_lsp::lsp_types::ServerInfo {
                name: "pdl-lsp".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
        })
    }

    async fn initialized(&self, _params: tower_lsp::lsp_types::InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "PDL language server initialized")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        self.update_document(params.text_document.uri, params.text_document.text)
            .await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let Some(change) = params.content_changes.into_iter().last() else {
            return;
        };
        self.update_document(params.text_document.uri, change.text)
            .await;
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        if let Some(text) = params.text {
            self.update_document(params.text_document.uri, text).await;
            return;
        }
        if let Some(state) = self.document_state(&params.text_document.uri) {
            self.publish_document_diagnostics(
                params.text_document.uri,
                &state.source,
                state.path.as_deref(),
            )
            .await;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        self.documents.remove(&params.text_document.uri);
        self.client
            .publish_diagnostics(params.text_document.uri, Vec::new(), None)
            .await;
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let text_position = params.text_document_position;
        let Some(state) = self.document_state(&text_position.text_document.uri) else {
            return Ok(None);
        };
        let items = pdl_editor_services::completions(
            &state.source,
            state.path.as_deref(),
            from_lsp_position(text_position.position),
        )
        .into_iter()
        .map(lsp_completion)
        .collect();
        Ok(Some(CompletionResponse::Array(items)))
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let text_position = params.text_document_position_params;
        let Some(state) = self.document_state(&text_position.text_document.uri) else {
            return Ok(None);
        };
        Ok(pdl_editor_services::hover(
            &state.source,
            state.path.as_deref(),
            from_lsp_position(text_position.position),
        )
        .map(|hover| Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: hover.markdown,
            }),
            range: Some(lsp_range(hover.range)),
        }))
    }

    async fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        let Some(state) = self.document_state(&params.text_document.uri) else {
            return Ok(None);
        };
        Ok(pdl_editor_services::formatting_edit(&state.source)
            .map(|edit| vec![lsp_text_edit(edit)]))
    }

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        let Some(state) = self.document_state(&params.text_document.uri) else {
            return Ok(None);
        };
        let data = encode_semantic_tokens(pdl_editor_services::semantic_tokens(&state.source));
        Ok(Some(SemanticTokensResult::Tokens(SemanticTokens {
            result_id: None,
            data,
        })))
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let Some(state) = self.document_state(&params.text_document.uri) else {
            return Ok(None);
        };
        let symbols = pdl_editor_services::document_symbols(&state.source)
            .into_iter()
            .map(lsp_document_symbol)
            .collect();
        Ok(Some(DocumentSymbolResponse::Nested(symbols)))
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let text_position = params.text_document_position_params;
        let uri = text_position.text_document.uri;
        let Some(state) = self.document_state(&uri) else {
            return Ok(None);
        };
        Ok(pdl_editor_services::binding_definition(
            &state.source,
            from_lsp_position(text_position.position),
        )
        .map(|location| {
            GotoDefinitionResponse::Scalar(Location {
                uri,
                range: lsp_location_range(location),
            })
        }))
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let text_position = params.text_document_position;
        let uri = text_position.text_document.uri;
        let Some(state) = self.document_state(&uri) else {
            return Ok(None);
        };
        let locations = pdl_editor_services::binding_references(
            &state.source,
            from_lsp_position(text_position.position),
        )
        .into_iter()
        .map(|location| Location {
            uri: uri.clone(),
            range: lsp_location_range(location),
        })
        .collect();
        Ok(Some(locations))
    }

    async fn rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
        let text_position = params.text_document_position;
        let uri = text_position.text_document.uri;
        let Some(state) = self.document_state(&uri) else {
            return Ok(None);
        };
        let edits = pdl_editor_services::rename_binding_edits(
            &state.source,
            from_lsp_position(text_position.position),
            &params.new_name,
        );
        if edits.is_empty() {
            return Ok(None);
        }

        let mut changes = HashMap::new();
        changes.insert(uri, edits.into_iter().map(lsp_text_edit).collect());
        Ok(Some(WorkspaceEdit {
            changes: Some(changes),
            ..WorkspaceEdit::default()
        }))
    }
}

pub async fn run_stdio() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let (service, socket) = LspService::new(Backend::new);
    Server::new(stdin, stdout, socket).serve(service).await;
}

fn lsp_diagnostic(diagnostic: EditorDiagnostic) -> LspDiagnostic {
    LspDiagnostic {
        range: lsp_range(diagnostic.range),
        severity: Some(match diagnostic.severity {
            pdl_core::Severity::Error => DiagnosticSeverity::ERROR,
            pdl_core::Severity::Warning => DiagnosticSeverity::WARNING,
            pdl_core::Severity::Info => DiagnosticSeverity::INFORMATION,
            pdl_core::Severity::Hint => DiagnosticSeverity::HINT,
        }),
        code: Some(NumberOrString::String(diagnostic.code)),
        source: Some("pdl".to_string()),
        message: diagnostic.message,
        ..LspDiagnostic::default()
    }
}

fn lsp_completion(item: EditorCompletion) -> CompletionItem {
    CompletionItem {
        label: item.label,
        kind: Some(match item.kind {
            CompletionKind::Binding => CompletionItemKind::VARIABLE,
            CompletionKind::Column => CompletionItemKind::FIELD,
            CompletionKind::Format => CompletionItemKind::ENUM_MEMBER,
            CompletionKind::Function => CompletionItemKind::FUNCTION,
            CompletionKind::Keyword => CompletionItemKind::KEYWORD,
            CompletionKind::Stage => CompletionItemKind::KEYWORD,
        }),
        detail: Some(item.detail),
        insert_text: Some(item.insert_text),
        ..CompletionItem::default()
    }
}

fn lsp_text_edit(edit: EditorTextEdit) -> TextEdit {
    TextEdit {
        range: lsp_range(edit.range),
        new_text: edit.new_text,
    }
}

#[allow(deprecated)]
fn lsp_document_symbol(symbol: EditorDocumentSymbol) -> DocumentSymbol {
    DocumentSymbol {
        name: symbol.name,
        detail: Some(symbol.detail),
        kind: match symbol.kind {
            DocumentSymbolKind::Binding => SymbolKind::VARIABLE,
            DocumentSymbolKind::Function => SymbolKind::FUNCTION,
            DocumentSymbolKind::Stage => SymbolKind::METHOD,
        },
        tags: None,
        deprecated: None,
        range: lsp_range(symbol.range),
        selection_range: lsp_range(symbol.selection_range),
        children: Some(
            symbol
                .children
                .into_iter()
                .map(lsp_document_symbol)
                .collect(),
        ),
    }
}

fn lsp_location_range(location: EditorLocation) -> Range {
    lsp_range(location.range)
}

fn lsp_range(range: TextRange) -> Range {
    Range {
        start: lsp_position(range.start),
        end: lsp_position(range.end),
    }
}

fn lsp_position(position: TextPosition) -> Position {
    Position {
        line: position.line,
        character: position.character,
    }
}

fn from_lsp_position(position: Position) -> TextPosition {
    TextPosition {
        line: position.line,
        character: position.character,
    }
}

fn semantic_tokens_legend() -> SemanticTokensLegend {
    SemanticTokensLegend {
        token_types: vec![
            SemanticTokenType::KEYWORD,
            SemanticTokenType::FUNCTION,
            SemanticTokenType::VARIABLE,
            SemanticTokenType::STRING,
            SemanticTokenType::NUMBER,
            SemanticTokenType::OPERATOR,
        ],
        token_modifiers: Vec::new(),
    }
}

fn encode_semantic_tokens(tokens: Vec<EditorSemanticToken>) -> Vec<SemanticToken> {
    let mut data = Vec::new();
    let mut previous_line = 0u32;
    let mut previous_start = 0u32;

    for token in tokens {
        if token.range.start.line != token.range.end.line {
            continue;
        }
        let delta_line = token.range.start.line.saturating_sub(previous_line);
        let delta_start = if delta_line == 0 {
            token.range.start.character.saturating_sub(previous_start)
        } else {
            token.range.start.character
        };
        let length = token
            .range
            .end
            .character
            .saturating_sub(token.range.start.character);
        if length == 0 {
            continue;
        }

        data.push(SemanticToken {
            delta_line,
            delta_start,
            length,
            token_type: semantic_token_index(token.token_type),
            token_modifiers_bitset: 0,
        });
        previous_line = token.range.start.line;
        previous_start = token.range.start.character;
    }

    data
}

fn semantic_token_index(kind: SemanticTokenKind) -> u32 {
    match kind {
        SemanticTokenKind::Keyword => 0,
        SemanticTokenKind::Function => 1,
        SemanticTokenKind::Variable => 2,
        SemanticTokenKind::String => 3,
        SemanticTokenKind::Number => 4,
        SemanticTokenKind::Operator => 5,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lsp_diagnostic_preserves_code_severity_and_range() {
        let diagnostic = EditorDiagnostic {
            range: TextRange {
                start: TextPosition {
                    line: 1,
                    character: 2,
                },
                end: TextPosition {
                    line: 1,
                    character: 7,
                },
            },
            severity: pdl_core::Severity::Hint,
            code: "H3001".to_string(),
            message: "use col(...)".to_string(),
        };

        let lsp = lsp_diagnostic(diagnostic);

        assert_eq!(lsp.severity, Some(DiagnosticSeverity::HINT));
        assert_eq!(lsp.code, Some(NumberOrString::String("H3001".to_string())));
        assert_eq!(lsp.source.as_deref(), Some("pdl"));
        assert_eq!(lsp.range.start.line, 1);
        assert_eq!(lsp.range.start.character, 2);
        assert_eq!(lsp.range.end.character, 7);
    }
}
