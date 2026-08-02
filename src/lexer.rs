/// Lexer for the Que language.
///
/// Tokenizes source text into a stream of `Token`s, handling all Que
/// literal forms: strings with interpolation, path/glob/regex/semver
/// literals, command literals (backticks), duration literals, and all
/// operators/keywords specified in the spec.

use crate::error::QueError;
use crate::token::*;

/// Captured position at the start of a token. Bundled to avoid threading
/// three separate `usize`s through every `lex_*` helper signature.
#[derive(Copy, Clone)]
struct LexStart {
    pos: usize,
    line: usize,
    col: usize,
}

pub struct Lexer<'src> {
    source: &'src [u8],
    pos: usize,
    line: usize,
    col: usize,
    /// True until the first token that is not a newline or a pragma. `#!`
    /// only means "shebang or pragma" in that prologue; anywhere else it is
    /// the same error it has always been.
    prologue: bool,
}

impl<'src> Lexer<'src> {
    pub fn new(source: &'src str) -> Self {
        Self {
            source: source.as_bytes(),
            pos: 0,
            line: 1,
            col: 1,
            prologue: true,
        }
    }

    /// Tokenize the entire source, returning all tokens (including Eof).
    pub fn tokenize(&mut self) -> Result<Vec<Token>, QueError> {
        // Rough heuristic: average Que token spans ~4 bytes, so pre-allocate
        // accordingly to avoid Vec re-growth on non-trivial inputs (LEX-7).
        let mut tokens = Vec::with_capacity(self.source.len() / 4 + 1);
        loop {
            let tok = self.next_token()?;
            let is_eof = tok.kind == TokenKind::Eof;
            tokens.push(tok);
            if is_eof {
                break;
            }
        }
        Ok(tokens)
    }

    /// Snapshot the current position. Used at the start of `next_token`
    /// and propagated into the `lex_*` helpers.
    fn mark(&self) -> LexStart {
        LexStart { pos: self.pos, line: self.line, col: self.col }
    }

    fn peek(&self) -> Option<u8> {
        self.source.get(self.pos).copied()
    }

    fn peek_at(&self, offset: usize) -> Option<u8> {
        self.source.get(self.pos + offset).copied()
    }

    fn advance(&mut self) -> Option<u8> {
        let ch = self.source.get(self.pos).copied();
        if let Some(c) = ch {
            self.pos += 1;
            if c == b'\n' {
                self.line += 1;
                self.col = 1;
            } else {
                self.col += 1;
            }
        }
        ch
    }

    fn span_from(&self, start: usize, start_line: usize, start_col: usize) -> Span {
        Span::new(start, self.pos, start_line, start_col)
    }

    /// Advance one full Unicode scalar from the source and return it.
    /// Correctly handles multi-byte UTF-8 sequences (LEX-3 fix).
    fn advance_char(&mut self) -> Option<char> {
        let b = self.peek()?;
        // Fast path for ASCII: no multi-byte decoding needed.
        if b < 0x80 {
            self.pos += 1;
            if b == b'\n' {
                self.line += 1;
                self.col = 1;
            } else {
                self.col += 1;
            }
            return Some(b as char);
        }
        // Multi-byte UTF-8: decode the full scalar from the source slice.
        // Safe: `self.source` is the bytes of a valid UTF-8 `&str`.
        let s = std::str::from_utf8(&self.source[self.pos..])
            .expect("source is valid UTF-8");
        let ch = s.chars().next().unwrap();
        self.pos += ch.len_utf8();
        self.col += 1;
        Some(ch)
    }

    /// Decode one escape sequence (called after `\` has been consumed).
    /// Returns the decoded character, or errors on unknown sequences (LEX-2 fix).
    fn lex_escape_seq(
        &mut self,
        start: usize,
        start_line: usize,
        start_col: usize,
    ) -> Result<char, QueError> {
        match self.advance() {
            Some(b'n')  => Ok('\n'),
            Some(b't')  => Ok('\t'),
            Some(b'r')  => Ok('\r'),
            Some(b'\\') => Ok('\\'),
            Some(b'"')  => Ok('"'),
            Some(b'$')  => Ok('$'),
            Some(b'0')  => Ok('\0'),
            Some(b'e')  => Ok('\x1b'),
            Some(b'x')  => {
                let mut hex = String::new();
                for _ in 0..2 {
                    match self.advance() {
                        Some(c) if (c as char).is_ascii_hexdigit() => hex.push(c as char),
                        _ => return Err(QueError {
                            kind: crate::error::ErrorKind::InvalidEscape,
                            message: r"\xHH escape requires exactly 2 hex digits".into(),
                            span: Some(self.span_from(start, start_line, start_col)),
                            file: None,
                            backtrace: Vec::new(),
                            exit_code: None,
                        }),
                    }
                }
                let code = u32::from_str_radix(&hex, 16).unwrap();
                Ok(char::from_u32(code).unwrap_or('\u{FFFD}'))
            }
            Some(c) => Err(QueError {
                kind: crate::error::ErrorKind::InvalidEscape,
                message: format!("unknown escape sequence: \\{}", c as char),
                span: Some(self.span_from(start, start_line, start_col)),
                file: None,
                backtrace: Vec::new(),
                exit_code: None,
            }),
            None => Err(QueError {
                kind: crate::error::ErrorKind::UnterminatedString,
                message: "unterminated escape sequence".into(),
                span: Some(self.span_from(start, start_line, start_col)),
                file: None,
                backtrace: Vec::new(),
                exit_code: None,
            }),
        }
    }

    fn skip_whitespace_and_comments(&mut self) {
        loop {
            match self.peek() {
                Some(b' ') | Some(b'\t') | Some(b'\r') => {
                    self.advance();
                }
                Some(b'/') => {
                    if self.peek_at(1) == Some(b'/') {
                        // Line comment — consume until newline
                        while let Some(c) = self.peek() {
                            if c == b'\n' {
                                break;
                            }
                            self.advance();
                        }
                    } else if self.peek_at(1) == Some(b'*') {
                        // Block comment — consume until */
                        self.advance(); // /
                        self.advance(); // *
                        let mut depth = 1;
                        while depth > 0 {
                            match self.advance() {
                                Some(b'/') if self.peek() == Some(b'*') => {
                                    self.advance();
                                    depth += 1;
                                }
                                Some(b'*') if self.peek() == Some(b'/') => {
                                    self.advance();
                                    depth -= 1;
                                }
                                None => break,
                                _ => {}
                            }
                        }
                    } else {
                        break;
                    }
                }
                _ => break,
            }
        }
    }

    pub fn next_token(&mut self) -> Result<Token, QueError> {
        let token = self.scan_token()?;
        // The prologue ends at the first token that carries meaning. Newlines
        // and pragmas do not, so `#!strict` may follow a shebang line.
        if !matches!(
            token.kind,
            TokenKind::Newline | TokenKind::Pragma(_) | TokenKind::Shebang(_)
        ) {
            self.prologue = false;
        }
        Ok(token)
    }

