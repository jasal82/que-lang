/// Name resolution and arity checking — the static half of Que's error
/// reporting.
///
/// Que is dynamically typed, so a typo in a variable name or a call with the
/// wrong number of arguments used to surface only when that line executed —
/// often halfway through a deploy. This pass walks the AST with a scope stack
/// and reports both without running anything.
///
/// It deliberately checks only what can be known for certain from the syntax:
///
/// - `undefined-name`: an identifier that is not a parameter, a local, a
///   top-level declaration, an import binding, or a builtin.
/// - `arity`: a call to a function declared in this module with too few or too
///   many arguments.
///
/// Anything that depends on values or on another module's contents (field
/// names, method names, types) is left to the interpreter. The rule is that a
/// diagnostic here must never be wrong; a false positive on a working script is
/// worse than a missed typo.

use crate::ast::*;
use crate::interpreter::BUILTIN_NAMES;
use crate::linter::{LintDiagnostic, Severity};
use std::collections::{HashMap, HashSet};

/// Names the interpreter defines as globals that are not plain builtin
/// functions. Kept next to the `BUILTIN_NAMES` import so both stay visible.
const GLOBAL_OBJECTS: &[&str] = &["os", "TempDir", "TempFile"];

/// The argument count a declared function accepts.
#[derive(Clone, Copy)]
struct Arity {
    required: usize,
    total: usize,
}

pub struct Resolver {
    scopes: Vec<HashSet<String>>,
    /// Arity of every function declared at module top level, by name.
    fns: HashMap<String, Arity>,
    diagnostics: Vec<LintDiagnostic>,
    line: Option<usize>,
    /// Names already reported, so one typo inside a loop yields one diagnostic.
    reported: HashSet<String>,
    /// Set by `import x { * }`, which pulls in an unknown set of names. Once
    /// that happens no identifier can be proven undefined, so the check stops.
    wildcard_import: bool,
    /// True while walking the call on the right of a `|>`, which receives an
    /// extra implicit argument from the left.
    in_pipe_rhs: bool,
}

/// Resolve a module, returning one diagnostic per problem found.
pub fn resolve_module(module: &Module) -> Vec<LintDiagnostic> {
    Resolver::new().run(module)
}

impl Resolver {
    fn new() -> Self {
        let mut global: HashSet<String> = BUILTIN_NAMES.iter().map(|s| s.to_string()).collect();
        global.extend(GLOBAL_OBJECTS.iter().map(|s| s.to_string()));
        // `_` is the partial-application placeholder, e.g. `add(5, _)`.
        global.insert("_".to_string());
        Self {
            scopes: vec![global],
            fns: HashMap::new(),
            diagnostics: Vec::new(),
            line: None,
            reported: HashSet::new(),
            wildcard_import: false,
            in_pipe_rhs: false,
        }
    }

    fn run(mut self, module: &Module) -> Vec<LintDiagnostic> {
        // Pass 1: hoist every top-level name. Declaration order does not
        // matter at module scope — a function may call one declared below it,
        // and a function body may read a `let` that appears after it as long
        // as the call happens later. Hoisting everything keeps this pass from
        // guessing about execution order.
        self.push_scope();
        for (_, item) in &module.items {
            self.declare_item(item);
        }

        // Pass 2: walk the bodies now that all top-level names are known.
        for (span, item) in &module.items {
            self.line = Some(span.line);
            self.walk_item(item);
        }
        self.diagnostics
    }

    // ── Scope handling ──

