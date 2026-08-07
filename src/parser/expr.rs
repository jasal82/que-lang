//! Expression parsing (Pratt-style precedence climbing) for the Que parser.

use super::{BraceKind, Parser};
use crate::ast::*;
use crate::error::{ErrorKind, QueError};
use crate::lexer::Lexer;
use crate::token::*;

impl Parser {
    // ── Expressions (Pratt parser) ──

    pub fn parse_expr(&mut self) -> Result<Expr, QueError> {
        self.parse_pipe_expr()
    }

    fn parse_pipe_expr(&mut self) -> Result<Expr, QueError> {
        let mut left = self.parse_null_coalesce()?;
        while self.at_binary_op(&[TokenKind::PipeArrow]) {
            self.advance();
            self.skip_newlines();
            let right = self.parse_null_coalesce()?;
            left = Expr::Pipe {
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_null_coalesce(&mut self) -> Result<Expr, QueError> {
        let mut left = self.parse_or()?;
        while self.at_binary_op(&[TokenKind::NullCoalesce]) {
            self.advance();
            self.skip_newlines();
            let right = self.parse_or()?;
            left = Expr::NullCoalesce {
                left: Box::new(left),
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_or(&mut self) -> Result<Expr, QueError> {
        let mut left = self.parse_and()?;
        while self.at_binary_op(&[TokenKind::Or]) {
            self.advance();
            self.skip_newlines();
            let right = self.parse_and()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op: BinOp::Or,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expr, QueError> {
        let mut left = self.parse_bitor()?;
        while self.at_binary_op(&[TokenKind::And]) {
            self.advance();
            self.skip_newlines();
            let right = self.parse_bitor()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op: BinOp::And,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_bitor(&mut self) -> Result<Expr, QueError> {
        let mut left = self.parse_bitxor()?;
        while self.at_binary_op(&[TokenKind::Pipe]) {
            self.advance();
            self.skip_newlines();
            let right = self.parse_bitxor()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op: BinOp::BitOr,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_bitxor(&mut self) -> Result<Expr, QueError> {
        let mut left = self.parse_bitand()?;
        while self.at_binary_op(&[TokenKind::BitXor]) {
            self.advance();
            self.skip_newlines();
            let right = self.parse_bitand()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op: BinOp::BitXor,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_bitand(&mut self) -> Result<Expr, QueError> {
        let mut left = self.parse_equality()?;
        while self.at_binary_op(&[TokenKind::BitAnd]) {
            self.advance();
            self.skip_newlines();
            let right = self.parse_equality()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op: BinOp::BitAnd,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_equality(&mut self) -> Result<Expr, QueError> {
        let mut left = self.parse_comparison()?;
        while self.at_binary_op(&[TokenKind::EqEq, TokenKind::BangEq]) {
            let op = match self.advance().kind {
                TokenKind::EqEq => BinOp::Eq,
                TokenKind::BangEq => BinOp::NotEq,
                _ => unreachable!(),
            };
            self.skip_newlines();
            let right = self.parse_comparison()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_comparison(&mut self) -> Result<Expr, QueError> {
        let mut left = self.parse_shift()?;
        while self.at_binary_op(&[
            TokenKind::Lt,
            TokenKind::Gt,
            TokenKind::LtEq,
            TokenKind::GtEq,
        ]) {
            let op = match self.advance().kind {
                TokenKind::Lt => BinOp::Lt,
                TokenKind::Gt => BinOp::Gt,
                TokenKind::LtEq => BinOp::LtEq,
                TokenKind::GtEq => BinOp::GtEq,
                _ => unreachable!(),
            };
            self.skip_newlines();
            let right = self.parse_shift()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_shift(&mut self) -> Result<Expr, QueError> {
        let mut left = self.parse_range()?;
        while self.at_binary_op(&[TokenKind::Shl, TokenKind::Shr]) {
            let op = match self.advance().kind {
                TokenKind::Shl => BinOp::Shl,
                TokenKind::Shr => BinOp::Shr,
                _ => unreachable!(),
            };
            self.skip_newlines();
            let right = self.parse_range()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_range(&mut self) -> Result<Expr, QueError> {
        let left = self.parse_addition()?;
        if self.at_binary_op(&[TokenKind::Range, TokenKind::RangeInc]) {
            let inclusive = matches!(self.peek(), TokenKind::RangeInc);
            self.advance();
            self.skip_newlines();
            let right = self.parse_addition()?;
            Ok(Expr::Range {
                start: Some(Box::new(left)),
                end: Some(Box::new(right)),
                inclusive,
            })
        } else {
            Ok(left)
        }
    }

    fn parse_addition(&mut self) -> Result<Expr, QueError> {
        let mut left = self.parse_multiplication()?;
        while self.at_binary_op(&[TokenKind::Plus, TokenKind::Minus]) {
            let op = match self.advance().kind {
                TokenKind::Plus => BinOp::Add,
                TokenKind::Minus => BinOp::Sub,
                _ => unreachable!(),
            };
            self.skip_newlines();
            let right = self.parse_multiplication()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_multiplication(&mut self) -> Result<Expr, QueError> {
        let mut left = self.parse_power()?;
        while self.at_binary_op(&[
            TokenKind::Star,
            TokenKind::Slash,
            TokenKind::Percent,
        ]) {
            // `/` always lowers to BinOp::Div; path-join semantics are
            // resolved at eval time based on operand types.
            let op = match self.advance().kind {
                TokenKind::Star => BinOp::Mul,
                TokenKind::Slash => BinOp::Div,
                TokenKind::Percent => BinOp::Mod,
                _ => unreachable!(),
            };
            self.skip_newlines();
            let right = self.parse_power()?;
            left = Expr::BinaryOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_power(&mut self) -> Result<Expr, QueError> {
        let left = self.parse_unary()?;
        if self.at_binary_op(&[TokenKind::Power]) {
            self.advance();
            self.skip_newlines();
            let right = self.parse_power()?; // right-associative
            Ok(Expr::BinaryOp {
                left: Box::new(left),
                op: BinOp::Pow,
                right: Box::new(right),
            })
        } else {
            Ok(left)
        }
    }

    fn parse_unary(&mut self) -> Result<Expr, QueError> {
        match self.peek() {
            TokenKind::Minus => {
                self.advance();
                let expr = self.parse_unary()?;
                Ok(Expr::UnaryOp {
                    op: UnaryOp::Neg,
                    expr: Box::new(expr),
                })
            }
            TokenKind::Bang => {
                self.advance();
                let expr = self.parse_unary()?;
                Ok(Expr::UnaryOp {
                    op: UnaryOp::Not,
                    expr: Box::new(expr),
                })
            }
            TokenKind::Tilde => {
                self.advance();
                let expr = self.parse_unary()?;
                Ok(Expr::UnaryOp {
                    op: UnaryOp::BitNot,
                    expr: Box::new(expr),
                })
            }
            _ => self.parse_postfix(),
        }
    }

    fn parse_postfix(&mut self) -> Result<Expr, QueError> {
        let mut expr = self.parse_primary()?;

        loop {
            match self.peek() {
                // Allow newlines before `.` for multiline chains like:
                //   foo
                //     .bar()
                //     .baz()
                TokenKind::Newline => {
                    // Peek past newlines to see if a `.` follows.
                    let mut lookahead = self.pos;
                    while lookahead < self.tokens.len()
                        && matches!(self.tokens[lookahead].kind, TokenKind::Newline)
                    {
                        lookahead += 1;
                    }
                    if lookahead < self.tokens.len()
                        && matches!(
                            self.tokens[lookahead].kind,
                            TokenKind::Dot | TokenKind::QuestionDot
                        )
                    {
                        self.skip_newlines();
                        // Continue into the Dot branch below.
                    } else {
                        break;
                    }
                }
                TokenKind::Dot => {}
                TokenKind::QuestionDot => {}
                TokenKind::LParen => {}
                TokenKind::LBracket => {}
                TokenKind::Question => {}
                _ => break,
            }

            // Now actually handle the postfix operator.
            match self.peek() {
                TokenKind::Dot => {
                    self.advance();
                    let field = self.expect_member_name()?;
                    if matches!(self.peek(), TokenKind::LParen) {
                        self.advance();
                        let args = self.parse_call_args()?;
                        self.expect(&TokenKind::RParen)?;
                        expr = Expr::MethodCall {
                            object: Box::new(expr),
                            method: field,
                            args,
                        };
                    } else if self.at_variant_literal_brace(&field) {
                        // `Enum.Variant { field: value, ... }` — same meaning as
                        // `Enum.Variant(field: value, ...)`, mirroring the
                        // declaration syntax. Desugars to the named-argument call.
                        let args = self.parse_brace_named_args()?;
                        expr = Expr::MethodCall {
                            object: Box::new(expr),
                            method: field,
                            args,
                        };
                    } else {
                        expr = Expr::FieldAccess {
                            object: Box::new(expr),
                            field,
                        };
                    }
                }
                TokenKind::QuestionDot => {
                    self.advance();
                    let field = self.expect_member_name()?;
                    let args = if matches!(self.peek(), TokenKind::LParen) {
                        self.advance();
                        let args = self.parse_call_args()?;
                        self.expect(&TokenKind::RParen)?;
                        Some(args)
                    } else {
                        None
                    };
                    expr = Expr::OptionalAccess {
                        object: Box::new(expr),
                        field,
                        args,
                    };
                }
                TokenKind::LParen => {
                    self.advance();
                    let args = self.parse_call_args()?;
                    self.expect(&TokenKind::RParen)?;
                    expr = Expr::Call {
                        callee: Box::new(expr),
                        args,
                    };
                }
                TokenKind::LBracket => {
                    self.advance();
                    self.skip_newlines();
                    let index = self.parse_expr()?;
                    self.skip_newlines();
                    self.expect(&TokenKind::RBracket)?;
                    expr = Expr::Index {
                        object: Box::new(expr),
                        index: Box::new(index),
                    };
                }
                TokenKind::Question => {
                    self.advance();
                    expr = Expr::Try(Box::new(expr));
                }
                _ => break,
            }
        }

        Ok(expr)
    }

    /// True if `Enum.Variant` is immediately followed by a brace that starts a
    /// variant literal (`{ field: ...`) rather than an ordinary block.
    ///
    /// The lookahead is deliberately narrow: only an uppercase-initial field
    /// followed by `{ ident :` qualifies, so `if s == State.Idle { ... }` and
    /// `for x in m.items { ... }` keep parsing as blocks.
    fn at_variant_literal_brace(&self, field: &str) -> bool {
        if !field.chars().next().map(char::is_uppercase).unwrap_or(false) {
            return false;
        }
        if !matches!(self.peek(), TokenKind::LBrace) {
            return false;
        }
        let mut i = self.pos + 1;
        while matches!(self.peek_at(i), TokenKind::Newline) {
            i += 1;
        }
        matches!(self.peek_at(i), TokenKind::Ident(_))
            && matches!(self.peek_at(i + 1), TokenKind::Colon)
    }

    /// Parse `{ field: expr, ... }` into named call arguments.
    /// Assumes the current token is `{`.
    fn parse_brace_named_args(&mut self) -> Result<Vec<CallArg>, QueError> {
        self.expect(&TokenKind::LBrace)?;
        let mut args = Vec::new();
        self.skip_newlines();
        while !matches!(self.peek(), TokenKind::RBrace | TokenKind::Eof) {
            let name = self.expect_ident()?;
            self.expect(&TokenKind::Colon)?;
            self.skip_newlines();
            let value = self.parse_expr()?;
            args.push(CallArg {
                name: Some(name),
                value,
            });
            self.skip_newlines();
            if matches!(self.peek(), TokenKind::Comma) {
                self.advance();
                self.skip_newlines();
            }
        }
        self.expect(&TokenKind::RBrace)?;
        Ok(args)
    }

    fn parse_call_args(&mut self) -> Result<Vec<CallArg>, QueError> {
        let mut args = Vec::new();
        self.skip_newlines();
        while !matches!(self.peek(), TokenKind::RParen | TokenKind::Eof) {
            // Check for named argument: `name: value`
            let arg = if let TokenKind::Ident(name) = self.peek().clone() {
                if matches!(self.peek_nth(1), TokenKind::Colon) {
                    // Could be named arg or a map-style expression — peek ahead
                    // Named arg: `ident: expr`
                    self.advance();
                    self.advance(); // colon
                    self.skip_newlines();
                    let value = self.parse_expr()?;
                    CallArg {
                        name: Some(name),
                        value,
                    }
                } else {
                    let value = self.parse_expr()?;
                    CallArg { name: None, value }
                }
            } else {
                // Check for closure argument |x| ...
                let value = self.parse_expr()?;
                CallArg { name: None, value }
            };

            args.push(arg);
            self.skip_newlines();
            if matches!(self.peek(), TokenKind::Comma) {
                self.advance();
                self.skip_newlines();
            }
        }
        Ok(args)
    }

    pub(crate) fn parse_expr_list(&mut self, end: &TokenKind) -> Result<Vec<Expr>, QueError> {
        let mut exprs = Vec::new();
        self.skip_newlines();
        while std::mem::discriminant(self.peek()) != std::mem::discriminant(end) && !self.is_at_end()
        {
            exprs.push(self.parse_expr()?);
            self.skip_newlines();
            if matches!(self.peek(), TokenKind::Comma) {
                self.advance();
                self.skip_newlines();
            }
        }
        Ok(exprs)
    }

    fn parse_primary(&mut self) -> Result<Expr, QueError> {
        match self.peek().clone() {
            TokenKind::IntLit(n) => {
                self.advance();
                Ok(Expr::IntLit(n))
            }
            TokenKind::FloatLit(f) => {
                self.advance();
                Ok(Expr::FloatLit(f))
            }
            TokenKind::StringLit(s) => {
                self.advance();
                Ok(Expr::StringLit(s))
            }
            TokenKind::InterpolatedString(parts) => {
                self.advance();
                Ok(Expr::InterpolatedString(self.parse_string_parts(parts)?))
            }
            TokenKind::True => {
                self.advance();
                Ok(Expr::BoolLit(true))
            }
            TokenKind::False => {
                self.advance();
                Ok(Expr::BoolLit(false))
            }
            TokenKind::Null => {
                self.advance();
                Ok(Expr::NullLit)
            }
            TokenKind::CmdLit(parts) => {
                self.advance();
                Ok(Expr::CmdLit(self.parse_string_parts(parts)?))
            }
            TokenKind::DurationLit(val, unit) => {
                self.advance();
                Ok(Expr::DurationLit(val, unit))
            }
            TokenKind::RegexLit(r) => {
                self.advance();
                Ok(Expr::RegexLit(r))
            }
            TokenKind::SemverLit(v) => {
                self.advance();
                Ok(Expr::SemverLit(v))
            }
            TokenKind::PathLit(parts) => {
                self.advance();
                Ok(Expr::PathLit(self.parse_string_parts(parts)?))
            }
            TokenKind::GlobLit(parts) => {
                self.advance();
                Ok(Expr::GlobLit(self.parse_string_parts(parts)?))
            }

            // ── Identifier or struct literal ──
            TokenKind::Ident(_) => {
                let name = self.expect_ident()?;
                // Struct literal: `TypeName { field: value, ... }`
                // Only triggered when name starts with uppercase AND `{` immediately follows
                // (no newline between) — prevents ambiguity with block statements.
                if name.chars().next().map(|c| c.is_uppercase()).unwrap_or(false)
                    && matches!(self.peek(), TokenKind::LBrace)
                {
                    self.advance(); // consume `{`
                    self.skip_newlines();
                    let mut fields = Vec::new();
                    while !matches!(self.peek(), TokenKind::RBrace | TokenKind::Eof) {
                        let field_name = self.expect_ident()?;
                        let value = if matches!(self.peek(), TokenKind::Colon) {
                            self.advance(); // consume `:`
                            self.skip_newlines();
                            self.parse_expr()?
                        } else {
                            // Shorthand: `field` means `field: field`
                            Expr::Ident(field_name.clone())
                        };
                        fields.push((field_name, value));
                        self.skip_newlines();
                        if matches!(self.peek(), TokenKind::Comma) {
                            self.advance();
                            self.skip_newlines();
                        }
                    }
                    self.expect(&TokenKind::RBrace)?;
                    Ok(Expr::StructLit { name, fields })
                } else {
                    Ok(Expr::Ident(name))
                }
            }

            // ── Parenthesized expression or tuple ──
            TokenKind::LParen => {
                self.advance();
                self.skip_newlines();
                if matches!(self.peek(), TokenKind::RParen) {
                    self.advance();
                    return Ok(Expr::TupleLit(Vec::new())); // unit tuple
                }
                let first = self.parse_expr()?;
                self.skip_newlines();
                if matches!(self.peek(), TokenKind::Comma) {
                    // Tuple
                    self.advance();
                    self.skip_newlines();
                    let mut elems = vec![first];
                    while !matches!(self.peek(), TokenKind::RParen | TokenKind::Eof) {
                        elems.push(self.parse_expr()?);
                        self.skip_newlines();
                        if matches!(self.peek(), TokenKind::Comma) {
                            self.advance();
                            self.skip_newlines();
                        }
                    }
                    self.expect(&TokenKind::RParen)?;
                    Ok(Expr::TupleLit(elems))
                } else {
                    self.expect(&TokenKind::RParen)?;
                    Ok(first) // Parenthesized expression
                }
            }

            // ── List literal ──
            TokenKind::LBracket => {
                self.advance();
                let elems = self.parse_expr_list(&TokenKind::RBracket)?;
                self.expect(&TokenKind::RBracket)?;
                Ok(Expr::ListLit(elems))
            }

            // ── Set literal ──
            // `#{}` is the empty set; `#{a, b}` a populated one.
            TokenKind::HashBrace => {
                self.advance();
                self.skip_newlines();
                let mut elems = Vec::new();
                while !matches!(self.peek(), TokenKind::RBrace | TokenKind::Eof) {
                    elems.push(self.parse_expr()?);
                    self.skip_newlines();
                    if matches!(self.peek(), TokenKind::Comma) {
                        self.advance();
                        self.skip_newlines();
                    }
                }
                self.expect(&TokenKind::RBrace)?;
                Ok(Expr::SetLit(elems))
            }

            // ── Map literal or block ──
            TokenKind::LBrace => {
                match self.classify_braces() {
                    BraceKind::Map => {
                        self.advance();
                        self.skip_newlines();
                        let mut entries = Vec::new();
                        while !matches!(self.peek(), TokenKind::RBrace | TokenKind::Eof) {
                            if matches!(self.peek(), TokenKind::Spread) {
                                self.advance();
                                let expr = self.parse_postfix()?;
                                entries.push(MapEntry::Spread(expr));
                            } else {
                                // Key: identifier treated as string key, or expression
                                let key = if let TokenKind::Ident(name) = self.peek().clone() {
                                    if matches!(self.peek_nth(1), TokenKind::Colon)
                                    {
                                        self.advance();
                                        Expr::StringLit(name)
                                    } else {
                                        self.parse_expr()?
                                    }
                                } else {
                                    self.parse_expr()?
                                };
                                self.expect(&TokenKind::Colon)?;
                                self.skip_newlines();
                                let val = self.parse_expr()?;
                                entries.push(MapEntry::Pair(key, val));
                            }
                            self.skip_newlines();
                            if matches!(self.peek(), TokenKind::Comma) {
                                self.advance();
                                self.skip_newlines();
                            }
                        }
                        self.expect(&TokenKind::RBrace)?;
                        Ok(Expr::MapLit(entries))
                    }
                    BraceKind::Set => Err(QueError::new(
                        crate::error::ErrorKind::UnexpectedToken,
                        "brace set literals were removed; write a set as `#{ ... }`",
                    )),
                    BraceKind::Block => {
                        let block = self.parse_block()?;
                        Ok(Expr::Block(block))
                    }
                }
            }

            // ── If expression ──
            TokenKind::If => {
                self.advance();
                self.skip_newlines();

                // Check for if-let
                if matches!(self.peek(), TokenKind::Let) {
                    self.advance();
                    let pattern = self.parse_match_pattern()?;
                    self.expect(&TokenKind::Eq)?;
                    self.skip_newlines();
                    let value = self.parse_expr()?;
                    self.skip_newlines();
                    let then_branch = self.parse_block()?;
                    let else_branch = self.parse_optional_else()?;
                    return Ok(Expr::IfLet {
                        pattern,
                        value: Box::new(value),
                        then_branch,
                        else_branch,
                    });
                }

                let condition = self.parse_expr()?;
                self.skip_newlines();
                let then_branch = self.parse_block()?;
                let else_branch = self.parse_optional_else()?;
                Ok(Expr::If {
                    condition: Box::new(condition),
                    then_branch,
                    else_branch,
                })
            }

            // ── Match expression ──
            TokenKind::Match => {
                self.advance();
                self.skip_newlines();
                let subject = self.parse_expr()?;
                self.skip_newlines();
                self.expect(&TokenKind::LBrace)?;
                self.skip_newlines();
                let mut arms = Vec::new();
                while !matches!(self.peek(), TokenKind::RBrace | TokenKind::Eof) {
                    arms.push(self.parse_match_arm()?);
                    self.skip_newlines();
                    // consume optional comma
                    if matches!(self.peek(), TokenKind::Comma) {
                        self.advance();
                        self.skip_newlines();
                    }
                }
                self.expect(&TokenKind::RBrace)?;
                Ok(Expr::Match {
                    subject: Box::new(subject),
                    arms,
                })
            }

            // ── With expression ──
            // `with EXPR [as NAME] { BODY }` — Contextual context manager.
            // The `as NAME` binding is optional; omit it when the entered
            // resource is not needed (e.g. `with env.scope({ ... }) { ... }`).
            TokenKind::With => {
                self.advance();
                self.skip_newlines();
                // `with env { MAP } { BODY }` was removed — `env` is a plain
                // namespace object now, with no special status after `with`.
                if self.peek_ident_eq("env") && matches!(self.peek_nth(1), TokenKind::LBrace) {
                    return Err(QueError::new(
                        crate::error::ErrorKind::UnexpectedToken,
                        "`with env { KEY: value } { ... }` was removed; use `with env.scope({ KEY: value }) { ... }`",
                    ));
                }
                let manager = self.parse_expr()?;
                self.skip_newlines();
                let name = if matches!(self.peek(), TokenKind::As) {
                    self.advance();
                    self.skip_newlines();
                    let n = self.expect_ident()?;
                    self.skip_newlines();
                    n
                } else {
                    "_".to_string()
                };
                let body = self.parse_block()?;
                self.skip_newlines();
                Ok(Expr::WithContext { manager: Box::new(manager), name, body })
            }

            // ── spawn expr — launch command in background ──
            TokenKind::Spawn => {
                self.advance();
                self.skip_newlines();
                let expr = self.parse_unary()?;
                Ok(Expr::Spawn(Box::new(expr)))
            }

            // ── parallel { branches } — run branches concurrently ──
            TokenKind::Parallel => {
                self.advance();
                self.skip_newlines();
                self.expect(&TokenKind::LBrace)?;
                self.skip_newlines();
                let mut branches = Vec::new();
                while !matches!(self.peek(), TokenKind::RBrace | TokenKind::Eof) {
                    // Check for named branch: `name: expr`
                    let label = if let TokenKind::Ident(name) = self.peek().clone() {
                        if matches!(self.peek_nth(1), TokenKind::Colon) {
                            self.advance(); // consume name
                            self.advance(); // consume ':'
                            self.skip_newlines();
                            Some(name)
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    let body = self.parse_expr()?;
                    branches.push(crate::ast::ParallelBranch { label, body });
                    self.skip_newlines();
                    if matches!(self.peek(), TokenKind::Comma) {
                        self.advance();
                        self.skip_newlines();
                    }
                }
                self.expect(&TokenKind::RBrace)?;
                Ok(Expr::Parallel(branches))
            }

            // ── `fn` is a declaration keyword only; closures use |params| ──
            TokenKind::Fn => Err(QueError::parser(
                ErrorKind::UnexpectedToken,
                "`fn` declares a named function; for a closure write `|x| expr` or `|x| { ... }`",
                self.current_span(),
            )),

            // ── Zero-param closure: || expr  OR  || { body } ──
            TokenKind::Or => {
                self.advance();
                self.skip_newlines();
                let body = if matches!(self.peek(), TokenKind::LBrace) {
                    let block = self.parse_block()?;
                    Expr::Block(block)
                } else {
                    self.parse_expr()?
                };
                Ok(Expr::Lambda {
                    params: vec![],
                    body: Box::new(body),
                })
            }

            // ── Closure: |params| expr  OR  |params| { body } ──
            TokenKind::Pipe => {
                self.advance();
                let mut params = Vec::new();
                while !matches!(self.peek(), TokenKind::Pipe | TokenKind::Eof) {
                    let name = self.expect_ident()?;
                    let type_ann = if matches!(self.peek(), TokenKind::Colon) {
                        self.advance();
                        Some(self.parse_type_expr()?)
                    } else {
                        None
                    };
                    let default = if matches!(self.peek(), TokenKind::Eq) {
                        self.advance();
                        // Parse below bitwise-or precedence: a bare `|` here would
                        // otherwise be swallowed as an operator instead of closing
                        // the parameter list. Wrap in parens to use `|`, `||`, `??`.
                        Some(self.parse_bitxor()?)
                    } else {
                        None
                    };
                    params.push(Param {
                        name,
                        type_ann,
                        default,
                        rest: false,
                    });
                    if matches!(self.peek(), TokenKind::Comma) {
                        self.advance();
                    }
                }
                self.expect(&TokenKind::Pipe)?;
                self.skip_newlines();
                let body = if matches!(self.peek(), TokenKind::LBrace) {
                    let block = self.parse_block()?;
                    Expr::Block(block)
                } else {
                    self.parse_expr()?
                };
                Ok(Expr::Lambda {
                    params,
                    body: Box::new(body),
                })
            }

            // ── Loop expression (returns value via break) ──
            TokenKind::Loop => {
                self.advance();
                self.skip_newlines();
                let body = self.parse_block()?;
                Ok(Expr::Loop { body })
            }

            // ── Spread ──
            TokenKind::Spread => {
                self.advance();
                let expr = self.parse_postfix()?;
                Ok(Expr::Spread(Box::new(expr)))
            }

            _ => Err(QueError::parser(
                ErrorKind::ExpectedExpression,
                format!("expected expression, got {:?}", self.peek()),
                self.current_span(),
            )),
        }
    }

    fn parse_else_branch(&mut self) -> Result<Expr, QueError> {
        if matches!(self.peek(), TokenKind::If) {
            // else if
            self.parse_primary() // will match the If branch
        } else {
            let block = self.parse_block()?;
            Ok(Expr::Block(block))
        }
    }

    /// Parse an optional `else` / `else if` branch after an if-body block.
    ///
    /// Looks ahead past newlines to find `else`, but only consumes the newlines
    /// if `else` is actually present. This prevents a `(` or `[` at the start
    /// of the next line from being parsed as a postfix call/index on the if result.
    fn parse_optional_else(&mut self) -> Result<Option<Box<Expr>>, QueError> {
        let mut la = self.pos;
        while la < self.tokens.len()
            && matches!(self.tokens[la].kind, TokenKind::Newline | TokenKind::Semicolon)
        {
            la += 1;
        }
        if la < self.tokens.len() && matches!(self.tokens[la].kind, TokenKind::Else) {
            self.skip_newlines(); // consume the newlines we peeked past
            self.advance();       // consume `else`
            self.skip_newlines();
            Ok(Some(Box::new(self.parse_else_branch()?)))
        } else {
            Ok(None)
        }
    }

    // ── Brace classification: map or block ──

    /// Classify `{ ... }` in expression position as map or block.
    ///
    /// Sets have their own opener (`#{ ... }`), so `{` only ever means a map
    /// or a block:
    ///   - `{}` → empty map
    ///   - `{"k": v, ...}` / `{ident: v, ...}` / `{...spread}` → map
    ///   - `{ stmt; ...; expr }` or `{ expr }` → block
    ///
    /// `BraceKind::Set` is still returned for the old comma-separated brace
    /// form (`{a, b}`) so the caller can raise a migration error pointing at
    /// `#{a, b}` instead of a confusing "expected `:`".
    ///
    /// Strategy:
    ///   - O(1) fast paths cover the empty/map/spread cases.
    ///   - Slow path scans forward for the first `,` or `:` at the *top
    ///     level* of the brace contents. Nested `(...)`, `[...]`, `{...}`
    ///     groups are jumped over in O(1) using the pre-computed
    ///     `bracket_pairs` table built in `Parser::new` (PAR-14), so total
    ///     work is O(top-level elements) regardless of nesting depth —
    ///     instead of the previous O(tokens-inside-the-braces) per call,
    ///     which compounded to O(n²) on deeply nested literals.
    fn classify_braces(&self) -> BraceKind {
        // Start after `{`
        let mut i = self.pos + 1;
        while i < self.tokens.len() && matches!(self.tokens[i].kind, TokenKind::Newline) {
            i += 1;
        }
        let t1 = self.peek_at(i);
        let t2 = self.peek_at(i + 1);

        // Quick checks
        match (t1, t2) {
            // Empty braces → empty map
            (TokenKind::RBrace, _) => return BraceKind::Map,
            // "string": ... → map
            (TokenKind::StringLit(_), TokenKind::Colon) => return BraceKind::Map,
            // ident: ... → map
            (TokenKind::Ident(_), TokenKind::Colon) => return BraceKind::Map,
            // ...spread → map
            (TokenKind::Spread, _) => return BraceKind::Map,
            _ => {}
        }

        // Scan forward for first `,` or `:` at the top level inside our `{`.
        // Nested groups are skipped in O(1) using the pre-computed bracket
        // pair table, so this loop only visits tokens at the top level of
        // the brace contents.
        let mut j = i;
        while j < self.tokens.len() {
            match &self.tokens[j].kind {
                TokenKind::LParen
                | TokenKind::LBracket
                | TokenKind::LBrace
                | TokenKind::HashBrace => {
                    let close = self.bracket_pairs[j];
                    if close == usize::MAX || close <= j {
                        // Unbalanced — fall back to single-step advance so we
                        // still terminate. The parser will surface the real
                        // mismatch error in due course.
                        j += 1;
                    } else {
                        j = close + 1;
                    }
                    continue;
                }
                TokenKind::RBrace => {
                    // Reached our closing `}` without seeing `,` or `:` at the
                    // top level — single-expression block, e.g. `{ x + 1 }`.
                    return BraceKind::Block;
                }
                TokenKind::Colon => return BraceKind::Map,
                TokenKind::Comma => return BraceKind::Set,
                TokenKind::Eof => break,
                _ => {}
            }
            j += 1;
        }
        BraceKind::Block
    }

    // ── Match arm ──

    fn parse_match_arm(&mut self) -> Result<MatchArm, QueError> {
        let pattern = self.parse_match_pattern()?;

        let guard = if matches!(self.peek(), TokenKind::If) {
            self.advance();
            Some(self.parse_expr()?)
        } else {
            None
        };

        self.expect(&TokenKind::FatArrow)?;
        self.skip_newlines();

        let body = if matches!(self.peek(), TokenKind::LBrace) {
            let block = self.parse_block()?;
            Expr::Block(block)
        } else {
            self.parse_expr()?
        };

        Ok(MatchArm {
            pattern,
            guard,
            body,
        })
    }

    /// Convert lexer `StringPart`s (which hold raw source text in `Expr`/`RawExpr`
    /// variants) into `AstStringPart`s where those source strings are fully parsed
    /// into `Expr` AST nodes. Syntax errors inside `${...}` are detected here,
    /// at parse time, rather than being deferred to runtime evaluation.
    fn parse_string_parts(&self, parts: Vec<crate::token::StringPart>) -> Result<Vec<AstStringPart>, QueError> {
        let span = self.current_span();
        let mut result = Vec::with_capacity(parts.len());
        for part in parts {
            match part {
                crate::token::StringPart::Literal(s) => result.push(AstStringPart::Literal(s)),
                crate::token::StringPart::Expr(src) => {
                    let expr = Self::parse_interp_source(&src, span)?;
                    result.push(AstStringPart::Expr(Box::new(expr)));
                }
                crate::token::StringPart::RawExpr(src) => {
                    let expr = Self::parse_interp_source(&src, span)?;
                    result.push(AstStringPart::RawExpr(Box::new(expr)));
                }
            }
        }
        Ok(result)
    }

    /// Parse a single expression from a raw interpolation source string.
    /// Errors are tagged with `fallback_span` if no more specific span is available.
    fn parse_interp_source(src: &str, fallback_span: Span) -> Result<Expr, QueError> {
        let mut lexer = Lexer::new(src);
        let tokens = lexer.tokenize().map_err(|mut e| {
            if e.span.is_none() {
                e.span = Some(fallback_span);
            }
            e
        })?;
        let mut parser = Parser::new(tokens);
        parser.parse_expr().map_err(|mut e| {
            if e.span.is_none() {
                e.span = Some(fallback_span);
            }
            e
        })
    }

}