    fn scan_token(&mut self) -> Result<Token, QueError> {
        self.skip_whitespace_and_comments();

        let mark = self.mark();
        let start = mark.pos;
        let start_line = mark.line;
        let start_col = mark.col;

        let ch = match self.advance() {
            Some(c) => c,
            None => {
                return Ok(Token::new(
                    TokenKind::Eof,
                    self.span_from(start, start_line, start_col),
                ));
            }
        };

        let kind = match ch {
            // ── Newline ──
            b'\n' => TokenKind::Newline,

            // ── Delimiters ──
            b'(' => TokenKind::LParen,
            b')' => TokenKind::RParen,
            b'{' => TokenKind::LBrace,
            b'}' => TokenKind::RBrace,
            b'[' => TokenKind::LBracket,
            b']' => TokenKind::RBracket,
            b',' => TokenKind::Comma,
            b':' => TokenKind::Colon,
            b';' => TokenKind::Semicolon,
            b'~' => TokenKind::Tilde,
            b'^' => TokenKind::BitXor,
            b'@' => TokenKind::At,

            // ── Set literal opener ──
            b'#' if self.peek() == Some(b'{') => {
                self.advance();
                TokenKind::HashBrace
            }

            // ── `#!` prologue: interpreter line and file-level pragmas ──
            b'#' if self.prologue && self.peek() == Some(b'!') => {
                self.advance(); // !
                let text_start = self.pos;
                while let Some(c) = self.peek() {
                    if c == b'\n' {
                        break;
                    }
                    self.advance();
                }
                let text = std::str::from_utf8(&self.source[text_start..self.pos])
                    .expect("source is valid UTF-8")
                    .trim();
                // `#!/usr/bin/env que` — the kernel's line, not the language's,
                // and only on the very first one.
                if text.starts_with('/') {
                    if start != 0 {
                        return Err(QueError::lexer(
                            "an interpreter line belongs on the first line of the file".to_string(),
                            self.span_from(start, start_line, start_col),
                        ));
                    }
                    TokenKind::Shebang(text.to_string())
                } else if text.is_empty()
                    || !text.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                {
                    return Err(QueError::lexer(
                        format!("`#!{}` is not a pragma; expected a bare name such as `#!strict`", text),
                        self.span_from(start, start_line, start_col),
                    ));
                } else {
                    TokenKind::Pragma(text.to_string())
                }
            }

            // ── Multi-char operators ──
            b'+' => {
                if self.peek() == Some(b'=') {
                    self.advance();
                    TokenKind::PlusEq
                } else {
                    TokenKind::Plus
                }
            }
            b'-' => {
                if self.peek() == Some(b'>') {
                    self.advance();
                    TokenKind::Arrow
                } else if self.peek() == Some(b'=') {
                    self.advance();
                    TokenKind::MinusEq
                } else {
                    TokenKind::Minus
                }
            }
            b'*' => {
                if self.peek() == Some(b'*') {
                    self.advance();
                    TokenKind::Power
                } else if self.peek() == Some(b'=') {
                    self.advance();
                    TokenKind::StarEq
                } else {
                    TokenKind::Star
                }
            }
            b'/' => {
                if self.peek() == Some(b'=') {
                    self.advance();
                    TokenKind::SlashEq
                } else {
                    TokenKind::Slash
                }
            }
            b'%' => TokenKind::Percent,
            b'=' => {
                if self.peek() == Some(b'=') {
                    self.advance();
                    TokenKind::EqEq
                } else if self.peek() == Some(b'>') {
                    self.advance();
                    TokenKind::FatArrow
                } else {
                    TokenKind::Eq
                }
            }
            b'!' => {
                if self.peek() == Some(b'=') {
                    self.advance();
                    TokenKind::BangEq
                } else {
                    TokenKind::Bang
                }
            }
            b'<' => {
                if self.peek() == Some(b'=') {
                    self.advance();
                    TokenKind::LtEq
                } else if self.peek() == Some(b'<') {
                    self.advance();
                    TokenKind::Shl
                } else {
                    TokenKind::Lt
                }
            }
            b'>' => {
                if self.peek() == Some(b'=') {
                    self.advance();
                    TokenKind::GtEq
                } else if self.peek() == Some(b'>') {
                    self.advance();
                    TokenKind::Shr
                } else {
                    TokenKind::Gt
                }
            }
            b'&' => {
                if self.peek() == Some(b'&') {
                    self.advance();
                    TokenKind::And
                } else {
                    TokenKind::BitAnd
                }
            }
            b'|' => {
                if self.peek() == Some(b'|') {
                    self.advance();
                    TokenKind::Or
                } else if self.peek() == Some(b'>') {
                    self.advance();
                    TokenKind::PipeArrow
                } else {
                    TokenKind::Pipe
                }
            }
            b'?' => {
                if self.peek() == Some(b'?') {
                    self.advance();
                    TokenKind::NullCoalesce
                } else if self.peek() == Some(b'.') {
                    self.advance();
                    TokenKind::QuestionDot
                } else {
                    TokenKind::Question
                }
            }
            b'.' => {
                if self.peek() == Some(b'.') {
                    self.advance();
                    if self.peek() == Some(b'.') {
                        self.advance();
                        TokenKind::Spread
                    } else if self.peek() == Some(b'=') {
                        self.advance();
                        TokenKind::RangeInc
                    } else {
                        TokenKind::Range
                    }
                } else {
                    TokenKind::Dot
                }
            }

            // ── Command literal (backtick) ──
            b'`' => return self.lex_command(mark),

            // ── String literal or prefixed literal ──
            b'"' => {
                // Check for triple-quote
                if self.peek() == Some(b'"') && self.peek_at(1) == Some(b'"') {
                    self.advance();
                    self.advance();
                    return self.lex_multiline_string(mark);
                }
                return self.lex_string(mark);
            }

            // ── Prefixed literals: p"...", g"...", re"...", v"...", r"...", r#"..."# ──
            b'p' if self.peek() == Some(b'"') => {
                self.advance(); // consume '"'
                return self.lex_path_literal(mark);
            }
            b'g' if self.peek() == Some(b'"') => {
                self.advance(); // consume '"'
                return self.lex_glob_literal(mark);
            }
            b'r' if self.peek() == Some(b'e') && self.peek_at(1) == Some(b'"') => {
                self.advance(); // consume 'e'
                self.advance(); // consume '"'
                return self.lex_prefixed_string(mark, "regex");
            }
            b'r' if self.peek() == Some(b'"') => {
                self.advance(); // consume '"'
                return self.lex_raw_string(mark, 0);
            }
            b'r' if self.peek() == Some(b'#') => {
                // Count number of '#' characters
                let mut hash_count = 0;
                while self.peek_at(hash_count) == Some(b'#') {
                    hash_count += 1;
                }
                // Must be followed by '"' after the hashes
                if self.peek_at(hash_count) == Some(b'"') {
                    for _ in 0..hash_count {
                        self.advance(); // consume each '#'
                    }
                    self.advance(); // consume '"'
                    return self.lex_raw_string(mark, hash_count);
                }
                // Otherwise fall through to identifier lexing
                return self.lex_identifier(ch, mark);
            }
            b'v' if self.peek() == Some(b'"') => {
                self.advance();
                return self.lex_prefixed_string(mark, "semver");
            }

            // ── Numbers ──
            b'0'..=b'9' => return self.lex_number(ch, mark),

            // ── Identifiers and keywords ──
            b'a'..=b'z' | b'A'..=b'Z' | b'_' => {
                return self.lex_identifier(ch, mark);
            }

            other => {
                return Err(QueError::lexer(
                    format!("unexpected character: '{}'", other as char),
                    self.span_from(start, start_line, start_col),
                ));
            }
        };

        Ok(Token::new(
            kind,
            self.span_from(start, start_line, start_col),
        ))
    }

    // ── String lexing ──

