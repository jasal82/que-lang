/// Completion provider — supplies keyword, built-in function, type, and
/// context-aware completions.
///
/// The provider detects the cursor context (inside `@deps([...])`, at the
/// start of a task attribute, inside a string or comment, etc.) and narrows
/// the suggestions accordingly.

use crate::analysis;
use crate::builtins::{self, BuiltinKind};
use tower_lsp::lsp_types::*;

// ── Main entry point ────────────────────────────────────────────────────────

/// Compute completions for a given document position.
pub fn completions(source: &str, position: &Position) -> Vec<CompletionItem> {
    let mut items = Vec::new();

    let prefix = current_word_prefix(source, position);
    let lines: Vec<&str> = source.lines().collect();
    let line_idx = position.line as usize;
    let col = position.character as usize;
    let line_text = lines.get(line_idx).copied().unwrap_or("");
    let before_cursor = if col <= line_text.len() {
        &line_text[..col]
    } else {
        line_text
    };

    // ── Suppress completions inside comments and string literals ─────────
    if is_in_comment(before_cursor) || is_in_string_literal(before_cursor) {
        return items;
    }

    // ── Context-specific completion branches ─────────────────────────────

    if before_cursor.trim_end().ends_with('.')
        || (before_cursor.contains('.') && !before_cursor.ends_with(' '))
    {
        // Method completion — try to infer receiver type
        items.extend(method_completions(before_cursor));
    } else if before_cursor.trim_start().starts_with("import") {
        items.extend(import_completions());
    } else if is_in_deps_attribute(before_cursor) {
        // Inside @deps([...]) — only suggest task names from this file
        items.extend(task_name_completions(source));
    } else if is_at_attribute_position(before_cursor) {
        // Typing `@` above a task — suggest the attributes it can carry
        items.extend(task_attribute_completions());
    } else {
        // General completions: keywords + builtins + types + local symbols
        items.extend(keyword_completions());
        items.extend(builtin_function_completions());
        items.extend(type_completions());
        items.extend(local_symbol_completions(source));
        items.extend(snippet_completions());
    }

    // Filter by prefix if there is one
    if !prefix.is_empty() {
        items.retain(|item| {
            item.label
                .to_lowercase()
                .starts_with(&prefix.to_lowercase())
        });
    }

    items
}

// ── Context detection helpers ───────────────────────────────────────────────

/// Check if the cursor is inside a `// …` line comment.
fn is_in_comment(before_cursor: &str) -> bool {
    let mut in_string = false;
    let mut prev = '\0';
    for ch in before_cursor.chars() {
        if !in_string && ch == '/' && prev == '/' {
            return true;
        }
        if ch == '"' && prev != '\\' {
            in_string = !in_string;
        }
        prev = ch;
    }
    false
}

/// Check if the cursor is inside a string literal (unclosed `"`),
/// but **not** inside a `${…}` interpolation expression within that string.
fn is_in_string_literal(before_cursor: &str) -> bool {
    let mut in_string = false;
    let mut escape = false;
    // Stack depth for nested `${…}` interpolations inside a string.
    // When depth > 0 we are inside an interpolation expression, not plain string text.
    let mut interp_depth: usize = 0;
    let mut prev = '\0';

    for ch in before_cursor.chars() {
        if escape {
            escape = false;
            prev = ch;
            continue;
        }

        if in_string && interp_depth == 0 {
            // Inside string text (not inside an interpolation)
            if ch == '\\' {
                escape = true;
                prev = ch;
                continue;
            }
            if ch == '"' {
                in_string = false;
                prev = ch;
                continue;
            }
            if ch == '{' && prev == '$' {
                // Entering a ${…} interpolation
                interp_depth += 1;
                prev = ch;
                continue;
            }
        } else if in_string && interp_depth > 0 {
            // Inside an interpolation expression within a string
            if ch == '{' {
                interp_depth += 1;
            } else if ch == '}' {
                interp_depth -= 1;
            } else if ch == '"' {
                // Nested string inside interpolation — toggle tracking.
                // For simplicity we treat it as a new string context.
                // This handles cases like "outer ${if x "inner" else "other"} rest".
            }
            prev = ch;
            continue;
        } else {
            // Outside any string
            if ch == '"' {
                in_string = true;
                prev = ch;
                continue;
            }
        }

        prev = ch;
    }

    // We are "in a string" only if in_string is true AND we are not inside
    // an interpolation expression (interp_depth == 0).
    in_string && interp_depth == 0
}

