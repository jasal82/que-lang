/// Abstract Syntax Tree types for the Que language.
///
/// Covers all declarations, statements, expressions, and patterns
/// described in the spec.

use crate::token::{DurationUnit, Span};

/// A complete Que source module.
#[derive(Debug, Clone, PartialEq)]
pub struct Module {
    /// Top-level items paired with their source span.
    pub items: Vec<(Span, Item)>,
    /// `#!/...` interpreter line, without the trailing newline.
    pub shebang: Option<String>,
    /// `#!strict` appeared in the file prologue. A property of the source
    /// text, so the linter and the LSP can see it without running anything.
    pub strict: bool,
}

/// Top-level items in a module.
#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Stmt(Stmt),
    FnDecl(FnDecl),
    TaskDecl(TaskDecl),
    TypeDecl(TypeDecl),
    EnumDecl(EnumDecl),
    Import(ImportDecl),
    StructDecl(StructDecl),
    ImplDecl(ImplDecl),
    TraitDecl(TraitDecl),
    TraitImplDecl(TraitImplDecl),
    /// `pub let pattern = expr` — a public binding exported from modules.
    PubLet {
        pattern: Pattern,
        type_ann: Option<TypeExpr>,
        value: Expr,
    },
}

// ── Declarations ──

#[derive(Debug, Clone, PartialEq)]
pub struct StructDecl {
    pub name: String,
    pub fields: Vec<StructField>,
    pub is_pub: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StructField {
    pub name: String,
    pub type_ann: Option<TypeExpr>,
    pub default: Option<Expr>,
}

/// An `impl TypeName { fn ... }` block.
#[derive(Debug, Clone, PartialEq)]
pub struct ImplDecl {
    pub type_name: String,
    pub methods: Vec<FnDecl>,
}

/// A `trait TraitName { fn ... }` declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct TraitDecl {
    pub name: String,
    pub methods: Vec<TraitMethod>,
    pub is_pub: bool,
}

/// A method signature inside a trait, with an optional default body.
#[derive(Debug, Clone, PartialEq)]
pub struct TraitMethod {
    pub name: String,
    pub params: Vec<Param>,
    pub return_type: Option<TypeExpr>,
    pub default_body: Option<Block>,
}

