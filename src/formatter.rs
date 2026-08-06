/// Source code formatter for the Que language.
///
/// Parses source → AST → re-emits with consistent style:
/// - 4-space indentation
/// - Spaces around operators
/// - One blank line between top-level declarations
/// - Newlines as statement terminators (no semicolons)
/// - Trailing commas in multi-line collections

use crate::ast::*;
use crate::ast::AstStringPart;

pub struct Formatter {
    output: String,
    indent: usize,
}

const INDENT: &str = "    ";

impl Formatter {
    pub fn new() -> Self {
        Self {
            output: String::new(),
            indent: 0,
        }
    }

    pub fn format_module(mut self, module: &Module) -> String {
        let mut prev_was_decl = false;
        let mut first = true;

        if let Some(line) = &module.shebang {
            self.push_str(&format!("#!{}", line));
            self.newline();
        }
        if module.strict {
            self.push_str("#!strict");
            self.newline();
        }
        if module.shebang.is_some() || module.strict {
            self.newline();
        }

        for (_, item) in &module.items {

            let is_decl = matches!(
                item,
                Item::FnDecl(_)
                    | Item::TaskDecl(_)
                    | Item::StructDecl(_)
                    | Item::EnumDecl(_)
                    | Item::TypeDecl(_)
                    | Item::ImplDecl(_)
                    | Item::TraitDecl(_)
                    | Item::TraitImplDecl(_)
            );

            // Blank line between top-level declarations
            if !first && (is_decl || prev_was_decl) {
                self.newline();
            }
            first = false;

            self.format_item(item);
            self.newline();

            prev_was_decl = is_decl;
        }

        // Ensure trailing newline
        if !self.output.ends_with('\n') {
            self.output.push('\n');
        }
        // Remove excess trailing blank lines
        while self.output.ends_with("\n\n\n") {
            self.output.pop();
        }
        self.output
    }

    fn format_item(&mut self, item: &Item) {
        match item {
            Item::Stmt(stmt) => self.format_stmt(stmt),
            Item::FnDecl(f) => self.format_fn_decl(f),
            Item::TaskDecl(t) => self.format_task_decl(t),
            Item::TypeDecl(t) => self.format_type_decl(t),
            Item::EnumDecl(e) => self.format_enum_decl(e),
            Item::Import(i) => self.format_import(i),
            Item::StructDecl(s) => self.format_struct_decl(s),
            Item::ImplDecl(i) => self.format_impl_decl(i),
            Item::TraitDecl(t) => self.format_trait_decl(t),
            Item::TraitImplDecl(t) => self.format_trait_impl_decl(t),
            Item::PubLet { pattern, type_ann, value } => {
                self.format_pub_let(pattern, type_ann, value);
            }
        }
    }

    // ── Declarations ──

    fn format_fn_decl(&mut self, f: &FnDecl) {
        self.write_indent();
        if f.is_pub {
            self.push_str("pub ");
        }
        self.push_str("fn ");
        self.push_str(&f.name);
        self.push_str("(");
        if f.mutates_self {
            self.push_str("mut ");
        }
        self.format_params(&f.params);
        self.push_str(")");
        if let Some(ref ty) = f.return_type {
            self.push_str(" -> ");
            self.format_type_expr(ty);
        }
        self.push_str(" ");
        self.format_block(&f.body);
    }

    fn format_task_decl(&mut self, t: &TaskDecl) {
        // Metadata is written above the task, not inside it: the braces hold
        // the body and nothing else.
        if let Some(ref desc) = t.description {
            self.write_indent();
            self.push_str("@description(\"");
            self.push_str(&escape_string(desc));
            self.push_str("\")");
            self.newline();
        }
        self.format_task_attr_exprs("deps", &t.depends_on);
        self.format_task_attr_exprs("inputs", &t.inputs);
        self.format_task_attr_exprs("outputs", &t.outputs);
        self.format_task_attr_names("aliases", &t.aliases);
        self.format_task_attr_names("env", &t.env_keys);

        self.write_indent();
        self.push_str("task ");
        self.push_str(&t.name);
        if !t.params.is_empty() {
            self.push_str("(");
            self.format_params(&t.params);
            self.push_str(")");
        }
        self.push_str(" ");
        self.format_block(&t.body);
    }

    fn format_task_attr_exprs(&mut self, name: &str, values: &[Expr]) {
        if values.is_empty() {
            return;
        }
        self.write_indent();
        self.push_str("@");
        self.push_str(name);
        self.push_str("([");
        for (i, value) in values.iter().enumerate() {
            if i > 0 {
                self.push_str(", ");
            }
            self.format_expr(value);
        }
        self.push_str("])");
        self.newline();
    }

