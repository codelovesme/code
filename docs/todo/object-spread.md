# Object spread: `{ ...source, k: v }`

Object literals cannot copy another object's fields. The old grammar had
spread on both plain objects and particles — `Expression::Object { spread:
Option<Box<Expression>>, fields }` (`old/src/ast.rs:309–318`) — with full
implementations in the interpreter (`old/src/interpreter.rs:1122–1360`) and
a large dedicated codegen section (`old/src/codegen.rs:2349–2627`, including
`compile_spread_particle` which expanded spread against a type schema).
None of it survived the rewrite; `Expr::Object(Vec<(String, Expr)>)` has no
spread slot, and the old `obj + obj` merge semantics went with it (the new
`BinOp` docs make object+object a type error — that decision stands).

## Semantics

- `{ ...source, k: v, j: w }` builds an object containing every field of
  `source` except those named again in the literal, then the literal's
  fields. Later literal fields win over earlier ones; `...source` always
  comes first (one source, fixed position — the old grammar enforced this
  too, and it keeps evaluation order trivially left-to-right).
- `source` must be an object at runtime; anything else is a runtime error
  naming the offending value ("spread source must be an object").
- Particle construction spreads too: `Reply { ...base, text: "ok" }` —
  desugars to the same `Expr::Object` with `"_class"` prepended, so spread
  interacts with the existing `primary` desugar rather than getting its own
  syntax. Decision: allow it (old allowed it; it's the natural shape for
  building a reply from a received particle).
- Nested spread (`{ ...a, ...b }`) is out of scope — one source covers the
  cases in the old fixture corpus; revisit if asked.

## Fix direction

1. **Lexer** (`src/lexer.rs`): `Token::Spread` for `...` — three-dot check
   alongside the existing two-char checks (`--`, `+=`), before the
   single-char table where `.` currently maps to `Dot`. A lone `..` or
   trailing `..` stays an error as today.
2. **AST** (`src/ast.rs`): `Expr::Object` becomes
   `Object { spread: Option<Box<Expr>>, fields: Vec<(String, Expr)> }` —
   or, cheaper for the rest of the codebase, a new sibling variant
   `Expr::SpreadObject(Box<Expr>, Vec<(String, Expr)>)`. Prefer the sibling:
   it leaves every existing `Expr::Object` match arm untouched (there are
   several: interpreter, codegen, `eval_literal`, LSP) and makes the
   "exactly one spread, first position" rule structurally obvious.
3. **Parser** (`src/parser.rs`): in `primary`'s brace-literal path, after
   `{`, if the next token is `Spread`, consume it, require at least… no —
   allow bare `{ ...source }` (zero explicit fields), then parse the usual
   comma-separated `name : expr` fields. Same entry point serves particle
   construction since `Name { … }` routes through the same brace parsing.
4. **Interpreter** (`src/interpreter.rs`): eval source, require
   `Value::Object`, clone its fields into a fresh vec/map, then append the
   literal's fields (later wins — insertion order preserved, matching how
   the existing object literal builds).
5. **Codegen** (`src/codegen.rs`): the runtime already has object
   constructors; add `CodeValue *code_object_merge(const CodeValue *src,
   i32 n, const char **names, const CodeValue **vals)` to `runtime.c` —
   allocates src's count + n fields, copies src's entries skipping any name
   present in the override list, appends overrides. One call site in
   codegen, no LLVM loops. Vendor-sync after.
6. **Fixtures**: `spread_basic.code`, `spread_override_wins.code`,
   `spread_particle.code` (`Reply { ...received, text: "ok" }`),
   `spread_only.code` (`{ ...other }`), `fail_spread_non_object.code`,
   `fail_spread_double.code` (`{ ...a, ...b }` rejected at parse time).
   Dual-mode.

## Why it matters

It is the everyday verb for "take this particle/object, change a few
fields, pass it on" — the exact pattern handler chains will use once
[user-defined handlers](user-defined-handlers.md) land. Cheap enough to do
first; the handlers todo assumes it exists.
