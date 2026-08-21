/// A parsed program: a flat sequence of statements. Nothing nests yet
/// (no blocks, no control flow) — this is the smallest slice that exercises
/// the whole pipeline (lexer -> parser -> AST -> interpreter).
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub statements: Vec<Stmt>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    /// `name = expr` — both first binding and reassignment; there is no
    /// separate declaration form (see memory: bare assignment, no `mut`).
    Assign { name: String, value: Expr },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Number(f64),
    Str(String),
    Bool(bool),
    Null,
    /// A reference to a previously-bound name.
    Ident(String),
    Array(Vec<Expr>),
    /// Insertion-ordered `key: value` pairs — keys are always string
    /// literals, values are any expression (which may itself reference a
    /// variable, unlike strict JSON).
    Object(Vec<(String, Expr)>),
    /// `expr.field` — reading a field, not writing one; there is no
    /// `expr.field = ...` assignment form (yet — see memory
    /// `new-code-memory-management` on why mutation is a separate, deferred
    /// decision). Invalid access (non-object, missing field) is not a
    /// parse-time concern — see `Field`'s evaluation for the runtime rule.
    Field(Box<Expr>, String),
    /// `expr[index]` — same read-only scope as `Field`. `index` is itself
    /// an expression, not restricted to a literal.
    Index(Box<Expr>, Box<Expr>),
    Binary(Box<Expr>, BinOp, Box<Expr>),
    Unary(UnOp, Box<Expr>),
}

/// Operand type rules (decided 2026-08-21, do not re-propose alternatives):
/// - `Add`/`Sub`/`Mul`/`Div`: `Number` only, except `Add` which also
///   concatenates `Str+Str` and `Array+Array` — any other type pairing
///   (including mixed kinds, e.g. `Number+Str`) is a runtime type error.
///   `Div` by zero is also a runtime error, not `Infinity` — the value
///   model is JSON, which has no way to represent that.
/// - `Eq`/`Ne`: well-defined for *any* two values, including mismatched
///   kinds (`1 == "1"` is simply `false`, never an error) — deep structural
///   equality.
/// - `Lt`/`Gt`/`Le`/`Ge`: `Number` or `Str` (lexicographic) only, both
///   operands must be the same of those two kinds — everything else
///   (`Bool`/`Null`/`Array`/`Object`, or mismatched kinds) is a runtime
///   type error; there is no natural order for them.
/// - `And`/`Or`: `Bool` only, short-circuiting.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    And,
    Or,
}

/// `Neg` (`-x`): `Number` only. `Not`: `Bool` only. Both error on any other
/// operand type.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UnOp {
    Neg,
    Not,
}
