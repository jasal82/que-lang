/// Analysis module — runs the Que lexer + parser on document text and
/// collects symbols, definitions, and any errors encountered.

use que_lang::ast::*;
use que_lang::lexer::Lexer;
use que_lang::parser::Parser;
use que_lang::token::Span;
use tower_lsp::lsp_types::{Position, Range};

/// A symbol extracted from a Que module.
#[derive(Debug, Clone)]
pub struct SymbolInfo {
    pub name: String,
    pub kind: SymbolKind,
    pub span: Span,
    pub detail: Option<String>,
    pub children: Vec<SymbolInfo>,
}

/// The kind of a Que symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum SymbolKind {
    Function,
    Task,
    Variable,
    Parameter,
    Type,
    Enum,
    EnumVariant,
    Struct,
    StructField,
    Trait,
    Impl,
    Module,
}

/// An error produced during analysis.
#[derive(Debug, Clone)]
pub struct AnalysisError {
    pub message: String,
    pub span: Option<Span>,
}

/// The result of analyzing a Que source file.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct AnalysisResult {
    pub module: Option<Module>,
    pub symbols: Vec<SymbolInfo>,
    pub errors: Vec<AnalysisError>,
    /// Findings from the linter and name resolver. Empty when the file does
    /// not parse, since there is no AST to analyse.
    pub lints: Vec<que_lang::linter::LintDiagnostic>,
}

/// Analyze Que source text: lex → parse → extract symbols.
pub fn analyze(source: &str) -> AnalysisResult {
    // Lex
    let mut lexer = Lexer::new(source);
    let tokens = match lexer.tokenize() {
        Ok(tokens) => tokens,
        Err(e) => {
            return AnalysisResult {
                module: None,
                symbols: vec![],
                errors: vec![AnalysisError {
                    message: e.message.clone(),
                    span: e.span,
                }],
                lints: vec![],
            };
        }
    };

    // Parse
    let mut parser = Parser::new(tokens);
    let module = match parser.parse_module() {
        Ok(module) => module,
        Err(e) => {
            return AnalysisResult {
                module: None,
                symbols: vec![],
                errors: vec![AnalysisError {
                    message: e.message.clone(),
                    span: e.span,
                }],
                lints: vec![],
            };
        }
    };

    // Extract symbols
    let symbols = extract_symbols(&module);
    let lints = que_lang::linter::Linter::new().lint_module(&module);

    AnalysisResult {
        module: Some(module),
        symbols,
        errors: vec![],
        lints,
    }
}