    fn push_scope(&mut self) {
        self.scopes.push(HashSet::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn define(&mut self, name: &str) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.to_string());
        }
    }

    fn is_defined(&self, name: &str) -> bool {
        self.scopes.iter().any(|s| s.contains(name))
    }

    fn report(&mut self, rule: &'static str, message: String) {
        self.diagnostics.push(LintDiagnostic {
            rule,
            severity: Severity::Error,
            message,
            line: self.line,
        });
    }

    // ── Pass 1: hoisting ──

    fn declare_item(&mut self, item: &Item) {
        match item {
            Item::FnDecl(f) => {
                self.define(&f.name);
                self.fns.insert(f.name.clone(), arity_of(&f.params));
            }
            Item::StructDecl(d) => self.define(&d.name),
            Item::EnumDecl(d) => {
                self.define(&d.name);
                // A unit variant can also be referred to bare: `North` as well
                // as `Direction.North`.
                for v in &d.variants {
                    self.define(&v.name);
                }
            }
            Item::TypeDecl(d) => self.define(&d.name),
            Item::TraitDecl(d) => self.define(&d.name),
            Item::Import(i) => self.declare_import(i),
            Item::Stmt(stmt) => self.declare_stmt_bindings(stmt),
            Item::PubLet { pattern, .. } => self.bind_pattern(pattern),
            // A task is callable as a function too: `task build { }` / `build()`.
            Item::TaskDecl(t) => {
                self.define(&t.name);
                self.fns.insert(t.name.clone(), arity_of(&t.params));
                for alias in &t.aliases {
                    self.define(alias);
                    self.fns.insert(alias.clone(), arity_of(&t.params));
                }
            }
            Item::ImplDecl(_) | Item::TraitImplDecl(_) => {}
        }
    }

    fn declare_stmt_bindings(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let { pattern, .. } => self.bind_pattern(pattern),
            Stmt::Mut { name, .. } => self.define(name),
            _ => {}
        }
    }

    fn declare_import(&mut self, decl: &ImportDecl) {
        if let Some(items) = &decl.items {
            // `import std.fs { readText }` brings the listed names into scope.
            for name in items {
                if name == "*" {
                    self.wildcard_import = true;
                }
                self.define(name);
            }
            return;
        }
        // Otherwise the module itself is bound, under its alias if given.
        let bound = decl
            .alias
            .clone()
            .or_else(|| decl.path.last().cloned())
            .unwrap_or_default();
        if !bound.is_empty() {
            self.define(&bound);
        }
    }

    // ── Pass 2: walking ──

    fn walk_item(&mut self, item: &Item) {
        match item {
            Item::Stmt(stmt) => self.walk_stmt(stmt),
            Item::FnDecl(f) => self.walk_fn(f),
            Item::TaskDecl(t) => {
                self.push_scope();
                for p in &t.params {
                    if let Some(d) = &p.default {
                        self.walk_expr(d);
                    }
                    self.define(&p.name);
                }
                // `deps [build, test]` names tasks, not variables, so those
                // identifiers are deliberately not resolved here.
                for e in t.inputs.iter().chain(&t.outputs) {
                    self.walk_expr(e);
                }
                self.walk_block(&t.body);
                self.pop_scope();
            }
            Item::PubLet { value, .. } => self.walk_expr(value),
            Item::ImplDecl(i) => {
                for m in &i.methods {
                    self.walk_fn(m);
                }
            }
            Item::TraitImplDecl(i) => {
                for m in &i.methods {
                    self.walk_fn(m);
                }
            }
            Item::TraitDecl(t) => {
                for m in &t.methods {
                    if let Some(body) = &m.default_body {
                        self.push_scope();
                        for p in &m.params {
                            self.define(&p.name);
                        }
                        self.walk_block(body);
                        self.pop_scope();
                    }
                }
            }
            Item::StructDecl(_)
            | Item::EnumDecl(_)
            | Item::TypeDecl(_)
            | Item::Import(_) => {}
        }
    }

    fn walk_fn(&mut self, f: &FnDecl) {
        self.push_scope();
        for p in &f.params {
            // A default is evaluated where the function is declared, so it
            // cannot see later parameters.
            if let Some(d) = &p.default {
                self.walk_expr(d);
            }
            self.define(&p.name);
        }
        self.walk_block(&f.body);
        self.pop_scope();
    }

    fn walk_block(&mut self, block: &Block) {
        self.push_scope();
        self.walk_block_inner(block);
        self.pop_scope();
    }

    /// Walk a block's contents in the *current* scope. Used where the caller
    /// has already pushed a scope holding pattern bindings.
    fn walk_block_inner(&mut self, block: &Block) {
        for (span, stmt) in &block.stmts {
            self.line = Some(span.line);
            self.walk_stmt(stmt);
        }
        if let Some(expr) = &block.expr {
            self.walk_expr(expr);
        }
    }

    fn walk_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let { pattern, value, .. } => {
                // The initialiser is evaluated before the binding exists.
                self.walk_expr(value);
                self.bind_pattern(pattern);
            }
            Stmt::Mut { name, value, .. } => {
                self.walk_expr(value);
                self.define(name);
            }
            Stmt::Expr(e) | Stmt::Defer(e) => self.walk_expr(e),
            Stmt::Return(v) | Stmt::Break(v) => {
                if let Some(e) = v {
                    self.walk_expr(e);
                }
            }
            Stmt::Continue => {}
            Stmt::For {
                pattern,
                iterable,
                body,
            } => {
                self.walk_expr(iterable);
                self.push_scope();
                self.bind_pattern(pattern);
                self.walk_block_inner(body);
                self.pop_scope();
            }
            Stmt::While { condition, body } => {
                self.walk_expr(condition);
                self.walk_block(body);
            }
            Stmt::Loop { body } => self.walk_block(body),
            Stmt::TryCatch {
                try_body,
                catches,
                finally_body,
            } => {
                self.walk_block(try_body);
                for catch in catches {
                    self.push_scope();
                    if let Some(b) = &catch.binding {
                        self.define(b);
                    }
                    self.walk_block_inner(&catch.body);
                    self.pop_scope();
                }
                if let Some(f) = finally_body {
                    self.walk_block(f);
                }
            }
            Stmt::Assign { target, value } | Stmt::CompoundAssign { target, value, .. } => {
                self.walk_expr(value);
                self.walk_expr(target);
            }
        }
    }

    fn walk_expr(&mut self, expr: &Expr) {
        // The flag applies only to the expression directly to the right of a
        // `|>`, never to anything nested inside it.
        let piped = std::mem::take(&mut self.in_pipe_rhs);
        match expr {
            Expr::Ident(name) => self.check_name(name),

            Expr::Call { callee, args } => {
                if let Expr::Ident(name) = callee.as_ref() {
                    self.check_name(name);
                    if !piped {
                        self.check_arity(name, args);
                    }
                } else {
                    self.walk_expr(callee);
                }
                for a in args {
                    self.walk_expr(&a.value);
                }
            }

            Expr::Lambda { params, body } => {
                self.push_scope();
                for p in params {
                    if let Some(d) = &p.default {
                        self.walk_expr(d);
                    }
                    self.define(&p.name);
                }
                self.walk_expr(body);
                self.pop_scope();
            }

            Expr::IfLet {
                pattern,
                value,
                then_branch,
                else_branch,
            } => {
                self.walk_expr(value);
                self.push_scope();
                self.bind_pattern(pattern);
                self.walk_block_inner(then_branch);
                self.pop_scope();
                if let Some(e) = else_branch {
                    self.walk_expr(e);
                }
            }

            Expr::Match { subject, arms } => {
                self.walk_expr(subject);
                for arm in arms {
                    self.push_scope();
                    self.bind_pattern(&arm.pattern);
                    if let Some(g) = &arm.guard {
                        self.walk_expr(g);
                    }
                    self.walk_expr(&arm.body);
                    self.pop_scope();
                }
            }

            Expr::WithContext { manager, name, body } => {
                self.walk_expr(manager);
                self.push_scope();
                if !name.is_empty() {
                    self.define(name);
                }
                self.walk_block_inner(body);
                self.pop_scope();
            }

            Expr::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.walk_expr(condition);
                self.walk_block(then_branch);
                if let Some(e) = else_branch {
                    self.walk_expr(e);
                }
            }

            Expr::Block(b) | Expr::Loop { body: b } => self.walk_block(b),

            // `obj.field` and `obj.method(..)` — only the receiver is a name
            // we can resolve; the field is a runtime property.
            Expr::FieldAccess { object, .. } => self.walk_expr(object),
            Expr::MethodCall { object, args, .. } => {
                self.walk_expr(object);
                for a in args {
                    self.walk_expr(&a.value);
                }
            }
            Expr::OptionalAccess { object, args, .. } => {
                self.walk_expr(object);
                if let Some(args) = args {
                    for a in args {
                        self.walk_expr(&a.value);
                    }
                }
            }

            Expr::StructLit { fields, .. } => {
                // The type name may come from another module, so it is not
                // checked here; the field initialisers still are.
                for (_, v) in fields {
                    self.walk_expr(v);
                }
            }

            Expr::Pipe { left, right } => {
                self.walk_expr(left);
                // `x |> f(a)` calls `f(x, a)`, so the written argument list is
                // one short of the real one.
                self.in_pipe_rhs = true;
                self.walk_expr(right);
                self.in_pipe_rhs = false;
            }
            Expr::BinaryOp { left, right, .. } | Expr::NullCoalesce { left, right } => {
                self.walk_expr(left);
                self.walk_expr(right);
            }
            Expr::UnaryOp { expr, .. }
            | Expr::Try(expr)
            | Expr::Spread(expr)
            | Expr::Spawn(expr) => self.walk_expr(expr),
            Expr::Index { object, index } => {
                self.walk_expr(object);
                self.walk_expr(index);
            }
            Expr::Range { start, end, .. } => {
                if let Some(s) = start {
                    self.walk_expr(s);
                }
                if let Some(e) = end {
                    self.walk_expr(e);
                }
            }
            Expr::ListLit(items) | Expr::SetLit(items) | Expr::TupleLit(items) => {
                for i in items {
                    self.walk_expr(i);
                }
            }
            Expr::MapLit(entries) => {
                for entry in entries {
                    match entry {
                        MapEntry::Pair(k, v) => {
                            self.walk_expr(k);
                            self.walk_expr(v);
                        }
                        MapEntry::Spread(e) => self.walk_expr(e),
                    }
                }
            }
            Expr::Parallel(branches) => {
                for b in branches {
                    self.walk_expr(&b.body);
                }
            }
            Expr::InterpolatedString(parts)
            | Expr::CmdLit(parts)
            | Expr::PathLit(parts)
            | Expr::GlobLit(parts) => {
                for part in parts {
                    match part {
                        AstStringPart::Expr(e) | AstStringPart::RawExpr(e) => self.walk_expr(e),
                        AstStringPart::Literal(_) => {}
                    }
                }
            }

            Expr::IntLit(_)
            | Expr::FloatLit(_)
            | Expr::StringLit(_)
            | Expr::BoolLit(_)
            | Expr::NullLit
            | Expr::DurationLit(_, _)
            | Expr::RegexLit(_)
            | Expr::SemverLit(_) => {}
        }
    }

    // ── Checks ──

    fn check_name(&mut self, name: &str) {
        if self.wildcard_import || self.is_defined(name) || self.reported.contains(name) {
            return;
        }
        self.reported.insert(name.to_string());
        let hint = self
            .closest_name(name)
            .map(|s| format!(" (did you mean '{}'?)", s))
            .unwrap_or_default();
        self.report("undefined-name", format!("'{}' is not defined{}", name, hint));
    }

    fn check_arity(&mut self, name: &str, args: &[CallArg]) {
        let Some(arity) = self.fns.get(name).copied() else {
            return;
        };
        // A spread argument expands to an unknown number of values.
        if args.iter().any(|a| matches!(a.value, Expr::Spread(_))) {
            return;
        }
        let n = args.len();
        if n >= arity.required && n <= arity.total {
            return;
        }
        let expected = if arity.required == arity.total {
            format!("{}", arity.total)
        } else {
            format!("{} to {}", arity.required, arity.total)
        };
        self.report(
            "arity",
            format!(
                "'{}' takes {} argument(s), but {} were given",
                name, expected, n
            ),
        );
    }

    /// Best in-scope name within edit distance 2, to turn "not defined" into an
    /// actionable message for the common case: a typo.
    fn closest_name(&self, name: &str) -> Option<String> {
        let limit = if name.len() <= 4 { 1 } else { 2 };
        self.scopes
            .iter()
            .flatten()
            .filter(|c| c.len() > 1 && !c.starts_with('_'))
            .map(|c| (edit_distance(name, c), c))
            .filter(|(d, _)| *d <= limit)
            .min_by_key(|(d, _)| *d)
            .map(|(_, c)| c.clone())
    }

    // ── Patterns ──

    fn bind_pattern(&mut self, pattern: &Pattern) {
        match pattern {
            Pattern::Ident(name) => self.define(name),
            Pattern::Binding(name, inner) => {
                self.define(name);
                self.bind_pattern(inner);
            }
            Pattern::List(items, rest) => {
                for p in items {
                    self.bind_pattern(p);
                }
                if let Some(r) = rest {
                    self.bind_pattern(r);
                }
            }
            Pattern::Tuple(items) | Pattern::Or(items) => {
                for p in items {
                    self.bind_pattern(p);
                }
            }
            Pattern::Enum(_, _, items) => {
                for p in items {
                    self.bind_pattern(p);
                }
            }
            Pattern::Struct(fields, rest) | Pattern::Instance(_, _, fields, rest) => {
                for (field, sub) in fields {
                    match sub {
                        // `{ name }` is shorthand that binds `name` itself.
                        None => self.define(field),
                        Some(p) => self.bind_pattern(p),
                    }
                }
                if let Some(r) = rest {
                    self.define(r);
                }
            }
            Pattern::Range(lo, hi, _) => {
                if let Some(p) = lo {
                    self.bind_pattern(p);
                }
                if let Some(p) = hi {
                    self.bind_pattern(p);
                }
            }
            Pattern::Wildcard
            | Pattern::IntLit(_)
            | Pattern::FloatLit(_)
            | Pattern::StringLit(_)
            | Pattern::BoolLit(_)
            | Pattern::NullLit
            | Pattern::Glob(_) => {}
        }
    }
}

