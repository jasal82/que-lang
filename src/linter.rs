/// Static analysis linter for the Que language.
///
/// Walks the AST without executing it, reporting potential issues:
/// - undefined-name / arity: see `crate::resolver`
/// - unused-result: HTTP result discarded
/// - unscoped-cd: cd() result discarded, leaving nothing to move back with
/// - unreachable-code: statements after return/break/continue/fail
/// - secret-interpolation: Secret value used in string interpolation
/// - empty-block: empty block body (likely mistake)

use crate::ast::*;
use crate::ast::AstStringPart;

#[derive(Debug, Clone)]
pub struct LintDiagnostic {
    pub rule: &'static str,
    pub severity: Severity,
    pub message: String,
    pub line: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Severity {
    Warning,
    Error,
}

impl std::fmt::Display for LintDiagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let sev = match self.severity {
            Severity::Warning => "warning",
            Severity::Error => "error",
        };
        if let Some(line) = self.line {
            write!(f, "{}:{}: [{}] {}", line, sev, self.rule, self.message)
        } else {
            write!(f, "{}: [{}] {}", sev, self.rule, self.message)
        }
    }
}

pub struct Linter {
    diagnostics: Vec<LintDiagnostic>,
    current_line: Option<usize>,
}

impl Linter {
    pub fn new() -> Self {
        Self {
            diagnostics: Vec::new(),
            current_line: None,
        }
    }

    pub fn lint_module(mut self, module: &Module) -> Vec<LintDiagnostic> {
        // Name resolution and arity checking run first so hard errors are
        // reported before stylistic warnings.
        self.diagnostics = crate::resolver::resolve_module(module);
        for (span, item) in &module.items {
            self.current_line = Some(span.line);
            self.lint_item(item);
        }
        self.diagnostics
    }

    fn warn(&mut self, rule: &'static str, message: impl Into<String>) {
        self.diagnostics.push(LintDiagnostic {
            rule,
            severity: Severity::Warning,
            message: message.into(),
            line: self.current_line,
        });
    }

    fn lint_item(&mut self, item: &Item) {
        match item {
            Item::Stmt(stmt) => self.lint_stmt(stmt),
            Item::FnDecl(f) => self.lint_fn_decl(f),
            Item::TaskDecl(t) => self.lint_task_decl(t),
            Item::StructDecl(_) => {}
            Item::EnumDecl(_) => {}
            Item::TypeDecl(_) => {}
            Item::Import(_) => {}
            Item::ImplDecl(i) => {
                for method in &i.methods {
                    self.lint_fn_decl(method);
                }
            }
            Item::TraitDecl(t) => {
                for method in &t.methods {
                    if let Some(ref body) = method.default_body {
                        self.lint_block(body);
                    }
                }
            }
            Item::TraitImplDecl(t) => {
                for method in &t.methods {
                    self.lint_fn_decl(method);
                }
            }
            Item::PubLet { .. } => {}
        }
    }

    fn lint_fn_decl(&mut self, f: &FnDecl) {
        if f.body.stmts.is_empty() && f.body.expr.is_none() {
            self.warn(
                "empty-block",
                format!("function '{}' has an empty body", f.name),
            );
        }
        self.lint_block(&f.body);
    }

    fn lint_task_decl(&mut self, t: &TaskDecl) {
        self.lint_block(&t.body);
    }

    fn lint_block(&mut self, block: &Block) {
        self.check_unreachable(&block.stmts, block.expr.is_some());
        for (span, stmt) in &block.stmts {
            self.current_line = Some(span.line);
            self.lint_stmt(stmt);
        }
        if let Some(ref expr) = block.expr {
            self.lint_expr(expr);
        }
    }