/// Walk the AST and extract named symbols (functions, tasks, variables, types, etc.)
fn extract_symbols(module: &Module) -> Vec<SymbolInfo> {
    let mut symbols = Vec::new();
    for (_, item) in &module.items {
        match item {
            Item::FnDecl(f) => {
                let params_str = f
                    .params
                    .iter()
                    .map(|p| {
                        if let Some(t) = &p.type_ann {
                            format!("{}: {}", p.name, t)
                        } else {
                            p.name.clone()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                let ret = f
                    .return_type
                    .as_ref()
                    .map(|t| format!(" -> {}", t))
                    .unwrap_or_default();
                let detail = format!("fn {}({}){}", f.name, params_str, ret);

                let children = f
                    .params
                    .iter()
                    .map(|p| SymbolInfo {
                        name: p.name.clone(),
                        kind: SymbolKind::Parameter,
                        span: Span::new(0, 0, 0, 0), // params don't have spans yet
                        detail: p.type_ann.as_ref().map(|t| t.to_string()),
                        children: vec![],
                    })
                    .collect();

                symbols.push(SymbolInfo {
                    name: f.name.clone(),
                    kind: SymbolKind::Function,
                    span: Span::new(0, 0, 0, 0), // TODO: attach spans from parser
                    detail: Some(detail),
                    children,
                });
            }
            Item::TaskDecl(t) => {
                let desc = t
                    .description
                    .as_ref()
                    .map(|d| format!("task {} — {}", t.name, d))
                    .unwrap_or_else(|| format!("task {}", t.name));
                symbols.push(SymbolInfo {
                    name: t.name.clone(),
                    kind: SymbolKind::Task,
                    span: Span::new(0, 0, 0, 0),
                    detail: Some(desc),
                    children: vec![],
                });
            }
            Item::TypeDecl(td) => {
                symbols.push(SymbolInfo {
                    name: td.name.clone(),
                    kind: SymbolKind::Type,
                    span: Span::new(0, 0, 0, 0),
                    detail: Some(format!("type {} = {}", td.name, td.type_expr)),
                    children: vec![],
                });
            }
            Item::EnumDecl(e) => {
                let children = e
                    .variants
                    .iter()
                    .map(|v| SymbolInfo {
                        name: v.name.clone(),
                        kind: SymbolKind::EnumVariant,
                        span: Span::new(0, 0, 0, 0),
                        detail: None,
                        children: vec![],
                    })
                    .collect();
                symbols.push(SymbolInfo {
                    name: e.name.clone(),
                    kind: SymbolKind::Enum,
                    span: Span::new(0, 0, 0, 0),
                    detail: Some(format!("enum {}", e.name)),
                    children,
                });
            }
            Item::StructDecl(s) => {
                let children = s
                    .fields
                    .iter()
                    .map(|f| SymbolInfo {
                        name: f.name.clone(),
                        kind: SymbolKind::StructField,
                        span: Span::new(0, 0, 0, 0),
                        detail: f.type_ann.as_ref().map(|t| t.to_string()),
                        children: vec![],
                    })
                    .collect();
                symbols.push(SymbolInfo {
                    name: s.name.clone(),
                    kind: SymbolKind::Struct,
                    span: Span::new(0, 0, 0, 0),
                    detail: Some(format!("struct {}", s.name)),
                    children,
                });
            }
            Item::ImplDecl(i) => {
                let children = i
                    .methods
                    .iter()
                    .map(|m| SymbolInfo {
                        name: m.name.clone(),
                        kind: SymbolKind::Function,
                        span: Span::new(0, 0, 0, 0),
                        detail: Some(format!("fn {}", m.name)),
                        children: vec![],
                    })
                    .collect();
                symbols.push(SymbolInfo {
                    name: format!("impl {}", i.type_name),
                    kind: SymbolKind::Impl,
                    span: Span::new(0, 0, 0, 0),
                    detail: Some(format!("impl {}", i.type_name)),
                    children,
                });
            }
            Item::TraitDecl(t) => {
                let children = t
                    .methods
                    .iter()
                    .map(|m| SymbolInfo {
                        name: m.name.clone(),
                        kind: SymbolKind::Function,
                        span: Span::new(0, 0, 0, 0),
                        detail: Some(format!("fn {}", m.name)),
                        children: vec![],
                    })
                    .collect();
                symbols.push(SymbolInfo {
                    name: t.name.clone(),
                    kind: SymbolKind::Trait,
                    span: Span::new(0, 0, 0, 0),
                    detail: Some(format!("trait {}", t.name)),
                    children,
                });
            }
            Item::TraitImplDecl(ti) => {
                let children = ti
                    .methods
                    .iter()
                    .map(|m| SymbolInfo {
                        name: m.name.clone(),
                        kind: SymbolKind::Function,
                        span: Span::new(0, 0, 0, 0),
                        detail: Some(format!("fn {}", m.name)),
                        children: vec![],
                    })
                    .collect();
                symbols.push(SymbolInfo {
                    name: format!("impl {} for {}", ti.trait_name, ti.type_name),
                    kind: SymbolKind::Impl,
                    span: Span::new(0, 0, 0, 0),
                    detail: Some(format!("impl {} for {}", ti.trait_name, ti.type_name)),
                    children,
                });
            }
            Item::Stmt(stmt) => {
                extract_stmt_symbols(stmt, &mut symbols);
            }
            Item::Import(_) => {
                // Imports aren't symbols we surface in the outline
            }
            Item::PubLet { pattern, .. } => {
                // Extract variable names from the pattern as symbols
                extract_pattern_symbols(pattern, &mut symbols);
            }
        }
    }
    symbols
}

/// Extract variable definitions from statements.
fn extract_stmt_symbols(stmt: &Stmt, symbols: &mut Vec<SymbolInfo>) {
    match stmt {
        Stmt::Let { pattern, type_ann, .. } => {
            if let Some(name) = pattern_name(pattern) {
                symbols.push(SymbolInfo {
                    name,
                    kind: SymbolKind::Variable,
                    span: Span::new(0, 0, 0, 0),
                    detail: type_ann.as_ref().map(|t| t.to_string()),
                    children: vec![],
                });
            }
        }
        Stmt::Mut { name, type_ann, .. } => {
            symbols.push(SymbolInfo {
                name: name.clone(),
                kind: SymbolKind::Variable,
                span: Span::new(0, 0, 0, 0),
                detail: type_ann.as_ref().map(|t| t.to_string()),
                children: vec![],
            });
        }
        _ => {}
    }
}

fn extract_pattern_symbols(pattern: &Pattern, symbols: &mut Vec<SymbolInfo>) {
    if let Some(name) = pattern_name(pattern) {
        symbols.push(SymbolInfo {
            name,
            kind: SymbolKind::Variable,
            span: Span::new(0, 0, 0, 0),
            detail: None,
            children: vec![],
        });
    }
}

fn pattern_name(pattern: &Pattern) -> Option<String> {
    match pattern {
        Pattern::Ident(name) => Some(name.clone()),
        _ => None,
    }
}

// ── Span ↔ LSP Position helpers ──

/// Convert a Que `Span` to an LSP `Range`.
/// Que spans use 1-based line numbers; LSP uses 0-based.
pub fn span_to_range(span: &Span) -> Range {
    let line = if span.line > 0 { span.line - 1 } else { 0 } as u32;
    let col = if span.col > 0 { span.col - 1 } else { 0 } as u32;
    // Estimate end column from span byte offsets
    let len = (span.end.saturating_sub(span.start)) as u32;
    Range {
        start: Position::new(line, col),
        end: Position::new(line, col + len),
    }
}

/// Get the word at a given LSP position from source text.
pub fn word_at_position(source: &str, position: &Position) -> Option<String> {
    let line_idx = position.line as usize;
    let col_utf16 = position.character as usize;
    let line = source.lines().nth(line_idx)?;

    // Convert UTF-16 column offset to byte offset; return None if column is out of range
    let col_idx = {
        let mut byte_idx = 0;
        let mut utf16_remaining = col_utf16;
        for c in line.chars() {
            if utf16_remaining == 0 {
                break;
            }
            let units = c.len_utf16();
            if utf16_remaining < units {
                return None;
            }
            utf16_remaining -= units;
            byte_idx += c.len_utf8();
        }
        if utf16_remaining > 0 {
            return None;
        }
        byte_idx
    };

    // Scan left from cursor to find word start (use char_indices to skip past delimiter safely)
    let start = line[..col_idx]
        .char_indices()
        .rfind(|(_, c)| !c.is_alphanumeric() && *c != '_')
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(0);

    // Scan right from cursor to find word end
    let end = line[col_idx..]
        .find(|c: char| !c.is_alphanumeric() && c != '_')
        .map(|i| i + col_idx)
        .unwrap_or(line.len());

    if start >= end {
        return None;
    }

    Some(line[start..end].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower_lsp::lsp_types::Position;

    fn pos(line: u32, character: u32) -> Position {
        Position { line, character }
    }

    #[test]
    fn test_word_at_position_ascii() {
        let src = "let foo = bar";
        assert_eq!(word_at_position(src, &pos(0, 4)), Some("foo".to_string()));
        assert_eq!(word_at_position(src, &pos(0, 10)), Some("bar".to_string()));
    }

    #[test]
    fn test_word_at_position_unicode_delimiter_no_panic() {
        // '─' is U+2500, 3 bytes in UTF-8 — this triggered the original crash
        let src = "        // ── Emulator ZIP (Windows + Linux/crosstool-ng)\n        let archive = foo";
        // Second line: "        let archive = foo"
        //               0       8   12      20 22
        // 'f' in 'foo' is at UTF-16 col 22
        assert_eq!(word_at_position(src, &pos(1, 22)), Some("foo".to_string()));
        // Cursor inside the box-drawing comment line must not panic
        let _ = word_at_position(src, &pos(0, 15));
    }

    #[test]
    fn test_word_at_position_utf16_column() {
        // '─' is U+2500: 3 bytes UTF-8, 1 UTF-16 code unit
        // Line: "─ hello", cursor at UTF-16 col 2 (after '─ ') → "hello"
        let src = "─ hello";
        assert_eq!(word_at_position(src, &pos(0, 2)), Some("hello".to_string()));
    }

    #[test]
    fn test_word_at_position_out_of_bounds() {
        let src = "let x = 1";
        assert_eq!(word_at_position(src, &pos(5, 0)), None);
        assert_eq!(word_at_position(src, &pos(0, 100)), None);
    }
}