    fn format_task_attr_names(&mut self, name: &str, values: &[String]) {
        if values.is_empty() {
            return;
        }
        self.write_indent();
        self.push_str("@");
        self.push_str(name);
        self.push_str("([");
        for (i, value) in values.iter().enumerate() {
            if i > 0 {
                self.push_str(", ");
            }
            self.push_str("\"");
            self.push_str(&escape_string(value));
            self.push_str("\"");
        }
        self.push_str("])");
        self.newline();
    }

    fn format_type_decl(&mut self, t: &TypeDecl) {
        self.write_indent();
        if t.is_pub {
            self.push_str("pub ");
        }
        self.push_str("type ");
        self.push_str(&t.name);
        self.push_str(" = ");
        self.format_type_expr(&t.type_expr);
    }

    fn format_enum_decl(&mut self, e: &EnumDecl) {
        self.write_indent();
        if e.is_pub {
            self.push_str("pub ");
        }
        self.push_str("enum ");
        self.push_str(&e.name);
        self.push_str(" {");
        self.indent += 1;
        for variant in &e.variants {
            self.newline();
            self.write_indent();
            self.push_str(&variant.name);
            if !variant.fields.is_empty() {
                self.push_str("(");
                for (i, (name, ty)) in variant.fields.iter().enumerate() {
                    if i > 0 {
                        self.push_str(", ");
                    }
                    self.push_str(name);
                    self.push_str(": ");
                    self.format_type_expr(ty);
                }
                self.push_str(")");
            }
            self.push_str(",");
        }
        self.indent -= 1;
        self.newline();
        self.write_indent();
        self.push_str("}");
    }

    fn format_pub_let(&mut self, pattern: &Pattern, type_ann: &Option<TypeExpr>, value: &Expr) {
        self.write_indent();
        self.push_str("pub let ");
        self.format_pattern(pattern);
        if let Some(ref ty) = type_ann {
            self.push_str(": ");
            self.format_type_expr(ty);
        }
        self.push_str(" = ");
        self.format_expr(value);
    }

    fn format_struct_decl(&mut self, s: &StructDecl) {
        self.write_indent();
        if s.is_pub {
            self.push_str("pub ");
        }
        self.push_str("struct ");
        self.push_str(&s.name);
        self.push_str(" {");
        self.indent += 1;
        for field in &s.fields {
            self.newline();
            self.write_indent();
            self.push_str(&field.name);
            if let Some(ref ty) = field.type_ann {
                self.push_str(": ");
                self.format_type_expr(ty);
            }
            if let Some(ref default) = field.default {
                self.push_str(" = ");
                self.format_expr(default);
            }
            self.push_str(",");
        }
        self.indent -= 1;
        self.newline();
        self.write_indent();
        self.push_str("}");
    }

    fn format_impl_decl(&mut self, i: &ImplDecl) {
        self.write_indent();
        self.push_str("impl ");
        self.push_str(&i.type_name);
        self.push_str(" {");
        self.indent += 1;
        for (idx, method) in i.methods.iter().enumerate() {
            if idx > 0 {
                self.newline();
            }
            self.newline();
            self.format_fn_decl(method);
        }
        self.indent -= 1;
        self.newline();
        self.write_indent();
        self.push_str("}");
    }

    fn format_trait_decl(&mut self, t: &TraitDecl) {
        self.write_indent();
        if t.is_pub {
            self.push_str("pub ");
        }
        self.push_str("trait ");
        self.push_str(&t.name);
        self.push_str(" {");
        self.indent += 1;
        for method in &t.methods {
            self.newline();
            self.write_indent();
            self.push_str("fn ");
            self.push_str(&method.name);
            self.push_str("(");
            self.format_params(&method.params);
            self.push_str(")");
            if let Some(ref ty) = method.return_type {
                self.push_str(" -> ");
                self.format_type_expr(ty);
            }
            if let Some(ref body) = method.default_body {
                self.push_str(" ");
                self.format_block(body);
            }
        }
        self.indent -= 1;
        self.newline();
        self.write_indent();
        self.push_str("}");
    }

    fn format_trait_impl_decl(&mut self, t: &TraitImplDecl) {
        self.write_indent();
        self.push_str("impl ");
        self.push_str(&t.trait_name);
        self.push_str(" for ");
        self.push_str(&t.type_name);
        self.push_str(" {");
        self.indent += 1;
        for (idx, method) in t.methods.iter().enumerate() {
            if idx > 0 {
                self.newline();
            }
            self.newline();
            self.format_fn_decl(method);
        }
        self.indent -= 1;
        self.newline();
        self.write_indent();
        self.push_str("}");
    }

