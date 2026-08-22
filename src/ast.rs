/// A parsed program: a flat sequence of statements — `Stmt::If`'s `body` is
/// where nesting actually happens now (see its doc comment).
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub statements: Vec<Stmt>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    /// `let name = expr` — the *only* way to introduce a name (decided
    /// 2026-08-21, reversing the original "no declaration keyword"
    /// design — see memory `new-code-let-keyword`). Always creates a
    /// *new* binding in the current (innermost) scope, shadowing any
    /// same-named outer binding for the rest of that scope — even
    /// re-`let`-ing the same name in the same scope is fine, it just
    /// rebinds. This is what makes `Assign` below unambiguous.
    Let {
        name: String,
        value: Expr,
        /// `export let name = ...` — this name is part of what a linking
        /// file sees. **Private is the default**, the opposite of the old
        /// language's public-by-default-with-`private`. Only meaningful at
        /// a module's top level, which is the only place the parser allows
        /// `export` at all: a name declared inside a block is block-local,
        /// so exporting it could never mean anything.
        exported: bool,
    },
    /// `name = expr` (no `let`) — reassignment only. Searches the scope
    /// chain outward for an existing binding of `name` and updates it in
    /// place; an error if `name` isn't bound anywhere (interpreter and
    /// compiler both — see memory `new-code-let-keyword`).
    Assign { name: String, value: Expr },
    /// `assert expr` — `expr` must evaluate to a `Bool`; `false` or any
    /// other kind aborts the program (interpreter: `Err`; compiled binary:
    /// `code_runtime_error` + exit 1). Silent on success — no output, no
    /// binding. `assert` is a reserved word, not a callable/expression.
    Assert(Expr),
    /// `if condition { body }` — no `else`, ever (deliberate language
    /// decision, not a missing feature). `condition` must be `Bool`.
    /// `body` runs in its own scope (see memory `new-code-if-scoping` and
    /// `new-code-let-keyword`): a `let` inside always creates a binding
    /// local to `body`, even shadowing a same-named outer one; a bare
    /// (`let`-less) assignment always mutates an existing outer binding
    /// (visible after the `if`, whether or not the branch actually ran —
    /// an untaken branch simply leaves it unchanged), and is an error if
    /// no such outer binding exists.
    If { condition: Expr, body: Vec<Stmt> },
    /// A bare `{ body }` — unconditionally runs `body` in a new scope
    /// (same scoping rule as `If`'s `body`, minus the condition: always
    /// executes). Lets a user open a scope on demand, e.g. to shadow a
    /// throwaway local without it ever being reachable outside.
    Block(Vec<Stmt>),
    /// `loop var over iterable { body }` — the language's only iteration
    /// construct (decided 2026-08-21). `iterable` is evaluated *once*,
    /// before the first iteration, and must be an `Array` — any other kind
    /// is a runtime type error, deliberately unlike `Field`/`Index`'s
    /// permissive null. `body` then runs once per element, in order, in its
    /// own scope with `var` bound to that element (shadowing any outer
    /// same-named binding, exactly like a `let`).
    ///
    /// There is no `while`, no bare `loop { }`, and no collect/`yield` form:
    /// every loop is bounded by an array's length, so no program can spin
    /// forever, and accumulating a result is just `acc = acc + [x]` with the
    /// array `+` that already exists. Deliberately narrower than the old
    /// language's five loop forms — the ones left out either depended on the
    /// constraint system (domain enumeration) or duplicated `+`.
    Loop {
        var: String,
        /// `loop item, i over ...` — the zero-based position, bound as a
        /// `Number`. Scoped and shadowing exactly like `var`.
        index: Option<String>,
        iterable: Expr,
        body: Vec<Stmt>,
    },
    /// `link "path"` / `link "path" as alias` — brings another module's
    /// exports into this file. Top-level only, like `export`.
    ///
    /// **Never reaches the interpreter or codegen**: `loader.rs` resolves
    /// every `Link` into an `Import` before either backend sees the program,
    /// which is what lets one implementation of module resolution — path
    /// lookup, recursion, cycle detection — serve both output modes.
    ///
    /// The path is a quoted string rather than a bare `a/b` path, matching
    /// how object keys must also be quoted. It also keeps working when the
    /// target is a native module, whose names carry extensions (`.so`,
    /// `.wasm`) that a bare-path lexer rule would have to special-case.
    Link { path: String, alias: Option<String> },
    /// A resolved `Link`, produced only by `loader.rs`.
    ///
    /// Runs `body` in its own scope, then binds the names in `exports`:
    /// with an alias, gathered into an object (so `alias.name` is ordinary
    /// field access, needing no new lookup rule); without one, defined in
    /// the enclosing scope, where a collision is an error rather than
    /// silent shadowing across a module boundary.
    ///
    /// `exports` lists only this module's own `export let`s. A module that
    /// links another does not re-export it — linking is not exporting.
    ///
    /// Deliberately shaped as "produce name/value pairs, then bind them":
    /// running `body` is one way to produce them, and reading a native
    /// module's descriptor would be another, reusing the binding half
    /// unchanged.
    Import {
        alias: Option<String>,
        body: Vec<Stmt>,
        exports: Vec<String>,
    },
    /// A resolved native-module `Link` (`link "x.so" as x`), produced only
    /// by `loader.rs`. Unlike `Import`, there is no `body` to run — a
    /// native module's values come from its `code_module_vars` export, read
    /// at runtime by whichever backend is running, not from statements to
    /// execute.
    ///
    /// `alias` is mandatory (unlike `Link`'s optional one): with no name,
    /// nothing could ever refer to the module again. It is bound to an
    /// *object* of the module's exported variables (so `alias.name` is
    /// ordinary field access, exactly like `Import`'s alias binding), and
    /// it is also the target `emit ... to x` dispatches to. A module with
    /// no `code_module_vars` export binds `alias` to an empty object. See
    /// `docs/todo/native-module-linking.md`.
    ImportNative {
        alias: String,
        path: String,
        format: NativeFormat,
    },
    /// `emit <particle> to <target> [get <name>]` — invokes a handler.
    /// Which one is chosen at **runtime**, by reading the particle's own
    /// `"_class"` field — never resolved at parse or compile time, even
    /// when `particle` is a literal `ClassName { ... }` written right here,
    /// so a particle built earlier, stored, and passed around dispatches
    /// exactly the same way a literal one does (see memory
    /// `new-code-particle`: that's the reason particles carry `_class` with
    /// them in the first place). Consistent with every other operand-type
    /// rule in this language: always a runtime check, never special-cased
    /// for the literal case (see ast.rs's `BinOp` doc comment).
    ///
    /// `get <name>` always **declares** a new binding, shadowing like
    /// `let` — never a reassignment, the same rule `Stmt::Import`'s alias
    /// follows. Omitting it runs the handler and discards the result.
    Emit {
        particle: Expr,
        target: EmitTarget,
        result: Option<String>,
    },
    /// `break` — exits the innermost enclosing `Loop` immediately, skipping
    /// the rest of that iteration's body. `break` outside any loop is a
    /// *parse* error rather than a later check, so the interpreter and the
    /// compiler reject it identically without either needing its own rule.
    Break,
}

