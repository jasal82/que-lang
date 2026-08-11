/// Token types for the Que language.
///
/// Covers all keywords, operators, delimiters, and literal forms
/// specified in the Que v0.1 spec.

/// Source location for error reporting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub line: usize,
    pub col: usize,
}

impl Span {
    pub const fn new(start: usize, end: usize, line: usize, col: usize) -> Self {
        Self {
            start,
            end,
            line,
            col,
        }
    }
}

/// A token with its kind and source span.
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

impl Token {
    pub fn new(kind: TokenKind, span: Span) -> Self {
        Self { kind, span }
    }
}

/// Parts of an interpolated string or command literal.
#[derive(Debug, Clone, PartialEq)]
pub enum StringPart {
    /// Literal text segment.
    Literal(String),
    /// An expression inside `${...}`.
    Expr(String),
    /// A raw (unescaped) expression inside `!{...}` (commands only).
    RawExpr(String),
    /// A `\`-at-end-of-line continuation (command literals only).
    ///
    /// It means a single space, but it is kept as its own part rather than
    /// folded into the surrounding text so that `que fmt` can put the line
    /// break back where the author wrote it.
    Continuation,
}

/// Duration units for duration literals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DurationUnit {
    Milliseconds,
    Seconds,
    Minutes,
    Hours,
    Days,
}

impl std::fmt::Display for DurationUnit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DurationUnit::Milliseconds => write!(f, "ms"),
            DurationUnit::Seconds => write!(f, "s"),
            DurationUnit::Minutes => write!(f, "m"),
            DurationUnit::Hours => write!(f, "h"),
            DurationUnit::Days => write!(f, "d"),
        }
    }
}

/// All token kinds produced by the lexer.
#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // ── Literals ──
    IntLit(i64),
    FloatLit(f64),
    StringLit(String),
    InterpolatedString(Vec<StringPart>),
    CmdLit(Vec<StringPart>),
    DurationLit(f64, DurationUnit),
    RegexLit(String),
    SemverLit(String),
    /// Path literal: `p"..."` with optional `${...}` interpolation.
    PathLit(Vec<StringPart>),
    /// Glob literal: `g"..."` with optional `${...}` interpolation.
    GlobLit(Vec<StringPart>),

    // ── Identifier ──
    Ident(String),

    // ── Keywords ──
    Let,
    Mut,
    Fn,
    Task,
    Type,
    Enum,
    Struct,
    If,
    Else,
    Match,
    For,
    In,
    While,
    Loop,
    Return,
    Break,
    Continue,
    Import,
    As,
    From,
    Pub,
    True,
    False,
    Null,
    Try,
    Catch,
    Finally,
    Defer,

    Spawn,
    Parallel,
    Where,
    With,
    Impl,
    Trait,

    // ── Operators ──
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Power,        // **
    Eq,           // =
    PlusEq,       // +=
    MinusEq,      // -=
    StarEq,       // *=
    SlashEq,      // /=
    EqEq,         // ==
    BangEq,       // !=
    Lt,           // <
    Gt,           // >
    LtEq,         // <=
    GtEq,         // >=
    And,          // &&
    Or,           // ||
    Bang,         // !
    BitAnd,       // & (single)
    Pipe,         // | (single — bitwise or, process pipe, closure delim)
    BitXor,       // ^
    Tilde,        // ~
    Shl,          // <<
    Shr,          // >>
    PipeArrow,    // |>
    NullCoalesce, // ??
    QuestionDot,  // ?.
    Question,     // ?
    Range,        // ..
    RangeInc,     // ..=
    Spread,       // ...
    FatArrow,     // =>
    Arrow,        // ->
    Dot,          // .
    At,           // @

    // ── Delimiters ──
    LParen,
    RParen,
    LBrace,
    RBrace,
    /// `#{` — opens a set literal. Closed by `RBrace`.
    HashBrace,
    LBracket,
    RBracket,
    Comma,
    Colon,
    Semicolon,

    // ── Special ──
    /// `#!/...` on the very first line — the kernel's interpreter line.
    /// Carried through so the formatter can put it back.
    Shebang(String),
    /// `#!name` on a line of its own, before anything else in the file.
    /// Carries the bare name (`strict`), not the `#!`.
    Pragma(String),
    Newline,
    Eof,
}