    fn format_import(&mut self, i: &ImportDecl) {
        self.write_indent();
        if i.is_pub {
            self.push_str("pub ");
        }
        self.push_str("import ");
        if i.is_local {
            self.push_str(".");
        }
        self.push_str(&i.path.join("."));
        if let Some(ref items) = i.items {
            self.push_str(" { ");
            self.push_str(&items.join(", "));
            self.push_str(" }");
        }
        if let Some(ref alias) = i.alias {
            self.push_str(" as ");
            self.push_str(alias);
        }
    }

    // ── Statements ──

    fn format_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let {
                pattern,
                type_ann,
                value,
            } => {
                self.write_indent();
                self.push_str("let ");
                self.format_pattern(pattern);
                if let Some(ref ty) = type_ann {
                    self.push_str(": ");
                    self.format_type_expr(ty);
                }
                self.push_str(" = ");
                self.format_expr(value);
            }
            Stmt::Mut {
                name,
                type_ann,
                value,
            } => {
                self.write_indent();
                self.push_str("mut ");
                self.push_str(name);
                if let Some(ref ty) = type_ann {
                    self.push_str(": ");
                    self.format_type_expr(ty);
                }
                self.push_str(" = ");
                self.format_expr(value);
            }
            Stmt::Expr(expr) => {
                self.write_indent();
                self.format_expr(expr);
            }
            Stmt::Return(val) => {
                self.write_indent();
                self.push_str("return");
                if let Some(ref e) = val {
                    self.push_str(" ");
                    self.format_expr(e);
                }
            }
            Stmt::Break(val) => {
                self.write_indent();
                self.push_str("break");
                if let Some(ref e) = val {
                    self.push_str(" ");
                    self.format_expr(e);
                }
            }
            Stmt::Continue => {
                self.write_indent();
                self.push_str("continue");
            }
            Stmt::For {
                pattern,
                iterable,
                body,
            } => {
                self.write_indent();
                self.push_str("for ");
                self.format_pattern(pattern);
                self.push_str(" in ");
                self.format_expr(iterable);
                self.push_str(" ");
                self.format_block(body);
            }
            Stmt::While { condition, body } => {
                self.write_indent();
                self.push_str("while ");
                self.format_expr(condition);
                self.push_str(" ");
                self.format_block(body);
            }
            Stmt::Loop { body } => {
                self.write_indent();
                self.push_str("loop ");
                self.format_block(body);
            }
            Stmt::Defer(expr) => {
                self.write_indent();
                self.push_str("defer ");
                self.format_expr(expr);
            }
            Stmt::TryCatch {
                try_body,
                catches,
                finally_body,
            } => {
                self.write_indent();
                self.push_str("try ");
                self.format_block(try_body);
                for catch in catches {
                    self.push_str(" catch ");
                    if let Some(ref ty) = catch.error_type {
                        self.push_str(ty);
                        self.push_str(" ");
                    }
                    if let Some(ref name) = catch.binding {
                        self.push_str(name);
                        self.push_str(" ");
                    }
                    self.format_block(&catch.body);
                }
                if let Some(ref finally) = finally_body {
                    self.push_str(" finally ");
                    self.format_block(finally);
                }
            }
            Stmt::Assign { target, value } => {
                self.write_indent();
                self.format_expr(target);
                self.push_str(" = ");
                self.format_expr(value);
            }
            Stmt::CompoundAssign { target, op, value } => {
                self.write_indent();
                self.format_expr(target);
                self.push_str(" ");
                self.push_str(compound_op_str(*op));
                self.push_str(" ");
                self.format_expr(value);
            }
        }
    }

    // ── Expressions ──

    /// Render a single expression back to source.
    ///
    /// `assert` uses this to quote the condition it was handed, so a failure
    /// can name the code that failed rather than just the `false` it reduced
    /// to. Expressions carry no spans, so re-emitting the AST is the only way
    /// to get the text back.
    pub fn expr_to_source(expr: &Expr) -> String {
        let mut f = Formatter::new();
        f.format_expr(expr);
        f.output
    }

    fn format_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::IntLit(n) => {
                self.push_str(&n.to_string());
            }
            Expr::FloatLit(f) => {
                self.push_str(&format_float(*f));
            }
            Expr::StringLit(s) => {
                self.push_str("\"");
                self.push_str(&escape_string(s));
                self.push_str("\"");
            }
            Expr::InterpolatedString(parts) => {
                self.push_str("\"");
                for part in parts {
                    match part {
                        AstStringPart::Literal(s) => self.push_str(&escape_string(s)),
                        AstStringPart::Expr(e) => {
                            self.push_str("${");
                            self.format_expr(e);
                            self.push_str("}");
                        }
                        AstStringPart::RawExpr(e) => {
                            self.push_str("!{");
                            self.format_expr(e);
                            self.push_str("}");
                        }
                    }
                }
                self.push_str("\"");
            }
            Expr::BoolLit(b) => {
                self.push_str(if *b { "true" } else { "false" });
            }
            Expr::NullLit => {
                self.push_str("null");
            }
            Expr::ListLit(elems) => {
                if elems.is_empty() {
                    self.push_str("[]");
                } else if elems.len() <= 3 && !has_complex_expr(elems) {
                    self.push_str("[");
                    for (i, e) in elems.iter().enumerate() {
                        if i > 0 {
                            self.push_str(", ");
                        }
                        self.format_expr(e);
                    }
                    self.push_str("]");
                } else {
                    self.push_str("[");
                    self.indent += 1;
                    for e in elems {
                        self.newline();
                        self.write_indent();
                        self.format_expr(e);
                        self.push_str(",");
                    }
                    self.indent -= 1;
                    self.newline();
                    self.write_indent();
                    self.push_str("]");
                }
            }
            Expr::MapLit(entries) => {
                if entries.is_empty() {
                    self.push_str("{}");
                } else if entries.len() <= 2 && !has_complex_map_entries(entries) {
                    self.push_str("{ ");
                    for (i, entry) in entries.iter().enumerate() {
                        if i > 0 {
                            self.push_str(", ");
                        }
                        match entry {
                            MapEntry::Pair(k, v) => {
                                self.format_expr(k);
                                self.push_str(": ");
                                self.format_expr(v);
                            }
                            MapEntry::Spread(e) => {
                                self.push_str("...");
                                self.format_expr(e);
                            }
                        }
                    }
                    self.push_str(" }");
                } else {
                    self.push_str("{");
                    self.indent += 1;
                    for entry in entries {
                        self.newline();
                        self.write_indent();
                        match entry {
                            MapEntry::Pair(k, v) => {
                                self.format_expr(k);
                                self.push_str(": ");
                                self.format_expr(v);
                                self.push_str(",");
                            }
                            MapEntry::Spread(e) => {
                                self.push_str("...");
                                self.format_expr(e);
                                self.push_str(",");
                            }
                        }
                    }
                    self.indent -= 1;
                    self.newline();
                    self.write_indent();
                    self.push_str("}");
                }
            }
            Expr::SetLit(elems) => {
                self.push_str("#{");
                for (i, e) in elems.iter().enumerate() {
                    if i > 0 {
                        self.push_str(", ");
                    }
                    self.format_expr(e);
                }
                self.push_str("}");
            }
            Expr::TupleLit(elems) => {
                self.push_str("(");
                for (i, e) in elems.iter().enumerate() {
                    if i > 0 {
                        self.push_str(", ");
                    }
                    self.format_expr(e);
                }
                if elems.len() == 1 {
                    self.push_str(",");
                }
                self.push_str(")");
            }
            Expr::CmdLit(parts) => {
                self.push_str("`");
                for part in parts {
                    match part {
                        AstStringPart::Literal(s) => self.push_str(s),
                        AstStringPart::Expr(e) => {
                            self.push_str("${");
                            self.format_expr(e);
                            self.push_str("}");
                        }
                        AstStringPart::RawExpr(e) => {
                            self.push_str("!{");
                            self.format_expr(e);
                            self.push_str("}");
                        }
                    }
                }
                self.push_str("`");
            }
            Expr::DurationLit(val, unit) => {
                // Format duration: strip trailing .0 if integer
                if *val == (*val as i64) as f64 {
                    self.push_str(&(*val as i64).to_string());
                } else {
                    self.push_str(&val.to_string());
                }
                self.push_str(&unit.to_string());
            }
            Expr::RegexLit(r) => {
                self.push_str("re\"");
                // The lexer resolves \" → " and \\ → \ inside regex literals;
                // everything else (\d, \s, …) is stored verbatim as two chars.
                // Re-escape: a bare " always came from \", and a \ that is
                // followed by \ or " (or sits at end-of-string) came from \\.
                let chars: Vec<char> = r.chars().collect();
                let mut i = 0;
                while i < chars.len() {
                    match chars[i] {
                        '"' => {
                            self.push_str("\\\"");
                            i += 1;
                        }
                        '\\' => {
                            let next = chars.get(i + 1).copied();
                            if next == Some('\\') || next == Some('"') || next.is_none() {
                                // Came from \\ in source — re-escape
                                self.push_str("\\\\");
                            } else {
                                // First char of \d, \s, etc. — emit both chars as-is
                                self.output.push('\\');
                                self.output.push(next.unwrap());
                                i += 1; // skip the already-emitted second char
                            }
                            i += 1;
                        }
                        c => {
                            self.output.push(c);
                            i += 1;
                        }
                    }
                }
                self.push_str("\"");
            }
            Expr::SemverLit(v) => {
                self.push_str("v\"");
                self.push_str(v);
                self.push_str("\"");
            }
            Expr::Ident(name) => {
                self.push_str(name);
            }
            Expr::BinaryOp { left, op, right } => {
                let needs_parens_left = needs_parens(left, *op, true);
                let needs_parens_right = needs_parens(right, *op, false);

                if needs_parens_left {
                    self.push_str("(");
                }
                self.format_expr(left);
                if needs_parens_left {
                    self.push_str(")");
                }

                self.push_str(" ");
                self.push_str(bin_op_str(*op));
                self.push_str(" ");

                if needs_parens_right {
                    self.push_str("(");
                }
                self.format_expr(right);
                if needs_parens_right {
                    self.push_str(")");
                }
            }
            Expr::UnaryOp { op, expr } => {
                self.push_str(unary_op_str(*op));
                let parens = needs_parens_for_prefix(expr);
                if parens {
                    self.push_str("(");
                }
                self.format_expr(expr);
                if parens {
                    self.push_str(")");
                }
            }
            Expr::Call { callee, args } => {
                self.format_expr(callee);
                self.push_str("(");
                self.format_call_args(args);
                self.push_str(")");
            }
            Expr::MethodCall {
                object,
                method,
                args,
            } => {
                let parens = needs_parens_for_postfix(object);
                if parens { self.push_str("("); }
                self.format_expr(object);
                if parens { self.push_str(")"); }
                self.push_str(".");
                self.push_str(method);
                self.push_str("(");
                self.format_call_args(args);
                self.push_str(")");
            }
            Expr::FieldAccess { object, field } => {
                let parens = needs_parens_for_postfix(object);
                if parens { self.push_str("("); }
                self.format_expr(object);
                if parens { self.push_str(")"); }
                self.push_str(".");
                self.push_str(field);
            }
            Expr::OptionalAccess { object, field, args } => {
                let parens = needs_parens_for_postfix(object);
                if parens { self.push_str("("); }
                self.format_expr(object);
                if parens { self.push_str(")"); }
                self.push_str("?.");
                self.push_str(field);
                if let Some(args) = args {
                    self.push_str("(");
                    self.format_call_args(args);
                    self.push_str(")");
                }
            }
            Expr::Index { object, index } => {
                let parens = needs_parens_for_postfix(object);
                if parens { self.push_str("("); }
                self.format_expr(object);
                if parens { self.push_str(")"); }
                self.push_str("[");
                self.format_expr(index);
                self.push_str("]");
            }
            Expr::Lambda { params, body } => {
                self.push_str("|");
                self.format_params(params);
                self.push_str("| ");
                self.format_expr(body);
            }
            Expr::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.push_str("if ");
                self.format_expr(condition);
                self.push_str(" ");
                self.format_block(then_branch);
                if let Some(ref else_expr) = else_branch {
                    self.push_str(" else ");
                    match else_expr.as_ref() {
                        Expr::If { .. } => self.format_expr(else_expr),
                        Expr::Block(block) => self.format_block(block),
                        _ => self.format_expr(else_expr),
                    }
                }
            }
            Expr::IfLet {
                pattern,
                value,
                then_branch,
                else_branch,
            } => {
                self.push_str("if let ");
                self.format_pattern(pattern);
                self.push_str(" = ");
                self.format_expr(value);
                self.push_str(" ");
                self.format_block(then_branch);
                if let Some(ref else_expr) = else_branch {
                    self.push_str(" else ");
                    match else_expr.as_ref() {
                        Expr::Block(block) => self.format_block(block),
                        _ => self.format_expr(else_expr),
                    }
                }
            }
            Expr::Match { subject, arms } => {
                self.push_str("match ");
                self.format_expr(subject);
                self.push_str(" {");
                self.indent += 1;
                for arm in arms {
                    self.newline();
                    self.write_indent();
                    self.format_pattern(&arm.pattern);
                    if let Some(ref guard) = arm.guard {
                        self.push_str(" if ");
                        self.format_expr(guard);
                    }
                    self.push_str(" => ");
                    self.format_expr(&arm.body);
                    self.push_str(",");
                }
                self.indent -= 1;
                self.newline();
                self.write_indent();
                self.push_str("}");
            }
            Expr::Block(block) => {
                self.format_block(block);
            }
            Expr::WithContext {
                manager,
                name,
                body,
            } => {
                self.push_str("with ");
                self.format_expr(manager);
                if name != "_" {
                    self.push_str(" as ");
                    self.push_str(name);
                }
                self.push_str(" ");
                self.format_block(body);
            }
            Expr::Pipe { left, right } => {
                self.format_expr(left);
                self.newline();
                self.write_indent();
                self.push_str("|> ");
                self.format_expr(right);
            }
            Expr::Try(inner) => {
                self.format_expr(inner);
                self.push_str("?");
            }
            Expr::NullCoalesce { left, right } => {
                self.format_expr(left);
                self.push_str(" ?? ");
                self.format_expr(right);
            }
            Expr::Range {
                start,
                end,
                inclusive,
            } => {
                if let Some(ref s) = start {
                    self.format_expr(s);
                }
                self.push_str(if *inclusive { "..=" } else { ".." });
                if let Some(ref e) = end {
                    self.format_expr(e);
                }
            }
            Expr::Loop { body } => {
                self.push_str("loop ");
                self.format_block(body);
            }
            Expr::Spread(inner) => {
                self.push_str("...");
                self.format_expr(inner);
            }
            Expr::StructLit { name, fields } => {
                self.push_str(name);
                if fields.is_empty() {
                    self.push_str(" {}");
                } else if fields.len() <= 2 && !has_complex_struct_fields(fields) {
                    self.push_str(" { ");
                    for (i, (fname, fval)) in fields.iter().enumerate() {
                        if i > 0 {
                            self.push_str(", ");
                        }
                        self.push_str(fname);
                        self.push_str(": ");
                        self.format_expr(fval);
                    }
                    self.push_str(" }");
                } else {
                    self.push_str(" {");
                    self.indent += 1;
                    for (fname, fval) in fields {
                        self.newline();
                        self.write_indent();
                        self.push_str(fname);
                        self.push_str(": ");
                        self.format_expr(fval);
                        self.push_str(",");
                    }
                    self.indent -= 1;
                    self.newline();
                    self.write_indent();
                    self.push_str("}");
                }
            }
            Expr::Spawn(inner) => {
                self.push_str("spawn ");
                self.format_expr(inner);
            }
            Expr::Parallel(branches) => {
                self.push_str("parallel {");
                self.indent += 1;
                for branch in branches {
                    self.newline();
                    self.write_indent();
                    if let Some(ref label) = branch.label {
                        self.push_str(label);
                        self.push_str(": ");
                    }
                    self.format_expr(&branch.body);
                    self.push_str(",");
                }
                self.indent -= 1;
                self.newline();
                self.write_indent();
                self.push_str("}");
            }
            Expr::PathLit(parts) => {
                self.push_str("p\"");
                for part in parts {
                    match part {
                        AstStringPart::Literal(s) => self.push_str(s),
                        AstStringPart::Expr(e) => {
                            self.push_str("${");
                            self.format_expr(e);
                            self.push_str("}");
                        }
                        AstStringPart::RawExpr(e) => {
                            self.push_str("${");
                            self.format_expr(e);
                            self.push_str("}");
                        }
                    }
                }
                self.push_str("\"");
            }
            Expr::GlobLit(parts) => {
                self.push_str("g\"");
                for part in parts {
                    match part {
                        AstStringPart::Literal(s) => self.push_str(s),
                        AstStringPart::Expr(e) => {
                            self.push_str("${");
                            self.format_expr(e);
                            self.push_str("}");
                        }
                        AstStringPart::RawExpr(e) => {
                            self.push_str("${");
                            self.format_expr(e);
                            self.push_str("}");
                        }
                    }
                }
                self.push_str("\"");
            }
        }
    }

    // ── Helpers ──

    fn format_block(&mut self, block: &Block) {
        self.push_str("{");
        if block.stmts.is_empty() && block.expr.is_none() {
            self.push_str("}");
            return;
        }
        self.indent += 1;
        for (_, stmt) in &block.stmts {
            self.newline();
            self.format_stmt(stmt);
        }
        if let Some(ref expr) = block.expr {
            self.newline();
            self.write_indent();
            self.format_expr(expr);
        }
        self.indent -= 1;
        self.newline();
        self.write_indent();
        self.push_str("}");
    }

    fn format_params(&mut self, params: &[Param]) {
        for (i, param) in params.iter().enumerate() {
            if i > 0 {
                self.push_str(", ");
            }
            if param.rest {
                self.push_str("...");
            }
            self.push_str(&param.name);
            if let Some(ref ty) = param.type_ann {
                self.push_str(": ");
                self.format_type_expr(ty);
            }
            if let Some(ref default) = param.default {
                self.push_str(" = ");
                self.format_expr(default);
            }
        }
    }

    fn format_call_args(&mut self, args: &[CallArg]) {
        for (i, arg) in args.iter().enumerate() {
            if i > 0 {
                self.push_str(", ");
            }
            if let Some(ref name) = arg.name {
                self.push_str(name);
                self.push_str(": ");
            }
            self.format_expr(&arg.value);
        }
    }

    fn format_type_expr(&mut self, ty: &TypeExpr) {
        match ty {
            TypeExpr::Named(name) => self.push_str(name),
            TypeExpr::Generic(name, args) => {
                self.push_str(name);
                self.push_str("<");
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        self.push_str(", ");
                    }
                    self.format_type_expr(arg);
                }
                self.push_str(">");
            }
        }
    }

    fn format_pattern(&mut self, pattern: &Pattern) {
        match pattern {
            Pattern::Wildcard => self.push_str("_"),
            Pattern::Ident(name) => self.push_str(name),
            Pattern::IntLit(n) => self.push_str(&n.to_string()),
            Pattern::FloatLit(f) => self.push_str(&format_float(*f)),
            Pattern::StringLit(s) => {
                self.push_str("\"");
                self.push_str(&escape_string(s));
                self.push_str("\"");
            }
            Pattern::BoolLit(b) => self.push_str(if *b { "true" } else { "false" }),
            Pattern::NullLit => self.push_str("null"),
            Pattern::Glob(g) => {
                self.push_str("glob(\"");
                self.push_str(g);
                self.push_str("\")");
            }
            Pattern::List(elems, rest) => {
                self.push_str("[");
                for (i, p) in elems.iter().enumerate() {
                    if i > 0 {
                        self.push_str(", ");
                    }
                    self.format_pattern(p);
                }
                if let Some(ref rest_pat) = rest {
                    if !elems.is_empty() {
                        self.push_str(", ");
                    }
                    self.push_str("...");
                    self.format_pattern(rest_pat);
                }
                self.push_str("]");
            }
            Pattern::Tuple(elems) => {
                self.push_str("(");
                for (i, p) in elems.iter().enumerate() {
                    if i > 0 {
                        self.push_str(", ");
                    }
                    self.format_pattern(p);
                }
                self.push_str(")");
            }
            Pattern::Struct(fields, rest) => {
                self.push_str("{ ");
                for (i, (name, pat)) in fields.iter().enumerate() {
                    if i > 0 {
                        self.push_str(", ");
                    }
                    self.push_str(name);
                    if let Some(ref p) = pat {
                        self.push_str(": ");
                        self.format_pattern(p);
                    }
                }
                if let Some(ref rest_name) = rest {
                    if !fields.is_empty() {
                        self.push_str(", ");
                    }
                    self.push_str("...");
                    self.push_str(rest_name);
                }
                self.push_str(" }");
            }
            Pattern::Instance(enum_name, type_name, fields, rest) => {
                if let Some(enum_name) = enum_name {
                    self.push_str(enum_name);
                    self.push_str(".");
                }
                self.push_str(type_name);
                self.push_str(" { ");
                for (i, (name, pat)) in fields.iter().enumerate() {
                    if i > 0 {
                        self.push_str(", ");
                    }
                    self.push_str(name);
                    if let Some(ref p) = pat {
                        self.push_str(": ");
                        self.format_pattern(p);
                    }
                }
                if let Some(ref rest_name) = rest {
                    if !fields.is_empty() {
                        self.push_str(", ");
                    }
                    self.push_str("...");
                    self.push_str(rest_name);
                }
                self.push_str(" }");
            }
            Pattern::Enum(enum_name, variant, args) => {
                if let Some(enum_name) = enum_name {
                    self.push_str(enum_name);
                    self.push_str(".");
                }
                self.push_str(variant);
                if !args.is_empty() {
                    self.push_str("(");
                    for (i, p) in args.iter().enumerate() {
                        if i > 0 {
                            self.push_str(", ");
                        }
                        self.format_pattern(p);
                    }
                    self.push_str(")");
                }
            }
            Pattern::Range(start, end, inclusive) => {
                if let Some(ref s) = start {
                    self.format_pattern(s);
                }
                self.push_str(if *inclusive { "..=" } else { ".." });
                if let Some(ref e) = end {
                    self.format_pattern(e);
                }
            }
            Pattern::Or(patterns) => {
                for (i, p) in patterns.iter().enumerate() {
                    if i > 0 {
                        self.push_str(" | ");
                    }
                    self.format_pattern(p);
                }
            }
            Pattern::Binding(name, pat) => {
                self.push_str(name);
                self.push_str(" @ ");
                self.format_pattern(pat);
            }
        }
    }

    // ── Low-level output ──

    fn push_str(&mut self, s: &str) {
        self.output.push_str(s);
    }

    fn newline(&mut self) {
        self.output.push('\n');
    }

    fn write_indent(&mut self) {
        for _ in 0..self.indent {
            self.output.push_str(INDENT);
        }
    }
}