/// Check if the cursor is inside `@deps([ … ])`.  Attributes are written on a
/// line of their own, so looking at the current line alone is enough.
fn is_in_deps_attribute(before_cursor: &str) -> bool {
    match before_cursor.rfind("@deps(") {
        Some(pos) => !before_cursor[pos..].contains(')'),
        None => false,
    }
}

/// Check if the cursor sits right after the `@` that opens an attribute, i.e.
/// the attribute's name has not been closed off by its parenthesis yet.
fn is_at_attribute_position(before_cursor: &str) -> bool {
    let trimmed = before_cursor.trim_start();
    trimmed.starts_with('@') && !trimmed.contains('(')
}

// ── Prefix extraction ───────────────────────────────────────────────────────

/// Extract the word prefix being typed at the cursor position.
fn current_word_prefix(source: &str, position: &Position) -> String {
    let line = source.lines().nth(position.line as usize).unwrap_or("");
    let col = position.character as usize;
    if col > line.len() {
        return String::new();
    }
    let before = &line[..col];
    let start = before
        .rfind(|c: char| !c.is_alphanumeric() && c != '_')
        .map(|i| i + 1)
        .unwrap_or(0);
    before[start..].to_string()
}

// ── Completion generators ───────────────────────────────────────────────────

fn keyword_completions() -> Vec<CompletionItem> {
    builtins::KEYWORDS
        .iter()
        .map(|kw| CompletionItem {
            label: kw.to_string(),
            kind: Some(CompletionItemKind::KEYWORD),
            detail: Some("keyword".to_string()),
            ..Default::default()
        })
        .collect()
}

fn builtin_function_completions() -> Vec<CompletionItem> {
    builtins::builtin_functions()
        .into_iter()
        .map(|bi| CompletionItem {
            label: bi.name.to_string(),
            kind: Some(match bi.kind {
                BuiltinKind::Function => CompletionItemKind::FUNCTION,
                BuiltinKind::Keyword => CompletionItemKind::KEYWORD,
                BuiltinKind::Type => CompletionItemKind::CLASS,
                BuiltinKind::Constant => CompletionItemKind::CONSTANT,
                BuiltinKind::Namespace => CompletionItemKind::MODULE,
            }),
            detail: Some(bi.signature.to_string()),
            documentation: Some(Documentation::MarkupContent(MarkupContent {
                kind: MarkupKind::Markdown,
                value: bi.documentation.to_string(),
            })),
            insert_text: Some(format!("{}($0)", bi.name)),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            ..Default::default()
        })
        .collect()
}

fn type_completions() -> Vec<CompletionItem> {
    builtins::TYPES
        .iter()
        .map(|ty| CompletionItem {
            label: ty.to_string(),
            kind: Some(CompletionItemKind::CLASS),
            detail: Some("type".to_string()),
            ..Default::default()
        })
        .collect()
}

fn local_symbol_completions(source: &str) -> Vec<CompletionItem> {
    let result = analysis::analyze(source);
    result
        .symbols
        .iter()
        .map(|sym| {
            let kind = match sym.kind {
                analysis::SymbolKind::Function => CompletionItemKind::FUNCTION,
                analysis::SymbolKind::Task => CompletionItemKind::CLASS,
                analysis::SymbolKind::Variable => CompletionItemKind::VARIABLE,
                analysis::SymbolKind::Parameter => CompletionItemKind::VARIABLE,
                analysis::SymbolKind::Type => CompletionItemKind::CLASS,
                analysis::SymbolKind::Enum => CompletionItemKind::ENUM,
                analysis::SymbolKind::EnumVariant => CompletionItemKind::ENUM_MEMBER,
                analysis::SymbolKind::Struct => CompletionItemKind::STRUCT,
                analysis::SymbolKind::StructField => CompletionItemKind::FIELD,
                analysis::SymbolKind::Trait => CompletionItemKind::INTERFACE,
                analysis::SymbolKind::Impl => CompletionItemKind::CLASS,
                analysis::SymbolKind::Module => CompletionItemKind::MODULE,
            };
            CompletionItem {
                label: sym.name.clone(),
                kind: Some(kind),
                detail: sym.detail.clone(),
                ..Default::default()
            }
        })
        .collect()
}

