/// The Que Language Server — ties all providers together behind the
/// `tower-lsp` `LanguageServer` trait.

use crate::completion;
use crate::diagnostics;
use crate::document::DocumentStore;
use crate::goto;
use crate::hover;
use crate::symbols;

use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

pub struct QueLanguageServer {
    client: Client,
    documents: DocumentStore,
}

impl QueLanguageServer {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            documents: DocumentStore::new(),
        }
    }

    /// Publish diagnostics for a single document.
    async fn publish_diagnostics(&self, uri: Url) {
        let source = match self.documents.get_text(&uri) {
            Some(s) => s,
            None => return,
        };
        let diags = diagnostics::compute_diagnostics(&source);
        self.client
            .publish_diagnostics(uri, diags, None)
            .await;
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for QueLanguageServer {
    async fn initialize(&self, _params: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                // Full document sync — the client sends the entire text on each change.
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                // Completions triggered by `.` and `:`
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec![".".to_string(), ":".to_string()]),
                    resolve_provider: Some(false),
                    ..Default::default()
                }),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                definition_provider: Some(OneOf::Left(true)),
                document_symbol_provider: Some(OneOf::Left(true)),
                // Semantic tokens for syntax highlighting beyond TextMate grammars
                semantic_tokens_provider: Some(
                    SemanticTokensServerCapabilities::SemanticTokensOptions(
                        SemanticTokensOptions {
                            legend: semantic_token_legend(),
                            full: Some(SemanticTokensFullOptions::Bool(true)),
                            range: Some(false),
                            ..Default::default()
                        },
                    ),
                ),
                ..Default::default()
            },
            server_info: Some(ServerInfo {
                name: "que-lsp".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
        })
    }

    async fn initialized(&self, _params: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "Que language server initialized")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    // ── Document synchronization ──

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri;
        self.documents.open(uri.clone(), &params.text_document.text);
        self.publish_diagnostics(uri).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        if let Some(change) = params.content_changes.into_iter().last() {
            self.documents.update(&uri, &change.text);
        }
        self.publish_diagnostics(uri).await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        // Clear diagnostics when a file is closed
        self.client
            .publish_diagnostics(uri.clone(), vec![], None)
            .await;
        self.documents.close(&uri);
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        // Re-publish diagnostics on save
        self.publish_diagnostics(params.text_document.uri).await;
    }

    // ── Completions ──

    async fn completion(
        &self,
        params: CompletionParams,
    ) -> Result<Option<CompletionResponse>> {
        let uri = &params.text_document_position.text_document.uri;
        let position = &params.text_document_position.position;

        let source = match self.documents.get_text(uri) {
            Some(s) => s,
            None => return Ok(None),
        };

        let items = completion::completions(&source, position);
        Ok(Some(CompletionResponse::Array(items)))
    }

    // ── Hover ──

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let position = &params.text_document_position_params.position;

        let source = match self.documents.get_text(uri) {
            Some(s) => s,
            None => return Ok(None),
        };

        Ok(hover::hover(&source, position))
    }

    // ── Go to definition ──

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let position = &params.text_document_position_params.position;

        let source = match self.documents.get_text(uri) {
            Some(s) => s,
            None => return Ok(None),
        };

        Ok(goto::goto_definition(uri, &source, position))
    }

    // ── Document symbols (outline) ──

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let uri = &params.text_document.uri;

        let source = match self.documents.get_text(uri) {
            Some(s) => s,
            None => return Ok(None),
        };

        let syms = symbols::document_symbols(&source);
        Ok(Some(DocumentSymbolResponse::Nested(syms)))
    }

    // ── Semantic tokens ──

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        let uri = &params.text_document.uri;

        let source = match self.documents.get_text(uri) {
            Some(s) => s,
            None => return Ok(None),
        };

        let tokens = compute_semantic_tokens(&source);
        Ok(Some(SemanticTokensResult::Tokens(SemanticTokens {
            result_id: None,
            data: tokens,
        })))
    }
}