// ── Free helper functions ──

fn escape_string(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '"' => result.push_str("\\\""),
            '\\' => result.push_str("\\\\"),
            '\n' => result.push_str("\\n"),
            '\t' => result.push_str("\\t"),
            '\r' => result.push_str("\\r"),
            c => result.push(c),
        }
    }
    result
}

fn format_float(f: f64) -> String {
    let s = f.to_string();
    if s.contains('.') {
        s
    } else {
        format!("{}.0", s)
    }
}

pub fn bin_op_str(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Mod => "%",
        BinOp::Pow => "**",
        BinOp::Eq => "==",
        BinOp::NotEq => "!=",
        BinOp::Lt => "<",
        BinOp::Gt => ">",
        BinOp::LtEq => "<=",
        BinOp::GtEq => ">=",
        BinOp::And => "&&",
        BinOp::Or => "||",
        BinOp::BitAnd => "&",
        BinOp::BitOr => "|",
        BinOp::BitXor => "^",
        BinOp::Shl => "<<",
        BinOp::Shr => ">>",
    }
}

fn compound_op_str(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "+=",
        BinOp::Sub => "-=",
        BinOp::Mul => "*=",
        BinOp::Div => "/=",
        _ => "+=", // fallback (shouldn't happen)
    }
}

fn unary_op_str(op: UnaryOp) -> &'static str {
    match op {
        UnaryOp::Neg => "-",
        UnaryOp::Not => "!",
        UnaryOp::BitNot => "~",
    }
}