/// Which format a resolved `Stmt::ImportNative` came from — produced only by
/// `loader.rs`, consumed differently by each backend (interpreter.rs refuses
/// `Static` outright; codegen.rs is the only one that can act on it). See
/// `docs/todo/native-module-linking.md`.
#[derive(Debug, Clone, PartialEq)]
pub enum NativeFormat {
    /// `.so` — `dlopen`/`dlsym` at runtime, in both `code run` and `code
    /// build`. The module is self-contained (its own copy of the runtime),
    /// so a call across the boundary needs a deep copy both ways — see
    /// `code_abi.h`.
    Dynamic,
    /// `.a` — linked straight into the host binary by `cc`, `code build`
    /// only (there is no `dlopen` for a static archive). The module shares
    /// the host's own runtime, so no copy is needed either way; `prefix` is
    /// what `loader.rs` found by running `nm` on the archive, the module
    /// author's chosen unique name for `<prefix>_code_module_dispatch` /
    /// `_code_module_abi_version` / (optionally) `_code_module_vars`. See
    /// `code_abi.h`'s "`.a` static modules" section.
    Static { prefix: String, has_vars: bool },
}

/// `emit`'s `to` clause. `Core` is the compiled-in handler set (`core` is
/// its own reserved word — see `Stmt::Emit`'s doc comment). `Module` names a
/// `link`ed native module by its (mandatory) alias — resolved against
/// whatever `link "....so" as <alias>` bound, at runtime, not parse time
/// (an undefined alias is a runtime error, matching every other name lookup
/// in this language).
#[derive(Debug, Clone, PartialEq)]
pub enum EmitTarget {
    Core,
    Module(String),
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
/// - `Eq` (`=`) / `Ne` (`≠`): well-defined for *any* two values, including
///   mismatched kinds (`1 = "1"` is simply `false`, never an error) — deep
///   structural equality.
/// - `Lt` (`<`) / `Gt` (`>`) / `Le` (`≤`) / `Ge` (`≥`): `Number` only.
///   Everything else — strings included, so no lexicographic ordering —
///   is a runtime type error.
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
