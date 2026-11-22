pub mod ast;
pub mod completion;
pub mod config;
pub mod docs;
pub mod embed;
pub mod environment;
pub mod error;
pub mod formatter;
pub mod http;
pub mod install;
pub mod interpreter;
pub mod interrupt;
pub mod lexer;
pub mod linter;
pub mod manifest;
pub mod module_loader;
pub mod parser;
pub mod permissions;
pub mod resolver;
pub mod test_runner;
pub mod token;
pub mod value;

pub use embed::{language_version, LANGUAGE_VERSION};

/// Compiles and runs the Rust examples in `docs/embedding.md`.
#[cfg(doctest)]
#[doc = include_str!("../docs/embedding.md")]
pub struct EmbeddingGuide;
