use std::collections::HashMap;
use std::sync::Mutex;

use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

use code_lang::format::format_document;
use code_lang::parser;

mod tokens;

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

/// A symbol found in a .code document.
#[derive(Debug, Clone)]
struct SymbolEntry {
    name: String,
    kind: SymbolKind,
    line: u32,
    col: u32,
    end_col: u32,
    detail: String,
}

/// Cached per-document state.
struct DocumentState {
    text: String,
    symbols: Vec<SymbolEntry>,
    diagnostics: Vec<Diagnostic>,
}

// ---------------------------------------------------------------------------
// Analysis helpers
// ---------------------------------------------------------------------------

/// Convert a byte offset in `source` to an LSP `Position` (0-based line/col).
fn offset_to_position(source: &str, offset: usize) -> Position {
    let mut line: u32 = 0;
    let mut col: u32 = 0;
    for (i, ch) in source.char_indices() {
        if i >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }
    Position::new(line, col)
}

/// Analyse a document: parse for errors, scan for symbols.
fn analyse(text: &str) -> DocumentState {
    let mut diagnostics = Vec::new();
    let mut symbols = Vec::new();

    // Parse with error recovery
    let (_program, errors) = parser::parse_source(text);
    for err in &errors {
        let start = offset_to_position(text, err.start);
        let end = offset_to_position(text, err.end);
        diagnostics.push(Diagnostic {
            range: Range::new(start, end),
            severity: Some(DiagnosticSeverity::ERROR),
            source: Some("code-lsp".into()),
            message: err.message.clone(),
            ..Default::default()
        });
    }

    // Scan lines for symbols (text-based – reliable positions)
    for (line_idx, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        let line_num = line_idx as u32;
        let leading = (line.len() - trimmed.len()) as u32;

        // Skip empty lines and comments
        if trimmed.is_empty() || trimmed.starts_with("->") {
            continue;
        }

        // Type declaration: `type Name { ... }` or `type Name = ...`
        if trimmed.starts_with("type ") {
            if let Some(name) = extract_after_keyword(trimmed, "type") {
                let col = line.find(&name).unwrap_or(0) as u32;
                let detail = trimmed.to_string();
                let is_alias = trimmed[5 + name.len()..].trim_start().starts_with('=');
                symbols.push(SymbolEntry {
                    name: name.clone(),
                    kind: if is_alias { SymbolKind::TYPE_PARAMETER } else { SymbolKind::STRUCT },
                    line: line_num,
                    col,
                    end_col: col + name.len() as u32,
                    detail,
                });
            }
            continue;
        }

        // Link statement: `link path [as alias]`
        if trimmed.starts_with("link ") {
            let rest = &trimmed[5..];
            let module_ref: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '/' || *c == '.' || *c == '-')
                .collect();
            if !module_ref.is_empty() {
                let col = line.find(&module_ref).unwrap_or(0) as u32;
                symbols.push(SymbolEntry {
                    name: module_ref.clone(),
                    kind: SymbolKind::MODULE,
                    line: line_num,
                    col,
                    end_col: col + module_ref.len() as u32,
                    detail: trimmed.to_string(),
                });
                // Also capture alias
                let after = rest[module_ref.len()..].trim_start();
                if after.starts_with("as ") {
                    let alias_name: String = after[3..]
                        .trim_start()
                        .chars()
                        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                        .collect();
                    if !alias_name.is_empty() {
                        let col2 = line.rfind(&alias_name).unwrap_or(0) as u32;
                        symbols.push(SymbolEntry {
                            name: alias_name.clone(),
                            kind: SymbolKind::NAMESPACE,
                            line: line_num,
                            col: col2,
                            end_col: col2 + alias_name.len() as u32,
                            detail: format!("alias for {}", module_ref),
                        });
                    }
                }
            }
            continue;
        }

        // Handler definition: `ClassName => { ... }` or `ClassName{...} => { ... }`
        if is_handler_def(trimmed) {
            let name = extract_class_name(trimmed);
            if !name.is_empty() {
                let col = leading;
                symbols.push(SymbolEntry {
                    name: format!("{} handler", name),
                    kind: SymbolKind::METHOD,
                    line: line_num,
                    col,
                    end_col: col + name.len() as u32,
                    detail: trimmed.to_string(),
                });
            }
            continue;
        }

        // Private assignment: `private name = expr` or `private name:Type = expr`
        if trimmed.starts_with("private ") {
            let inner = &trimmed[8..];
            if let Some((vname, detail)) = extract_assignment_parts(inner) {
                let col = line.find(&vname).unwrap_or(0) as u32;
                let kind = if detail.contains("=>") {
                    SymbolKind::FUNCTION
                } else {
                    SymbolKind::VARIABLE
                };
                symbols.push(SymbolEntry {
                    name: vname.clone(),
                    kind,
                    line: line_num,
                    col,
                    end_col: col + vname.len() as u32,
                    detail: format!("private {}", detail),
                });
            }
            continue;
        }

        // Plain or typed assignment: `name = expr` or `name:Type = expr`
        if let Some((vname, detail)) = extract_assignment_parts(trimmed) {
            let col = leading;
            let kind = if detail.contains("=>") {
                SymbolKind::FUNCTION
            } else {
                SymbolKind::VARIABLE
            };
            symbols.push(SymbolEntry {
                name: vname.clone(),
                kind,
                line: line_num,
                col,
                end_col: col + vname.len() as u32,
                detail,
            });
        }
    }

    DocumentState {
        text: text.to_string(),
        symbols,
        diagnostics,
    }
}

