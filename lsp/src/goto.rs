/// Go-to-definition — find the definition site of a symbol in the same document.

use crate::analysis;
use tower_lsp::lsp_types::*;

/// Find the definition of the symbol at the given position.
pub fn goto_definition(
    uri: &Url,
    source: &str,
    position: &Position,
) -> Option<GotoDefinitionResponse> {
    let word = analysis::word_at_position(source, position)?;
    let result = analysis::analyze(source);

    // Search symbols for a matching name
    for sym in &result.symbols {
        if sym.name == word {
            let range = analysis::span_to_range(&sym.span);
            return Some(GotoDefinitionResponse::Scalar(Location {
                uri: uri.clone(),
                range,
            }));
        }
        // Also check children (e.g. enum variants, params)
        for child in &sym.children {
            if child.name == word {
                let range = analysis::span_to_range(&child.span);
                return Some(GotoDefinitionResponse::Scalar(Location {
                    uri: uri.clone(),
                    range,
                }));
            }
        }
    }

    // Fallback: scan source for variable/function definitions matching the word
    if let Some(location) = find_definition_in_source(uri, source, &word) {
        return Some(GotoDefinitionResponse::Scalar(location));
    }

    None
}

/// Scan the raw source for common definition patterns.
fn find_definition_in_source(uri: &Url, source: &str, name: &str) -> Option<Location> {
    for (line_idx, line) in source.lines().enumerate() {
        let trimmed = line.trim_start();

        // let name = ...
        // mut name = ...
        if let Some(rest) = trimmed.strip_prefix("let ").or_else(|| trimmed.strip_prefix("mut ")) {
            if rest.starts_with(name)
                && rest[name.len()..]
                    .chars()
                    .next()
                    .map_or(false, |c| !c.is_alphanumeric() && c != '_')
            {
                let col = line.find(name).unwrap_or(0) as u32;
                return Some(Location {
                    uri: uri.clone(),
                    range: Range::new(
                        Position::new(line_idx as u32, col),
                        Position::new(line_idx as u32, col + name.len() as u32),
                    ),
                });
            }
        }

        // fn name(...)
        if let Some(rest) = trimmed.strip_prefix("fn ") {
            if rest.starts_with(name)
                && rest[name.len()..]
                    .chars()
                    .next()
                    .map_or(false, |c| c == '(' || c == ' ' || c == '<')
            {
                let col = line.find(name).unwrap_or(0) as u32;
                return Some(Location {
                    uri: uri.clone(),
                    range: Range::new(
                        Position::new(line_idx as u32, col),
                        Position::new(line_idx as u32, col + name.len() as u32),
                    ),
                });
            }
        }

        // task name { ...
        if let Some(rest) = trimmed.strip_prefix("task ") {
            if rest.starts_with(name)
                && rest[name.len()..]
                    .chars()
                    .next()
                    .map_or(false, |c| c == '{' || c == ' ' || c == '(')
            {
                let col = line.find(name).unwrap_or(0) as u32;
                return Some(Location {
                    uri: uri.clone(),
                    range: Range::new(
                        Position::new(line_idx as u32, col),
                        Position::new(line_idx as u32, col + name.len() as u32),
                    ),
                });
            }
        }

        // type name = ...
        if let Some(rest) = trimmed.strip_prefix("type ") {
            if rest.starts_with(name)
                && rest[name.len()..]
                    .chars()
                    .next()
                    .map_or(false, |c| c == '=' || c == ' ' || c == '<')
            {
                let col = line.find(name).unwrap_or(0) as u32;
                return Some(Location {
                    uri: uri.clone(),
                    range: Range::new(
                        Position::new(line_idx as u32, col),
                        Position::new(line_idx as u32, col + name.len() as u32),
                    ),
                });
            }
        }

        // enum name { ...
        if let Some(rest) = trimmed.strip_prefix("enum ") {
            if rest.starts_with(name)
                && rest[name.len()..]
                    .chars()
                    .next()
                    .map_or(false, |c| c == '{' || c == ' ')
            {
                let col = line.find(name).unwrap_or(0) as u32;
                return Some(Location {
                    uri: uri.clone(),
                    range: Range::new(
                        Position::new(line_idx as u32, col),
                        Position::new(line_idx as u32, col + name.len() as u32),
                    ),
                });
            }
        }

        // Also handle pub fn, pub task, etc.
        if let Some(rest) = trimmed.strip_prefix("pub ") {
            let rest_trimmed = rest.trim_start();
            for prefix in &["fn ", "task ", "type ", "enum "] {
                if let Some(after) = rest_trimmed.strip_prefix(prefix) {
                    if after.starts_with(name) {
                        let col = line.find(name).unwrap_or(0) as u32;
                        return Some(Location {
                            uri: uri.clone(),
                            range: Range::new(
                                Position::new(line_idx as u32, col),
                                Position::new(line_idx as u32, col + name.len() as u32),
                            ),
                        });
                    }
                }
            }
        }
    }

    None
}