fn op_precedence(op: BinOp) -> u8 {
    match op {
        BinOp::Or => 1,
        BinOp::And => 2,
        BinOp::BitOr => 3,
        BinOp::BitXor => 4,
        BinOp::BitAnd => 5,
        BinOp::Eq | BinOp::NotEq => 6,
        BinOp::Lt | BinOp::Gt | BinOp::LtEq | BinOp::GtEq => 7,
        BinOp::Shl | BinOp::Shr => 8,
        BinOp::Add | BinOp::Sub => 9,
        BinOp::Mul | BinOp::Div | BinOp::Mod => 10,
        BinOp::Pow => 11,
    }
}

fn needs_parens(expr: &Expr, parent_op: BinOp, is_left: bool) -> bool {
    match expr {
        Expr::BinaryOp { op, .. } => {
            let (child, parent) = (op_precedence(*op), op_precedence(parent_op));
            if child < parent {
                return true;
            }
            // Equal precedence only reassociates safely on the left. `a - b - c`
            // is `(a - b) - c`, so a right-hand child at the same level has to
            // keep its parentheses or `a - (b - c)` changes meaning.
            child == parent && !is_left
        }
        // Infix forms with no precedence entry of their own; parenthesise them
        // rather than guess.
        Expr::Pipe { .. } | Expr::NullCoalesce { .. } | Expr::Lambda { .. } => true,
        _ => false,
    }
}