/// Extract the name after a keyword like `type` — next word starting with uppercase.
fn extract_after_keyword(trimmed: &str, keyword: &str) -> Option<String> {
    let rest = trimmed[keyword.len()..].trim_start();
    let name: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    if name.is_empty()
        || !name.chars().next().map_or(false, |c| c.is_ascii_uppercase())
    {
        return None;
    }
    Some(name)
}

/// Check if a trimmed line looks like a handler definition.
fn is_handler_def(trimmed: &str) -> bool {
    let first = trimmed.chars().next().unwrap_or(' ');
    if !first.is_ascii_uppercase() {
        return false;
    }
    // Extract class name
    let name_end = trimmed
        .char_indices()
        .find(|(_, c)| !c.is_ascii_alphanumeric() && *c != '_')
        .map(|(i, _)| i)
        .unwrap_or(trimmed.len());
    let after = trimmed[name_end..].trim_start();
    // `ClassName =>` (bare handler def)
    if after.starts_with("=>") {
        let after_arrow = after[2..].trim_start();
        return after_arrow.starts_with('{');
    }
    // `ClassName{field:Type} =>`  (combined handler def)
    if after.starts_with('{') {
        // Find matching }
        if let Some(close) = find_matching_brace(after) {
            let after_brace = after[close + 1..].trim_start();
            return after_brace.starts_with("=>");
        }
    }
    false
}

