//! Pattern parsing for the Que parser.

use super::{Parser, PatternMode};
use crate::ast::*;
use crate::error::{ErrorKind, QueError};
use crate::token::*;

impl Parser {
    // ── Patterns ──

    fn parse_qualified_pattern_head(
        &mut self,
        first: String,
    ) -> Result<(Option<String>, String), QueError> {
        if matches!(self.peek(), TokenKind::Dot) {
            self.advance();
            let variant = self.expect_ident()?;
            Ok((Some(first), variant))
        } else {
            Ok((None, first))
        }
    }

    /// Parse a destructuring pattern (let/for binding). Bare `Ident` is
    /// always a variable binding.
    pub fn parse_pattern(&mut self) -> Result<Pattern, QueError> {
        let prev = std::mem::replace(&mut self.pattern_mode, PatternMode::Bind);
        let result = self.parse_pattern_inner();
        self.pattern_mode = prev;
        result
    }

    /// Parse a match-arm pattern. Bare uppercase `Ident` matches an enum
    /// unit variant; lowercase still binds.
    pub fn parse_match_pattern(&mut self) -> Result<Pattern, QueError> {
        let prev = std::mem::replace(&mut self.pattern_mode, PatternMode::Match);
        let result = self.parse_pattern_inner();
        self.pattern_mode = prev;
        result
    }

    fn parse_pattern_inner(&mut self) -> Result<Pattern, QueError> {
        let pat = self.parse_primary_pattern()?;

        // Check for `|` (or-pattern)
        if matches!(self.peek(), TokenKind::Pipe) {
            let mut alternatives = vec![pat];
            while matches!(self.peek(), TokenKind::Pipe) {
                self.advance();
                self.skip_newlines();
                alternatives.push(self.parse_primary_pattern()?);
            }
            return Ok(Pattern::Or(alternatives));
        }

        // Check for range pattern (after primary)
        if matches!(self.peek(), TokenKind::Range | TokenKind::RangeInc) {
            let inclusive = matches!(self.peek(), TokenKind::RangeInc);
            self.advance();
            let end = self.parse_primary_pattern()?;
            return Ok(Pattern::Range(
                Some(Box::new(pat)),
                Some(Box::new(end)),
                inclusive,
            ));
        }

        Ok(pat)
    }

