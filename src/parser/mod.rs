/// Recursive descent parser with Pratt-style operator precedence climbing
/// for the Que language.
///
/// Parses a token stream into an AST (see `ast.rs`).

use crate::ast::*;
use crate::error::{ErrorKind, QueError};
use crate::token::*;

mod expr;
mod patterns;

/// Classification of `{ ... }` in expression position.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum BraceKind {
    Map,
    /// The removed comma-separated brace-set form, kept only so the parser
    /// can point at `#{ ... }`.
    Set,
    Block,
}

/// How to interpret a bare uppercase identifier inside a pattern.
///
/// In a destructuring `let`/`for` binding, `Foo` is a variable name. In a
/// `match` arm, `Foo` is a unit-variant pattern. The pattern parser used to
/// thread a `bool` through every recursive call to make this distinction
/// (PAR-18); now it reads this field once per parse, set by the public
/// entry point (`parse_pattern` vs `parse_match_pattern`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum PatternMode {
    /// Bare `Ident` is always a variable binding.
    Bind,
    /// Bare uppercase `Ident` matches an enum unit variant; lowercase still binds.
    Match,
}

/// Metadata gathered from the `@...` attributes written above a `task`.
///
/// It lives outside the braces because it describes the task rather than
/// running as part of it, which is the distinction the old in-body fields
/// blurred.
#[derive(Default)]
pub(super) struct TaskAttrs {
    depends_on: Vec<Expr>,
    inputs: Vec<Expr>,
    outputs: Vec<Expr>,
    env_keys: Vec<String>,
    description: Option<String>,
    aliases: Vec<String>,
}

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    /// Current pattern-parsing mode; see `PatternMode`. Defaults to `Bind`.
    pub(super) pattern_mode: PatternMode,
    /// For each token index `i`, if `tokens[i]` is an opening or closing
    /// bracket (`(`, `[`, `{`, `)`, `]`, `}`), `bracket_pairs[i]` is the
    /// index of the matching counterpart. `usize::MAX` for non-bracket
    /// tokens and for unbalanced brackets. Built once in `new` so that
    /// helpers like `classify_braces` can skip over nested groups in O(1)
    /// instead of re-walking their interior (PAR-14).
    pub(super) bracket_pairs: Vec<usize>,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        let bracket_pairs = Self::build_bracket_pairs(&tokens);
        Self { tokens, pos: 0, pattern_mode: PatternMode::Bind, bracket_pairs }
    }

    /// Pre-compute the matching-bracket index for every opener and closer
    /// in the token stream. Each bracket family (`()`, `[]`, `{}`) is
    /// tracked on its own stack. Stray closers and unmatched openers leave
    /// `usize::MAX` in their slot; callers must handle that fallback.
    fn build_bracket_pairs(tokens: &[Token]) -> Vec<usize> {
        let mut pairs = vec![usize::MAX; tokens.len()];
        let mut paren_stack: Vec<usize> = Vec::new();
        let mut bracket_stack: Vec<usize> = Vec::new();
        let mut brace_stack: Vec<usize> = Vec::new();
        for (i, tok) in tokens.iter().enumerate() {
            match &tok.kind {
                TokenKind::LParen => paren_stack.push(i),
                TokenKind::LBracket => bracket_stack.push(i),
                TokenKind::LBrace | TokenKind::HashBrace => brace_stack.push(i),
                TokenKind::RParen => {
                    if let Some(open) = paren_stack.pop() {
                        pairs[open] = i;
                        pairs[i] = open;
                    }
                }
                TokenKind::RBracket => {
                    if let Some(open) = bracket_stack.pop() {
                        pairs[open] = i;
                        pairs[i] = open;
                    }
                }
                TokenKind::RBrace => {
                    if let Some(open) = brace_stack.pop() {
                        pairs[open] = i;
                        pairs[i] = open;
                    }
                }
                _ => {}
            }
        }
        pairs
    }

    /// Parse a complete module.
    pub fn parse_module(&mut self) -> Result<Module, QueError> {
        let mut items = Vec::new();
        self.skip_newlines();
        let (shebang, strict) = self.parse_prologue()?;
        while !self.is_at_end() {
            let span = self.current_span();
            items.push((span, self.parse_item()?));
            self.skip_newlines();
        }
        Ok(Module { items, shebang, strict })
    }

    /// Consume the `#!` lines at the top of the file: an optional interpreter
    /// line followed by any number of pragmas.
    fn parse_prologue(&mut self) -> Result<(Option<String>, bool), QueError> {
        let mut shebang = None;
        if let TokenKind::Shebang(line) = self.peek().clone() {
            shebang = Some(line);
            self.advance();
            self.skip_newlines();
        }
        let mut strict = false;
        while let TokenKind::Pragma(name) = self.peek().clone() {
            match name.as_str() {
                "strict" => strict = true,
                other => {
                    return Err(QueError::parser(
                        crate::error::ErrorKind::UnexpectedToken,
                        format!("unknown pragma `#!{}`; the only one is `#!strict`", other),
                        self.current_span(),
                    ))
                }
            }
            self.advance();
            self.skip_newlines();
        }
        Ok((shebang, strict))
    }

    // ── Helpers ──

    fn peek(&self) -> &TokenKind {
        self.tokens
            .get(self.pos)
            .map(|t| &t.kind)
            .unwrap_or(&TokenKind::Eof)
    }

    /// Look ahead `n` tokens past the current position without consuming.
    /// Returns `&TokenKind::Eof` if past end of stream.
    fn peek_nth(&self, n: usize) -> &TokenKind {
        self.tokens
            .get(self.pos + n)
            .map(|t| &t.kind)
            .unwrap_or(&TokenKind::Eof)
    }

    /// Look up a token at an absolute index without consuming.
    /// Returns `&TokenKind::Eof` if past end of stream.
    fn peek_at(&self, idx: usize) -> &TokenKind {
        self.tokens
            .get(idx)
            .map(|t| &t.kind)
            .unwrap_or(&TokenKind::Eof)
    }

    /// True if the current token is `Ident(name)` for the given soft keyword.
    /// Used for contextual keywords that are not reserved (e.g. task metadata
    /// field names) so they can still be used as ordinary identifiers
    /// elsewhere.
    fn peek_ident_eq(&self, name: &str) -> bool {
        matches!(self.peek(), TokenKind::Ident(n) if n == name)
    }

    fn peek_token(&self) -> &Token {
        static EOF_TOKEN: Token = Token {
            kind: TokenKind::Eof,
            span: Span::new(0, 0, 0, 0),
        };
        self.tokens.get(self.pos).unwrap_or(&EOF_TOKEN)
    }

    fn current_span(&self) -> Span {
        self.peek_token().span
    }

    fn advance(&mut self) -> &Token {
        let tok = &self.tokens[self.pos.min(self.tokens.len() - 1)];
        if self.pos < self.tokens.len() {
            self.pos += 1;
        }
        tok
    }

    /// Consume the current token if its variant matches `kind`. Matching is
    /// by variant discriminant only — the inner payload of `kind` is ignored,
    /// so callers can pass dummy values like `TokenKind::Ident(String::new())`
    /// when they just want to assert "any identifier". Error messages use the
    /// canonical `display_name()` of the expected and actual kinds to avoid
    /// leaking the dummy payload (PAR-17).
    fn expect(&mut self, kind: &TokenKind) -> Result<&Token, QueError> {
        if std::mem::discriminant(self.peek()) == std::mem::discriminant(kind) {
            Ok(self.advance())
        } else {
            Err(QueError::parser(
                ErrorKind::ExpectedToken,
                format!(
                    "expected {}, got {}",
                    kind.display_name(),
                    self.peek().display_name()
                ),
                self.current_span(),
            ))
        }
    }

    fn expect_ident(&mut self) -> Result<String, QueError> {
        match self.peek().clone() {
            TokenKind::Ident(name) => {
                self.advance();
                Ok(name)
            }
            _ => Err(QueError::parser(
                ErrorKind::ExpectedToken,
                format!("expected identifier, got {:?}", self.peek()),
                self.current_span(),
            )),
        }
    }

    /// Like [`Self::expect_ident`], but also accepts keywords that double as
    /// method names after a `.` (currently only `try`, as in `` `cmd`.try() ``).
    fn expect_member_name(&mut self) -> Result<String, QueError> {
        if matches!(self.peek(), TokenKind::Try) {
            self.advance();
            return Ok("try".to_string());
        }
        self.expect_ident()
    }

    #[allow(dead_code)]
    fn check(&self, kind: &TokenKind) -> bool {
        std::mem::discriminant(self.peek()) == std::mem::discriminant(kind)
    }

    fn is_at_end(&self) -> bool {
        matches!(self.peek(), TokenKind::Eof)
    }

    fn skip_newlines(&mut self) {
        while matches!(self.peek(), TokenKind::Newline | TokenKind::Semicolon) {
            self.advance();
        }
    }

    fn skip_separator(&mut self) {
        while matches!(
            self.peek(),
            TokenKind::Newline | TokenKind::Semicolon | TokenKind::Comma
        ) {
            self.advance();
        }
    }

    // ── Top-level items ──

    fn parse_item(&mut self) -> Result<Item, QueError> {
        // `@name(...)` attributes describe the declaration that follows.
        if matches!(self.peek(), TokenKind::At) {
            let attrs = self.parse_task_attrs()?;
            return match self.peek() {
                TokenKind::Task => {
                    self.advance();
                    Ok(Item::TaskDecl(self.parse_task_decl(attrs)?))
                }
                other => Err(QueError::parser(
                    ErrorKind::UnexpectedToken,
                    format!(
                        "task attributes must be followed by a `task` declaration, got {}",
                        other.display_name()
                    ),
                    self.current_span(),
                )),
            };
        }

        let is_pub = if matches!(self.peek(), TokenKind::Pub) {
            self.advance();
            self.skip_newlines();
            true
        } else {
            false
        };

        match self.peek() {
            TokenKind::Let if is_pub => {
                self.advance();
                let pattern = self.parse_pattern()?;
                let type_ann = if matches!(self.peek(), TokenKind::Colon) {
                    self.advance();
                    Some(self.parse_type_expr()?)
                } else {
                    None
                };
                self.expect(&TokenKind::Eq)?;
                self.skip_newlines();
                let value = self.parse_expr()?;
                self.skip_newlines();
                Ok(Item::PubLet { pattern, type_ann, value })
            }
            TokenKind::Fn => {
                self.advance();
                let decl = self.parse_fn_decl(is_pub)?;
                Ok(Item::FnDecl(decl))
            }
            TokenKind::Task => {
                self.advance();
                Ok(Item::TaskDecl(self.parse_task_decl(TaskAttrs::default())?))
            }
            TokenKind::Type => {
                self.advance();
                let decl = self.parse_type_decl(is_pub)?;
                Ok(Item::TypeDecl(decl))
            }
            TokenKind::Enum => {
                self.advance();
                let decl = self.parse_enum_decl(is_pub)?;
                Ok(Item::EnumDecl(decl))
            }
            TokenKind::Import => {
                self.advance();
                let decl = self.parse_import(is_pub)?;
                Ok(Item::Import(decl))
            }
            TokenKind::Struct => {
                self.advance();
                let decl = self.parse_struct_decl(is_pub)?;
                Ok(Item::StructDecl(decl))
            }
            TokenKind::Impl => {
                self.advance();
                self.skip_newlines();
                // Check for `impl TraitName for TypeName { ... }`
                let first_name = self.expect_ident()?;
                self.skip_newlines();
                if matches!(self.peek(), TokenKind::For) {
                    self.advance(); // consume `for`
                    self.skip_newlines();
                    let type_name = self.expect_ident()?;
                    self.skip_newlines();
                    let methods = self.parse_impl_body()?;
                    Ok(Item::TraitImplDecl(TraitImplDecl {
                        trait_name: first_name,
                        type_name,
                        methods,
                    }))
                } else {
                    // `impl TypeName { ... }`
                    let methods = self.parse_impl_body()?;
                    Ok(Item::ImplDecl(ImplDecl {
                        type_name: first_name,
                        methods,
                    }))
                }
            }
            TokenKind::Trait => {
                self.advance();
                let decl = self.parse_trait_decl(is_pub)?;
                Ok(Item::TraitDecl(decl))
            }
            _ => Ok(Item::Stmt(self.parse_stmt()?)),
        }
    }

    // ── Struct declaration ──

    fn parse_struct_decl(&mut self, is_pub: bool) -> Result<StructDecl, QueError> {
        self.skip_newlines();
        let name = self.expect_ident()?;
        self.skip_newlines();
        self.expect(&TokenKind::LBrace)?;
        self.skip_newlines();
        let mut fields = Vec::new();
        while !matches!(self.peek(), TokenKind::RBrace | TokenKind::Eof) {
            let field_name = self.expect_ident()?;
            let type_ann = if matches!(self.peek(), TokenKind::Colon) {
                self.advance();
                self.skip_newlines();
                Some(self.parse_type_expr()?)
            } else {
                None
            };
            let default = if matches!(self.peek(), TokenKind::Eq) {
                self.advance();
                self.skip_newlines();
                Some(self.parse_expr()?)
            } else {
                None
            };
            fields.push(StructField { name: field_name, type_ann, default });
            self.skip_separator();
        }
        self.expect(&TokenKind::RBrace)?;
        Ok(StructDecl { name, fields, is_pub })
    }

    // ── Impl body (shared between impl Type and impl Trait for Type) ──

    fn parse_impl_body(&mut self) -> Result<Vec<FnDecl>, QueError> {
        self.expect(&TokenKind::LBrace)?;
        self.skip_newlines();
        let mut methods = Vec::new();
        while !matches!(self.peek(), TokenKind::RBrace | TokenKind::Eof) {
            let is_pub = if matches!(self.peek(), TokenKind::Pub) {
                self.advance();
                self.skip_newlines();
                true
            } else {
                false
            };
            self.expect(&TokenKind::Fn)?;
            let decl = self.parse_fn_decl(is_pub)?;
            methods.push(decl);
            self.skip_newlines();
        }
        self.expect(&TokenKind::RBrace)?;
        Ok(methods)
    }

    // ── Trait declaration ──

    fn parse_trait_decl(&mut self, is_pub: bool) -> Result<TraitDecl, QueError> {
        self.skip_newlines();
        let name = self.expect_ident()?;
        self.skip_newlines();
        self.expect(&TokenKind::LBrace)?;
        self.skip_newlines();
        let mut methods = Vec::new();
        while !matches!(self.peek(), TokenKind::RBrace | TokenKind::Eof) {
            self.expect(&TokenKind::Fn)?;
            let method_name = self.expect_ident()?;
            self.expect(&TokenKind::LParen)?;
            let params = self.parse_param_list()?;
            self.expect(&TokenKind::RParen)?;
            let return_type = if matches!(self.peek(), TokenKind::Arrow) {
                self.advance();
                Some(self.parse_type_expr()?)
            } else {
                None
            };
            self.skip_newlines();
            // Optional default body
            let default_body = if matches!(self.peek(), TokenKind::LBrace) {
                Some(self.parse_block()?)
            } else {
                None
            };
            methods.push(TraitMethod { name: method_name, params, return_type, default_body });
            self.skip_newlines();
        }
        self.expect(&TokenKind::RBrace)?;
        Ok(TraitDecl { name, methods, is_pub })
    }

    // ── Function declaration ──

    fn parse_fn_decl(&mut self, is_pub: bool) -> Result<FnDecl, QueError> {
        self.skip_newlines();
        let name = self.expect_ident()?;
        self.expect(&TokenKind::LParen)?;
        // `mut` is only meaningful on the receiver: it is what turns a method
        // into one that writes its changes back over the value it was called
        // on. On any other parameter it would suggest the caller's argument
        // changes too, which is not what happens, so it is rejected here
        // rather than quietly ignored.
        let mutates_self = if matches!(self.peek(), TokenKind::Mut) {
            self.advance();
            match self.peek() {
                TokenKind::Ident(n) if n == "self" => {}
                _ => {
                    return Err(QueError::parser(
                        crate::error::ErrorKind::UnexpectedToken,
                        "only `self` can be declared `mut`; a parameter is a copy the caller never sees again",
                        self.current_span(),
                    ))
                }
            }
            true
        } else {
            false
        };
        let params = self.parse_param_list()?;
        self.expect(&TokenKind::RParen)?;

        let return_type = if matches!(self.peek(), TokenKind::Arrow) {
            self.advance();
            Some(self.parse_type_expr()?)
        } else {
            None
        };

        self.skip_newlines();
        let body = self.parse_block()?;

        Ok(FnDecl {
            name,
            params,
            return_type,
            body,
            is_pub,
            mutates_self,
        })
    }

    fn parse_param_list(&mut self) -> Result<Vec<Param>, QueError> {
        self.parse_param_list_inner(false)
    }

    /// `allow_rest` opens the `...name` spelling, which collects every
    /// remaining positional argument into a list. Only tasks take it: they are
    /// the ones fed straight from a command line, where the argument count is
    /// not something the caller controls.
    fn parse_param_list_inner(&mut self, allow_rest: bool) -> Result<Vec<Param>, QueError> {
        let mut params: Vec<Param> = Vec::new();
        self.skip_newlines();
        while !matches!(self.peek(), TokenKind::RParen | TokenKind::Eof) {
            let rest_span = self.current_span();
            let rest = if matches!(self.peek(), TokenKind::Spread) {
                self.advance();
                true
            } else {
                false
            };
            if rest && !allow_rest {
                return Err(QueError::parser(
                    crate::error::ErrorKind::UnexpectedToken,
                    "`...name` collects command-line arguments and is only allowed on a task parameter",
                    rest_span,
                ));
            }
            // A rest parameter that is not last could never receive anything
            // the parameters after it would not also claim, so the ambiguity
            // is refused rather than resolved.
            if let Some(prev) = params.last() {
                if prev.rest {
                    return Err(QueError::parser(
                        crate::error::ErrorKind::UnexpectedToken,
                        format!(
                            "`...{}` must be the last parameter",
                            prev.name
                        ),
                        rest_span,
                    ));
                }
            }
            let name = self.expect_ident()?;
            let type_ann = if matches!(self.peek(), TokenKind::Colon) {
                self.advance();
                Some(self.parse_type_expr()?)
            } else {
                None
            };
            let default = if matches!(self.peek(), TokenKind::Eq) {
                self.advance();
                Some(self.parse_expr()?)
            } else {
                None
            };
            // A rest parameter already has a default: no arguments left means
            // an empty list. A second one would only be reachable by never
            // being used.
            if rest && default.is_some() {
                return Err(QueError::parser(
                    crate::error::ErrorKind::UnexpectedToken,
                    format!(
                        "`...{}` cannot have a default; it is an empty list when no arguments remain",
                        name
                    ),
                    rest_span,
                ));
            }
            params.push(Param {
                name,
                type_ann,
                default,
                rest,
            });
            self.skip_newlines();
            if matches!(self.peek(), TokenKind::Comma) {
                self.advance();
                self.skip_newlines();
            }
        }
        Ok(params)
    }

    // ── Task declaration ──

    fn parse_task_decl(&mut self, attrs: TaskAttrs) -> Result<TaskDecl, QueError> {
        self.skip_newlines();
        let name = self.expect_ident()?;

        let params = if matches!(self.peek(), TokenKind::LParen) {
            self.advance();
            let p = self.parse_param_list_inner(true)?;
            self.expect(&TokenKind::RParen)?;
            p
        } else {
            Vec::new()
        };

        if let TokenKind::Ident(kw) = self.peek().clone() {
            if kw == "deps" || kw == "dependsOn" {
                return Err(QueError::parser(
                    crate::error::ErrorKind::UnexpectedToken,
                    format!(
                        "`{} [...]` after the task name moved above it; write `@deps([...])` on its own line",
                        kw
                    ),
                    self.current_span(),
                ));
            }
        }

        self.skip_newlines();
        let body = self.parse_task_body()?;

        Ok(TaskDecl {
            name,
            params,
            depends_on: attrs.depends_on,
            inputs: attrs.inputs,
            outputs: attrs.outputs,
            env_keys: attrs.env_keys,
            description: attrs.description,
            aliases: attrs.aliases,
            body,
        })
    }

    /// A task body is a block and nothing else. The metadata that used to
    /// share this scope now lives in attributes above the declaration, so the
    /// old spellings are recognised only to say where they went.
    fn parse_task_body(&mut self) -> Result<Block, QueError> {
        // The first token inside the braces, past any leading newlines.
        let mut i = self.pos;
        if matches!(self.peek_at(i), TokenKind::LBrace) {
            i += 1;
            while matches!(self.peek_at(i), TokenKind::Newline | TokenKind::Semicolon) {
                i += 1;
            }
            if let TokenKind::Ident(kw) = self.peek_at(i).clone() {
                if matches!(
                    kw.as_str(),
                    "description" | "inputs" | "outputs" | "deps" | "aliases" | "env"
                ) && matches!(self.peek_at(i + 1), TokenKind::Colon)
                {
                    return Err(QueError::parser(
                        crate::error::ErrorKind::UnexpectedToken,
                        format!(
                            "`{}:` is no longer a field inside the task body; write `@{}(...)` above the task",
                            kw, kw
                        ),
                        self.current_span(),
                    ));
                }
                if kw == "run"
                    && matches!(self.peek_at(i + 1), TokenKind::LBrace | TokenKind::Newline)
                {
                    return Err(QueError::parser(
                        crate::error::ErrorKind::UnexpectedToken,
                        "the `run { ... }` wrapper is gone; the task body is the block itself",
                        self.current_span(),
                    ));
                }
            }
        }
        self.parse_block()
    }

    /// Parse the `@name(...)` attributes that precede a `task`.
    fn parse_task_attrs(&mut self) -> Result<TaskAttrs, QueError> {
        let mut attrs = TaskAttrs::default();
        let mut seen: Vec<String> = Vec::new();
        while matches!(self.peek(), TokenKind::At) {
            self.advance(); // @
            let name = self.expect_ident()?;
            if seen.contains(&name) {
                return Err(QueError::parser(
                    crate::error::ErrorKind::UnexpectedToken,
                    format!(
                        "`@{}` is given twice on the same task; list every entry in one attribute",
                        name
                    ),
                    self.current_span(),
                ));
            }
            seen.push(name.clone());
            self.expect(&TokenKind::LParen)?;
            self.skip_newlines();
            match name.as_str() {
                "description" => {
                    match self.peek().clone() {
                        TokenKind::StringLit(s) => {
                            self.advance();
                            attrs.description = Some(s);
                        }
                        other => {
                            return Err(QueError::parser(
                                crate::error::ErrorKind::UnexpectedToken,
                                format!("@description takes a string, got {}", other.display_name()),
                                self.current_span(),
                            ))
                        }
                    }
                }
                "inputs" => attrs.inputs = self.parse_attr_expr_list()?,
                "outputs" => attrs.outputs = self.parse_attr_expr_list()?,
                "deps" => attrs.depends_on = self.parse_attr_dep_list()?,
                "aliases" => attrs.aliases = self.parse_attr_name_list("alias")?,
                "env" => attrs.env_keys = self.parse_attr_name_list("env var")?,
                other => {
                    return Err(QueError::parser(
                        crate::error::ErrorKind::UnexpectedToken,
                        format!(
                            "unknown task attribute `@{}`; expected one of \
                             description, deps, inputs, outputs, aliases, env",
                            other
                        ),
                        self.current_span(),
                    ))
                }
            }
            self.skip_newlines();
            self.expect(&TokenKind::RParen)?;
            self.skip_newlines();
        }
        Ok(attrs)
    }

    /// `[a, b, c]` inside an attribute's parentheses.
    fn parse_attr_expr_list(&mut self) -> Result<Vec<Expr>, QueError> {
        self.expect(&TokenKind::LBracket)?;
        let items = self.parse_expr_list(&TokenKind::RBracket)?;
        self.expect(&TokenKind::RBracket)?;
        Ok(items)
    }

    /// `[NAME, "NAME"]` for `@deps` — names, like `@aliases` and `@env`.
    ///
    /// A dependency is resolved by *name* when the task runs, not by the value
    /// written here, so anything that is not a name cannot work. It used to be
    /// dropped without a word: `@deps(["setup"])` — a natural thing to write,
    /// since the two neighbouring attributes do take quoted names — produced a
    /// task with no dependencies at all, and the missing dependency showed up
    /// as whatever the task did wrong for want of it. Accept the quoted form
    /// the neighbours accept, and refuse the rest here, where there is a line
    /// number to point at.
    fn parse_attr_dep_list(&mut self) -> Result<Vec<Expr>, QueError> {
        Ok(self
            .parse_attr_name_list("dependency")?
            .into_iter()
            .map(Expr::Ident)
            .collect())
    }

    /// `[NAME, "NAME"]` inside an attribute's parentheses — names, not values.
    fn parse_attr_name_list(&mut self, what: &str) -> Result<Vec<String>, QueError> {
        self.expect(&TokenKind::LBracket)?;
        self.skip_newlines();
        let mut names = Vec::new();
        while !matches!(self.peek(), TokenKind::RBracket | TokenKind::Eof) {
            match self.peek().clone() {
                TokenKind::Ident(n) | TokenKind::StringLit(n) => {
                    names.push(n);
                    self.advance();
                }
                other => {
                    return Err(QueError::parser(
                        crate::error::ErrorKind::UnexpectedToken,
                        format!("expected {} name, got {}", what, other.display_name()),
                        self.current_span(),
                    ))
                }
            }
            if matches!(self.peek(), TokenKind::Comma) {
                self.advance();
            }
            self.skip_newlines();
        }
        self.expect(&TokenKind::RBracket)?;
        Ok(names)
    }

    // ── Type / Enum declarations ──

    fn parse_type_decl(&mut self, is_pub: bool) -> Result<TypeDecl, QueError> {
        self.skip_newlines();
        let name = self.expect_ident()?;
        self.expect(&TokenKind::Eq)?;
        self.skip_newlines();
        let type_expr = self.parse_type_expr()?;
        Ok(TypeDecl {
            name,
            type_expr,
            is_pub,
        })
    }

    fn parse_enum_decl(&mut self, is_pub: bool) -> Result<EnumDecl, QueError> {
        self.skip_newlines();
        let name = self.expect_ident()?;
        self.skip_newlines();
        self.expect(&TokenKind::LBrace)?;
        self.skip_newlines();
        let mut variants = Vec::new();
        while !matches!(self.peek(), TokenKind::RBrace | TokenKind::Eof) {
            let vname = self.expect_ident()?;
            let mut fields = Vec::new();
            // A data variant may be declared with either delimiter:
            //   Running { pid: Int }   or   Running(pid: Int)
            // Both construct the same way, with either delimiter.
            let close = match self.peek() {
                TokenKind::LBrace => Some(TokenKind::RBrace),
                TokenKind::LParen => Some(TokenKind::RParen),
                _ => None,
            };
            if let Some(close) = close {
                self.advance();
                self.skip_newlines();
                while !matches!(self.peek(), TokenKind::Eof) && self.peek() != &close {
                    let fname = self.expect_ident()?;
                    self.expect(&TokenKind::Colon)?;
                    let ftype = self.parse_type_expr()?;
                    fields.push((fname, ftype));
                    self.skip_newlines();
                    if matches!(self.peek(), TokenKind::Comma) {
                        self.advance();
                        self.skip_newlines();
                    }
                }
                self.expect(&close)?;
            }
            variants.push(EnumVariant {
                name: vname,
                fields,
            });
            self.skip_newlines();
            if matches!(self.peek(), TokenKind::Comma) {
                self.advance();
                self.skip_newlines();
            }
        }
        self.expect(&TokenKind::RBrace)?;
        Ok(EnumDecl {
            name,
            variants,
            is_pub,
        })
    }

    // ── Import ──

    fn parse_import(&mut self, is_pub: bool) -> Result<ImportDecl, QueError> {
        self.skip_newlines();

        // Check for leading `.` → local import
        let is_local = if matches!(self.peek(), TokenKind::Dot) {
            self.advance();
            // After `.`, handle `.{a, b}` multi-module shorthand
            if matches!(self.peek(), TokenKind::LBrace) {
                self.advance();
                self.skip_newlines();
                let mut items = Vec::new();
                while !matches!(self.peek(), TokenKind::RBrace | TokenKind::Eof) {
                    items.push(self.expect_ident()?);
                    self.skip_newlines();
                    if matches!(self.peek(), TokenKind::Comma) {
                        self.advance();
                        self.skip_newlines();
                    }
                }
                self.expect(&TokenKind::RBrace)?;
                return Ok(ImportDecl {
                    path: vec![],
                    alias: None,
                    items: Some(items),
                    is_local: true,
                    is_pub,
                });
            }
            true
        } else {
            false
        };

        // Parse the first identifier segment
        let mut path = Vec::new();
        path.push(self.expect_ident()?);

        // Parse remaining `.segment` or `.{items}` 
        while matches!(self.peek(), TokenKind::Dot) {
            self.advance();
            // Handle `import std.{fs, path}` multi-module shorthand
            if matches!(self.peek(), TokenKind::LBrace) {
                self.advance();
                self.skip_newlines();
                let mut items = Vec::new();
                while !matches!(self.peek(), TokenKind::RBrace | TokenKind::Eof) {
                    items.push(self.expect_ident()?);
                    self.skip_newlines();
                    if matches!(self.peek(), TokenKind::Comma) {
                        self.advance();
                        self.skip_newlines();
                    }
                }
                self.expect(&TokenKind::RBrace)?;
                return Ok(ImportDecl {
                    path,
                    alias: None,
                    items: Some(items),
                    is_local,
                    is_pub,
                });
            }
            path.push(self.expect_ident()?);
        }

        // Check for `as alias` or `{ items }`
        let alias = if matches!(self.peek(), TokenKind::As) {
            self.advance();
            Some(self.expect_ident()?)
        } else {
            None
        };

        let items = if matches!(self.peek(), TokenKind::LBrace) {
            self.advance();
            self.skip_newlines();
            let mut items = Vec::new();
            while !matches!(self.peek(), TokenKind::RBrace | TokenKind::Eof) {
                // Accept `*` as a wildcard (import all exports)
                if matches!(self.peek(), TokenKind::Star) {
                    self.advance();
                    items.push("*".to_string());
                } else {
                    items.push(self.expect_ident()?);
                }
                self.skip_newlines();
                if matches!(self.peek(), TokenKind::Comma) {
                    self.advance();
                    self.skip_newlines();
                }
            }
            self.expect(&TokenKind::RBrace)?;
            Some(items)
        } else {
            None
        };

        Ok(ImportDecl { path, alias, items, is_local, is_pub })
    }

    // ── Type expressions ──

    fn parse_type_expr(&mut self) -> Result<TypeExpr, QueError> {
        let name = self.expect_ident()?;
        if matches!(self.peek(), TokenKind::Lt) {
            self.advance();
            let mut params = Vec::new();
            loop {
                params.push(self.parse_type_expr()?);
                if matches!(self.peek(), TokenKind::Comma) {
                    self.advance();
                } else {
                    break;
                }
            }
            self.expect(&TokenKind::Gt)?;
            Ok(TypeExpr::Generic(name, params))
        } else {
            Ok(TypeExpr::Named(name))
        }
    }

    // ── Statements ──

    fn parse_stmt(&mut self) -> Result<Stmt, QueError> {
        self.skip_newlines();
        match self.peek() {
            TokenKind::Let => {
                self.advance();
                let pattern = self.parse_pattern()?;
                let type_ann = if matches!(self.peek(), TokenKind::Colon) {
                    self.advance();
                    Some(self.parse_type_expr()?)
                } else {
                    None
                };
                self.expect(&TokenKind::Eq)?;
                self.skip_newlines();
                let value = self.parse_expr()?;
                self.skip_newlines();
                Ok(Stmt::Let {
                    pattern,
                    type_ann,
                    value,
                })
            }
            TokenKind::Mut => {
                self.advance();
                let name = self.expect_ident()?;
                let type_ann = if matches!(self.peek(), TokenKind::Colon) {
                    self.advance();
                    Some(self.parse_type_expr()?)
                } else {
                    None
                };
                self.expect(&TokenKind::Eq)?;
                self.skip_newlines();
                let value = self.parse_expr()?;
                self.skip_newlines();
                Ok(Stmt::Mut {
                    name,
                    type_ann,
                    value,
                })
            }
            TokenKind::Return => {
                self.advance();
                let val = if matches!(
                    self.peek(),
                    TokenKind::Newline | TokenKind::Semicolon | TokenKind::RBrace | TokenKind::Eof
                ) {
                    None
                } else {
                    Some(self.parse_expr()?)
                };
                self.skip_newlines();
                Ok(Stmt::Return(val))
            }
            TokenKind::Break => {
                self.advance();
                let val = if matches!(
                    self.peek(),
                    TokenKind::Newline | TokenKind::Semicolon | TokenKind::RBrace | TokenKind::Eof
                ) {
                    None
                } else {
                    Some(self.parse_expr()?)
                };
                self.skip_newlines();
                Ok(Stmt::Break(val))
            }
            TokenKind::Continue => {
                self.advance();
                self.skip_newlines();
                Ok(Stmt::Continue)
            }
            TokenKind::For => {
                self.advance();
                let pattern = self.parse_pattern()?;
                self.expect(&TokenKind::In)?;
                let iterable = self.parse_expr()?;
                self.skip_newlines();
                let body = self.parse_block()?;
                self.skip_newlines();
                Ok(Stmt::For {
                    pattern,
                    iterable,
                    body,
                })
            }
            TokenKind::While => {
                self.advance();
                let condition = self.parse_expr()?;
                self.skip_newlines();
                let body = self.parse_block()?;
                self.skip_newlines();
                Ok(Stmt::While { condition, body })
            }
            TokenKind::Loop => {
                self.advance();
                self.skip_newlines();
                let body = self.parse_block()?;
                self.skip_newlines();
                Ok(Stmt::Loop { body })
            }

            TokenKind::Defer => {
                self.advance();
                let expr = self.parse_expr()?;
                self.skip_newlines();
                Ok(Stmt::Defer(expr))
            }
            TokenKind::Try => {
                self.advance();
                self.skip_newlines();
                let try_body = self.parse_block()?;
                self.skip_newlines();
                let mut catches = Vec::new();
                while matches!(self.peek(), TokenKind::Catch) {
                    self.advance();
                    self.skip_newlines();
                    // Parse catch clause variants:
                    //   catch { ... }               — no type, no binding
                    //   catch e { ... }             — binding only (e is the error variable)
                    //   catch IOError as e { ... }  — error type + binding
                    let (error_type, binding) = if let TokenKind::Ident(name) = self.peek().clone() {
                        self.advance();
                        if matches!(self.peek(), TokenKind::As) {
                            // `catch ErrorType as binding { ... }`
                            self.advance();
                            let bind = self.expect_ident()?;
                            (Some(name), Some(bind))
                        } else {
                            // `catch e { ... }` — single ident is the binding, not error type
                            (None, Some(name))
                        }
                    } else {
                        // `catch { ... }` — no type, no binding
                        (None, None)
                    };
                    self.skip_newlines();
                    let body = self.parse_block()?;
                    self.skip_newlines();
                    catches.push(CatchClause {
                        error_type,
                        binding,
                        body,
                    });
                }
                let finally_body = if matches!(self.peek(), TokenKind::Finally) {
                    self.advance();
                    self.skip_newlines();
                    Some(self.parse_block()?)
                } else {
                    None
                };
                self.skip_newlines();
                Ok(Stmt::TryCatch {
                    try_body,
                    catches,
                    finally_body,
                })
            }
            _ => {
                let expr = self.parse_expr()?;
                // Check for assignment
                match self.peek() {
                    TokenKind::Eq => {
                        self.advance();
                        self.skip_newlines();
                        let value = self.parse_expr()?;
                        self.skip_newlines();
                        Ok(Stmt::Assign {
                            target: expr,
                            value,
                        })
                    }
                    TokenKind::PlusEq
                    | TokenKind::MinusEq
                    | TokenKind::StarEq
                    | TokenKind::SlashEq => {
                        let op = match self.advance().kind {
                            TokenKind::PlusEq => BinOp::Add,
                            TokenKind::MinusEq => BinOp::Sub,
                            TokenKind::StarEq => BinOp::Mul,
                            TokenKind::SlashEq => BinOp::Div,
                            _ => unreachable!(),
                        };
                        self.skip_newlines();
                        let value = self.parse_expr()?;
                        self.skip_newlines();
                        Ok(Stmt::CompoundAssign {
                            target: expr,
                            op,
                            value,
                        })
                    }
                    _ => {
                        self.skip_newlines();
                        Ok(Stmt::Expr(expr))
                    }
                }
            }
        }
    }

    // ── Block ──

    fn parse_block(&mut self) -> Result<Block, QueError> {
        self.expect(&TokenKind::LBrace)?;
        self.skip_newlines();

        let mut stmts = Vec::new();
        let mut final_expr: Option<Box<Expr>> = None;

        while !matches!(self.peek(), TokenKind::RBrace | TokenKind::Eof) {
            // Record position before parsing each statement for error reporting.
            let span = self.current_span();

            // Try to determine if this is a statement or trailing expression
            let stmt = self.parse_stmt()?;
            self.skip_newlines();

            if matches!(self.peek(), TokenKind::RBrace) {
                // Last item in block — if it's just an expression, it's the block value.
                if let Stmt::Expr(expr) = stmt {
                    final_expr = Some(Box::new(expr));
                } else {
                    stmts.push((span, stmt));
                }
            } else {
                stmts.push((span, stmt));
            }
        }

        self.expect(&TokenKind::RBrace)?;
        Ok(Block {
            stmts,
            expr: final_expr,
        })
    }

}

