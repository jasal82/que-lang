/// Document store — maintains open file contents as rope text buffers,
/// keyed by URI. Thread-safe via `DashMap`.

use dashmap::DashMap;
use ropey::Rope;
use tower_lsp::lsp_types::Url;

/// In-memory store for open documents.
#[derive(Debug, Default)]
pub struct DocumentStore {
    docs: DashMap<Url, Rope>,
}

#[allow(dead_code)]
impl DocumentStore {
    pub fn new() -> Self {
        Self {
            docs: DashMap::new(),
        }
    }

    /// Insert or replace a document.
    pub fn open(&self, uri: Url, text: &str) {
        self.docs.insert(uri, Rope::from_str(text));
    }

    /// Apply a full-content change (we use full sync mode).
    pub fn update(&self, uri: &Url, text: &str) {
        self.docs.insert(uri.clone(), Rope::from_str(text));
    }

    /// Remove a document from the store.
    pub fn close(&self, uri: &Url) {
        self.docs.remove(uri);
    }

    /// Get the full text of a document.
    pub fn get_text(&self, uri: &Url) -> Option<String> {
        self.docs.get(uri).map(|r| r.to_string())
    }

    /// Get a rope reference for line/offset operations.
    pub fn get_rope(&self, uri: &Url) -> Option<Rope> {
        self.docs.get(uri).map(|r| r.clone())
    }

    /// List all open document URIs.
    pub fn uris(&self) -> Vec<Url> {
        self.docs.iter().map(|r| r.key().clone()).collect()
    }
}