fn arity_of(params: &[Param]) -> Arity {
    Arity {
        required: params.iter().filter(|p| p.default.is_none()).count(),
        total: params.len(),
    }
}

/// Levenshtein distance, used only for the "did you mean" hint.
fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for i in 1..=a.len() {
        cur[0] = i;
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    fn check(source: &str) -> Vec<String> {
        let tokens = Lexer::new(source).tokenize().expect("lex");
        let module = Parser::new(tokens).parse_module().expect("parse");
        resolve_module(&module)
            .into_iter()
            .map(|d| format!("{}: {}", d.rule, d.message))
            .collect()
    }

    fn assert_clean(source: &str) {
        let found = check(source);
        assert!(found.is_empty(), "expected no diagnostics, got {:?}", found);
    }

    #[test]
    fn reports_an_undefined_name() {
        let found = check("println(totl)");
        assert_eq!(found.len(), 1);
        assert!(found[0].starts_with("undefined-name: 'totl' is not defined"));
    }

    #[test]
    fn suggests_a_close_name() {
        let found = check("let total = 1\nprintln(totl)");
        assert!(found[0].contains("did you mean 'total'?"), "{:?}", found);
    }

    #[test]
    fn a_name_is_reported_once() {
        assert_eq!(check("for i in 0..3 { println(oops) }").len(), 1);
    }

    #[test]
    fn functions_may_be_called_before_they_are_declared() {
        assert_clean("greet()\nfn greet() { println(\"hi\") }");
    }

    #[test]
    fn bindings_do_not_escape_their_block() {
        let found = check("if true { let inner = 1 }\nprintln(inner)");
        assert_eq!(found.len(), 1);
        assert!(found[0].contains("'inner'"));
    }

    #[test]
    fn patterns_bind_their_names() {
        assert_clean(
            r#"
let [a, b] = [1, 2]
let { name } = { "name": "que" }
for (k, v) in [(1, 2)] { println("${k}${v}") }
match 1 { n => println(n) }
println("${a}${b}${name}")
"#,
        );
    }

    #[test]
    fn catch_and_lambda_bindings_are_visible() {
        assert_clean(
            r#"
try { error("x") } catch e { println(e) }
let double = |n| n * 2
println(double(2))
"#,
        );
    }

    #[test]
    fn enum_variants_may_be_used_bare() {
        assert_clean("enum Direction { North, South }\nprintln(North)");
    }

    #[test]
    fn a_wildcard_import_disables_the_check() {
        assert_clean("import std.json { * }\nprintln(stringify({}))");
    }

    #[test]
    fn reports_too_few_arguments() {
        let found = check("fn add(a, b) { a + b }\nadd(1)");
        assert_eq!(found, vec!["arity: 'add' takes 2 argument(s), but 1 were given"]);
    }

    #[test]
    fn reports_too_many_arguments() {
        let found = check("fn add(a, b) { a + b }\nadd(1, 2, 3)");
        assert_eq!(found, vec!["arity: 'add' takes 2 argument(s), but 3 were given"]);
    }

    #[test]
    fn default_parameters_widen_the_accepted_range() {
        assert_clean("fn greet(name, greeting = \"hi\") { greeting + name }\ngreet(\"a\")\ngreet(\"a\", \"yo\")");
        let found = check("fn greet(name, greeting = \"hi\") { greeting + name }\ngreet()");
        assert_eq!(
            found,
            vec!["arity: 'greet' takes 1 to 2 argument(s), but 0 were given"]
        );
    }

    #[test]
    fn a_piped_call_receives_an_implicit_argument() {
        assert_clean("fn add(a, b) { a + b }\nprintln(5 |> add(3))");
    }

    #[test]
    fn a_spread_argument_suppresses_the_arity_check() {
        assert_clean("fn add(a, b) { a + b }\nlet xs = [1, 2]\nadd(...xs)");
    }

    #[test]
    fn tasks_are_callable_by_name_and_alias() {
        assert_clean(
            r#"
@aliases(["b"])
task build {
    println("building")
}
build()
b()
"#,
        );
    }

    #[test]
    fn task_dependencies_are_not_variables() {
        assert_clean(
            r#"
task prepare { println("prepare") }
@deps([prepare])
task build { println("build") }
"#,
        );
    }
}