    fn lex_string(&mut self, start: LexStart) -> Result<Token, QueError> {
        let LexStart { pos: start, line: start_line, col: start_col } = start;
        let mut parts: Vec<StringPart> = Vec::new();
        let mut buf = String::new();
        let mut has_interpolation = false;

        loop {
            match self.peek() {
                None | Some(b'\n') => {
                    return Err(QueError {
                        kind: crate::error::ErrorKind::UnterminatedString,
                        message: "unterminated string literal".into(),
                        span: Some(self.span_from(start, start_line, start_col)),
                        file: None,
                        backtrace: Vec::new(),
                        exit_code: None,
                    });
                }
                Some(b'"') => {
                    self.advance();
                    break;
                }
                Some(b'\\') => {
                    self.advance();
                    buf.push(self.lex_escape_seq(start, start_line, start_col)?);
                }
                Some(b'$') if self.peek_at(1) == Some(b'{') => {
                    has_interpolation = true;
                    self.advance(); // $
                    self.advance(); // {
                    if !buf.is_empty() {
                        parts.push(StringPart::Literal(std::mem::take(&mut buf)));
                    }
                    let expr = self.read_interpolation_expr()?;
                    parts.push(StringPart::Expr(expr));
                }
                Some(_) => {
                    if let Some(ch) = self.advance_char() {
                        buf.push(ch);
                    }
                }
            }
        }

        let span = self.span_from(start, start_line, start_col);
        if has_interpolation {
            if !buf.is_empty() {
                parts.push(StringPart::Literal(buf));
            }
            Ok(Token::new(TokenKind::InterpolatedString(parts), span))
        } else {
            Ok(Token::new(TokenKind::StringLit(buf), span))
        }
    }

    fn lex_multiline_string(&mut self, start: LexStart) -> Result<Token, QueError> {
        let LexStart { pos: start, line: start_line, col: start_col } = start;
        let mut parts: Vec<StringPart> = Vec::new();
        let mut buf = String::new();
        let mut has_interpolation = false;

        loop {
            match self.peek() {
                None => {
                    return Err(QueError {
                        kind: crate::error::ErrorKind::UnterminatedString,
                        message: "unterminated multi-line string".into(),
                        span: Some(self.span_from(start, start_line, start_col)),
                        file: None,
                        backtrace: Vec::new(),
                        exit_code: None,
                    });
                }
                Some(b'"') if self.peek_at(1) == Some(b'"') && self.peek_at(2) == Some(b'"') => {
                    self.advance();
                    self.advance();
                    self.advance();
                    break;
                }
                Some(b'\\') => {
                    self.advance();
                    buf.push(self.lex_escape_seq(start, start_line, start_col)?);
                }
                Some(b'$') if self.peek_at(1) == Some(b'{') => {
                    has_interpolation = true;
                    self.advance(); // $
                    self.advance(); // {
                    if !buf.is_empty() {
                        parts.push(StringPart::Literal(std::mem::take(&mut buf)));
                    }
                    let expr = self.read_interpolation_expr()?;
                    parts.push(StringPart::Expr(expr));
                }
                Some(_) => {
                    if let Some(ch) = self.advance_char() {
                        buf.push(ch);
                    }
                }
            }
        }

        // Flush remaining literal
        if !buf.is_empty() {
            parts.push(StringPart::Literal(buf));
        }

        // Strip common leading whitespace from literal parts
        let parts = strip_common_indent_parts(parts);

        let span = self.span_from(start, start_line, start_col);
        if has_interpolation {
            Ok(Token::new(TokenKind::InterpolatedString(parts), span))
        } else {
            // Collapse all literal parts into a single string
            let s = parts
                .into_iter()
                .map(|p| match p {
                    StringPart::Literal(s) => s,
                    _ => unreachable!(),
                })
                .collect::<String>();
            Ok(Token::new(TokenKind::StringLit(s), span))
        }
    }

    /// Lex a raw string literal: `r"..."` (hash_count=0) or `r#"..."#`, `r##"..."##`, etc.
    /// No escape processing is done; the string ends at `"` followed by the same
    /// number of `#` characters that appeared in the opening delimiter.
    fn lex_raw_string(
        &mut self,
        start: LexStart,
        hash_count: usize,
    ) -> Result<Token, QueError> {
        let LexStart { pos: start, line: start_line, col: start_col } = start;
        let mut buf = String::new();
        loop {
            match self.peek() {
                None => {
                    return Err(QueError {
                        kind: crate::error::ErrorKind::UnterminatedString,
                        message: "unterminated raw string literal".into(),
                        span: Some(self.span_from(start, start_line, start_col)),
                        file: None,
                        backtrace: Vec::new(),
                        exit_code: None,
                    });
                }
                Some(b'"') => {
                    self.advance(); // consume '"'
                    // Check if followed by the right number of '#'
                    let mut matched = true;
                    for i in 0..hash_count {
                        if self.peek_at(i) != Some(b'#') {
                            matched = false;
                            break;
                        }
                    }
                    if matched {
                        // Consume the closing '#' characters
                        for _ in 0..hash_count {
                            self.advance();
                        }
                        break;
                    } else {
                        // Not the closing delimiter; include the '"' in the string
                        buf.push('"');
                    }
                }
                Some(_) => {
                    if let Some(ch) = self.advance_char() {
                        buf.push(ch);
                    }
                }
            }
        }
        Ok(Token::new(
            TokenKind::StringLit(buf),
            self.span_from(start, start_line, start_col),
        ))
    }

    /// Lex a path literal: `p"..."`. Supports `${...}` interpolation.
    /// Double slashes in the literal text are collapsed to single slashes.
    fn lex_path_literal(&mut self, start: LexStart) -> Result<Token, QueError> {
        let LexStart { pos: start, line: start_line, col: start_col } = start;
        let mut parts: Vec<StringPart> = Vec::new();
        let mut buf = String::new();

        loop {
            match self.peek() {
                None | Some(b'\n') => {
                    return Err(QueError {
                        kind: crate::error::ErrorKind::UnterminatedString,
                        message: "unterminated path literal".into(),
                        span: Some(self.span_from(start, start_line, start_col)),
                        file: None,
                        backtrace: Vec::new(),
                        exit_code: None,
                    });
                }
                Some(b'"') => {
                    self.advance();
                    break;
                }
                Some(b'$') if self.peek_at(1) == Some(b'{') => {
                    self.advance(); // $
                    self.advance(); // {
                    if !buf.is_empty() {
                        parts.push(StringPart::Literal(std::mem::take(&mut buf)));
                    }
                    let expr = self.read_interpolation_expr()?;
                    parts.push(StringPart::Expr(expr));
                }
                Some(b'/') => {
                    self.advance();
                    // Collapse consecutive slashes
                    while self.peek() == Some(b'/') {
                        self.advance();
                    }
                    buf.push('/');
                }
                Some(_) => {
                    if let Some(ch) = self.advance_char() {
                        buf.push(ch);
                    }
                }
            }
        }

        // Flush remaining literal
        if !buf.is_empty() {
            parts.push(StringPart::Literal(buf));
        }
        // Remove trailing slash from last literal part (unless it's just "/")
        if let Some(StringPart::Literal(s)) = parts.last_mut() {
            if s.len() > 1 && s.ends_with('/') {
                s.pop();
            }
        }
        Ok(Token::new(
            TokenKind::PathLit(parts),
            self.span_from(start, start_line, start_col),
        ))
    }

