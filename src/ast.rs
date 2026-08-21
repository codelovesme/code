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
}