// ── Unit Tests ──

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;

    fn parse(src: &str) -> Module {
        let mut lexer = Lexer::new(src);
        let tokens = lexer.tokenize().expect("lexer failed");
        let mut parser = Parser::new(tokens);
        parser.parse_module().expect("parser failed")
    }

    /// Return the first item in a module.
    fn first_item(module: &Module) -> &Item {
        module.items.iter()
            .map(|(_, i)| i)
            .next()
            .expect("no items in module")
    }

    fn parse_expr(src: &str) -> Expr {
        let module = parse(src);
        let item = module.items.iter().map(|(_, i)| i).next();
        match item {
            Some(Item::Stmt(Stmt::Expr(e))) => e.clone(),
            other => panic!("expected expression statement, got {:?}", other),
        }
    }

    fn parse_stmt_item(src: &str) -> Stmt {
        let module = parse(src);
        let item = module.items.iter().map(|(_, i)| i).next();
        match item {
            Some(Item::Stmt(s)) => s.clone(),
            _ => panic!("expected statement"),
        }
    }

    // ── Expression tests ──

    #[test]
    fn test_integer_literal() {
        assert_eq!(parse_expr("42"), Expr::IntLit(42));
    }

    #[test]
    fn test_float_literal() {
        assert_eq!(parse_expr("3.14"), Expr::FloatLit(3.14));
    }

    #[test]
    fn test_string_literal() {
        assert_eq!(
            parse_expr(r#""hello""#),
            Expr::StringLit("hello".into())
        );
    }

    #[test]
    fn test_bool_literals() {
        assert_eq!(parse_expr("true"), Expr::BoolLit(true));
        assert_eq!(parse_expr("false"), Expr::BoolLit(false));
    }

    #[test]
    fn test_null_literal() {
        assert_eq!(parse_expr("null"), Expr::NullLit);
    }

    #[test]
    fn test_path_builtin() {
        let expr = parse_expr(r#"path("./src")"#);
        match expr {
            Expr::Call { callee, args, .. } => {
                assert_eq!(*callee, Expr::Ident("path".into()));
                assert_eq!(args.len(), 1);
            }
            other => panic!("expected Call, got {:?}", other),
        }
    }

    #[test]
    fn test_glob_builtin() {
        match parse_expr(r#"glob("*.rs")"#) {
            Expr::Call { callee, args, .. } => {
                assert_eq!(*callee, Expr::Ident("glob".into()));
                assert_eq!(args.len(), 1);
            }
            other => panic!("expected Call, got {:?}", other),
        }
    }

    #[test]
    fn test_duration_literal() {
        assert_eq!(
            parse_expr("5s"),
            Expr::DurationLit(5.0, DurationUnit::Seconds)
        );
    }

    #[test]
    fn test_binary_addition() {
        let expr = parse_expr("1 + 2");
        assert_eq!(
            expr,
            Expr::BinaryOp {
                left: Box::new(Expr::IntLit(1)),
                op: BinOp::Add,
                right: Box::new(Expr::IntLit(2)),
            }
        );
    }

    #[test]
    fn test_operator_precedence() {
        // 1 + 2 * 3 should be 1 + (2 * 3)
        let expr = parse_expr("1 + 2 * 3");
        assert_eq!(
            expr,
            Expr::BinaryOp {
                left: Box::new(Expr::IntLit(1)),
                op: BinOp::Add,
                right: Box::new(Expr::BinaryOp {
                    left: Box::new(Expr::IntLit(2)),
                    op: BinOp::Mul,
                    right: Box::new(Expr::IntLit(3)),
                }),
            }
        );
    }

    #[test]
    fn test_power_right_assoc() {
        // 2 ** 3 ** 2 should be 2 ** (3 ** 2)
        let expr = parse_expr("2 ** 3 ** 2");
        assert_eq!(
            expr,
            Expr::BinaryOp {
                left: Box::new(Expr::IntLit(2)),
                op: BinOp::Pow,
                right: Box::new(Expr::BinaryOp {
                    left: Box::new(Expr::IntLit(3)),
                    op: BinOp::Pow,
                    right: Box::new(Expr::IntLit(2)),
                }),
            }
        );
    }

    #[test]
    fn test_unary_negation() {
        let expr = parse_expr("-5");
        assert_eq!(
            expr,
            Expr::UnaryOp {
                op: UnaryOp::Neg,
                expr: Box::new(Expr::IntLit(5)),
            }
        );
    }

    #[test]
    fn test_comparison() {
        let expr = parse_expr("a >= b");
        assert_eq!(
            expr,
            Expr::BinaryOp {
                left: Box::new(Expr::Ident("a".into())),
                op: BinOp::GtEq,
                right: Box::new(Expr::Ident("b".into())),
            }
        );
    }

    #[test]
    fn test_logical_and_or() {
        let expr = parse_expr("a && b || c");
        assert_eq!(
            expr,
            Expr::BinaryOp {
                left: Box::new(Expr::BinaryOp {
                    left: Box::new(Expr::Ident("a".into())),
                    op: BinOp::And,
                    right: Box::new(Expr::Ident("b".into())),
                }),
                op: BinOp::Or,
                right: Box::new(Expr::Ident("c".into())),
            }
        );
    }

    #[test]
    fn test_function_call() {
        let expr = parse_expr("add(1, 2)");
        assert_eq!(
            expr,
            Expr::Call {
                callee: Box::new(Expr::Ident("add".into())),
                args: vec![
                    CallArg {
                        name: None,
                        value: Expr::IntLit(1)
                    },
                    CallArg {
                        name: None,
                        value: Expr::IntLit(2)
                    },
                ],
            }
        );
    }

    #[test]
    fn test_named_arguments() {
        let expr = parse_expr("deploy(target: \"prod\", dryRun: true)");
        match expr {
            Expr::Call { args, .. } => {
                assert_eq!(args[0].name, Some("target".into()));
                assert_eq!(args[1].name, Some("dryRun".into()));
            }
            _ => panic!("expected call"),
        }
    }

    #[test]
    fn test_method_call() {
        let expr = parse_expr("list.push(42)");
        assert_eq!(
            expr,
            Expr::MethodCall {
                object: Box::new(Expr::Ident("list".into())),
                method: "push".into(),
                args: vec![CallArg {
                    name: None,
                    value: Expr::IntLit(42)
                }],
            }
        );
    }

    #[test]
    fn test_field_access() {
        let expr = parse_expr("config.name");
        assert_eq!(
            expr,
            Expr::FieldAccess {
                object: Box::new(Expr::Ident("config".into())),
                field: "name".into(),
            }
        );
    }

    #[test]
    fn test_index_access() {
        let expr = parse_expr("list[0]");
        assert_eq!(
            expr,
            Expr::Index {
                object: Box::new(Expr::Ident("list".into())),
                index: Box::new(Expr::IntLit(0)),
            }
        );
    }

    #[test]
    fn test_try_operator() {
        let expr = parse_expr("foo()?");
        assert_eq!(
            expr,
            Expr::Try(Box::new(Expr::Call {
                callee: Box::new(Expr::Ident("foo".into())),
                args: vec![],
            }))
        );
    }

    #[test]
    fn test_null_coalesce() {
        let expr = parse_expr("a ?? b");
        assert_eq!(
            expr,
            Expr::NullCoalesce {
                left: Box::new(Expr::Ident("a".into())),
                right: Box::new(Expr::Ident("b".into())),
            }
        );
    }

    #[test]
    fn test_pipe_operator() {
        let expr = parse_expr("x |> f |> g");
        // Should be (x |> f) |> g — left-associative
        assert!(matches!(expr, Expr::Pipe { .. }));
    }

    #[test]
    fn test_list_literal() {
        let expr = parse_expr("[1, 2, 3]");
        assert_eq!(
            expr,
            Expr::ListLit(vec![Expr::IntLit(1), Expr::IntLit(2), Expr::IntLit(3)])
        );
    }

    #[test]
    fn test_map_literal() {
        let expr = parse_expr(r#"{"key": "value"}"#);
        assert_eq!(
            expr,
            Expr::MapLit(vec![MapEntry::Pair(
                Expr::StringLit("key".into()),
                Expr::StringLit("value".into())
            )])
        );
    }

    #[test]
    fn test_tuple_literal() {
        let expr = parse_expr("(1, 2, 3)");
        assert_eq!(
            expr,
            Expr::TupleLit(vec![Expr::IntLit(1), Expr::IntLit(2), Expr::IntLit(3)])
        );
    }

    #[test]
    fn test_lambda() {
        let expr = parse_expr("|x| x * 2");
        match expr {
            Expr::Lambda { params, body } => {
                assert_eq!(params.len(), 1);
                assert_eq!(params[0].name, "x");
                assert!(matches!(*body, Expr::BinaryOp { .. }));
            }
            _ => panic!("expected lambda"),
        }
    }

    #[test]
    fn test_if_expression() {
        let expr = parse_expr("if true { 1 } else { 2 }");
        match expr {
            Expr::If {
                condition,
                then_branch,
                else_branch,
            } => {
                assert_eq!(*condition, Expr::BoolLit(true));
                assert_eq!(then_branch.expr, Some(Box::new(Expr::IntLit(1))));
                assert!(else_branch.is_some());
            }
            _ => panic!("expected if expression"),
        }
    }

    #[test]
    fn test_range_expression() {
        let expr = parse_expr("0..10");
        assert_eq!(
            expr,
            Expr::Range {
                start: Some(Box::new(Expr::IntLit(0))),
                end: Some(Box::new(Expr::IntLit(10))),
                inclusive: false,
            }
        );
    }

    #[test]
    fn test_range_inclusive() {
        let expr = parse_expr("0..=10");
        assert_eq!(
            expr,
            Expr::Range {
                start: Some(Box::new(Expr::IntLit(0))),
                end: Some(Box::new(Expr::IntLit(10))),
                inclusive: true,
            }
        );
    }

    // ── Statement tests ──

    #[test]
    fn test_let_binding() {
        let stmt = parse_stmt_item("let x = 42");
        match stmt {
            Stmt::Let { pattern, value, .. } => {
                assert_eq!(pattern, Pattern::Ident("x".into()));
                assert_eq!(value, Expr::IntLit(42));
            }
            _ => panic!("expected let"),
        }
    }

    #[test]
    fn test_uppercase_let_binding_is_identifier() {
        let stmt = parse_stmt_item("let RESET = 42");
        match stmt {
            Stmt::Let { pattern, value, .. } => {
                assert_eq!(pattern, Pattern::Ident("RESET".into()));
                assert_eq!(value, Expr::IntLit(42));
            }
            _ => panic!("expected let"),
        }
    }

    #[test]
    fn test_uppercase_pub_let_binding_is_identifier() {
        let module = parse("pub let RESET = \"\\x1b[0m\"");
        match first_item(&module) {
            Item::PubLet { pattern, value, .. } => {
                assert_eq!(pattern, &Pattern::Ident("RESET".into()));
                assert_eq!(value, &Expr::StringLit("\x1b[0m".into()));
            }
            other => panic!("expected pub let, got {:?}", other),
        }
    }

    #[test]
    fn test_mut_binding() {
        let stmt = parse_stmt_item("mut counter = 0");
        match stmt {
            Stmt::Mut { name, value, .. } => {
                assert_eq!(name, "counter");
                assert_eq!(value, Expr::IntLit(0));
            }
            _ => panic!("expected mut"),
        }
    }

    #[test]
    fn test_for_loop() {
        let stmt = parse_stmt_item("for x in [1, 2, 3] { print(x) }");
        match stmt {
            Stmt::For {
                pattern, iterable, ..
            } => {
                assert_eq!(pattern, Pattern::Ident("x".into()));
                assert!(matches!(iterable, Expr::ListLit(_)));
            }
            _ => panic!("expected for"),
        }
    }

    #[test]
    fn test_while_loop() {
        let stmt = parse_stmt_item("while true { break }");
        assert!(matches!(stmt, Stmt::While { .. }));
    }


    // ── Match tests ──

    #[test]
    fn test_match_literals() {
        let expr = parse_expr("match x { 0 => \"zero\", 1 => \"one\", _ => \"other\" }");
        match expr {
            Expr::Match { arms, .. } => {
                assert_eq!(arms.len(), 3);
                assert_eq!(arms[0].pattern, Pattern::IntLit(0));
                assert_eq!(arms[2].pattern, Pattern::Wildcard);
            }
            _ => panic!("expected match"),
        }
    }

    #[test]
    fn test_match_with_guard() {
        let expr = parse_expr("match x { n if n > 0 => \"pos\", _ => \"other\" }");
        match expr {
            Expr::Match { arms, .. } => {
                assert!(arms[0].guard.is_some());
            }
            _ => panic!("expected match"),
        }
    }

    #[test]
    fn test_match_enum_pattern() {
        let expr = parse_expr("match r { Ok(v) => v, Err(e) => e }");
        match expr {
            Expr::Match { arms, .. } => {
                assert!(matches!(arms[0].pattern, Pattern::Enum(..)));
                assert!(matches!(arms[1].pattern, Pattern::Enum(..)));
            }
            _ => panic!("expected match"),
        }
    }

    #[test]
    fn test_match_unit_enum_pattern() {
        let expr = parse_expr("match dir { North => 1, South => 2 }");
        match expr {
            Expr::Match { arms, .. } => {
                assert_eq!(arms[0].pattern, Pattern::Enum(None, "North".into(), vec![]));
                assert_eq!(arms[1].pattern, Pattern::Enum(None, "South".into(), vec![]));
            }
            _ => panic!("expected match"),
        }
    }

    #[test]
    fn test_optional_chaining_parses() {
        let expr = parse_expr("a?.b?.c()");
        assert!(matches!(expr, Expr::OptionalAccess { args: Some(_), .. }));
    }

    #[test]
    fn test_match_qualified_lowercase_enum_pattern() {
        let expr = parse_expr("match value { Status.ok => 1, Status.err(code) => code } ");
        match expr {
            Expr::Match { arms, .. } => {
                assert_eq!(arms[0].pattern, Pattern::Enum(Some("Status".into()), "ok".into(), vec![]));
                assert_eq!(arms[1].pattern, Pattern::Enum(Some("Status".into()), "err".into(), vec![Pattern::Ident("code".into())]));
            }
            _ => panic!("expected match"),
        }
    }

    #[test]
    fn test_match_qualified_named_field_enum_pattern() {
        let expr = parse_expr("match value { Msg.write { text } => text }");
        match expr {
            Expr::Match { arms, .. } => {
                assert_eq!(
                    arms[0].pattern,
                    Pattern::Instance(
                        Some("Msg".into()),
                        "write".into(),
                        vec![("text".into(), None)],
                        None,
                    )
                );
            }
            _ => panic!("expected match"),
        }
    }

    #[test]
    fn test_match_list_pattern() {
        let expr =
            parse_expr("match args { [] => 0, [x] => 1, [x, ...rest] => 2, _ => 3 }");
        match expr {
            Expr::Match { arms, .. } => {
                assert_eq!(arms.len(), 4);
                assert!(matches!(arms[0].pattern, Pattern::List(ref pats, _) if pats.is_empty()));
                assert!(matches!(arms[2].pattern, Pattern::List(_, Some(_))));
            }
            _ => panic!("expected match"),
        }
    }

    // ── Declaration tests ──

    #[test]
    fn test_fn_decl() {
        let module = parse("fn add(a: Int, b: Int) -> Int { a + b }");
        match first_item(&module) {
            Item::FnDecl(f) => {
                assert_eq!(f.name, "add");
                assert_eq!(f.params.len(), 2);
                assert!(f.return_type.is_some());
            }
            _ => panic!("expected fn decl"),
        }
    }

    #[test]
    fn test_fn_default_params() {
        let module = parse("fn greet(name: String = \"world\") { print(name) }");
        match first_item(&module) {
            Item::FnDecl(f) => {
                assert!(f.params[0].default.is_some());
            }
            _ => panic!("expected fn decl"),
        }
    }

    #[test]
    fn test_task_decl() {
        let module = parse(
            r#"@description("Build the project")
               @inputs(["./src"])
               @outputs(["./build"])
               task build {
                    print("building")
               }"#,
        );
        match first_item(&module) {
            Item::TaskDecl(t) => {
                assert_eq!(t.name, "build");
                assert_eq!(t.description, Some("Build the project".into()));
                assert_eq!(t.inputs.len(), 1);
                assert_eq!(t.outputs.len(), 1);
            }
            _ => panic!("expected task decl"),
        }
    }

    #[test]
    fn test_task_depends_on() {
        let module = parse(
            r#"@deps([build, test])
               task deploy {
                    print("deploying")
               }"#,
        );
        match first_item(&module) {
            Item::TaskDecl(t) => {
                assert_eq!(t.name, "deploy");
                assert_eq!(t.depends_on.len(), 2);
            }
            _ => panic!("expected task decl"),
        }
    }

    #[test]
    fn test_import() {
        let module = parse("import std.fs as fs");
        match first_item(&module) {
            Item::Import(imp) => {
                assert_eq!(imp.path, vec!["std", "fs"]);
                assert_eq!(imp.alias, Some("fs".into()));
            }
            _ => panic!("expected import"),
        }
    }

    #[test]
    fn test_import_multi() {
        let module = parse("import std.{fs, path}");
        match first_item(&module) {
            Item::Import(imp) => {
                assert_eq!(imp.path, vec!["std"]);
                assert_eq!(
                    imp.items,
                    Some(vec!["fs".into(), "path".into()])
                );
            }
            _ => panic!("expected import"),
        }
    }

    #[test]
    fn test_enum_decl() {
        let module = parse(
            r#"enum Color {
                Red,
                Green,
                Blue,
            }"#,
        );
        match first_item(&module) {
            Item::EnumDecl(e) => {
                assert_eq!(e.name, "Color");
                assert_eq!(e.variants.len(), 3);
            }
            _ => panic!("expected enum decl"),
        }
    }

    // ── Complex expression tests ──

    #[test]
    fn test_chained_method_calls() {
        let expr = parse_expr(r#""hello".trim().toUpperCase()"#);
        match expr {
            Expr::MethodCall { method, .. } => assert_eq!(method, "toUpperCase"),
            _ => panic!("expected method call"),
        }
    }

    #[test]
    fn test_nested_field_access() {
        let expr = parse_expr("config.database.host");
        match expr {
            Expr::FieldAccess { field, .. } => assert_eq!(field, "host"),
            _ => panic!("expected field access"),
        }
    }

    #[test]
    fn test_parenthesized_expr() {
        let expr = parse_expr("(1 + 2) * 3");
        assert_eq!(
            expr,
            Expr::BinaryOp {
                left: Box::new(Expr::BinaryOp {
                    left: Box::new(Expr::IntLit(1)),
                    op: BinOp::Add,
                    right: Box::new(Expr::IntLit(2)),
                }),
                op: BinOp::Mul,
                right: Box::new(Expr::IntLit(3)),
            }
        );
    }

    #[test]
    fn test_multiline_program() {
        let module = parse(
            r#"
            let x = 10
            let y = 20
            let sum = x + y
            print(sum)
        "#,
        );
        let item_count = module.items.len();
        assert_eq!(item_count, 4);
    }
}