    /// Lex a glob literal: `g"..."`. Supports `${...}` interpolation.
    /// Bare `{a,b}` alternation is treated as literal glob syntax (not interpolation).
    fn lex_glob_literal(&mut self, start: LexStart) -> Result<Token, QueError> {
        let LexStart { pos: start, line: start_line, col: start_col } = start;
        let mut parts: Vec<StringPart> = Vec::new();
        let mut buf = String::new();

        loop {
            match self.peek() {
                None | Some(b'\n') => {
                    return Err(QueError {
                        kind: crate::error::ErrorKind::UnterminatedString,
                        message: "unterminated glob literal".into(),
                        span: Some(self.span_from(start, start_line, start_col)),
                        file: None,
                        backtrace: Vec::new(),
                        exit_code: None,
                    });
                }
                Some(b'"') => {
                    self.advance();
                    break;
                }
                Some(b'$') if self.peek_at(1) == Some(b'{') => {
                    self.advance(); // $
                    self.advance(); // {
                    if !buf.is_empty() {
                        parts.push(StringPart::Literal(std::mem::take(&mut buf)));
                    }
                    let expr = self.read_interpolation_expr()?;
                    parts.push(StringPart::Expr(expr));
                }
                Some(_) => {
                    if let Some(ch) = self.advance_char() {
                        buf.push(ch);
                    }
                }
            }
        }

        if !buf.is_empty() {
            parts.push(StringPart::Literal(buf));
        }
        Ok(Token::new(
            TokenKind::GlobLit(parts),
            self.span_from(start, start_line, start_col),
        ))
    }

    fn lex_prefixed_string(
        &mut self,
        start: LexStart,
        prefix: &str,
    ) -> Result<Token, QueError> {
        let LexStart { pos: start, line: start_line, col: start_col } = start;
        let mut buf = String::new();
        loop {
            match self.peek() {
                None | Some(b'\n') => {
                    return Err(QueError {
                        kind: crate::error::ErrorKind::UnterminatedString,
                        message: format!("unterminated {prefix} literal"),
                        span: Some(self.span_from(start, start_line, start_col)),
                        file: None,
                        backtrace: Vec::new(),
                        exit_code: None,
                    });
                }
                Some(b'"') => {
                    self.advance();
                    break;
                }
                Some(b'\\') => {
                    self.advance();
                    if let Some(c) = self.advance() {
                        match c {
                            b'"' => buf.push('"'),
                            b'\\' => buf.push('\\'),
                            // Pass through other escapes to the underlying engine
                            // (e.g. \d, \w in regex literals). `c` is always ASCII
                            // here since regex/semver escape sequences use ASCII.
                            _ => {
                                buf.push('\\');
                                buf.push(c as char);
                            }
                        }
                    }
                }
                Some(_) => {
                    if let Some(ch) = self.advance_char() {
                        buf.push(ch);
                    }
                }
            }
        }
        let span = self.span_from(start, start_line, start_col);
        let kind = match prefix {
            "regex" => TokenKind::RegexLit(buf),
            "semver" => TokenKind::SemverLit(buf),
            _ => unreachable!(),
        };
        Ok(Token::new(kind, span))
    }

    // ── Command literal (backtick) ──

    fn lex_command(&mut self, start: LexStart) -> Result<Token, QueError> {
        let LexStart { pos: start, line: start_line, col: start_col } = start;
        let mut parts: Vec<StringPart> = Vec::new();
        let mut buf = String::new();

        loop {
            match self.peek() {
                None => {
                    return Err(QueError {
                        kind: crate::error::ErrorKind::UnterminatedCommand,
                        message: "unterminated command literal".into(),
                        span: Some(self.span_from(start, start_line, start_col)),
                        file: None,
                        backtrace: Vec::new(),
                        exit_code: None,
                    });
                }
                Some(b'`') => {
                    self.advance();
                    break;
                }
                Some(b'$') if self.peek_at(1) == Some(b'{') => {
                    self.advance(); // $
                    self.advance(); // {
                    if !buf.is_empty() {
                        parts.push(StringPart::Literal(std::mem::take(&mut buf)));
                    }
                    let expr = self.read_interpolation_expr()?;
                    parts.push(StringPart::Expr(expr));
                }
                Some(b'!') if self.peek_at(1) == Some(b'{') => {
                    self.advance(); // !
                    self.advance(); // {
                    if !buf.is_empty() {
                        parts.push(StringPart::Literal(std::mem::take(&mut buf)));
                    }
                    let expr = self.read_interpolation_expr()?;
                    parts.push(StringPart::RawExpr(expr));
                }
                Some(b'\\') => {
                    self.advance();
                    match self.advance() {
                        Some(b'`') => buf.push('`'),
                        Some(b'\\') => buf.push('\\'),
                        // Shell commands use their own escape semantics;
                        // pass other sequences through unchanged. `c` is
                        // always an ASCII byte in a valid shell escape.
                        Some(c) => {
                            buf.push('\\');
                            buf.push(c as char);
                        }
                        None => {}
                    }
                }
                Some(_) => {
                    if let Some(ch) = self.advance_char() {
                        buf.push(ch);
                    }
                }
            }
        }

        if !buf.is_empty() {
            parts.push(StringPart::Literal(buf));
        }
        Ok(Token::new(
            TokenKind::CmdLit(parts),
            self.span_from(start, start_line, start_col),
        ))
    }

    /// Read an interpolation expression between `{` (already consumed) and `}`.
    /// Handles nested braces.
    fn read_interpolation_expr(&mut self) -> Result<String, QueError> {
        let mut depth = 1u32;
        let start = self.pos;
        let start_line = self.line;
        let start_col = self.col;

        // Scan byte-by-byte to track depth and skip string literals.
        // UTF-8 multi-byte sequence bytes are all >= 0x80 and can never be
        // confused with the ASCII delimiters '{' (0x7B), '}' (0x7D),
        // '"' (0x22), or '\' (0x5C), so brace counting is always correct.
        while depth > 0 {
            match self.advance() {
                Some(b'{') => depth += 1,
                Some(b'}') => depth -= 1,
                Some(b'"') => {
                    // Skip over a nested string literal so braces inside it
                    // are not miscounted.
                    loop {
                        match self.advance() {
                            Some(b'"') => break,
                            Some(b'\\') => { self.advance(); } // skip escaped byte
                            Some(_) => {}
                            None => break,
                        }
                    }
                }
                Some(_) => {}
                None => {
                    return Err(QueError {
                        kind: crate::error::ErrorKind::UnterminatedString,
                        message: "unterminated interpolation expression".into(),
                        span: Some(self.span_from(start, start_line, start_col)),
                        file: None,
                        backtrace: Vec::new(),
                        exit_code: None,
                    });
                }
            }
        }
        // `self.pos` is one past the closing `}`.
        // Capture the expression as a proper UTF-8 string from the source slice.
        let expr = std::str::from_utf8(&self.source[start..self.pos - 1])
            .expect("source is valid UTF-8")
            .to_owned();
        Ok(expr)
    }

    // ── Number lexing ──