/// An `impl TraitName for TypeName { fn ... }` block.
#[derive(Debug, Clone, PartialEq)]
pub struct TraitImplDecl {
    pub trait_name: String,
    pub type_name: String,
    pub methods: Vec<FnDecl>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FnDecl {
    pub name: String,
    pub params: Vec<Param>,
    pub return_type: Option<TypeExpr>,
    pub body: Block,
    pub is_pub: bool,
    /// Declared `fn m(mut self, ...)`: the method may reassign `self`, and the
    /// value it leaves behind is written back over the receiver.
    pub mutates_self: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: String,
    pub type_ann: Option<TypeExpr>,
    pub default: Option<Expr>,
    /// Declared `...name`: binds every remaining positional argument as a
    /// list rather than a single value. Only the last parameter may set it,
    /// and only on a task.
    pub rest: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TaskDecl {
    pub name: String,
    pub params: Vec<Param>,
    pub depends_on: Vec<Expr>,
    pub inputs: Vec<Expr>,
    pub outputs: Vec<Expr>,
    pub env_keys: Vec<String>,
    pub description: Option<String>,
    pub aliases: Vec<String>,
    pub body: Block,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypeDecl {
    pub name: String,
    pub type_expr: TypeExpr,
    pub is_pub: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnumDecl {
    pub name: String,
    pub variants: Vec<EnumVariant>,
    pub is_pub: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnumVariant {
    pub name: String,
    pub fields: Vec<(String, TypeExpr)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImportDecl {
    /// The dot-separated path segments (e.g. `std.fs` → `["std", "fs"]`).
    pub path: Vec<String>,
    /// Optional `as` alias (e.g. `import std.fs as io`).
    pub alias: Option<String>,
    /// Optional selective imports (e.g. `import std.fs { readText, writeText }`).
    pub items: Option<Vec<String>>,
    /// `true` when the path starts with `.` (local, package-root-relative).
    pub is_local: bool,
    /// `true` when prefixed with `pub` (re-export).
    pub is_pub: bool,
}

// ── Type expressions ──

#[derive(Debug, Clone, PartialEq)]
pub enum TypeExpr {
    Named(String),
    Generic(String, Vec<TypeExpr>),
    // PAR-19: Tuple, Function, and Struct variants were removed because
    // parse_type_expr never emits them. Add them back when (and only when)
    // the grammar actually supports tuple/function/struct type syntax.
}

impl std::fmt::Display for TypeExpr {
    /// Render a type back in source syntax (`Map<String, Int>`).
    ///
    /// Several places show annotations to the user — `que tasks`, `help()`,
    /// diagnostics — and they all need the source spelling, not `{:?}`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TypeExpr::Named(name) => f.write_str(name),
            TypeExpr::Generic(name, args) => {
                write!(f, "{}<", name)?;
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{}", arg)?;
                }
                f.write_str(">")
            }
        }
    }
}

// ── Interpolated string parts (post-parse) ──

/// Parts of a string, command, path, or glob literal after parse-time
/// interpolation parsing. Expressions are fully parsed AST nodes rather
/// than raw source text, so syntax errors are detected at parse time.
#[derive(Debug, Clone, PartialEq)]
pub enum AstStringPart {
    /// Literal text segment.
    Literal(String),
    /// A `${...}` expression, fully parsed at parse time.
    Expr(Box<Expr>),
    /// A raw (unescaped) `!{...}` expression (command literals only).
    RawExpr(Box<Expr>),
}

// ── Map entries ──

/// An entry in a map literal: either a key-value pair or a spread.
#[derive(Debug, Clone, PartialEq)]
pub enum MapEntry {
    Pair(Expr, Expr),
    Spread(Expr),
}

// ── Block ──

/// A block is a list of statements with an optional trailing expression.
/// Each statement is paired with its source span for error reporting.
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub stmts: Vec<(Span, Stmt)>,
    pub expr: Option<Box<Expr>>,
    /// Extra source positions only the formatter needs. Boxed because
    /// `Value::Function` carries a `Block` by value, so every byte here is a
    /// byte on every interpreter stack frame.
    pub source: Option<Box<BlockSource>>,
}

/// Where a block sat in the source text.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockSource {
    /// Span of the trailing expression's first token, when there is one.
    pub expr_span: Option<Span>,
    /// Byte offset just past the closing `}`.
    pub end: usize,
}

// ── Statements ──

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Let {
        pattern: Pattern,
        type_ann: Option<TypeExpr>,
        value: Expr,
    },
    Mut {
        name: String,
        type_ann: Option<TypeExpr>,
        value: Expr,
    },
    Expr(Expr),
    Return(Option<Expr>),
    Break(Option<Expr>),
    Continue,
    For {
        pattern: Pattern,
        iterable: Expr,
        body: Block,
    },
    While {
        condition: Expr,
        body: Block,
    },
    Loop {
        body: Block,
    },

    Defer(Expr),
    TryCatch {
        try_body: Block,
        catches: Vec<CatchClause>,
        finally_body: Option<Block>,
    },
    Assign {
        target: Expr,
        value: Expr,
    },
    CompoundAssign {
        target: Expr,
        op: BinOp,
        value: Expr,
    },
}


