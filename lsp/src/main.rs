use tower_lsp::{LspService, Server};

mod analysis;
/// Re-export builtin docs from the core `que_lang` crate so the rest of the
/// LSP can keep referring to `crate::builtins::...` unchanged.
mod builtins {
    pub use que_lang::docs::*;
}
mod completion;
mod diagnostics;
mod document;
mod goto;
mod hover;
mod server;
mod symbols;

#[tokio::main]
async fn main() {
    env_logger::init();

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(server::QueLanguageServer::new);
    Server::new(stdin, stdout, socket).serve(service).await;
}
