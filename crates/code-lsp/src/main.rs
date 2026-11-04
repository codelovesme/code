//! Stdio LSP: shared non-executing source checks, semantic tokens and formatting.
//! Open buffers override disk. No program or native module is executed.
//! Completion, hover, definitions, references and rename remain separate work.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

mod analysis;
mod diagnostics;
mod tokens;

#[derive(Default)]
struct State {
    documents: HashMap<Url, analysis::Document>,
    published: HashSet<Url>,
    revision: u64,
}

struct Backend {
    client: Client,
    state: Mutex<State>,
}

impl Backend {
    fn new(client: Client) -> Self {
        Self {
            client,
            state: Mutex::new(State::default()),
        }
    }

    async fn refresh(&self) {
        let (revision, publications) = {
            let mut state = self.state.lock().unwrap();
            let mut report = analysis::analyze(&state.documents);
            let next: HashSet<_> = report
                .keys()
                .chain(state.documents.keys())
                .cloned()
                .collect();
            let mut uris: Vec<_> = next.union(&state.published).cloned().collect();
            uris.sort_by(|a, b| a.as_str().cmp(b.as_str()));
            let publications: Vec<_> = uris
                .into_iter()
                .map(|uri| {
                    let version = state.documents.get(&uri).map(|doc| doc.version);
                    let diagnostics = report.remove(&uri).unwrap_or_default();
                    (uri, diagnostics, version)
                })
                .collect();
            state.published = next;
            (state.revision, publications)
        };
        for (uri, diagnostics, version) in publications {
            if self.state.lock().unwrap().revision != revision {
                break;
            }
            self.client
                .publish_diagnostics(uri, diagnostics, version)
                .await;
        }
    }

    async fn on_change(&self, uri: Url, text: String, version: i32) {
        {
            let mut state = self.state.lock().unwrap();
            if state
                .documents
                .get(&uri)
                .is_some_and(|doc| doc.version > version)
            {
                return;
            }
            state
                .documents
                .insert(uri, analysis::Document { text, version });
            state.revision += 1;
        }
        self.refresh().await;
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Options(
                    TextDocumentSyncOptions {
                        open_close: Some(true),
                        change: Some(TextDocumentSyncKind::FULL),
                        save: Some(TextDocumentSyncSaveOptions::Supported(true)),
                        ..Default::default()
                    },
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
        self.on_change(
            params.text_document.uri,
            params.text_document.text,
            params.text_document.version,
        )
        .await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        // FULL sync: the client always sends exactly one change event
        // holding the entire new document text.
        if let Some(change) = params.content_changes.into_iter().last() {
            self.on_change(
                params.text_document.uri,
                change.text,
                params.text_document.version,
            )
            .await;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        {
            let mut state = self.state.lock().unwrap();
            state.documents.remove(&params.text_document.uri);
            state.revision += 1;
        }
        self.refresh().await;
    }

    async fn did_save(&self, _: DidSaveTextDocumentParams) {
        self.state.lock().unwrap().revision += 1;
        self.refresh().await;
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
            .state
            .lock()
            .unwrap()
            .documents
            .get(&params.text_document.uri)
        {
            Some(doc) => doc.text.clone(),
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
            .state
            .lock()
            .unwrap()
            .documents
            .get(&params.text_document.uri)
        {
            Some(doc) => doc.text.clone(),
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
            whole_document("a = 1"),
            Range::new(Position::new(0, 0), Position::new(0, 5))
        );
    }

    /// A trailing newline puts the end on the next, empty line — every
    /// formatted file ends this way, so this is the common case rather than
    /// an edge one.
    #[test]
    fn whole_document_ends_on_the_empty_line_after_a_trailing_newline() {
        assert_eq!(
            whole_document("a = 1\nb = 2\n"),
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
