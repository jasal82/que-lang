//! Tab completion for the Que REPL.
//!
//! Provides [`QueHelper`], a `rustyline::Helper` implementation that
//! completes identifiers, builtins, keywords, std-module functions,
//! and REPL meta-commands.

use std::borrow::Cow;
use std::cell::RefCell;
use std::rc::Rc;

use colored::Colorize;
use rustyline::completion::{Completer, Pair};
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::validate::Validator;
use rustyline::{Context, Helper};

use crate::interpreter::Interpreter;
use crate::value::Value;

/// Prompt shown for the first line of an entry.
pub const PROMPT: &str = "que> ";
/// Prompt shown while an entry is still incomplete (unclosed bracket, ...).
pub const CONTINUATION_PROMPT: &str = "  ...> ";

/// Rustyline helper that shares the live `Interpreter` so it can
/// complete variable names and known type/method names dynamically.
pub struct QueHelper {
    interp: Rc<RefCell<Interpreter>>,
}

impl QueHelper {
    pub fn new(interp: Rc<RefCell<Interpreter>>) -> Self {
        Self { interp }
    }
}

impl Helper for QueHelper {}

impl Hinter for QueHelper {
    type Hint = String;
}

impl Highlighter for QueHelper {
    /// Colour the prompt so it stands out from the surrounding output.
    ///
    /// Rustyline measures the cursor position from the *uncoloured* prompt it
    /// was given, so the escape sequences added here do not shift the cursor.
    fn highlight_prompt<'b, 's: 'b, 'p: 'b>(
        &'s self,
        prompt: &'p str,
        _default: bool,
    ) -> Cow<'b, str> {
        if prompt == CONTINUATION_PROMPT {
            Cow::Owned(prompt.yellow().bold().to_string())
        } else if prompt == PROMPT {
            Cow::Owned(prompt.cyan().bold().to_string())
        } else {
            Cow::Borrowed(prompt)
        }
    }

    fn highlight_candidate<'c>(
        &self,
        candidate: &'c str,
        _completion: rustyline::CompletionType,
    ) -> Cow<'c, str> {
        Cow::Owned(candidate.green().to_string())
    }
}

impl Validator for QueHelper {}

impl Completer for QueHelper {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Pair>)> {
        Ok(self.compute(line, pos))
    }
}

impl QueHelper {
    fn compute(&self, line: &str, pos: usize) -> (usize, Vec<Pair>) {
        // ── Meta commands: `:` at the start of the (trimmed) line ──
        let leading_ws = line.len() - line.trim_start().len();
        let trimmed = line.trim_start();

        if trimmed.starts_with(':') && pos >= leading_ws {
            // Word covers the contiguous `:cmd` token.
            let token_start = leading_ws;
            let token_end = line[token_start..]
                .find(|c: char| c.is_whitespace())
                .map(|i| token_start + i)
                .unwrap_or(line.len());
            if pos <= token_end {
                let prefix = &line[token_start + 1..pos];
                let cmds: &[&str] = &[
                    "h", "help", "t", "type", "m", "methods", "i", "inspect",
                    "v", "vars", "load", "reset", "r", "q", "quit", "exit",
                ];
                let candidates = cmds
                    .iter()
                    .filter(|c| c.starts_with(prefix))
                    .map(|c| Pair {
                        display: format!(":{}", c),
                        replacement: format!(":{}", c),
                    })
                    .collect();
                return (token_start, candidates);
            }
        }

        // ── Word boundary scan for the rest ──
        // A "word" is a run of [A-Za-z0-9_.] possibly preceded by `?`.
        let bytes = line.as_bytes();
        let mut start = pos;
        while start > 0 {
            let c = bytes[start - 1] as char;
            if c.is_ascii_alphanumeric() || c == '_' || c == '.' {
                start -= 1;
            } else {
                break;
            }
        }

        // ── `?<ident>` shortcut: complete the identifier after `?` ──
        let q_prefix = start > 0 && bytes[start - 1] == b'?';
        if q_prefix {
            let word = &line[start..pos];
            // Disallow dotted access here (rare) — fall through to simple ident.
            if !word.contains('.') {
                let names = self.all_identifiers();
                let candidates = filter_to_pairs(&names, word);
                return (start, candidates);
            }
        }

        let word = &line[start..pos];

        // ── After `import ` token: complete module names ──
        if is_after_import_keyword(line, start) {
            let names = self.importable_names();
            return (start, filter_to_pairs(&names, word));
        }

        // ── Dotted access: `head.tail` ──
        if let Some(dot) = word.rfind('.') {
            let head = &word[..dot];
            let tail = &word[dot + 1..];
            let members = self.members_of(head);
            return (start + dot + 1, filter_to_pairs(&members, tail));
        }

        // ── Bare identifier ──
        let names = self.all_identifiers();
        (start, filter_to_pairs(&names, word))
    }

