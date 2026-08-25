# User-defined handlers: `Name => { … }`, `return`, `emit … to this/base`

The old language let programs define their own handlers; the rewrite dropped
the whole family. Today every handler must live in C (core) or a native
module — there is no way to write one in Code itself.

Old surface (all gone):

```
Greet => {
    emit Reply { text = "hi" } to this      # forward to another handler
    return Reply { text = "done" }          # handler result
}
emit Greet { who = "x" } to this get r      # invoke, capture result
emit Greet {} to base                       # invoke OUTSIDE the current handler
```

AST: `Statement::{HandlerDefinition, HandlerInvoke, HandlerInvokeAssign,
HandlerReturn}`, `HandlerTarget::{Core, This, Base, ModuleAlias}`
(`old/src/ast.rs:199–217`). Interpreter: `exec_handler_invoke`
(`old/src/interpreter.rs:958`), `in_handler_depth` / `handler_return_value`
state, root-level `Exception` reporting (`old/src/interpreter.rs:143`).
Compiled backend: **it existed too** — handler bodies compiled to inline
label blocks with a class-name dispatch jump
(`old/src/codegen.rs:823`, `4019`, `4176`). So this is a two-backend feature,
not an interpreter nicety.

## Semantics to port (from the old interpreter)

- **Definition.** `UpperName => { body }` registers `body` under the class
  name. Re-defining stacks: dispatch runs *every* registered body in
  registration order, each seeing the previous body's return as… nothing —
  each body runs independently and the last non-null return wins
  (`exec_handler_invoke` loops `handler_bodies`, `last_result` overwritten).
- **Scope.** A handler body runs in a fresh scope seeded with the particle's
  fields minus `_class`/`_created` — so `Greet { who = "x" }` binds `who`
  directly. Inside a handler, `let x = …` may *reassign* an outer binding
  (old rule: reassignment allowed iff `in_handler_depth > 0`); at top level
  it still fails.
- **`return expr`** ends the body early; the value must be a particle
  (object carrying `_class`) — otherwise an error. Outside any handler it is
  a parse-time error (cheap) or runtime error (old was runtime; pick parse
  time, it's stricter and free).
- **Targets.** `to this` → handlers defined in the current scope chain;
  `to base` → handlers visible *outside* the innermost enclosing handler
  (override-and-fallback); `to <alias>` → a linked module's handlers
  (already works today via `EmitTarget::Module`); `to core` unchanged.
- **No handler found** → result is null, not an error (old behavior; keeps
  `emit … get r` total).

## Fix direction

### Phase A — interpreter

1. **Lexer** (`src/lexer.rs`): `Token::Arrow` for `=>` (two-char check next
   to the existing `+=` one, ~line 155) and `Token::This`/`Token::Base`
   keywords. Note `=` alone stays comparison; `=>` must be checked before
   the single-char table.
2. **AST** (`src/ast.rs`): `Stmt::HandlerDef { class_name: String,
   body: Vec<Stmt> }`; extend `EmitTarget` with `This` and `Base` arms.
   `Stmt::Return(Expr)` — or fold `return` into the emit machinery? No:
   keep it a distinct stmt, the interpreter needs it to stop the body.
3. **Parser** (`src/parser.rs`): in `statement()`, an uppercase-first ident
   followed by `=>` parses a handler definition (block body reuses the
   existing block parser). In the `Stmt::Emit` arm (~line 169), accept
   `Token::This`/`Token::Base` as targets. `return` parses like `break`
   (keyword + end-of-statement check), rejecting it at parse time when
   `loop_depth == 0 && handler_depth == 0` — track `handler_depth` on the
   parser exactly like `loop_depth`.
4. **Interpreter** (`src/interpreter.rs`): `Environment` gains
   `handlers: HashMap<String, Vec<Vec<Stmt>>>` plus `define_handler` /
   `get_handlers_in_scope` / `get_handlers_outside_current` riding on the
   existing scope stack (mirror `declare`/`pop_scope`). Top-level `exec`
   gains `handler_depth: usize` and `handler_return: Option<Value>`; the
   `Stmt::Emit` arm consults the registry before falling through to
   `dispatch_core` / module dispatch, seeds a pushed scope with the
   particle's fields, walks the body stopping on `Flow::Return(val)` (new
   `Flow` arm) or `Break`-equivalent exhaustion, and validates the returned
   value carries `_class`.

### Phase B — codegen

The slot model (`Gen`, `alloc_slot`) is straight-line-friendly but handlers
need real control flow — `gen_and_or` already proves labels work here.
Per handler definition: emit its body as a labeled block range inside
`main`, with a dispatch block that compares the particle's `_class` string
against each known name and jumps in (the old `compile_handler_invoke`
shape, `old/src/codegen.rs:4244`). `return` stores the value in a
per-invocation result slot and branches past the body; the caller loads the
slot for `get`. Field-seeding is a loop over the particle's fields calling
the existing field-copy helpers. `verify_stmts` must walk handler bodies
and reject `return` outside one.

### Phase C — fixtures

`handler_basic.code` (define + `to this` + `get`),
`handler_fields_bind.code` (particle fields become locals),
`handler_shadow_base.code` (inner definition shadows, `to base` reaches
outer), `handler_reassign_outer.code` (allowed only inside),
`fail_handler_unknown_target.code`, `fail_handler_return_bare.code`,
`fail_handler_return_non_particle.code`. All dual-mode — the invariant is
that interpret and compile agree, and Phase B exists precisely because the
old tree proved both can.

## Deliberately NOT ported (yet)

The old built-in `Exception` class and its assert-integration
(`old/src/codegen.rs:1436`: failed asserts produced an `Exception`
particle instead of dying). The new backend's `assert` failure exits the
process in both modes; making exceptions catchable is a try/catch design
decision of its own, not part of handler mechanics. Parked here on purpose
so nobody mistakes it for an oversight.