    fn lint_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let { value, .. } => {
                self.lint_expr(value);
            }
            Stmt::Mut { value, .. } => {
                self.lint_expr(value);
            }
            Stmt::Expr(expr) => {
                // Check for unused command results
                self.check_unused_result(expr);
                self.lint_expr(expr);
            }
            Stmt::Return(val) => {
                if let Some(ref e) = val {
                    self.lint_expr(e);
                }
            }
            Stmt::Break(val) => {
                if let Some(ref e) = val {
                    self.lint_expr(e);
                }
            }
            Stmt::Continue => {}
            Stmt::For { iterable, body, .. } => {
                self.lint_expr(iterable);
                self.lint_block(body);
            }
            Stmt::While { condition, body } => {
                self.lint_expr(condition);
                self.lint_block(body);
            }
            Stmt::Loop { body } => {
                self.lint_block(body);
            }
            Stmt::Defer(expr) => {
                self.lint_expr(expr);
            }
            Stmt::TryCatch {
                try_body,
                catches,
                finally_body,
            } => {
                self.lint_block(try_body);
                for catch in catches {
                    self.lint_block(&catch.body);
                }
                if let Some(ref finally) = finally_body {
                    self.lint_block(finally);
                }
            }
            Stmt::Assign { target, value } => {
                self.lint_expr(target);
                self.lint_expr(value);
            }
            Stmt::CompoundAssign { target, value, .. } => {
                self.lint_expr(target);
                self.lint_expr(value);
            }
        }
    }

    fn lint_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::InterpolatedString(parts) => {
                self.check_secret_interpolation(parts);
            }
            Expr::CmdLit(parts) => {
                // Deliberately not checked. Handing a token to the process
                // that needs it is what a command is for, and a `Secret`
                // interpolated here is redacted in every rendering anyway.
                for part in parts {
                    if let AstStringPart::Expr(e) | AstStringPart::RawExpr(e) = part {
                        self.lint_expr(e);
                    }
                }
            }
            Expr::ListLit(elems) => {
                for e in elems {
                    self.lint_expr(e);
                }
            }
            Expr::MapLit(entries) => {
                for entry in entries {
                    match entry {
                        MapEntry::Pair(k, v) => {
                            self.lint_expr(k);
                            self.lint_expr(v);
                        }
                        MapEntry::Spread(e) => self.lint_expr(e),
                    }
                }
            }
            Expr::SetLit(elems) | Expr::TupleLit(elems) => {
                for e in elems {
                    self.lint_expr(e);
                }
            }
            Expr::BinaryOp { left, right, .. } => {
                self.lint_expr(left);
                self.lint_expr(right);
            }
            Expr::UnaryOp { expr: inner, .. } => {
                self.lint_expr(inner);
            }
            Expr::Call { callee, args } => {
                self.lint_expr(callee);
                for arg in args {
                    self.lint_expr(&arg.value);
                }
            }
            Expr::MethodCall { object, args, .. } => {
                self.lint_expr(object);
                for arg in args {
                    self.lint_expr(&arg.value);
                }
            }
            Expr::FieldAccess { object, .. } => {
                self.lint_expr(object);
            }
            Expr::OptionalAccess { object, args, .. } => {
                self.lint_expr(object);
                for arg in args.iter().flatten() {
                    self.lint_expr(&arg.value);
                }
            }
            Expr::Index { object, index } => {
                self.lint_expr(object);
                self.lint_expr(index);
            }
            Expr::Lambda { body, .. } => {
                self.lint_expr(body);
            }
            Expr::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.lint_expr(condition);
                self.lint_block(then_branch);
                if let Some(ref e) = else_branch {
                    self.lint_expr(e);
                }
            }
            Expr::IfLet {
                value,
                then_branch,
                else_branch,
                ..
            } => {
                self.lint_expr(value);
                self.lint_block(then_branch);
                if let Some(ref e) = else_branch {
                    self.lint_expr(e);
                }
            }
            Expr::Match { subject, arms } => {
                self.lint_expr(subject);
                for arm in arms {
                    self.lint_expr(&arm.body);
                }
            }
            Expr::Block(block) => {
                self.lint_block(block);
            }
            Expr::WithContext { manager, body, .. } => {
                self.lint_expr(manager);
                self.lint_block(body);
            }
            Expr::Pipe { left, right } => {
                self.lint_expr(left);
                self.lint_expr(right);
            }
            Expr::Try(inner) => {
                self.lint_expr(inner);
            }
            Expr::NullCoalesce { left, right } => {
                self.lint_expr(left);
                self.lint_expr(right);
            }
            Expr::Range { start, end, .. } => {
                if let Some(ref s) = start {
                    self.lint_expr(s);
                }
                if let Some(ref e) = end {
                    self.lint_expr(e);
                }
            }
            Expr::Loop { body } => {
                self.lint_block(body);
            }
            Expr::Spread(inner) => {
                self.lint_expr(inner);
            }
            Expr::StructLit { fields, .. } => {
                for (_, v) in fields {
                    self.lint_expr(v);
                }
            }
            Expr::Spawn(inner) => {
                self.lint_expr(inner);
            }
            Expr::Parallel(branches) => {
                for branch in branches {
                    self.lint_expr(&branch.body);
                }
            }
            _ => {}
        }
    }

    // ── Rule implementations ──

    /// Check for statements after return/break/continue/fail (unreachable code).
    fn check_unreachable(&mut self, stmts: &[(crate::token::Span, Stmt)], has_trailing_expr: bool) {
        let mut found_terminator = false;
        for (span, stmt) in stmts {
            self.current_line = Some(span.line);
            if found_terminator {
                self.warn(
                    "unreachable-code",
                    "this code is unreachable (after return/break/continue/fail)",
                );
                return; // Only warn once per block
            }
            match stmt {
                Stmt::Return(_) | Stmt::Break(_) | Stmt::Continue => {
                    found_terminator = true;
                }
                Stmt::Expr(Expr::Call { callee, .. }) => {
                    if let Expr::Ident(name) = callee.as_ref() {
                        if name == "fail" {
                            found_terminator = true;
                        }
                    }
                }
                _ => {}
            }
        }
        // Also warn if there's a trailing expression after a terminator
        if found_terminator && has_trailing_expr {
            self.warn(
                "unreachable-code",
                "this code is unreachable (after return/break/continue/fail)",
            );
        }
    }

    /// Check for results that are discarded (not assigned or checked).
    ///
    /// Commands are not flagged: a bare `` `cmd` `` statement now runs and
    /// raises on failure, so discarding its `ProcessResult` is intentional.
    fn check_unused_result(&mut self, expr: &Expr) {
        match expr {
            // A call to http.get/http.post/etc. not assigned
            Expr::Call { callee, .. } => {
                if let Expr::Ident(name) = callee.as_ref() {
                    if is_result_producing_fn(name) {
                        self.warn(
                            "unused-result",
                            format!(
                                "{}() result is discarded — consider checking the response",
                                name
                            ),
                        );
                    }
                    // `cd` returns the directory it left, and that return value
                    // is the only way back. Thrown away, the move outlives the
                    // block, the function and the task it appears in — it is
                    // the process that moved, and nothing restores it.
                    //
                    // Only the discarded form is flagged: `let previous = cd(x)`
                    // has kept what it needs, whether or not it uses it.
                    if name == "cd" {
                        self.warn(
                            "unscoped-cd",
                            "cd() result is discarded, so nothing can move back — \
                             use `with dir(...) { ... }` to scope the change to a \
                             block, or keep the returned path to return to it",
                        );
                    }
                }
            }
            _ => {}
        }
    }

    /// Check for Secret values potentially leaked in string interpolation.
    fn check_secret_interpolation(&mut self, parts: &[AstStringPart]) {
        fn collect_names(expr: &Expr, out: &mut Vec<String>) {
            match expr {
                Expr::Ident(name) => out.push(name.clone()),
                Expr::FieldAccess { object, field } => {
                    collect_names(object, out);
                    out.push(field.clone());
                }
                Expr::MethodCall { object, method, args } => {
                    collect_names(object, out);
                    out.push(method.clone());
                    for arg in args {
                        collect_names(&arg.value, out);
                    }
                }
                Expr::Index { object, index } => {
                    collect_names(object, out);
                    collect_names(index, out);
                }
                _ => {}
            }
        }
        for part in parts {
            if let AstStringPart::Expr(expr) = part {
                let mut names = Vec::new();
                collect_names(expr, &mut names);
                for name in &names {
                    let lower = name.to_lowercase();
                    if lower.contains("secret")
                        || lower.contains("password")
                        || lower.contains("api_key")
                        || lower.contains("token")
                        || lower.ends_with("_key")
                    {
                        self.warn(
                            "secret-interpolation",
                            format!(
                                "potential secret '{}' interpolated into a string — wrap it with secret() so it is redacted in output",
                                name
                            ),
                        );
                    }
                }
            }
        }
    }
}

/// Functions whose return values typically should not be discarded.
fn is_result_producing_fn(name: &str) -> bool {
    matches!(
        name,
        "get"
            | "post"
            | "put"
            | "patch"
            | "delete"
            | "request"
            | "download"
    )
}