/// Completions when the cursor is inside a `deps [...]` list — only
/// suggest task names defined in the same file.
///
/// Because the user is in the middle of editing (the `[` is likely unclosed),
/// the parser may fail.  We therefore fall back to a line-based scan for
/// `task <name>` when the parser doesn't return any task symbols.
fn task_name_completions(source: &str) -> Vec<CompletionItem> {
    let result = analysis::analyze(source);
    let mut names: Vec<String> = result
        .symbols
        .iter()
        .filter(|sym| sym.kind == analysis::SymbolKind::Task)
        .map(|sym| sym.name.clone())
        .collect();

    // Fallback: simple line scan for `task <name>` when the parser failed
    if names.is_empty() {
        for line in source.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("task ") {
                if let Some(name) = rest
                    .split(|c: char| !c.is_alphanumeric() && c != '_')
                    .next()
                    .filter(|n| !n.is_empty())
                {
                    let name = name.to_string();
                    if !names.contains(&name) {
                        names.push(name);
                    }
                }
            }
        }
    }

    names
        .into_iter()
        .map(|name| CompletionItem {
            label: name,
            kind: Some(CompletionItemKind::EVENT),
            detail: Some("task".to_string()),
            documentation: Some(Documentation::String("Task dependency".to_string())),
            ..Default::default()
        })
        .collect()
}

/// Completions for the `@...` attributes written above a task.
fn task_attribute_completions() -> Vec<CompletionItem> {
    let attrs = [
        ("description", "Task description string", "description(\"$0\")"),
        ("deps", "Tasks that must run first", "deps([$0])"),
        ("inputs", "Task input files / glob patterns", "inputs([$0])"),
        ("outputs", "Task output files / paths", "outputs([$0])"),
        ("aliases", "Alternative names for the task", "aliases([$0])"),
        ("env", "Required environment variables", "env([$0])"),
    ];
    attrs
        .iter()
        .enumerate()
        .map(|(i, (label, detail, snippet))| CompletionItem {
            label: label.to_string(),
            kind: Some(CompletionItemKind::PROPERTY),
            detail: Some(detail.to_string()),
            insert_text: Some(snippet.to_string()),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            sort_text: Some(format!("{}_{}", i, label)),
            ..Default::default()
        })
        .collect()
}

// ── Method completions with type inference ───────────────────────────────────

/// Provide method completions, attempting to narrow by receiver type.
fn method_completions(before_cursor: &str) -> Vec<CompletionItem> {
    if let Some(ty) = infer_receiver_type(before_cursor) {
        // We inferred the type — offer only its methods.
        builtins::methods_for_type(ty)
            .into_iter()
            .map(|(method, sig)| CompletionItem {
                label: method.to_string(),
                kind: Some(CompletionItemKind::METHOD),
                detail: Some(format!("{}{}", method, sig)),
                ..Default::default()
            })
            .collect()
    } else {
        // Fallback: offer all methods from all common types.
        let all_types = ["String", "List", "Map", "Set", "Path", "Stream"];
        let mut seen = std::collections::HashSet::new();
        let mut items = Vec::new();
        for ty in all_types {
            for (method, sig) in builtins::methods_for_type(ty) {
                if seen.insert(method) {
                    items.push(CompletionItem {
                        label: method.to_string(),
                        kind: Some(CompletionItemKind::METHOD),
                        detail: Some(format!("{}{}", method, sig)),
                        ..Default::default()
                    });
                }
            }
        }
        items
    }
}

/// Try to infer the receiver type from the expression before the `.`.
fn infer_receiver_type(before_cursor: &str) -> Option<&'static str> {
    let dot_pos = before_cursor.rfind('.')?;
    let before_dot = before_cursor[..dot_pos].trim_end();

    // String literal: …"
    if before_dot.ends_with('"') {
        return Some("String");
    }

    // List literal: …]  (but not string indexing like …"])
    if before_dot.ends_with(']') && !before_dot.ends_with("\"]") {
        return Some("List");
    }

    // Function / method call: …)
    if before_dot.ends_with(')') {
        if let Some(name) = last_call_name(before_dot) {
            return type_from_call_name(name);
        }
    }

    None
}