    /// Identifiers visible at the top level for completion:
    /// user variables, builtin functions, keywords, std module names,
    /// user-defined types, and primitive type names.
    fn all_identifiers(&self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        let interp = self.interp.borrow();

        // User-defined variables in scope.
        for (n, _, _) in interp.env.list_vars() {
            names.push(n);
        }
        // Struct / enum / trait names.
        for n in interp.struct_defs.keys() {
            names.push(n.clone());
        }
        for n in interp.enum_defs.keys() {
            names.push(n.clone());
        }
        for n in interp.trait_defs.keys() {
            names.push(n.clone());
        }
        drop(interp);

        // Builtins from the shared docs registry.
        for b in crate::docs::builtin_functions() {
            names.push(b.name.to_string());
        }
        // Keywords.
        for kw in crate::docs::KEYWORDS {
            names.push(kw.to_string());
        }
        // Primitive type names.
        for ty in crate::docs::TYPES {
            names.push(ty.to_string());
        }
        // Std module names.
        for m in crate::interpreter::std_modules::all_modules() {
            names.push(m.name.to_string());
        }

        dedup_sorted(names)
    }

    /// Names usable after `import ` (std modules + local packages).
    fn importable_names(&self) -> Vec<String> {
        let mut names: Vec<String> = crate::interpreter::std_modules::all_modules()
            .into_iter()
            .map(|m| m.name.to_string())
            .collect();
        if let Some(loader) = self.interp.borrow().module_loader.as_ref() {
            for pkg_dir in loader.package_dirs() {
                if let Ok(rd) = std::fs::read_dir(pkg_dir) {
                    for e in rd.flatten() {
                        if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                            let name = e.file_name().to_string_lossy().into_owned();
                            // `.sources/` holds git checkouts of `subdir`
                            // dependencies; it is not importable.
                            if !name.starts_with('.') {
                                names.push(name);
                            }
                        }
                    }
                }
            }
        }
        dedup_sorted(names)
    }

    /// Members of `head` for `head.tail` completion: either the exports
    /// of a module bound in scope, or the functions of a known std module,
    /// or the methods available on a value bound in scope.
    fn members_of(&self, head: &str) -> Vec<String> {
        let interp = self.interp.borrow();

        // Module dotted-walk first (e.g. `pkg.sub` where `pkg` is a Module).
        let resolved = resolve_dotted(&interp, head);
        if let Some(val) = &resolved {
            if let Value::Module { entries, .. } = val {
                let mut names: Vec<String> = entries.keys().cloned().collect();
                names.sort();
                return names;
            }
        }

        // If `head` is a single bare identifier referring to a known std
        // module (and no value `head` is bound in scope), use its functions.
        if !head.contains('.') {
            if let Some(m) = crate::interpreter::std_modules::all_modules()
                .into_iter()
                .find(|m| m.name == head)
            {
                let mut names: Vec<String> = m.functions.iter().map(|s| s.to_string()).collect();
                names.sort();
                return names;
            }
        }

        // Methods on a value bound in scope (or reached through a dotted
        // chain like `pair.0` — though we only walk Module dots above).
        if let Some(val) = resolved {
            let mut names: Vec<String> = val
                .available_methods()
                .into_iter()
                .map(|s| s.to_string())
                .collect();

            match &val {
                Value::Instance { type_name, fields } => {
                    // Field names are also valid `.member` lookups.
                    for f in fields.keys() {
                        names.push(f.clone());
                    }
                    if let Some(methods) = interp.impl_methods.get(type_name) {
                        for m in methods.iter().filter(|m| !m.is_static) {
                            names.push(m.name.clone());
                        }
                    }
                    let trait_keys: Vec<String> = interp
                        .trait_impls
                        .keys()
                        .filter(|(ty, _)| ty == type_name)
                        .map(|(_, t)| t.clone())
                        .collect();
                    for t in trait_keys {
                        if let Some(methods) =
                            interp.trait_impls.get(&(type_name.clone(), t))
                        {
                            for m in methods {
                                names.push(m.name.clone());
                            }
                        }
                    }
                }
                Value::TypeRef(type_name) => {
                    // Enum variants and static methods on `TypeName.<…>`.
                    if let Some(variants) = interp.enum_defs.get(type_name) {
                        for (v, _) in variants {
                            names.push(v.clone());
                        }
                    }
                    if let Some(methods) = interp.impl_methods.get(type_name) {
                        for m in methods.iter().filter(|m| m.is_static) {
                            names.push(m.name.clone());
                        }
                    }
                }
                _ => {}
            }

            names.sort();
            names.dedup();
            return names;
        }

        // Fall back: head is a bare user-defined type name not bound as a
        // value but registered (e.g. `MyEnum.<TAB>` where `MyEnum` doesn't
        // exist as a Value::TypeRef binding).
        if !head.contains('.') {
            let mut names = Vec::new();
            if let Some(variants) = interp.enum_defs.get(head) {
                for (v, _) in variants {
                    names.push(v.clone());
                }
            }
            if let Some(methods) = interp.impl_methods.get(head) {
                for m in methods.iter().filter(|m| m.is_static) {
                    names.push(m.name.clone());
                }
            }
            if !names.is_empty() {
                names.sort();
                names.dedup();
                return names;
            }
        }

        Vec::new()
    }
}

