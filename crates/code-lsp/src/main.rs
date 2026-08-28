//! `code-lsp` — stdio Language Server Protocol server for `.code` source.
//!
//! Three things, matching what's actually load-bearing for editing this
//! language today: diagnostics from the real lexer/parser (`diagnostics.rs`),
//! semantic-token-based highlighting (`tokens.rs`), since there is no
//! TextMate grammar for it anywhere and syntax coloring has to come from
//! somewhere, and whole-document formatting, which is `code format` reached
//! from an editor rather than the command line. Deliberately not: completion,
//! hover, go-to-definition,
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
                document_formatting_provider: Some(OneOf::Left(true)),
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

    /// `textDocument/formatting` — the whole document, as one edit.
    ///
    /// One replacement rather than a computed minimal diff: `code format`
    /// produces a whole file, and reconstructing per-line edits from it would
    /// mean a diff algorithm whose only purpose is to make the wire format
    /// prettier. Editors apply a full-range edit fine, and
    /// `TextDocumentSyncKind::FULL` above means this server is already
    /// whole-document everywhere else.
    ///
    /// A file that does not parse is left alone — `Ok(None)`, not an error.
    /// Formatting is usually reached by format-on-save, and the moment a file
    /// is most likely to be unparseable is mid-edit; reindenting a file with
    /// an unbalanced brace would scramble everything after it, and popping an
    /// error dialog on every save while someone is still typing is worse than
    /// doing nothing. The diagnostic for the parse error is already showing.
    async fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        let text = match self
            .documents
            .lock()
            .unwrap()
            .get(&params.text_document.uri)
        {
            Some(t) => t.clone(),
            None => return Ok(None),
        };
        let Ok(formatted) = code::format::format(&text) else {
            return Ok(None);
        };
        if formatted == text {
            return Ok(None);
        }
        Ok(Some(vec![TextEdit {
            range: whole_document(&text),
            new_text: formatted,
        }]))
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
        // `as_chunks::<5>` rather than `chunks_exact(5)`: the width is a
        // constant, so this yields `&[u32; 5]` and the five fields can be
        // named by destructuring instead of indexed positionally.
        // `encode_deltas` emits whole 5-tuples, so the remainder (`.1`) is
        // always empty.
        let data: Vec<SemanticToken> = tokens::encode_deltas(&toks)
            .as_chunks::<5>()
            .0
            .iter()
            .map(
                |&[delta_line, delta_start, length, token_type, token_modifiers_bitset]| {
                    SemanticToken {
                        delta_line,
                        delta_start,
                        length,
                        token_type,
                        token_modifiers_bitset,
                    }
                },
            )
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

/// The range covering all of `text`, for a whole-document replacement.
///
/// LSP positions are UTF-16 code units, not chars or bytes — the one place
/// that distinction bites here, since a `.code` file may contain `≠`, `≤`,
/// `≥` (the language's own operators) and any string literal at all. The end
/// position is one past the last line's final unit; a text ending in a
/// newline therefore ends on an empty line, which is exactly where the cursor
/// would be.
fn whole_document(text: &str) -> Range {
    let mut line = 0u32;
    let mut character = 0u32;
    for ch in text.chars() {
        if ch == '\n' {
            line += 1;
            character = 0;
        } else {
            character += ch.len_utf16() as u32;
        }
    }
    Range {
        start: Position::new(0, 0),
        end: Position::new(line, character),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whole_document_covers_an_empty_file() {
        assert_eq!(
            whole_document(""),
            Range::new(Position::new(0, 0), Position::new(0, 0))
        );
    }

    #[test]
    fn whole_document_ends_past_the_last_character() {
        assert_eq!(
            whole_document("let a = 1"),
            Range::new(Position::new(0, 0), Position::new(0, 9))
        );
    }

    /// A trailing newline puts the end on the next, empty line — every
    /// formatted file ends this way, so this is the common case rather than
    /// an edge one.
    #[test]
    fn whole_document_ends_on_the_empty_line_after_a_trailing_newline() {
        assert_eq!(
            whole_document("let a = 1\nlet b = 2\n"),
            Range::new(Position::new(0, 0), Position::new(2, 0))
        );
    }

    /// The reason this counts UTF-16 units rather than chars: `≠` is one
    /// char and one UTF-16 unit, but an emoji in a string literal is one
    /// char and *two*, and an editor asked to replace too short a range
    /// would leave a stray half behind.
    #[test]
    fn whole_document_counts_utf16_units_not_chars() {
        assert_eq!(whole_document("a ≠ b").end, Position::new(0, 5));
        assert_eq!(whole_document("\"🙂\"").end, Position::new(0, 4));
    }
}
