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
    /// `loop [var[, index] over iterable] [get name [= init]] { body }` —
    /// the language's only iteration construct.
    ///
    /// Both clauses are independent and optional, which is what makes one
    /// statement cover every form:
    ///
    /// ```text
    /// loop x over xs { }                 -- iterate
    /// loop x, i over xs { }              -- iterate with position
    /// loop { }                           -- until `break`
    /// loop x over xs get sum = 0 { }     -- accumulate
    /// loop x over xs get out = [] { }    -- ... which is also how you collect
    /// ```
    ///
    /// `while` still does not exist; `loop { }` is how an unbounded loop is
    /// written. Note what that gave up: until 2026-08-23 every loop was
    /// bounded by an array's length, so no program could spin forever. That
    /// guarantee was traded away deliberately (owner's call) for the bare
    /// form — a `loop { }` whose `break` is never reached now hangs, exactly
    /// like the old language's.
    ///
    /// Domain enumeration (`loop x { }` over a variable's possibility space)
    /// is *not* coming back: it enumerated a constraint domain, and this
    /// language has none.
    Loop {
        /// `None` is the bare `loop { }`. Grouped into one struct rather
        /// than three fields that would have to agree: there is no such
        /// thing as an iterable without a variable, or vice versa.
        over: Option<LoopOver>,
        /// `get name [= init]` — see `LoopAccumulator`.
        result: Option<LoopAccumulator>,
        body: Vec<Stmt>,
    },
    /// `continue` — skips the rest of this iteration and starts the next
    /// one. In a `loop ... over`, that means the next element; in a bare
    /// `loop { }`, it re-enters the body immediately. Rejected outside a
    /// loop at *parse* time, exactly like `Break`.
    Continue,
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

/// The `var[, index] over iterable` half of a `Stmt::Loop`.
#[derive(Debug, Clone, PartialEq)]
pub struct LoopOver {
    pub var: String,
    /// `loop item, i over ...` — the zero-based position, bound as a
    /// `Number`. Scoped and shadowing exactly like `var`.
    pub index: Option<String>,
    /// Evaluated *once*, before the first iteration, and must be an
    /// `Array` — any other kind is a runtime type error, the same rule
    /// `Field`/`Index` follow.
    pub iterable: Expr,
}

/// The `get name [= init]` half of a `Stmt::Loop`.
///
/// Deliberately not its own runtime concept: `name` is declared as an
/// ordinary binding in the scope *enclosing* the loop, initialized to
/// `init` before the first iteration. The body then updates it with the
/// same `Stmt::Assign` any other reassignment uses — which already resolves
/// outward through the scope chain — and it is simply still bound once the
/// loop ends. No accumulator stack, no new scoping rule, and nothing for
/// either backend to special-case beyond creating the binding.
///
/// ```text
/// loop x over xs get sum = 0 { sum = sum + x }      -- fold
/// loop x over xs get out = [] { out = out + [x] }   -- collect
/// ```
///
/// A `yield` statement was built and then removed the same day
/// (2026-08-23): `get out { yield e }` collecting into an array is exactly
/// `get out = [] { out = out + [e] }`, so it bought one more keyword, a
/// desugaring step, and a mutual-exclusion rule ("a body either yields or
/// assigns, never both") in exchange for one line of source. Not worth it.
/// Note there is no `+=` either — no compound assignment operator exists.
#[derive(Debug, Clone, PartialEq)]
pub struct LoopAccumulator {
    pub name: String,
    /// What `name` holds before the first iteration: the `= init`
    /// expression, or `null` when `= init` was omitted.
    pub init: Expr,
}

/// Which format a resolved `Stmt::ImportNative` came from — produced by
/// `loader.rs` (`Dynamic`/`Static`) or, for `crates/code-wasm`, its own
/// resolver (`JsBridge`) — consumed differently by each backend
/// (interpreter.rs refuses `Static` outright; codegen.rs is the only one
/// that can act on it; `JsBridge` never reaches codegen.rs at all). See
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
    /// A `crates/code-wasm`-only format: the alias dispatches to a plain
    /// synchronous JS callback (JSON string in, JSON string out) that the
    /// embedding JS host must have already registered — via
    /// `Environment::link_module` — before the program ever starts running.
    /// There is no file to open and nothing to resolve at the point this
    /// statement executes; the interpreter only checks the alias is present.
    /// Carries no payload — the alias on `Stmt::ImportNative` itself is the
    /// only thing needed to look it up. Never produced by `loader.rs`'s own
    /// `FilesystemResolver`/`NoModules`, and never reaches codegen.rs (there
    /// is no `code build` in a browser).
    JsBridge,
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
    /// decision).
    ///
    /// `.` **requires an object** and `[]` **requires an array**: anything
    /// else is a runtime error (revised 2026-08-23 — until then any invalid
    /// access quietly produced null, which hid mistakes like `"abc"[0]` and
    /// `"abc".length`).
    ///
    /// A member that is merely *absent* is still null, though: `obj.nope`
    /// and `arr[99]` (and a non-integer or negative index) evaluate to null
    /// rather than erroring. That half is load-bearing — reading a name a
    /// module did not export goes through an alias object as a missing
    /// field, and `link_default_private.code` asserts it is null.
    Field(Box<Expr>, String),
    /// `expr[index]` — same read-only scope, and the same
    /// wrong-kind-errors / absent-member-is-null split as `Field`. `index`
    /// is itself an expression, not restricted to a literal.
    Index(Box<Expr>, Box<Expr>),
    Binary(Box<Expr>, BinOp, Box<Expr>),
    Unary(UnOp, Box<Expr>),
}

/// Operand type rules (decided 2026-08-21, do not re-propose alternatives):
/// - `Sub`/`Mul`/`Div`: `Number` only. `Div` by zero is a runtime error,
///   not `Infinity` — the value model is JSON, which has no way to
///   represent that.
/// - `Add` is the one overloaded operator:
///   - `Number + Number` adds, `Str + Str` concatenates.
///   - `Array + Array` concatenates.
///   - With exactly *one* array operand, the other is a single element:
///     `[1,2] + 3` is `[1,2,3]` and `0 + [1,2]` is `[0,1,2]`. Any value
///     kind can be the element, so `[1] + [[2]]` appends an array as one
///     item (`[1,[2]]`) while `[1] + [2]` still concatenates (`[1,2]`) —
///     the two-array case is checked first, which is what keeps that
///     distinction available at all.
///   - Everything else, including mixed non-array kinds like `Number+Str`,
///     is a runtime type error.
///
/// `name += expr` is sugar for `name = name + expr`, rewritten by the
/// parser (see `Stmt::Assign`), so it inherits every rule above and needs
/// no support of its own anywhere downstream. It is the only compound
/// assignment operator — there is no `-=`, `*=`, or `/=`.
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