    fn parse_primary_pattern(&mut self) -> Result<Pattern, QueError> {
        match self.peek().clone() {
            TokenKind::Ident(name) if name == "_" => {
                self.advance();
                Ok(Pattern::Wildcard)
            }
            TokenKind::Ident(name) => {
                self.advance();
                // Check for @ binding pattern: name @ pattern
                if matches!(self.peek(), TokenKind::At) {
                    self.advance();
                    let inner = self.parse_primary_pattern()?;
                    return Ok(Pattern::Binding(name, Box::new(inner)));
                }
                let (enum_name, variant_name) = self.parse_qualified_pattern_head(name)?;
                // Check for glob() pattern: glob("*.rs")
                if enum_name.is_none() && variant_name == "glob" && matches!(self.peek(), TokenKind::LParen) {
                    self.advance(); // consume '('
                    let g = match self.peek() {
                        TokenKind::StringLit(s) => { let s = s.clone(); self.advance(); s }
                        _ => return Err(QueError::parser(
                            ErrorKind::ExpectedPattern,
                            "glob() pattern requires a string literal argument".to_string(),
                            self.current_span(),
                        )),
                    };
                    self.expect(&TokenKind::RParen)?;
                    return Ok(Pattern::Glob(g));
                }
                // Check for enum variant pattern: Ident(...)
                if matches!(self.peek(), TokenKind::LParen) {
                    self.advance();
                    self.skip_newlines();
                    let mut fields = Vec::new();
                    while !matches!(self.peek(), TokenKind::RParen | TokenKind::Eof) {
                        fields.push(self.parse_pattern_inner()?);
                        self.skip_newlines();
                        if matches!(self.peek(), TokenKind::Comma) {
                            self.advance();
                            self.skip_newlines();
                        }
                    }
                    self.expect(&TokenKind::RParen)?;
                    return Ok(Pattern::Enum(enum_name, variant_name, fields));
                }

                // Instance pattern: `TypeName { field, other: pat, ...rest }`
                // Also supports qualified enum-variant syntax: `EnumName.variant { ... }`.
                if (enum_name.is_some()
                    || variant_name.chars().next().map(|c| c.is_uppercase()).unwrap_or(false))
                    && matches!(self.peek(), TokenKind::LBrace)
                {
                    self.advance(); // consume `{`
                    self.skip_newlines();
                    let mut field_pats = Vec::new();
                    let mut rest = None;
                    while !matches!(self.peek(), TokenKind::RBrace | TokenKind::Eof) {
                        if matches!(self.peek(), TokenKind::Spread) {
                            self.advance();
                            let rest_name = self.expect_ident()?;
                            rest = Some(rest_name);
                            self.skip_newlines();
                            if matches!(self.peek(), TokenKind::Comma) {
                                self.advance();
                                self.skip_newlines();
                            }
                            break;
                        }
                        let key = self.expect_ident()?;
                        let pat = if matches!(self.peek(), TokenKind::Colon) {
                            self.advance();
                            self.skip_newlines();
                            Some(self.parse_pattern_inner()?)
                        } else {
                            None
                        };
                        field_pats.push((key, pat));
                        self.skip_newlines();
                        if matches!(self.peek(), TokenKind::Comma) {
                            self.advance();
                            self.skip_newlines();
                        }
                    }
                    self.expect(&TokenKind::RBrace)?;
                    return Ok(Pattern::Instance(enum_name, variant_name, field_pats, rest));
                }

                if enum_name.is_some()
                    || (self.pattern_mode == PatternMode::Match
                        && variant_name.chars().next().map(|c| c.is_uppercase()).unwrap_or(false))
                {
                    Ok(Pattern::Enum(enum_name, variant_name, vec![]))
                } else {
                    Ok(Pattern::Ident(variant_name))
                }
            }
            TokenKind::IntLit(n) => {
                self.advance();
                Ok(Pattern::IntLit(n))
            }
            TokenKind::FloatLit(f) => {
                self.advance();
                Ok(Pattern::FloatLit(f))
            }
            TokenKind::StringLit(s) => {
                self.advance();
                Ok(Pattern::StringLit(s))
            }
            TokenKind::True => {
                self.advance();
                Ok(Pattern::BoolLit(true))
            }
            TokenKind::False => {
                self.advance();
                Ok(Pattern::BoolLit(false))
            }
            TokenKind::Null => {
                self.advance();
                Ok(Pattern::NullLit)
            }
            // List pattern [a, b, ...rest]
            TokenKind::LBracket => {
                self.advance();
                self.skip_newlines();
                let mut pats = Vec::new();
                let mut rest = None;
                while !matches!(self.peek(), TokenKind::RBracket | TokenKind::Eof) {
                    if matches!(self.peek(), TokenKind::Spread) {
                        self.advance();
                        let rest_pat = self.parse_primary_pattern()?;
                        rest = Some(Box::new(rest_pat));
                        self.skip_newlines();
                        if matches!(self.peek(), TokenKind::Comma) {
                            self.advance();
                            self.skip_newlines();
                        }
                        break;
                    }
                    pats.push(self.parse_pattern_inner()?);
                    self.skip_newlines();
                    if matches!(self.peek(), TokenKind::Comma) {
                        self.advance();
                        self.skip_newlines();
                    }
                }
                self.expect(&TokenKind::RBracket)?;
                Ok(Pattern::List(pats, rest))
            }
            // Struct/map pattern { key: pat, ...rest }
            TokenKind::LBrace => {
                self.advance();
                self.skip_newlines();
                let mut fields = Vec::new();
                let mut rest = None;
                while !matches!(self.peek(), TokenKind::RBrace | TokenKind::Eof) {
                    // Check for ...rest spread
                    if matches!(self.peek(), TokenKind::Spread) {
                        self.advance();
                        let rest_name = self.expect_ident()?;
                        rest = Some(rest_name);
                        self.skip_newlines();
                        if matches!(self.peek(), TokenKind::Comma) {
                            self.advance();
                            self.skip_newlines();
                        }
                        break;
                    }
                    let key = self.expect_ident()?;
                    let pat = if matches!(self.peek(), TokenKind::Colon) {
                        self.advance();
                        self.skip_newlines();
                        Some(self.parse_pattern_inner()?)
                    } else {
                        None
                    };
                    fields.push((key, pat));
                    self.skip_newlines();
                    if matches!(self.peek(), TokenKind::Comma) {
                        self.advance();
                        self.skip_newlines();
                    }
                }
                self.expect(&TokenKind::RBrace)?;
                Ok(Pattern::Struct(fields, rest))
            }
            // Tuple pattern (a, b)
            TokenKind::LParen => {
                self.advance();
                self.skip_newlines();
                let mut pats = Vec::new();
                while !matches!(self.peek(), TokenKind::RParen | TokenKind::Eof) {
                    pats.push(self.parse_pattern_inner()?);
                    self.skip_newlines();
                    if matches!(self.peek(), TokenKind::Comma) {
                        self.advance();
                        self.skip_newlines();
                    }
                }
                self.expect(&TokenKind::RParen)?;
                Ok(Pattern::Tuple(pats))
            }
            _ => Err(QueError::parser(
                ErrorKind::ExpectedPattern,
                format!("expected pattern, got {:?}", self.peek()),
                self.current_span(),
            )),
        }
    }
}