/// Walk `head.sub.sub2…` through env-bound Module values.
fn resolve_dotted(interp: &Interpreter, head: &str) -> Option<Value> {
    let mut parts = head.split('.');
    let first = parts.next()?;
    let mut current = interp.env.get(first)?;
    for part in parts {
        match current {
            Value::Module { entries, .. } => {
                current = entries.get(part)?.clone();
            }
            _ => return None,
        }
    }
    Some(current)
}

/// Returns true if the token starting at byte offset `start` is preceded by
/// the `import` keyword (with optional whitespace), making this an import path.
fn is_after_import_keyword(line: &str, start: usize) -> bool {
    let before = line[..start].trim_end();
    before.ends_with("import")
        && (before.len() == 6
            || matches!(
                before.as_bytes().get(before.len().saturating_sub(7)).copied(),
                Some(b'\n') | Some(b';') | Some(b'\t') | Some(b' ') | None
            ))
}

fn filter_to_pairs(names: &[String], prefix: &str) -> Vec<Pair> {
    names
        .iter()
        .filter(|n| n.starts_with(prefix))
        .map(|n| Pair {
            display: n.clone(),
            replacement: n.clone(),
        })
        .collect()
}

fn dedup_sorted(mut v: Vec<String>) -> Vec<String> {
    v.sort();
    v.dedup();
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn helper() -> QueHelper {
        QueHelper::new(Rc::new(RefCell::new(Interpreter::new())))
    }

    fn complete(h: &QueHelper, line: &str) -> (usize, Vec<String>) {
        let (start, pairs) = h.compute(line, line.len());
        (start, pairs.into_iter().map(|p| p.replacement).collect())
    }

    #[test]
    fn meta_commands_complete() {
        let h = helper();
        let (start, items) = complete(&h, ":v");
        assert_eq!(start, 0);
        assert!(items.contains(&":vars".to_string()));
        assert!(items.contains(&":v".to_string()));
    }

    #[test]
    fn bare_ident_completes_builtin() {
        let h = helper();
        let (start, items) = complete(&h, "prin");
        assert_eq!(start, 0);
        assert!(items.contains(&"print".to_string()));
        assert!(items.contains(&"println".to_string()));
    }

    #[test]
    fn bare_ident_completes_std_module_name() {
        let h = helper();
        let (_, items) = complete(&h, "js");
        assert!(items.contains(&"json".to_string()));
    }

    #[test]
    fn dotted_completes_std_module_function() {
        let h = helper();
        let (start, items) = complete(&h, "fs.re");
        assert_eq!(start, 3); // replace just `re`
        assert!(items.contains(&"read".to_string()));
        assert!(items.contains(&"read_lines".to_string()));
        assert!(items.contains(&"remove_dir".to_string()));
    }

    #[test]
    fn after_import_completes_modules() {
        let h = helper();
        let (start, items) = complete(&h, "import js");
        assert_eq!(start, 7);
        assert!(items.contains(&"json".to_string()));
    }

    #[test]
    fn question_mark_completes_ident() {
        let h = helper();
        let (start, items) = complete(&h, "?prin");
        assert_eq!(start, 1); // replace `prin`, keep `?`
        assert!(items.contains(&"println".to_string()));
    }

    #[test]
    fn bare_ident_completes_user_variable() {
        let h = helper();
        h.interp
            .borrow_mut()
            .env
            .define("my_special_var", Value::Int(42), false);
        let (_, items) = complete(&h, "my_spec");
        assert!(items.contains(&"my_special_var".to_string()));
    }

    #[test]
    fn dotted_completes_value_methods() {
        let h = helper();
        h.interp
            .borrow_mut()
            .env
            .define("s", Value::Set(vec![Value::Int(1), Value::Int(2)]), false);
        let (start, items) = complete(&h, "s.");
        assert_eq!(start, 2);
        // Set should expose at least `contains` and `len`.
        assert!(items.contains(&"contains".to_string()), "got: {:?}", items);
        assert!(items.contains(&"len".to_string()), "got: {:?}", items);
    }

    #[test]
    fn dotted_completes_list_methods_with_prefix() {
        let h = helper();
        h.interp.borrow_mut().env.define(
            "xs",
            Value::List(vec![Value::Int(1), Value::Int(2)]),
            false,
        );
        let (start, items) = complete(&h, "xs.ma");
        assert_eq!(start, 3);
        assert!(items.contains(&"map".to_string()));
    }
}
