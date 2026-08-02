/// Document symbol provider — produces the outline tree shown in the
/// breadcrumb bar and the Outline panel.

use crate::analysis;
use tower_lsp::lsp_types::*;

/// Compute document symbols (outline) for a Que source file.
#[allow(deprecated)] // DocumentSymbol::deprecated is deprecated but required by the struct
pub fn document_symbols(source: &str) -> Vec<DocumentSymbol> {
    let result = analysis::analyze(source);
    result
        .symbols
        .iter()
        .map(|sym| symbol_to_doc_symbol(sym, source))
        .collect()
}

#[allow(deprecated)]
fn symbol_to_doc_symbol(sym: &analysis::SymbolInfo, source: &str) -> DocumentSymbol {
    let kind = match sym.kind {
        analysis::SymbolKind::Function => SymbolKind::FUNCTION,
        analysis::SymbolKind::Task => SymbolKind::CLASS,
        analysis::SymbolKind::Variable => SymbolKind::VARIABLE,
        analysis::SymbolKind::Parameter => SymbolKind::VARIABLE,
        analysis::SymbolKind::Type => SymbolKind::CLASS,
        analysis::SymbolKind::Enum => SymbolKind::ENUM,
        analysis::SymbolKind::EnumVariant => SymbolKind::ENUM_MEMBER,
        analysis::SymbolKind::Struct => SymbolKind::STRUCT,
        analysis::SymbolKind::StructField => SymbolKind::FIELD,
        analysis::SymbolKind::Trait => SymbolKind::INTERFACE,
        analysis::SymbolKind::Impl => SymbolKind::CLASS,
        analysis::SymbolKind::Module => SymbolKind::MODULE,
    };

    // Try to find the symbol in source to get a proper range
    let range = find_symbol_range(source, &sym.name, sym.kind)
        .unwrap_or_else(|| analysis::span_to_range(&sym.span));

    let children = if sym.children.is_empty() {
        None
    } else {
        Some(
            sym.children
                .iter()
                .map(|c| symbol_to_doc_symbol(c, source))
                .collect(),
        )
    };

    DocumentSymbol {
        name: sym.name.clone(),
        detail: sym.detail.clone(),
        kind,
        tags: None,
        deprecated: None,
        range,
        selection_range: range,
        children,
    }
}

/// Search source text for the definition of a named symbol.
fn find_symbol_range(
    source: &str,
    name: &str,
    kind: analysis::SymbolKind,
) -> Option<Range> {
    let prefix = match kind {
        analysis::SymbolKind::Function => "fn ",
        analysis::SymbolKind::Task => "task ",
        analysis::SymbolKind::Type => "type ",
        analysis::SymbolKind::Enum => "enum ",
        analysis::SymbolKind::Variable => "let ",
        _ => return None,
    };

    for (line_idx, line) in source.lines().enumerate() {
        let trimmed = line.trim_start();
        // Handle pub prefix
        let check = trimmed
            .strip_prefix("pub ")
            .unwrap_or(trimmed);
        if let Some(rest) = check.strip_prefix(prefix) {
            if rest.starts_with(name) {
                let col = line.find(name).unwrap_or(0) as u32;
                let line_u32 = line_idx as u32;
                return Some(Range::new(
                    Position::new(line_u32, col),
                    Position::new(line_u32, col + name.len() as u32),
                ));
            }
        }
        // Also check "mut " for variable kind
        if kind == analysis::SymbolKind::Variable {
            if let Some(rest) = trimmed.strip_prefix("mut ") {
                if rest.starts_with(name) {
                    let col = line.find(name).unwrap_or(0) as u32;
                    let line_u32 = line_idx as u32;
                    return Some(Range::new(
                        Position::new(line_u32, col),
                        Position::new(line_u32, col + name.len() as u32),
                    ));
                }
            }
        }
    }
    None
}