/// Returns true when `expr` must be wrapped in `(…)` after a prefix operator
/// (`!`, `-`, `~`).
///
/// Prefix operators bind tighter than every infix operator, so dropping the
/// parentheses from `!(a == b)` silently rewrites it as `(!a) == b` — a
/// different answer, not just different formatting.
fn needs_parens_for_prefix(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::BinaryOp { .. }
            | Expr::UnaryOp { .. }
            | Expr::Pipe { .. }
            | Expr::NullCoalesce { .. }
            | Expr::Lambda { .. }
    )
}

/// Returns true when `expr` must be wrapped in `(…)` before a postfix
/// operator (`.method()`, `.field`, `[index]`, `(call)`).  These operators
/// bind tighter than any infix operator, so any infix/unary/pipe expression
/// used as an object/callee needs parentheses to preserve semantics.
fn needs_parens_for_postfix(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::BinaryOp { .. }
            | Expr::UnaryOp { .. }
            | Expr::Pipe { .. }
            | Expr::NullCoalesce { .. }
            | Expr::Lambda { .. }
    )
}

fn has_complex_expr(exprs: &[Expr]) -> bool {
    exprs.iter().any(|e| match e {
        Expr::MapLit(_) => true,
        Expr::ListLit(v) => v.len() > 2,
        _ => false,
    })
}

fn has_complex_map_entries(entries: &[MapEntry]) -> bool {
    entries.iter().any(|e| match e {
        MapEntry::Pair(_, v) => matches!(v, Expr::MapLit(_) | Expr::ListLit(_) | Expr::Block(_)),
        MapEntry::Spread(_) => false,
    })
}

fn has_complex_struct_fields(fields: &[(String, Expr)]) -> bool {
    fields
        .iter()
        .any(|(_, v)| matches!(v, Expr::MapLit(_) | Expr::ListLit(_) | Expr::Block(_)))
}