// ── Expressions ──

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    // Literals
    IntLit(i64),
    FloatLit(f64),
    StringLit(String),
    InterpolatedString(Vec<AstStringPart>),
    BoolLit(bool),
    NullLit,
    ListLit(Vec<Expr>),
    MapLit(Vec<MapEntry>),
    SetLit(Vec<Expr>),
    TupleLit(Vec<Expr>),
    CmdLit(Vec<AstStringPart>),
    DurationLit(f64, DurationUnit),
    RegexLit(String),
    SemverLit(String),
    /// Path literal: `p"..."` with optional `${...}` interpolation.
    PathLit(Vec<AstStringPart>),
    /// Glob literal: `g"..."` with optional `${...}` interpolation.
    GlobLit(Vec<AstStringPart>),

    // Variable reference
    Ident(String),

    // Binary & Unary
    BinaryOp {
        left: Box<Expr>,
        op: BinOp,
        right: Box<Expr>,
    },
    UnaryOp {
        op: UnaryOp,
        expr: Box<Expr>,
    },

    // Calls
    Call {
        callee: Box<Expr>,
        args: Vec<CallArg>,
    },
    MethodCall {
        object: Box<Expr>,
        method: String,
        args: Vec<CallArg>,
    },
    FieldAccess {
        object: Box<Expr>,
        field: String,
    },
    /// `expr?.field` / `expr?.method(args)` — optional chaining.
    ///
    /// Short-circuits to `null` when `object` is `null`, otherwise behaves
    /// exactly like `FieldAccess` (`args: None`) or `MethodCall` (`args: Some`).
    OptionalAccess {
        object: Box<Expr>,
        field: String,
        args: Option<Vec<CallArg>>,
    },
    Index {
        object: Box<Expr>,
        index: Box<Expr>,
    },

    // Function / closure
    Lambda {
        params: Vec<Param>,
        body: Box<Expr>,
    },

    // Control flow as expressions
    If {
        condition: Box<Expr>,
        then_branch: Block,
        else_branch: Option<Box<Expr>>,
    },
    IfLet {
        pattern: Pattern,
        value: Box<Expr>,
        then_branch: Block,
        else_branch: Option<Box<Expr>>,
    },
    Match {
        subject: Box<Expr>,
        arms: Vec<MatchArm>,
    },
    Block(Block),

    /// `with <expr> [as <name>] { ... }` — context manager using Contextual trait.
    WithContext {
        manager: Box<Expr>,
        name: String,
        body: Block,
    },

    // Pipe
    Pipe {
        left: Box<Expr>,
        right: Box<Expr>,
    },

    // Error propagation (expr?)
    Try(Box<Expr>),

    // Null coalescing (expr ?? expr)
    NullCoalesce {
        left: Box<Expr>,
        right: Box<Expr>,
    },

    // Range
    Range {
        start: Option<Box<Expr>>,
        end: Option<Box<Expr>>,
        inclusive: bool,
    },

    // Loop expression (returns value via break)
    Loop {
        body: Block,
    },

    // Spread
    Spread(Box<Expr>),

    /// `TypeName { field: expr, ... }` — construct a struct instance.
    StructLit {
        name: String,
        fields: Vec<(String, Expr)>,
    },

    /// `spawn expr` — launch a command or callable in the background.
    /// Returns a ProcessHandle value.
    Spawn(Box<Expr>),

    /// `parallel { expr1, expr2, ... }` — evaluate branches concurrently.
    /// Returns a Tuple of results for unnamed branches,
    /// or a Map for named branches (`parallel { name: expr, ... }`).
    Parallel(Vec<ParallelBranch>),
}

/// A single branch in a `parallel { ... }` block.
#[derive(Debug, Clone, PartialEq)]
pub struct ParallelBranch {
    /// Optional label: `name: expr` inside parallel block.
    pub label: Option<String>,
    pub body: Expr,
}

/// Named or positional call argument.
#[derive(Debug, Clone, PartialEq)]
pub struct CallArg {
    pub name: Option<String>,
    pub value: Expr,
}

// ── Operators ──

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    Eq,
    NotEq,
    Lt,
    Gt,
    LtEq,
    GtEq,
    And,
    Or,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
    BitNot,
}

// ── Pattern Matching ──

#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub guard: Option<Expr>,
    pub body: Expr,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CatchClause {
    pub error_type: Option<String>,
    pub binding: Option<String>,
    pub body: Block,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    Wildcard,
    Ident(String),
    IntLit(i64),
    FloatLit(f64),
    StringLit(String),
    BoolLit(bool),
    NullLit,
    Glob(String),                                   // glob("*.rs")
    List(Vec<Pattern>, Option<Box<Pattern>>),     // [a, b, ...rest]
    Tuple(Vec<Pattern>),
    Struct(Vec<(String, Option<Pattern>)>, Option<String>), // { name, age: p, ...rest }
    /// `TypeName { field, other: pat, ... }` or `EnumName.Variant { ... }`.
    Instance(Option<String>, String, Vec<(String, Option<Pattern>)>, Option<String>),
    /// `Ok(value)` or `EnumName.variant(value)`.
    Enum(Option<String>, String, Vec<Pattern>),
    Range(Option<Box<Pattern>>, Option<Box<Pattern>>, bool), // 0..=255
    Or(Vec<Pattern>),
    Binding(String, Box<Pattern>),                  // x @ pattern
}