impl TokenKind {
    /// Short human-readable name for the token category, used in parser
    /// error messages. Avoids dumping the variant's inner payload (e.g.
    /// `Ident("")` becomes just `identifier`).
    pub fn display_name(&self) -> &'static str {
        match self {
            TokenKind::IntLit(_) => "integer literal",
            TokenKind::FloatLit(_) => "float literal",
            TokenKind::StringLit(_) => "string literal",
            TokenKind::InterpolatedString(_) => "interpolated string",
            TokenKind::CmdLit(_) => "command literal",
            TokenKind::DurationLit(..) => "duration literal",
            TokenKind::RegexLit(_) => "regex literal",
            TokenKind::SemverLit(_) => "semver literal",
            TokenKind::PathLit(_) => "path literal",
            TokenKind::GlobLit(_) => "glob literal",
            TokenKind::Ident(_) => "identifier",
            TokenKind::Let => "`let`",
            TokenKind::Mut => "`mut`",
            TokenKind::Fn => "`fn`",
            TokenKind::Task => "`task`",
            TokenKind::Type => "`type`",
            TokenKind::Enum => "`enum`",
            TokenKind::Struct => "`struct`",
            TokenKind::If => "`if`",
            TokenKind::Else => "`else`",
            TokenKind::Match => "`match`",
            TokenKind::For => "`for`",
            TokenKind::In => "`in`",
            TokenKind::While => "`while`",
            TokenKind::Loop => "`loop`",
            TokenKind::Return => "`return`",
            TokenKind::Break => "`break`",
            TokenKind::Continue => "`continue`",
            TokenKind::Import => "`import`",
            TokenKind::As => "`as`",
            TokenKind::From => "`from`",
            TokenKind::Pub => "`pub`",
            TokenKind::True => "`true`",
            TokenKind::False => "`false`",
            TokenKind::Null => "`null`",
            TokenKind::Try => "`try`",
            TokenKind::Catch => "`catch`",
            TokenKind::Finally => "`finally`",
            TokenKind::Defer => "`defer`",
            TokenKind::Spawn => "`spawn`",
            TokenKind::Parallel => "`parallel`",
            TokenKind::Where => "`where`",
            TokenKind::With => "`with`",
            TokenKind::Impl => "`impl`",
            TokenKind::Trait => "`trait`",
            TokenKind::Plus => "`+`",
            TokenKind::Minus => "`-`",
            TokenKind::Star => "`*`",
            TokenKind::Slash => "`/`",
            TokenKind::Percent => "`%`",
            TokenKind::Power => "`**`",
            TokenKind::Eq => "`=`",
            TokenKind::PlusEq => "`+=`",
            TokenKind::MinusEq => "`-=`",
            TokenKind::StarEq => "`*=`",
            TokenKind::SlashEq => "`/=`",
            TokenKind::EqEq => "`==`",
            TokenKind::BangEq => "`!=`",
            TokenKind::Lt => "`<`",
            TokenKind::Gt => "`>`",
            TokenKind::LtEq => "`<=`",
            TokenKind::GtEq => "`>=`",
            TokenKind::And => "`&&`",
            TokenKind::Or => "`||`",
            TokenKind::Bang => "`!`",
            TokenKind::BitAnd => "`&`",
            TokenKind::Pipe => "`|`",
            TokenKind::BitXor => "`^`",
            TokenKind::Tilde => "`~`",
            TokenKind::Shl => "`<<`",
            TokenKind::Shr => "`>>`",
            TokenKind::PipeArrow => "`|>`",
            TokenKind::NullCoalesce => "`??`",
            TokenKind::QuestionDot => "`?.`",
            TokenKind::Question => "`?`",
            TokenKind::Range => "`..`",
            TokenKind::RangeInc => "`..=`",
            TokenKind::Spread => "`...`",
            TokenKind::FatArrow => "`=>`",
            TokenKind::Arrow => "`->`",
            TokenKind::Dot => "`.`",
            TokenKind::At => "`@`",
            TokenKind::LParen => "`(`",
            TokenKind::RParen => "`)`",
            TokenKind::LBrace => "`{`",
            TokenKind::RBrace => "`}`",
            TokenKind::HashBrace => "`#{`",
            TokenKind::LBracket => "`[`",
            TokenKind::RBracket => "`]`",
            TokenKind::Comma => "`,`",
            TokenKind::Colon => "`:`",
            TokenKind::Semicolon => "`;`",
            TokenKind::Shebang(_) => "interpreter line",
            TokenKind::Pragma(_) => "file pragma",
            TokenKind::Newline => "newline",
            TokenKind::Eof => "end of input",
        }
    }
    /// Returns the keyword kind for a given identifier string, if any.
    pub fn keyword(s: &str) -> Option<TokenKind> {
        match s {
            "let" => Some(TokenKind::Let),
            "mut" => Some(TokenKind::Mut),
            "fn" => Some(TokenKind::Fn),
            "task" => Some(TokenKind::Task),
            "type" => Some(TokenKind::Type),
            "enum" => Some(TokenKind::Enum),
            "struct" => Some(TokenKind::Struct),
            "if" => Some(TokenKind::If),
            "else" => Some(TokenKind::Else),
            "match" => Some(TokenKind::Match),
            "for" => Some(TokenKind::For),
            "in" => Some(TokenKind::In),
            "while" => Some(TokenKind::While),
            "loop" => Some(TokenKind::Loop),
            "return" => Some(TokenKind::Return),
            "break" => Some(TokenKind::Break),
            "continue" => Some(TokenKind::Continue),
            "import" => Some(TokenKind::Import),
            "as" => Some(TokenKind::As),
            "from" => Some(TokenKind::From),
            "pub" => Some(TokenKind::Pub),
            "true" => Some(TokenKind::True),
            "false" => Some(TokenKind::False),
            "null" => Some(TokenKind::Null),
            "try" => Some(TokenKind::Try),
            "catch" => Some(TokenKind::Catch),
            "finally" => Some(TokenKind::Finally),
            "defer" => Some(TokenKind::Defer),

            "spawn" => Some(TokenKind::Spawn),
            "parallel" => Some(TokenKind::Parallel),
            "where" => Some(TokenKind::Where),
            "with" => Some(TokenKind::With),
            "impl" => Some(TokenKind::Impl),
            "trait" => Some(TokenKind::Trait),
            _ => None,
        }
    }
}
