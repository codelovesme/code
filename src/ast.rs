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
}
