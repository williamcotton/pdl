use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::{
    Diagnostic as LspDiagnostic, DiagnosticSeverity, DidOpenTextDocumentParams, InitializeParams,
    InitializeResult, MessageType, NumberOrString, Position, Range, ServerCapabilities,
    TextDocumentSyncCapability, TextDocumentSyncKind,
};
use tower_lsp::{async_trait, Client, LanguageServer, LspService, Server};

pub struct Backend {
    client: Client,
}

impl Backend {
    pub fn new(client: Client) -> Self {
        Self { client }
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
        let source = params.text_document.text;
        let diagnostics = {
            let parse = pdl_syntax::parse(&source);
            pdl_editor_services::diagnostics_for_editor(&source, &parse.diagnostics)
                .into_iter()
                .map(lsp_diagnostic)
                .collect()
        };
        self.client
            .publish_diagnostics(params.text_document.uri, diagnostics, None)
            .await;
    }
}

pub async fn run_stdio() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let (service, socket) = LspService::new(Backend::new);
    Server::new(stdin, stdout, socket).serve(service).await;
}

fn lsp_diagnostic(diagnostic: pdl_editor_services::EditorDiagnostic) -> LspDiagnostic {
    LspDiagnostic {
        range: Range {
            start: Position {
                line: diagnostic.range.start.line,
                character: diagnostic.range.start.character,
            },
            end: Position {
                line: diagnostic.range.end.line,
                character: diagnostic.range.end.character,
            },
        },
        severity: Some(match diagnostic.severity {
            pdl_core::Severity::Error => DiagnosticSeverity::ERROR,
            pdl_core::Severity::Warning => DiagnosticSeverity::WARNING,
            pdl_core::Severity::Info => DiagnosticSeverity::INFORMATION,
        }),
        code: Some(NumberOrString::String(diagnostic.code)),
        source: Some("pdl".to_string()),
        message: diagnostic.message,
        ..LspDiagnostic::default()
    }
}
