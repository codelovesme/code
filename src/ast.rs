/// A parsed program: a flat sequence of statements — `Stmt::If`'s `body` is
/// where nesting actually happens now (see its doc comment).
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub statements: Vec<Stmt>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    /// `name = expr` — both first binding and reassignment; there is no
    /// separate declaration form (see memory: bare assignment, no `mut`).
    Assign { name: String, value: Expr },
    /// `assert expr` — `expr` must evaluate to a `Bool`; `false` or any
    /// other kind aborts the program (interpreter: `Err`; compiled binary:
    /// `code_runtime_error` + exit 1). Silent on success — no output, no
    /// binding. `assert` is a reserved word, not a callable/expression.
    Assert(Expr),
    /// `if condition { body }` — no `else`, ever (deliberate language
    /// decision, not a missing feature). `condition` must be `Bool`.
    /// `body` runs in its own scope (see memory `new-code-if-scoping`):
    /// assigning a name already bound in an outer scope mutates that
    /// outer binding (visible after the `if`, whether or not the branch
    /// actually ran — an untaken branch simply leaves it unchanged);
    /// assigning a name that doesn't exist anywhere outer creates a new
    /// binding local to `body`, invisible once it ends.
    If { condition: Expr, body: Vec<Stmt> },
    /// A bare `{ body }` — unconditionally runs `body` in a new scope
    /// (same scoping rule as `If`'s `body`, minus the condition: always
    /// executes). Lets a user open a scope on demand, e.g. to shadow a
    /// throwaway local without it ever being reachable outside.
    Block(Vec<Stmt>),
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