/// Find the index of the matching `}` for a string starting with `{`.
fn find_matching_brace(s: &str) -> Option<usize> {
    let mut depth = 0i32;
    for (i, c) in s.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Extract the class name from the start of a line (uppercase identifier).
fn extract_class_name(trimmed: &str) -> String {
    trimmed
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect()
}

/// Extract `(name, detail)` from an assignment line such as `name = expr` or
/// `name:Type = expr`.
fn extract_assignment_parts(s: &str) -> Option<(String, String)> {
    let first = s.chars().next().unwrap_or(' ');
    if !first.is_ascii_lowercase() && first != '_' {
        return None;
    }
    let name: String = s
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    if name.is_empty() {
        return None;
    }
    let after = s[name.len()..].trim_start();
    // name:Type = expr
    if after.starts_with(':') || after.starts_with('=') {
        return Some((name, s.to_string()));
    }
    // name.field = expr  (property assignment — skip)
    if after.starts_with('.') {
        return None;
    }
    None
}

// ---------------------------------------------------------------------------
// Completion helpers
// ---------------------------------------------------------------------------

const KEYWORDS: &[(&str, CompletionItemKind)] = &[
    ("if", CompletionItemKind::KEYWORD),
    ("loop", CompletionItemKind::KEYWORD),
    ("over", CompletionItemKind::KEYWORD),
    ("break", CompletionItemKind::KEYWORD),
    ("return", CompletionItemKind::KEYWORD),
    ("assert", CompletionItemKind::KEYWORD),
    ("link", CompletionItemKind::KEYWORD),
    ("as", CompletionItemKind::KEYWORD),
    ("type", CompletionItemKind::KEYWORD),
    ("private", CompletionItemKind::KEYWORD),
    ("is", CompletionItemKind::KEYWORD),
    ("not", CompletionItemKind::KEYWORD),
    ("true", CompletionItemKind::KEYWORD),
    ("false", CompletionItemKind::KEYWORD),
    ("Null", CompletionItemKind::KEYWORD),
    ("this", CompletionItemKind::KEYWORD),
    ("base", CompletionItemKind::KEYWORD),
];

/// Standard snippet completions.
fn snippet_completions() -> Vec<CompletionItem> {
    vec![
        make_snippet(
            "type",
            "Type Declaration",
            "type ${1:Name} {\n    ${2:field}:${3:Type}\n}",
        ),
        make_snippet(
            "type alias",
            "Type Alias",
            "type ${1:Name} = ${2:Type}",
        ),
        make_snippet(
            "handler",
            "Handler Definition",
            "${1:ClassName} => {\n    $0\n}",
        ),
        make_snippet(
            "handler (combined)",
            "Combined Handler Definition",
            "${1:ClassName}{${2:field}:${3:Type}} => {\n    $0\n}",
        ),
        make_snippet(
            "if",
            "If Statement",
            "if ${1:condition} {\n    $0\n}",
        ),
        make_snippet(
            "loop",
            "Loop Over",
            "loop ${1:item} over ${2:collection} {\n    $0\n}",
        ),
        make_snippet("link", "Link Module", "link ${1:module}"),
        make_snippet(
            "link as",
            "Link Module With Alias",
            "link ${1:module} as ${2:alias}",
        ),
        make_snippet(
            "assert",
            "Assert",
            "assert ${1:expression}",
        ),
    ]
}

fn make_snippet(label: &str, detail: &str, body: &str) -> CompletionItem {
    CompletionItem {
        label: label.to_string(),
        kind: Some(CompletionItemKind::SNIPPET),
        detail: Some(detail.to_string()),
        insert_text: Some(body.to_string()),
        insert_text_format: Some(InsertTextFormat::SNIPPET),
        ..Default::default()
    }
}


// ---------------------------------------------------------------------------
// Hover helpers
// ---------------------------------------------------------------------------

/// Build hover content for a word by searching the document symbols.
fn build_hover(word: &str, state: &DocumentState) -> Option<String> {
    // Look for a type declaration
    for sym in &state.symbols {
        if sym.kind == SymbolKind::STRUCT && sym.name == word {
            return Some(format!("```code\n{}\n```", sym.detail));
        }
        if sym.kind == SymbolKind::TYPE_PARAMETER && sym.name == word {
            return Some(format!("```code\n{}\n```", sym.detail));
        }
    }
    // Look for a handler
    for sym in &state.symbols {
        let handler_name = format!("{} handler", word);
        if sym.kind == SymbolKind::METHOD && sym.name == handler_name {
            return Some(format!("```code\n{}\n```", sym.detail));
        }
    }
    // Look for a variable / function
    for sym in &state.symbols {
        if (sym.kind == SymbolKind::VARIABLE || sym.kind == SymbolKind::FUNCTION)
            && sym.name == word
        {
            return Some(format!("```code\n{}\n```", sym.detail));
        }
    }
    // Look for a namespace (link alias)
    for sym in &state.symbols {
        if sym.kind == SymbolKind::NAMESPACE && sym.name == word {
            return Some(format!("*module alias* — {}", sym.detail));
        }
    }
    None
}

/// Extract the word at a given position in source text.
fn word_at_position(text: &str, pos: &Position) -> Option<String> {
    let line = text.lines().nth(pos.line as usize)?;
    let col = pos.character as usize;
    if col > line.len() {
        return None;
    }
    let before: String = line[..col]
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let after: String = line[col..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    let word = format!("{}{}", before, after);
    if word.is_empty() {
        None
    } else {
        Some(word)
    }
}

// ---------------------------------------------------------------------------
// Go-to-definition helper
// ---------------------------------------------------------------------------

/// Find the definition location for a symbol name within the document.
fn find_definition(word: &str, state: &DocumentState, uri: &Url) -> Option<Location> {
    for sym in &state.symbols {
        let matches = sym.name == word
            || sym.name == format!("{} handler", word);
        if matches {
            let range = Range::new(
                Position::new(sym.line, sym.col),
                Position::new(sym.line, sym.end_col),
            );
            return Some(Location::new(uri.clone(), range));
        }
    }
    None
}

// ---------------------------------------------------------------------------
// References helper
// ---------------------------------------------------------------------------

/// Find all occurrences of `word` in the document text.
fn find_references(word: &str, text: &str, uri: &Url) -> Vec<Location> {
    let mut locations = Vec::new();
    for (line_idx, line) in text.lines().enumerate() {
        let mut start = 0;
        while let Some(pos) = line[start..].find(word) {
            let abs_pos = start + pos;
            let end_pos = abs_pos + word.len();
            // Ensure it's a whole word
            let before_ok = abs_pos == 0
                || !line.as_bytes()[abs_pos - 1].is_ascii_alphanumeric()
                    && line.as_bytes()[abs_pos - 1] != b'_';
            let after_ok = end_pos >= line.len()
                || !line.as_bytes()[end_pos].is_ascii_alphanumeric()
                    && line.as_bytes()[end_pos] != b'_';
            if before_ok && after_ok {
                let range = Range::new(
                    Position::new(line_idx as u32, abs_pos as u32),
                    Position::new(line_idx as u32, end_pos as u32),
                );
                locations.push(Location::new(uri.clone(), range));
            }
            start = end_pos;
        }
    }
    locations
}

// ---------------------------------------------------------------------------
// Backend
// ---------------------------------------------------------------------------

struct Backend {
    client: Client,
    documents: Mutex<HashMap<Url, DocumentState>>,
}

impl Backend {
    fn new(client: Client) -> Self {
        Self {
            client,
            documents: Mutex::new(HashMap::new()),
        }
    }

    async fn on_change(&self, uri: Url, text: String) {
        let state = analyse(&text);
        let diags = state.diagnostics.clone();
        self.documents.lock().unwrap().insert(uri.clone(), state);
        self.client
            .publish_diagnostics(uri, diags, None)
            .await;
    }
}

// ---------------------------------------------------------------------------
// LanguageServer implementation
// ---------------------------------------------------------------------------

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec![".".into(), ":".into(), "=".into()]),
                    ..Default::default()
                }),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                definition_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                document_symbol_provider: Some(OneOf::Left(true)),
                document_formatting_provider: Some(OneOf::Left(true)),
                rename_provider: Some(OneOf::Left(true)),
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
                version: Some("0.1.0".into()),
            }),
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "Code Language Server initialized")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    // -- Document sync -------------------------------------------------------

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri;
        let text = params.text_document.text;
        self.on_change(uri, text).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        if let Some(change) = params.content_changes.into_iter().last() {
            self.on_change(uri, change.text).await;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        self.documents.lock().unwrap().remove(&uri);
        // Clear diagnostics
        self.client
            .publish_diagnostics(uri, vec![], None)
            .await;
    }

    // -- Completion ----------------------------------------------------------

    async fn completion(
        &self,
        params: CompletionParams,
    ) -> Result<Option<CompletionResponse>> {
        let uri = &params.text_document_position.text_document.uri;
        let mut items: Vec<CompletionItem> = Vec::new();

        // Keywords
        for &(kw, kind) in KEYWORDS {
            items.push(CompletionItem {
                label: kw.to_string(),
                kind: Some(kind),
                ..Default::default()
            });
        }

        // Snippets
        items.extend(snippet_completions());

        // Symbols from the current document
        let docs = self.documents.lock().unwrap();
        if let Some(state) = docs.get(uri) {
            let mut seen = std::collections::HashSet::new();
            for sym in &state.symbols {
                if seen.insert(sym.name.clone()) {
                    let kind = match sym.kind {
                        SymbolKind::STRUCT | SymbolKind::TYPE_PARAMETER => {
                            CompletionItemKind::CLASS
                        }
                        SymbolKind::FUNCTION => CompletionItemKind::FUNCTION,
                        SymbolKind::METHOD => CompletionItemKind::METHOD,
                        SymbolKind::NAMESPACE => CompletionItemKind::MODULE,
                        SymbolKind::MODULE => CompletionItemKind::MODULE,
                        _ => CompletionItemKind::VARIABLE,
                    };
                    items.push(CompletionItem {
                        label: sym.name.clone(),
                        kind: Some(kind),
                        detail: Some(sym.detail.clone()),
                        ..Default::default()
                    });
                }
            }
        }

        Ok(Some(CompletionResponse::Array(items)))
    }

    // -- Hover ---------------------------------------------------------------

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let pos = &params.text_document_position_params.position;

        let docs = self.documents.lock().unwrap();
        let state = match docs.get(uri) {
            Some(s) => s,
            None => return Ok(None),
        };

        let word = match word_at_position(&state.text, pos) {
            Some(w) => w,
            None => return Ok(None),
        };

        let content = match build_hover(&word, state) {
            Some(c) => c,
            None => return Ok(None),
        };

        Ok(Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: content,
            }),
            range: None,
        }))
    }

    // -- Go to definition ----------------------------------------------------

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let pos = &params.text_document_position_params.position;

        let docs = self.documents.lock().unwrap();
        let state = match docs.get(uri) {
            Some(s) => s,
            None => return Ok(None),
        };

        let word = match word_at_position(&state.text, pos) {
            Some(w) => w,
            None => return Ok(None),
        };

        match find_definition(&word, state, uri) {
            Some(loc) => Ok(Some(GotoDefinitionResponse::Scalar(loc))),
            None => Ok(None),
        }
    }

    // -- References ----------------------------------------------------------

    async fn references(
        &self,
        params: ReferenceParams,
    ) -> Result<Option<Vec<Location>>> {
        let uri = &params.text_document_position.text_document.uri;
        let pos = &params.text_document_position.position;

        let docs = self.documents.lock().unwrap();
        let state = match docs.get(uri) {
            Some(s) => s,
            None => return Ok(None),
        };

        let word = match word_at_position(&state.text, pos) {
            Some(w) => w,
            None => return Ok(None),
        };

        let refs = find_references(&word, &state.text, uri);
        if refs.is_empty() {
            Ok(None)
        } else {
            Ok(Some(refs))
        }
    }

    // -- Document symbols ----------------------------------------------------

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let uri = &params.text_document.uri;

        let docs = self.documents.lock().unwrap();
        let state = match docs.get(uri) {
            Some(s) => s,
            None => return Ok(None),
        };

        let symbols: Vec<SymbolInformation> = state
            .symbols
            .iter()
            .map(|sym| {
                #[allow(deprecated)]
                SymbolInformation {
                    name: sym.name.clone(),
                    kind: sym.kind,
                    location: Location::new(
                        uri.clone(),
                        Range::new(
                            Position::new(sym.line, sym.col),
                            Position::new(sym.line, sym.end_col),
                        ),
                    ),
                    tags: None,
                    deprecated: None,
                    container_name: None,
                }
            })
            .collect();

        Ok(Some(DocumentSymbolResponse::Flat(symbols)))
    }

    // -- Formatting ----------------------------------------------------------

    // -- Semantic tokens -------------------------------------------------------

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        let uri = &params.text_document.uri;

        let docs = self.documents.lock().unwrap();
        let state = match docs.get(uri) {
            Some(s) => s,
            None => return Ok(None),
        };

        let toks = tokens::tokenize(&state.text);
        let data = tokens::encode_deltas(&toks);
        let lsp_data: Vec<SemanticToken> = data
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
            data: lsp_data,
        })))
    }

    async fn formatting(
        &self,
        params: DocumentFormattingParams,
    ) -> Result<Option<Vec<TextEdit>>> {
        let uri = &params.text_document.uri;
        let indent_size = params.options.tab_size as usize;

        let docs = self.documents.lock().unwrap();
        let state = match docs.get(uri) {
            Some(s) => s,
            None => return Ok(None),
        };

        let formatted = format_document(&state.text, indent_size);
        if formatted == state.text {
            return Ok(None);
        }

        // Replace entire document
        let line_count = state.text.lines().count() as u32;
        let last_line_len = state.text.lines().last().map(|l| l.len()).unwrap_or(0) as u32;
        let full_range = Range::new(
            Position::new(0, 0),
            Position::new(line_count, last_line_len),
        );

        Ok(Some(vec![TextEdit::new(full_range, formatted)]))
    }

    // -- Rename --------------------------------------------------------------

    async fn rename(
        &self,
        params: RenameParams,
    ) -> Result<Option<WorkspaceEdit>> {
        let uri = &params.text_document_position.text_document.uri;
        let pos = &params.text_document_position.position;
        let new_name = &params.new_name;

        let docs = self.documents.lock().unwrap();
        let state = match docs.get(uri) {
            Some(s) => s,
            None => return Ok(None),
        };

        let word = match word_at_position(&state.text, pos) {
            Some(w) => w,
            None => return Ok(None),
        };

        let refs = find_references(&word, &state.text, uri);
        if refs.is_empty() {
            return Ok(None);
        }

        let edits: Vec<TextEdit> = refs
            .into_iter()
            .map(|loc| TextEdit::new(loc.range, new_name.clone()))
            .collect();

        let mut changes = HashMap::new();
        changes.insert(uri.clone(), edits);

        Ok(Some(WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }))
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(Backend::new);
    Server::new(stdin, stdout, socket).serve(service).await;
}