// ── Semantic tokens ──

fn semantic_token_legend() -> SemanticTokensLegend {
    SemanticTokensLegend {
        token_types: vec![
            SemanticTokenType::KEYWORD,     // 0
            SemanticTokenType::FUNCTION,    // 1
            SemanticTokenType::VARIABLE,    // 2
            SemanticTokenType::STRING,      // 3
            SemanticTokenType::NUMBER,      // 4
            SemanticTokenType::COMMENT,     // 5
            SemanticTokenType::TYPE,        // 6
            SemanticTokenType::OPERATOR,    // 7
            SemanticTokenType::PARAMETER,   // 8
            SemanticTokenType::ENUM_MEMBER, // 9
            SemanticTokenType::STRUCT,      // 10
            SemanticTokenType::NAMESPACE,   // 11
        ],
        token_modifiers: vec![
            SemanticTokenModifier::DECLARATION,
            SemanticTokenModifier::DEFINITION,
            SemanticTokenModifier::READONLY,
        ],
    }
}

/// Compute semantic tokens from Que source by running the lexer.
fn compute_semantic_tokens(source: &str) -> Vec<SemanticToken> {
    use que_lang::lexer::Lexer;
    use que_lang::token::TokenKind;

    let mut lexer = Lexer::new(source);
    let tokens = match lexer.tokenize() {
        Ok(t) => t,
        Err(_) => return vec![],
    };

    let mut result = Vec::new();
    let mut prev_line: u32 = 0;
    let mut prev_col: u32 = 0;

    for tok in &tokens {
        let token_type = match &tok.kind {
            // Keywords
            TokenKind::Let
            | TokenKind::Mut
            | TokenKind::Fn
            | TokenKind::Task
            | TokenKind::Type
            | TokenKind::Enum
            | TokenKind::Struct
            | TokenKind::If
            | TokenKind::Else
            | TokenKind::Match
            | TokenKind::For
            | TokenKind::In
            | TokenKind::While
            | TokenKind::Loop
            | TokenKind::Return
            | TokenKind::Break
            | TokenKind::Continue
            | TokenKind::Import
            | TokenKind::As
            | TokenKind::From
            | TokenKind::Pub
            | TokenKind::Try
            | TokenKind::Catch
            | TokenKind::Finally
            | TokenKind::Defer

            | TokenKind::Spawn
            | TokenKind::Parallel
            | TokenKind::Where
            | TokenKind::With => 0, // KEYWORD

            TokenKind::True | TokenKind::False | TokenKind::Null => 0,

            // Strings
            TokenKind::StringLit(_)
            | TokenKind::InterpolatedString(_)
            | TokenKind::RegexLit(_)
            | TokenKind::SemverLit(_) => 3, // STRING

            // Numbers
            TokenKind::IntLit(_) | TokenKind::FloatLit(_) | TokenKind::DurationLit(..) => {
                4 // NUMBER
            }

            // Identifiers: we could further refine by checking builtin names
            TokenKind::Ident(name) => {
                if crate::builtins::TYPES.contains(&name.as_str()) {
                    6 // TYPE
                } else {
                    continue; // skip plain identifiers for now (handled by TextMate grammar)
                }
            }

            _ => continue,
        };

        let line = if tok.span.line > 0 {
            (tok.span.line - 1) as u32
        } else {
            0
        };
        let col = if tok.span.col > 0 {
            (tok.span.col - 1) as u32
        } else {
            0
        };
        let length = (tok.span.end.saturating_sub(tok.span.start)) as u32;

        let delta_line = line - prev_line;
        let delta_start = if delta_line == 0 {
            col - prev_col
        } else {
            col
        };

        result.push(SemanticToken {
            delta_line,
            delta_start,
            length,
            token_type,
            token_modifiers_bitset: 0,
        });

        prev_line = line;
        prev_col = col;
    }

    result
}