    fn lex_number(
        &mut self,
        first: u8,
        start: LexStart,
    ) -> Result<Token, QueError> {
        let LexStart { pos: start, line: start_line, col: start_col } = start;

        // Check for hex/binary/octal prefix
        if first == b'0' {
            match self.peek() {
                Some(b'x') | Some(b'X') => {
                    self.advance();
                    let mut num_str = String::new();
                    while let Some(c) = self.peek() {
                        if c.is_ascii_hexdigit() || c == b'_' {
                            self.advance();
                            if c != b'_' {
                                num_str.push(c as char);
                            }
                        } else {
                            break;
                        }
                    }
                    let val = i64::from_str_radix(&num_str, 16).map_err(|_| QueError {
                        kind: crate::error::ErrorKind::InvalidNumber,
                        message: format!("invalid hex literal: 0x{num_str}"),
                        span: Some(self.span_from(start, start_line, start_col)),
                        file: None,
                        backtrace: Vec::new(),
                        exit_code: None,
                    })?;
                    return Ok(Token::new(
                        TokenKind::IntLit(val),
                        self.span_from(start, start_line, start_col),
                    ));
                }
                Some(b'b') | Some(b'B') => {
                    self.advance();
                    let mut num_str = String::new();
                    while let Some(c) = self.peek() {
                        if c == b'0' || c == b'1' || c == b'_' {
                            self.advance();
                            if c != b'_' {
                                num_str.push(c as char);
                            }
                        } else {
                            break;
                        }
                    }
                    let val = i64::from_str_radix(&num_str, 2).map_err(|_| QueError {
                        kind: crate::error::ErrorKind::InvalidNumber,
                        message: format!("invalid binary literal: 0b{num_str}"),
                        span: Some(self.span_from(start, start_line, start_col)),
                        file: None,
                        backtrace: Vec::new(),
                        exit_code: None,
                    })?;
                    return Ok(Token::new(
                        TokenKind::IntLit(val),
                        self.span_from(start, start_line, start_col),
                    ));
                }
                Some(b'o') | Some(b'O') => {
                    self.advance();
                    let mut num_str = String::new();
                    while let Some(c) = self.peek() {
                        if (b'0'..=b'7').contains(&c) || c == b'_' {
                            self.advance();
                            if c != b'_' {
                                num_str.push(c as char);
                            }
                        } else {
                            break;
                        }
                    }
                    let val = i64::from_str_radix(&num_str, 8).map_err(|_| QueError {
                        kind: crate::error::ErrorKind::InvalidNumber,
                        message: format!("invalid octal literal: 0o{num_str}"),
                        span: Some(self.span_from(start, start_line, start_col)),
                        file: None,
                        backtrace: Vec::new(),
                        exit_code: None,
                    })?;
                    return Ok(Token::new(
                        TokenKind::IntLit(val),
                        self.span_from(start, start_line, start_col),
                    ));
                }
                _ => {}
            }
        }

        // Decimal integer or float: accumulate the integer value directly
        // (LEX-8) to avoid building an intermediate `String` for the common
        // integer case. The string is built lazily only when we transition
        // to a float, where the standard f64 parser is the cleanest option.
        // `first` is guaranteed to be an ASCII digit at this entry point.
        let mut val: i64 = (first - b'0') as i64;
        let mut is_float = false;

        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                self.advance();
                val = val
                    .checked_mul(10)
                    .and_then(|v| v.checked_add((c - b'0') as i64))
                    .ok_or_else(|| QueError {
                        kind: crate::error::ErrorKind::InvalidNumber,
                        message: "integer literal overflows i64".to_string(),
                        span: Some(self.span_from(start, start_line, start_col)),
                        file: None,
                        backtrace: Vec::new(),
                        exit_code: None,
                    })?;
            } else if c == b'_' {
                self.advance();
            } else if c == b'.' && self.peek_at(1).is_some_and(|n| n.is_ascii_digit()) {
                is_float = true;
                break;
            } else {
                break;
            }
        }

        // Float path: reconstruct the integer part and append the fractional
        // digits, then defer to `f64::from_str`.
        let float_str: Option<String> = if is_float {
            let mut s = val.to_string();
            self.advance(); // consume '.'
            s.push('.');
            while let Some(c) = self.peek() {
                if c.is_ascii_digit() || c == b'_' {
                    self.advance();
                    if c != b'_' {
                        s.push(c as char);
                    }
                } else {
                    break;
                }
            }
            Some(s)
        } else {
            None
        };

        // Check for duration suffix
        if let Some(c) = self.peek() {
            let dur = match c {
                b's' => {
                    // 's' is a duration suffix only when it is not followed by
                    // another identifier character (so `5s` is a duration but
                    // `5struct`, `5sec`, and `5_s` are not). The
                    // `is_ascii_alphabetic` check already covers `'t'`, so no
                    // extra `peek_at(1) != Some(b't')` guard is needed (LEX-9).
                    if self
                        .peek_at(1)
                        .is_none_or(|n| !n.is_ascii_alphabetic() && n != b'_')
                    {
                        self.advance();
                        Some(DurationUnit::Seconds)
                    } else {
                        None
                    }
                }
                b'm' => {
                    if self.peek_at(1) == Some(b's')
                        && self
                            .peek_at(2)
                            .is_none_or(|n| !n.is_ascii_alphabetic() && n != b'_')
                    {
                        self.advance();
                        self.advance();
                        Some(DurationUnit::Milliseconds)
                    } else if self
                        .peek_at(1)
                        .is_none_or(|n| !n.is_ascii_alphabetic() && n != b'_')
                    {
                        self.advance();
                        Some(DurationUnit::Minutes)
                    } else {
                        None
                    }
                }
                b'h'
                    if self
                        .peek_at(1)
                        .is_none_or(|n| !n.is_ascii_alphabetic() && n != b'_') =>
                {
                    self.advance();
                    Some(DurationUnit::Hours)
                }
                b'd'
                    if self
                        .peek_at(1)
                        .is_none_or(|n| !n.is_ascii_alphabetic() && n != b'_') =>
                {
                    self.advance();
                    Some(DurationUnit::Days)
                }
                _ => None,
            };

            if let Some(unit) = dur {
                let value = match &float_str {
                    Some(s) => s.parse::<f64>().map_err(|_| QueError {
                        kind: crate::error::ErrorKind::InvalidNumber,
                        message: format!("invalid duration number: {s}"),
                        span: Some(self.span_from(start, start_line, start_col)),
                        file: None,
                        backtrace: Vec::new(),
                        exit_code: None,
                    })?,
                    None => val as f64,
                };
                return Ok(Token::new(
                    TokenKind::DurationLit(value, unit),
                    self.span_from(start, start_line, start_col),
                ));
            }
        }

        let span = self.span_from(start, start_line, start_col);
        if let Some(s) = float_str {
            let v: f64 = s.parse().map_err(|_| QueError {
                kind: crate::error::ErrorKind::InvalidNumber,
                message: format!("invalid float: {s}"),
                span: Some(span),
                file: None,
                backtrace: Vec::new(),
                exit_code: None,
            })?;
            Ok(Token::new(TokenKind::FloatLit(v), span))
        } else {
            Ok(Token::new(TokenKind::IntLit(val), span))
        }
    }

    // ── Identifier / keyword lexing ──

    fn lex_identifier(
        &mut self,
        _first: u8,
        start: LexStart,
    ) -> Result<Token, QueError> {
        // The first byte was already consumed by next_token. Walk forward
        // until the identifier ends; then slice the source once. This avoids
        // pushing one byte at a time into a fresh String (LEX-6).
        //
        // Identifier bytes are always ASCII (alphanumeric + `_`), so the
        // slice from `start.pos` to `self.pos` is guaranteed valid UTF-8.
        while let Some(c) = self.peek() {
            if c.is_ascii_alphanumeric() || c == b'_' {
                self.advance();
            } else {
                break;
            }
        }
        let ident = std::str::from_utf8(&self.source[start.pos..self.pos])
            .expect("identifier bytes are ASCII")
            .to_owned();
        let span = Span::new(start.pos, self.pos, start.line, start.col);
        let kind = TokenKind::keyword(&ident).unwrap_or(TokenKind::Ident(ident));
        Ok(Token::new(kind, span))
    }
}

// ── Helper functions ──

