/// Hover provider — returns documentation for the symbol under the cursor.

use crate::analysis;
use crate::builtins;
use tower_lsp::lsp_types::*;

/// Compute hover information for the word at the given position.
pub fn hover(source: &str, position: &Position) -> Option<Hover> {
    let word = analysis::word_at_position(source, position)?;

    // 1. Check keywords
    if let Some(doc) = builtins::keyword_doc(&word) {
        return Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: doc.to_string(),
            }),
            range: None,
        });
    }

    // 2. Check built-in functions
    for bi in builtins::builtin_functions() {
        if bi.name == word {
            let md = format!(
                "```que\n{}\n```\n\n{}",
                bi.signature, bi.documentation
            );
            return Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: md,
                }),
                range: None,
            });
        }
    }

    // 3. Check types
    if builtins::TYPES.contains(&word.as_str()) {
        let doc = type_documentation(&word);
        return Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: doc,
            }),
            range: None,
        });
    }

    // 3.5. Check methods (type methods like .head(), .collect(), .enumerate_lines(), etc.)
    if let Some(doc) = builtins::method_doc(&word) {
        return Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: doc,
            }),
            range: None,
        });
    }

    // 4. Check user-defined symbols
    let result = analysis::analyze(source);
    for sym in &result.symbols {
        if sym.name == word {
            let detail = sym
                .detail
                .as_deref()
                .unwrap_or(&sym.name);
            let kind = match sym.kind {
                analysis::SymbolKind::Function => "function",
                analysis::SymbolKind::Task => "task",
                analysis::SymbolKind::Variable => "variable",
                analysis::SymbolKind::Parameter => "parameter",
                analysis::SymbolKind::Type => "type",
                analysis::SymbolKind::Enum => "enum",
                analysis::SymbolKind::EnumVariant => "variant",
                analysis::SymbolKind::Struct => "struct",
                analysis::SymbolKind::StructField => "field",
                analysis::SymbolKind::Trait => "trait",
                analysis::SymbolKind::Impl => "impl",
                analysis::SymbolKind::Module => "module",
            };
            let md = format!("```que\n{}\n```\n\n*{}*", detail, kind);
            return Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: md,
                }),
                range: None,
            });
        }
    }

    None
}

fn type_documentation(type_name: &str) -> String {
    match type_name {
        "Int" => "**Int** — 64-bit signed integer.\n\n```que\nlet x: Int = 42\n```".to_string(),
        "Float" => "**Float** — 64-bit IEEE 754 floating point.\n\n```que\nlet pi: Float = 3.14\n```".to_string(),
        "Bool" => "**Bool** — Boolean value (`true` or `false`).\n\n```que\nlet flag: Bool = true\n```".to_string(),
        "String" => "**String** — UTF-8 immutable string.\n\n```que\nlet s: String = \"hello\"\n```\n\nMethods: `.len()`, `.to_upper()`, `.to_lower()`, `.trim()`, `.split()`, `.contains()`, `.replace()`, `.starts_with()`, `.ends_with()`, `.lines()`".to_string(),
        "Bytes" => "**Bytes** — Raw byte buffer.".to_string(),
        "Path" => "**Path** — Typed filesystem path (not a bare string).\n\n```que\nlet p = path(\"./src\")\nlet full = p / \"main.rs\"\n```\n\nMethods: `.exists()`, `.is_file()`, `.is_dir()`, `.read()`, `.write()`, `.parent()`, `.file_name()`, `.extension()`, `.mkdir()`, `.ls()`".to_string(),
        "Glob" => "**Glob** — Typed glob pattern for file matching.\n\n```que\nlet g = glob(\"src/**/*.rs\")\n```".to_string(),
        "Cmd" => "**Cmd** — Unevaluated command literal.\n\n```que\nlet c = `git status`\nlet result = c.run()\n```".to_string(),
        "Duration" => "**Duration** — Time span with units.\n\n```que\nlet t = 5s       // 5 seconds\nlet d = 500ms    // 500 milliseconds\n```".to_string(),
        "Timestamp" => "**Timestamp** — UTC instant in time.\n\n```que\nlet t = now()\n```".to_string(),
        "Regex" => "**Regex** — Compiled regular expression.\n\n```que\nlet r = re\"^\\d{3}-\\d{4}$\"\n```".to_string(),
        "Semver" => "**Semver** — Semantic version.\n\n```que\nlet v = v\"1.2.3\"\n```".to_string(),
        "Secret" => "**Secret** — Redacted-by-default sensitive string.\n\n```que\nlet token = env.secret(\"API_TOKEN\").unwrap()\n`curl -H \"Authorization: Bearer ${token}\" $url`\n```\n\nInterpolating into a command sends the real value but shows `<redacted>` in dry-run and failure output. Methods: `.expose()`, `.len()`".to_string(),
        "List" => "**List\\<T\\>** — Ordered collection.\n\n```que\nlet xs = [1, 2, 3]\n```\n\nMethods: `.len()`, `.push()`, `.pop()`, `.first()`, `.last()`, `.map()`, `.filter()`, `.fold()`, `.sort()`, `.contains()`, `.reverse()`".to_string(),
        "Map" => "**Map\\<K, V\\>** — Key-value map.\n\n```que\nlet m = {\"name\": \"que\", \"version\": 1}\n```\n\nMethods: `.len()`, `.keys()`, `.values()`, `.entries()`, `.get()`, `.set()`, `.remove()`, `.contains_key()`, `.merge()`".to_string(),
        "Set" => "**Set\\<T\\>** — Unique collection (insertion-order preserved).\n\n```que\nlet s = #{1, 2, 3}\n```\n\nMethods: `.len()`, `.contains()`, `.add()`, `.remove()`, `.union()`, `.intersection()`, `.difference()`".to_string(),
        "Option" => "**Option** — removed. Use `null` with `??` and `?.` instead.".to_string(),
        "Result" => "**Result\\<T, E\\>** = `Ok(T)` | `Err(E)`\n\nRepresents a failable computation.".to_string(),
        "Any" => "**Any** — The top type; matches any value.".to_string(),
        "Null" => "**Null** — The unit/void type; only value is `null`.".to_string(),
        _ => format!("**{}**", type_name),
    }
}