/// Walk backwards through `s` (which ends with `)`) to find the matching `(`
/// and return the identifier immediately before it.
fn last_call_name(s: &str) -> Option<&str> {
    let bytes = s.as_bytes();
    let mut depth: i32 = 0;
    let mut i = bytes.len();
    while i > 0 {
        i -= 1;
        match bytes[i] {
            b')' => depth += 1,
            b'(' => {
                depth -= 1;
                if depth == 0 {
                    let before = s[..i].trim_end();
                    let name_start = before
                        .rfind(|c: char| !c.is_alphanumeric() && c != '_')
                        .map(|p| p + 1)
                        .unwrap_or(0);
                    let name = &before[name_start..];
                    if name.is_empty() {
                        return None;
                    }
                    return Some(name);
                }
            }
            _ => {}
        }
    }
    None
}

/// Map a function / method name to the type it is known to return.
fn type_from_call_name(name: &str) -> Option<&'static str> {
    match name {
        // Constructor functions
        "path" => Some("Path"),
        "glob" => Some("Glob"),
        "stream" => Some("Stream"),
        "str" | "string" => Some("String"),

        // Methods → List
        "split" | "lines" | "chars" | "keys" | "values" | "entries"
        | "expand" | "ls" | "to_list" | "sort" | "reverse" | "filter"
        | "map" | "take" | "skip" | "dedup" | "flatten" | "chunk"
        | "zip" | "enumerate" | "push" | "sort_by" | "slice"
        | "flat_map" | "matches" | "partition" => Some("List"),

        // Methods → String
        "trim" | "trim_start" | "trim_end" | "to_upper" | "to_lower"
        | "replace" | "join" | "read" | "collect" | "to_string"
        | "to_json" | "to_yaml" | "to_toml" | "file_name" | "extension"
        | "stem" | "pad_left" | "pad_right" | "repeat" | "name" => Some("String"),

        // Methods → Path
        "parent" | "mkdir" | "copy_to" | "move_to" | "to_absolute" => Some("Path"),

        // Methods → Map
        "group_by" | "merge" => Some("Map"),

        // Methods → Set
        "union" | "intersection" | "difference" | "symmetric_difference" => Some("Set"),

        // Methods → Stream
        "grep" | "head" | "tail" | "uniq" | "numbered" | "skip_empty"
        | "filter_lines" | "map_lines" | "words" | "upper" | "lower" => Some("Stream"),

        _ => None,
    }
}

// ── Other completion kinds ──────────────────────────────────────────────────

fn import_completions() -> Vec<CompletionItem> {
    // Derive the module list from the shared builtin registry so a new std
    // module shows up here the day it is documented, instead of drifting.
    let mut modules: Vec<String> = builtins::builtin_functions()
        .into_iter()
        .filter_map(|bi| bi.name.split_once('.').map(|(m, _)| m.to_string()))
        .collect();
    modules.sort();
    modules.dedup();

    modules
        .into_iter()
        .map(|name| CompletionItem {
            label: format!("std.{name}"),
            kind: Some(CompletionItemKind::MODULE),
            detail: Some(format!("standard library module `{name}`")),
            ..Default::default()
        })
        .collect()
}