/// Strip common leading whitespace from multiline string parts (with interpolation).
///
/// Works like `strip_common_indent` but operates on `Vec<StringPart>`, correctly
/// handling indentation that appears in `Literal` segments around `Expr` parts.
fn strip_common_indent_parts(parts: Vec<StringPart>) -> Vec<StringPart> {
    if parts.is_empty() {
        return parts;
    }

    // ── Step 1: Determine the minimum indent ──
    // Walk through literal parts tracking line boundaries. The "first line"
    // (content immediately after the opening `"""`) is skipped for indent
    // calculation, matching the behaviour of the plain `strip_common_indent`.
    let mut min_indent: Option<usize> = None;
    let mut at_line_start = true;
    let mut current_indent: usize = 0;
    let mut first_line = true;

    for part in &parts {
        match part {
            StringPart::Literal(s) => {
                for ch in s.chars() {
                    if ch == '\n' {
                        at_line_start = true;
                        current_indent = 0;
                        first_line = false;
                    } else if at_line_start {
                        if ch == ' ' || ch == '\t' {
                            current_indent += 1;
                        } else {
                            if !first_line {
                                min_indent = Some(match min_indent {
                                    Some(m) => m.min(current_indent),
                                    None => current_indent,
                                });
                            }
                            at_line_start = false;
                        }
                    }
                }
            }
            StringPart::Expr(_) => {
                // An interpolation counts as non-whitespace content
                if at_line_start && !first_line {
                    min_indent = Some(match min_indent {
                        Some(m) => m.min(current_indent),
                        None => current_indent,
                    });
                }
                at_line_start = false;
            }
            _ => {}
        }
    }

    let min_indent = min_indent.unwrap_or(0);

    // ── Step 2: Strip min_indent spaces from the start of each line ──
    let mut result: Vec<StringPart> = Vec::new();
    at_line_start = true;
    let mut chars_to_skip = 0usize;

    for part in parts {
        match part {
            StringPart::Literal(s) => {
                let mut new_s = String::new();
                for ch in s.chars() {
                    if ch == '\n' {
                        new_s.push('\n');
                        at_line_start = true;
                        chars_to_skip = min_indent;
                    } else if at_line_start && chars_to_skip > 0 && (ch == ' ' || ch == '\t') {
                        chars_to_skip -= 1;
                    } else {
                        at_line_start = false;
                        chars_to_skip = 0;
                        new_s.push(ch);
                    }
                }
                if !new_s.is_empty() {
                    result.push(StringPart::Literal(new_s));
                }
            }
            StringPart::Expr(e) => {
                at_line_start = false;
                chars_to_skip = 0;
                result.push(StringPart::Expr(e));
            }
            other => {
                result.push(other);
            }
        }
    }

    // ── Step 3: Remove leading/trailing empty lines ──
    // Leading: strip a leading '\n' from the first literal (the newline after """)
    if let Some(StringPart::Literal(s)) = result.first_mut() {
        if s.starts_with('\n') {
            *s = s[1..].to_string();
        }
    }
    if matches!(result.first(), Some(StringPart::Literal(s)) if s.is_empty()) {
        result.remove(0);
    }

    // Trailing: strip a trailing '\n' from the last literal (the newline before """)
    if let Some(StringPart::Literal(s)) = result.last_mut() {
        if s.ends_with('\n') {
            s.pop();
        }
    }
    if matches!(result.last(), Some(StringPart::Literal(s)) if s.is_empty()) {
        result.pop();
    }

    result
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;

    fn tokenize(src: &str) -> Vec<TokenKind> {
        let mut lexer = Lexer::new(src);
        lexer
            .tokenize()
            .unwrap()
            .into_iter()
            .map(|t| t.kind)
            .filter(|k| !matches!(k, TokenKind::Newline))
            .collect()
    }

    fn tokenize_all(src: &str) -> Vec<TokenKind> {
        let mut lexer = Lexer::new(src);
        lexer
            .tokenize()
            .unwrap()
            .into_iter()
            .map(|t| t.kind)
            .collect()
    }

    #[test]
    fn test_integer_literals() {
        assert_eq!(tokenize("42"), vec![TokenKind::IntLit(42), TokenKind::Eof]);
        assert_eq!(tokenize("0"), vec![TokenKind::IntLit(0), TokenKind::Eof]);
        assert_eq!(
            tokenize("1_000_000"),
            vec![TokenKind::IntLit(1_000_000), TokenKind::Eof]
        );
    }

    #[test]
    fn test_hex_and_binary() {
        assert_eq!(
            tokenize("0xFF"),
            vec![TokenKind::IntLit(255), TokenKind::Eof]
        );
        assert_eq!(
            tokenize("0b1010"),
            vec![TokenKind::IntLit(10), TokenKind::Eof]
        );
    }

    #[test]
    fn test_float_literals() {
        assert_eq!(
            tokenize("3.14"),
            vec![TokenKind::FloatLit(3.14), TokenKind::Eof]
        );
        assert_eq!(
            tokenize("0.5"),
            vec![TokenKind::FloatLit(0.5), TokenKind::Eof]
        );
    }

    #[test]
    fn test_duration_literals() {
        assert_eq!(
            tokenize("5s"),
            vec![
                TokenKind::DurationLit(5.0, DurationUnit::Seconds),
                TokenKind::Eof
            ]
        );
        assert_eq!(
            tokenize("500ms"),
            vec![
                TokenKind::DurationLit(500.0, DurationUnit::Milliseconds),
                TokenKind::Eof
            ]
        );
        assert_eq!(
            tokenize("10m"),
            vec![
                TokenKind::DurationLit(10.0, DurationUnit::Minutes),
                TokenKind::Eof
            ]
        );
        assert_eq!(
            tokenize("2h"),
            vec![
                TokenKind::DurationLit(2.0, DurationUnit::Hours),
                TokenKind::Eof
            ]
        );
        assert_eq!(
            tokenize("1d"),
            vec![
                TokenKind::DurationLit(1.0, DurationUnit::Days),
                TokenKind::Eof
            ]
        );
    }

    #[test]
    fn test_string_literal() {
        assert_eq!(
            tokenize(r#""hello""#),
            vec![TokenKind::StringLit("hello".into()), TokenKind::Eof]
        );
    }

    #[test]
    fn test_string_with_escapes() {
        assert_eq!(
            tokenize(r#""hello\nworld""#),
            vec![
                TokenKind::StringLit("hello\nworld".into()),
                TokenKind::Eof
            ]
        );
    }

    #[test]
    fn test_hex_escape() {
        // \x1b is ESC (0x1B = 27)
        assert_eq!(
            tokenize(r#""\x1b[31m""#),
            vec![
                TokenKind::StringLit("\x1b[31m".into()),
                TokenKind::Eof
            ]
        );
    }

    #[test]
    fn test_hex_escape_uppercase() {
        assert_eq!(
            tokenize(r#""\x1B""#),
            vec![
                TokenKind::StringLit("\x1b".into()),
                TokenKind::Eof
            ]
        );
    }

    #[test]
    fn test_escape_e() {
        // \e is shorthand for ESC
        assert_eq!(
            tokenize(r#""\e[0m""#),
            vec![
                TokenKind::StringLit("\x1b[0m".into()),
                TokenKind::Eof
            ]
        );
    }

    #[test]
    fn test_interpolated_string() {
        let tokens = tokenize(r#""hello ${name}!""#);
        assert_eq!(
            tokens,
            vec![
                TokenKind::InterpolatedString(vec![
                    StringPart::Literal("hello ".into()),
                    StringPart::Expr("name".into()),
                    StringPart::Literal("!".into()),
                ]),
                TokenKind::Eof
            ]
        );
    }

    #[test]
    fn test_multi_interpolation() {
        let tokens = tokenize(r#""${a} + ${b} = ${c}""#);
        assert_eq!(
            tokens,
            vec![
                TokenKind::InterpolatedString(vec![
                    StringPart::Expr("a".into()),
                    StringPart::Literal(" + ".into()),
                    StringPart::Expr("b".into()),
                    StringPart::Literal(" = ".into()),
                    StringPart::Expr("c".into()),
                ]),
                TokenKind::Eof
            ]
        );
    }

    #[test]
    fn test_p_ident_without_quote() {
        // p followed by a space (not ") is a plain identifier
        assert_eq!(
            tokenize(r#"p "hello""#),
            vec![
                TokenKind::Ident("p".into()),
                TokenKind::StringLit("hello".into()),
                TokenKind::Eof
            ]
        );
    }

    #[test]
    fn test_path_literal_basic() {
        assert_eq!(
            tokenize(r#"p"/usr/local/bin""#),
            vec![
                TokenKind::PathLit(vec![StringPart::Literal("/usr/local/bin".into())]),
                TokenKind::Eof
            ]
        );
    }

    #[test]
    fn test_path_literal_empty() {
        assert_eq!(
            tokenize(r#"p"""#),
            vec![
                TokenKind::PathLit(vec![]),
                TokenKind::Eof
            ]
        );
    }

    #[test]
    fn test_path_literal_normalize_double_slash() {
        assert_eq!(
            tokenize(r#"p"/a//b""#),
            vec![
                TokenKind::PathLit(vec![StringPart::Literal("/a/b".into())]),
                TokenKind::Eof
            ]
        );
    }

    #[test]
    fn test_path_literal_interpolation() {
        assert_eq!(
            tokenize(r#"p"/opt/${app}/bin""#),
            vec![
                TokenKind::PathLit(vec![
                    StringPart::Literal("/opt/".into()),
                    StringPart::Expr("app".into()),
                    StringPart::Literal("/bin".into()),
                ]),
                TokenKind::Eof
            ]
        );
    }

    #[test]
    fn test_glob_literal_basic() {
        assert_eq!(
            tokenize(r#"g"/tmp/*.log""#),
            vec![
                TokenKind::GlobLit(vec![StringPart::Literal("/tmp/*.log".into())]),
                TokenKind::Eof
            ]
        );
    }

    #[test]
    fn test_glob_literal_alternation() {
        // {main,test} is literal glob alternation, not interpolation
        assert_eq!(
            tokenize(r#"g"/src/{main,test}/**/*.ts""#),
            vec![
                TokenKind::GlobLit(vec![StringPart::Literal("/src/{main,test}/**/*.ts".into())]),
                TokenKind::Eof
            ]
        );
    }

    #[test]
    fn test_glob_literal_interpolation() {
        assert_eq!(
            tokenize(r#"g"/etc/${app}/*.conf""#),
            vec![
                TokenKind::GlobLit(vec![
                    StringPart::Literal("/etc/".into()),
                    StringPart::Expr("app".into()),
                    StringPart::Literal("/*.conf".into()),
                ]),
                TokenKind::Eof
            ]
        );
    }

    #[test]
    fn test_regex_literal() {
        assert_eq!(
            tokenize(r#"re"^\d{3}-\d{4}$""#),
            vec![
                TokenKind::RegexLit(r"^\d{3}-\d{4}$".into()),
                TokenKind::Eof
            ]
        );
    }

    #[test]
    fn test_raw_string_basic() {
        assert_eq!(
            tokenize(r#"r"hello\nworld""#),
            vec![
                TokenKind::StringLit(r"hello\nworld".into()),
                TokenKind::Eof
            ]
        );
    }

    #[test]
    fn test_raw_string_with_hashes() {
        // r#"she said "hello""#  →  she said "hello"
        let src = r###"r#"she said "hello""#"###;
        assert_eq!(
            tokenize(src),
            vec![
                TokenKind::StringLit(r#"she said "hello""#.into()),
                TokenKind::Eof
            ]
        );
    }

    #[test]
    fn test_raw_string_double_hash() {
        // r##"contains "# inside"##  →  contains "# inside
        let src = r####"r##"contains "# inside"##"####;
        assert_eq!(
            tokenize(src),
            vec![
                TokenKind::StringLit("contains \"# inside".into()),
                TokenKind::Eof
            ]
        );
    }

    #[test]
    fn test_raw_string_no_escape_processing() {
        // Raw string should preserve backslashes literally
        assert_eq!(
            tokenize(r#"r"\t\n\\""#),
            vec![
                TokenKind::StringLit("\\t\\n\\\\".into()),
                TokenKind::Eof
            ]
        );
    }

    #[test]
    fn test_raw_string_with_dollar_brace() {
        // r"${not_interp}" → literal ${not_interp}
        assert_eq!(
            tokenize(r#"r"${not_interp}""#),
            vec![
                TokenKind::StringLit("${not_interp}".into()),
                TokenKind::Eof
            ]
        );
    }

    #[test]
    fn test_unterminated_raw_string() {
        let mut lexer = Lexer::new(r#"r"hello"#);
        assert!(lexer.tokenize().is_err());
    }

    #[test]
    fn test_raw_string_multiline() {
        // Real newlines inside a raw string should be preserved
        let src = "r\"line one\nline two\"";
        assert_eq!(
            tokenize(src),
            vec![
                TokenKind::StringLit("line one\nline two".into()),
                TokenKind::Eof
            ]
        );
    }

    #[test]
    fn test_raw_string_multiline_with_hashes() {
        // Multiline raw string with hash delimiters
        let src = "r#\"first\nsecond\nthird\"#";
        assert_eq!(
            tokenize(src),
            vec![
                TokenKind::StringLit("first\nsecond\nthird".into()),
                TokenKind::Eof
            ]
        );
    }

    #[test]
    fn test_semver_literal() {
        assert_eq!(
            tokenize(r#"v"1.2.3""#),
            vec![TokenKind::SemverLit("1.2.3".into()), TokenKind::Eof]
        );
    }

    #[test]
    fn test_command_literal() {
        let tokens = tokenize("`git status`");
        assert_eq!(
            tokens,
            vec![
                TokenKind::CmdLit(vec![StringPart::Literal("git status".into())]),
                TokenKind::Eof
            ]
        );
    }

    #[test]
    fn test_command_with_interpolation() {
        let tokens = tokenize("`docker build -t ${tag} .`");
        assert_eq!(
            tokens,
            vec![
                TokenKind::CmdLit(vec![
                    StringPart::Literal("docker build -t ".into()),
                    StringPart::Expr("tag".into()),
                    StringPart::Literal(" .".into()),
                ]),
                TokenKind::Eof
            ]
        );
    }

    #[test]
    fn test_command_with_raw_interpolation() {
        let tokens = tokenize("`cargo build !{flags}`");
        assert_eq!(
            tokens,
            vec![
                TokenKind::CmdLit(vec![
                    StringPart::Literal("cargo build ".into()),
                    StringPart::RawExpr("flags".into()),
                ]),
                TokenKind::Eof
            ]
        );
    }

    #[test]
    fn test_keywords() {
        assert_eq!(
            tokenize("let mut fn if else match for in while"),
            vec![
                TokenKind::Let,
                TokenKind::Mut,
                TokenKind::Fn,
                TokenKind::If,
                TokenKind::Else,
                TokenKind::Match,
                TokenKind::For,
                TokenKind::In,
                TokenKind::While,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn test_more_keywords() {
        assert_eq!(
            tokenize("task return break continue import true false null"),
            vec![
                TokenKind::Task,
                TokenKind::Return,
                TokenKind::Break,
                TokenKind::Continue,
                TokenKind::Import,
                TokenKind::True,
                TokenKind::False,
                TokenKind::Null,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn test_identifiers() {
        assert_eq!(
            tokenize("foo bar_baz _priv x1"),
            vec![
                TokenKind::Ident("foo".into()),
                TokenKind::Ident("bar_baz".into()),
                TokenKind::Ident("_priv".into()),
                TokenKind::Ident("x1".into()),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn test_operators() {
        assert_eq!(
            tokenize("+ - * / % **"),
            vec![
                TokenKind::Plus,
                TokenKind::Minus,
                TokenKind::Star,
                TokenKind::Slash,
                TokenKind::Percent,
                TokenKind::Power,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn test_comparison_operators() {
        assert_eq!(
            tokenize("== != < > <= >="),
            vec![
                TokenKind::EqEq,
                TokenKind::BangEq,
                TokenKind::Lt,
                TokenKind::Gt,
                TokenKind::LtEq,
                TokenKind::GtEq,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn test_logical_operators() {
        assert_eq!(
            tokenize("&& || !"),
            vec![TokenKind::And, TokenKind::Or, TokenKind::Bang, TokenKind::Eof]
        );
    }

    #[test]
    fn test_pipe_operators() {
        assert_eq!(
            tokenize("|> | ??"),
            vec![
                TokenKind::PipeArrow,
                TokenKind::Pipe,
                TokenKind::NullCoalesce,
                TokenKind::Eof
            ]
        );
    }

    #[test]
    fn test_range_and_spread() {
        assert_eq!(
            tokenize(".. ..= ..."),
            vec![
                TokenKind::Range,
                TokenKind::RangeInc,
                TokenKind::Spread,
                TokenKind::Eof
            ]
        );
    }

    #[test]
    fn test_arrows() {
        assert_eq!(
            tokenize("=> ->"),
            vec![TokenKind::FatArrow, TokenKind::Arrow, TokenKind::Eof]
        );
    }

    #[test]
    fn test_assignment_operators() {
        assert_eq!(
            tokenize("= += -= *= /="),
            vec![
                TokenKind::Eq,
                TokenKind::PlusEq,
                TokenKind::MinusEq,
                TokenKind::StarEq,
                TokenKind::SlashEq,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn test_delimiters() {
        assert_eq!(
            tokenize("( ) { } [ ] , : ;"),
            vec![
                TokenKind::LParen,
                TokenKind::RParen,
                TokenKind::LBrace,
                TokenKind::RBrace,
                TokenKind::LBracket,
                TokenKind::RBracket,
                TokenKind::Comma,
                TokenKind::Colon,
                TokenKind::Semicolon,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn test_line_comment() {
        assert_eq!(
            tokenize("42 // this is a comment\n7"),
            vec![TokenKind::IntLit(42), TokenKind::IntLit(7), TokenKind::Eof]
        );
    }

    #[test]
    fn test_block_comment() {
        assert_eq!(
            tokenize("42 /* block comment */ 7"),
            vec![TokenKind::IntLit(42), TokenKind::IntLit(7), TokenKind::Eof]
        );
    }

    #[test]
    fn test_nested_block_comments() {
        assert_eq!(
            tokenize("42 /* outer /* inner */ still comment */ 7"),
            vec![TokenKind::IntLit(42), TokenKind::IntLit(7), TokenKind::Eof]
        );
    }

    #[test]
    fn test_newlines_preserved() {
        let tokens = tokenize_all("a\nb");
        assert_eq!(
            tokens,
            vec![
                TokenKind::Ident("a".into()),
                TokenKind::Newline,
                TokenKind::Ident("b".into()),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn test_let_binding() {
        assert_eq!(
            tokenize("let x = 42"),
            vec![
                TokenKind::Let,
                TokenKind::Ident("x".into()),
                TokenKind::Eq,
                TokenKind::IntLit(42),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn test_function_decl() {
        assert_eq!(
            tokenize("fn add(a: Int, b: Int) -> Int { a + b }"),
            vec![
                TokenKind::Fn,
                TokenKind::Ident("add".into()),
                TokenKind::LParen,
                TokenKind::Ident("a".into()),
                TokenKind::Colon,
                TokenKind::Ident("Int".into()),
                TokenKind::Comma,
                TokenKind::Ident("b".into()),
                TokenKind::Colon,
                TokenKind::Ident("Int".into()),
                TokenKind::RParen,
                TokenKind::Arrow,
                TokenKind::Ident("Int".into()),
                TokenKind::LBrace,
                TokenKind::Ident("a".into()),
                TokenKind::Plus,
                TokenKind::Ident("b".into()),
                TokenKind::RBrace,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn test_path_join_expression() {
        assert_eq!(
            tokenize(r#"path("./build") / "bin""#),
            vec![
                TokenKind::Ident("path".into()),
                TokenKind::LParen,
                TokenKind::StringLit("./build".into()),
                TokenKind::RParen,
                TokenKind::Slash,
                TokenKind::StringLit("bin".into()),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn test_pipe_chain() {
        assert_eq!(
            tokenize("x |> map(f) |> filter(g)"),
            vec![
                TokenKind::Ident("x".into()),
                TokenKind::PipeArrow,
                TokenKind::Ident("map".into()),
                TokenKind::LParen,
                TokenKind::Ident("f".into()),
                TokenKind::RParen,
                TokenKind::PipeArrow,
                TokenKind::Ident("filter".into()),
                TokenKind::LParen,
                TokenKind::Ident("g".into()),
                TokenKind::RParen,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn test_question_mark() {
        assert_eq!(
            tokenize("foo()?"),
            vec![
                TokenKind::Ident("foo".into()),
                TokenKind::LParen,
                TokenKind::RParen,
                TokenKind::Question,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn test_closure_syntax() {
        assert_eq!(
            tokenize("|x| x * x"),
            vec![
                TokenKind::Pipe,
                TokenKind::Ident("x".into()),
                TokenKind::Pipe,
                TokenKind::Ident("x".into()),
                TokenKind::Star,
                TokenKind::Ident("x".into()),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn test_multiline_string() {
        let src = r#""""
  hello
  world
""""#;
        let tokens = tokenize(src);
        assert_eq!(
            tokens,
            vec![
                TokenKind::StringLit("hello\nworld".into()),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn test_multiline_string_with_interpolation() {
        let src = "\"\"\"
    Hello, ${name}!
    Goodbye
\"\"\"";
        let tokens = tokenize(src);
        assert_eq!(
            tokens,
            vec![
                TokenKind::InterpolatedString(vec![
                    StringPart::Literal("Hello, ".into()),
                    StringPart::Expr("name".into()),
                    StringPart::Literal("!\nGoodbye".into()),
                ]),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn test_multiline_string_with_escapes() {
        // \t is processed as a tab; indent is stripped normally
        let src = "\"\"\"
    col1\\tcol2
    val1\\tval2
\"\"\"";
        let tokens = tokenize(src);
        assert_eq!(
            tokens,
            vec![
                TokenKind::StringLit("col1\tcol2\nval1\tval2".into()),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn test_multiline_string_escaped_newline() {
        // \n inside content creates a real newline — that line has zero indent,
        // so the min-indent is zero and no indent stripping occurs.
        let src = "\"\"\"
    line1\\nline2
\"\"\"";
        let tokens = tokenize(src);
        assert_eq!(
            tokens,
            vec![
                TokenKind::StringLit("    line1\nline2".into()),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn test_multiline_string_escaped_dollar() {
        // \$ should prevent interpolation
        let src = "\"\"\"
    cost: \\${amount}
\"\"\"";
        let tokens = tokenize(src);
        assert_eq!(
            tokens,
            vec![
                TokenKind::StringLit("cost: ${amount}".into()),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn test_error_unterminated_string() {
        let mut lexer = Lexer::new(r#""hello"#);
        assert!(lexer.tokenize().is_err());
    }

    #[test]
    fn test_error_unterminated_command() {
        let mut lexer = Lexer::new("`git status");
        assert!(lexer.tokenize().is_err());
    }

    #[test]
    fn test_error_unexpected_char() {
        let mut lexer = Lexer::new("§");
        assert!(lexer.tokenize().is_err());
    }
}
