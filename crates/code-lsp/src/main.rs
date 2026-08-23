//! `code-lsp` — stdio Language Server Protocol server for `.code` source.
//!
//! Two things, matching what's actually load-bearing for editing this
//! language today: diagnostics from the real lexer/parser (`diagnostics.rs`)
//! and semantic-token-based highlighting (`tokens.rs`), since there is no
//! TextMate grammar for it anywhere and syntax coloring has to come from
//! somewhere. Deliberately not: completion, hover, go-to-definition,
//! references, rename. Those need actual name resolution — a symbol table
//! respecting this language's block scoping (`let` shadowing rules the
//! interpreter enforces at runtime, in `interpreter.rs`) — which is a
//! separate, larger feature, not a natural extension of lexing and parsing.

use std::collections::HashMap;
use std::sync::Mutex;

use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

mod diagnostics;
mod tokens;

/// The document text keyed by URI. Whole-document `TextDocumentSyncKind::FULL`
/// (see `initialize` below) means `did_change` always hands back the
/// complete new text, so there's nothing incremental to reconcile here.
struct Backend {
    client: Client,
    documents: Mutex<HashMap<Url, String>>,
}

impl Backend {
    fn new(client: Client) -> Self {
        Self {
            client,
            documents: Mutex::new(HashMap::new()),
        }
    }

    /// Re-analyze one document and push fresh diagnostics for it — the only
    /// side effect of a document changing (semantic tokens are pulled by
    /// the client on demand instead, via `semantic_tokens_full`).
    async fn on_change(&self, uri: Url, text: String) {
        let diags = match code::lexer::tokenize(&text) {
            Err(err) => vec![diagnostics::from_located(&text, &err)],
            Ok(lexed) => match code::parser::parse(&lexed) {
                Err(err) => vec![diagnostics::from_located(&text, &err)],
                Ok(_program) => Vec::new(),
            },
        };
        self.documents.lock().unwrap().insert(uri.clone(), text);
        self.client.publish_diagnostics(uri, diags, None).await;
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                semantic_tokens_provider: Some(
                    SemanticTokensServerCapabilities::SemanticTokensOptions(
                        SemanticTokensOptions {
                            legend: SemanticTokensLegend {
                                token_types: tokens::LEGEND_TYPES
                                    .iter()
                                    .map(|s| SemanticTokenType::new(s))
                                    .collect(),
                                token_modifiers: vec![],
                            },
                            full: Some(SemanticTokensFullOptions::Bool(true)),
                            range: None,
                            work_done_progress_options: Default::default(),
                        },
                    ),
                ),
                ..Default::default()
            },
            server_info: Some(ServerInfo {
                name: "code-lsp".into(),
                version: Some(env!("CARGO_PKG_VERSION").into()),
            }),
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "code-lsp initialized")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        self.on_change(params.text_document.uri, params.text_document.text)
            .await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        // FULL sync: the client always sends exactly one change event
        // holding the entire new document text.
        if let Some(change) = params.content_changes.into_iter().last() {
            self.on_change(params.text_document.uri, change.text).await;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        self.documents.lock().unwrap().remove(&uri);
        self.client.publish_diagnostics(uri, Vec::new(), None).await;
    }

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        let text = match self
            .documents
            .lock()
            .unwrap()
            .get(&params.text_document.uri)
        {
            Some(t) => t.clone(),
            None => return Ok(None),
        };

        let toks = tokens::semantic_tokens(&text);
        let data: Vec<SemanticToken> = tokens::encode_deltas(&toks)
            .chunks_exact(5)
            .map(|c| SemanticToken {
                delta_line: c[0],
                delta_start: c[1],
                length: c[2],
                token_type: c[3],
                token_modifiers_bitset: c[4],
            })
            .collect();

        Ok(Some(SemanticTokensResult::Tokens(SemanticTokens {
            result_id: None,
            data,
        })))
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(Backend::new);
    Server::new(stdin, stdout, socket).serve(service).await;
}