fn snippet_completions() -> Vec<CompletionItem> {
    vec![
        CompletionItem {
            label: "fn".to_string(),
            kind: Some(CompletionItemKind::SNIPPET),
            detail: Some("Function declaration".to_string()),
            insert_text: Some("fn ${1:name}(${2:params}) {\n\t$0\n}".to_string()),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            ..Default::default()
        },
        CompletionItem {
            label: "task".to_string(),
            kind: Some(CompletionItemKind::SNIPPET),
            detail: Some("Task declaration".to_string()),
            insert_text: Some(
                "@description(\"${2:description}\")\ntask ${1:name} {\n\t$0\n}"
                    .to_string(),
            ),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            ..Default::default()
        },
        CompletionItem {
            label: "if".to_string(),
            kind: Some(CompletionItemKind::SNIPPET),
            detail: Some("If expression".to_string()),
            insert_text: Some("if ${1:condition} {\n\t$0\n}".to_string()),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            ..Default::default()
        },
        CompletionItem {
            label: "for".to_string(),
            kind: Some(CompletionItemKind::SNIPPET),
            detail: Some("For loop".to_string()),
            insert_text: Some("for ${1:item} in ${2:collection} {\n\t$0\n}".to_string()),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            ..Default::default()
        },
        CompletionItem {
            label: "match".to_string(),
            kind: Some(CompletionItemKind::SNIPPET),
            detail: Some("Match expression".to_string()),
            insert_text: Some("match ${1:value} {\n\t${2:pattern} => ${3:expr}\n\t_ => ${0:expr}\n}".to_string()),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            ..Default::default()
        },
        CompletionItem {
            label: "try".to_string(),
            kind: Some(CompletionItemKind::SNIPPET),
            detail: Some("Try-catch block".to_string()),
            insert_text: Some(
                "try {\n\t${1:// risky operation}\n} catch ${2:e} {\n\t$0\n}".to_string(),
            ),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            ..Default::default()
        },

        CompletionItem {
            label: "import".to_string(),
            kind: Some(CompletionItemKind::SNIPPET),
            detail: Some("Import declaration".to_string()),
            insert_text: Some("import ${0:.module}".to_string()),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            ..Default::default()
        },
    ]
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(items: &[CompletionItem]) -> Vec<String> {
        items.iter().map(|i| i.label.clone()).collect()
    }

    #[test]
    fn test_depends_on_only_tasks() {
        let src = "task clean { null }\ntask prepare { null }\n@deps([";
        // Cursor is at the end of the last line (inside the deps list)
        let line = src.lines().count() - 1;
        let col = src.lines().last().unwrap().len();
        let pos = Position::new(line as u32, col as u32);

        let items = completions(src, &pos);
        let names = labels(&items);
        // Should only contain task names, not keywords/builtins
        assert!(names.contains(&"clean".to_string()));
        assert!(names.contains(&"prepare".to_string()));
        assert!(!names.contains(&"let".to_string()));
        assert!(!names.contains(&"println".to_string()));
    }

    #[test]
    fn test_no_completions_in_comment() {
        let src = "// some comm";
        let pos = Position::new(0, src.len() as u32);
        let items = completions(src, &pos);
        assert!(items.is_empty());
    }

    #[test]
    fn test_no_completions_in_string() {
        let src = r#"let x = "hel"#;
        let pos = Position::new(0, src.len() as u32);
        let items = completions(src, &pos);
        assert!(items.is_empty());
    }

    #[test]
    fn test_task_attribute_completions() {
        let src = "@";
        let pos = Position::new(0, 1);
        let items = completions(src, &pos);
        let names = labels(&items);
        assert!(names.contains(&"description".to_string()));
        assert!(names.contains(&"inputs".to_string()));
        assert!(names.contains(&"outputs".to_string()));
        assert!(names.contains(&"deps".to_string()));
        // Should NOT contain general keywords
        assert!(!names.contains(&"let".to_string()));
        assert!(!names.contains(&"fn".to_string()));
    }

    #[test]
    fn test_method_completions_path() {
        let src = r#"path("./build")."#;
        let pos = Position::new(0, src.len() as u32);
        let items = completions(src, &pos);
        let names = labels(&items);
        assert!(names.contains(&"exists".to_string()));
        assert!(names.contains(&"is_dir".to_string()));
        // Should NOT contain String-only methods
        assert!(!names.contains(&"to_upper".to_string()));
    }

    #[test]
    fn test_method_completions_string() {
        let src = r#""hello"."#;
        let pos = Position::new(0, src.len() as u32);
        let items = completions(src, &pos);
        let names = labels(&items);
        assert!(names.contains(&"to_upper".to_string()));
        assert!(names.contains(&"split".to_string()));
        // Should NOT contain Path-only methods
        assert!(!names.contains(&"is_dir".to_string()));
    }

    #[test]
    fn test_method_completions_chained() {
        // stream(...).to_upper(). should infer String from to_upper
        let src = r#"stream(path("./f")).to_upper()."#;
        let pos = Position::new(0, src.len() as u32);
        let items = completions(src, &pos);
        let names = labels(&items);
        assert!(names.contains(&"split".to_string()));
        assert!(!names.contains(&"is_dir".to_string()));
    }
}
