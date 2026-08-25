# String interpolation: `"hello $name"`

The old parser spliced identifiers out of double-quoted strings
(`old/src/parser.rs:36–76`): `"ping $user at $n"` became
`InterpolatedString([Literal("ping "), Variable(user), Literal(" at "),
Variable(n)])`. The new lexer has no `$` handling at all — a `$` inside a
string is just a character, so `"hi $name"` silently produces the literal
text `hi $name`. That silent-wrong outcome is the worst kind: programs that
worked before the rewrite now print garbage instead of failing.

## Semantics

- `$ident` inside a string interpolates the variable `ident`. Rendering
  uses the same JSON rendering `Value`'s `Display` already provides
  (`src/value.rs:124`) — numbers, booleans, null, arrays, objects all have
  a defined spelling, so interpolation is total: no value is uninterpolable.
  (Old rendered null as `Null`; JSON says `null`. Take JSON — it's what
  everything else in the new runtime prints.)
- `$` not followed by an identifier-start character is a lex error
  (`"$" in a string must start an interpolation ($name)`). No escaping `$`
  needed if that holds — decide and state it in the language doc when one
  exists.
- A string with zero interpolations stays a plain `Expr::Str` — no new node
  for the common case, and the lexer's fast path is untouched.

## Fix direction

1. **Lexer** (`src/lexer.rs`, the `c == '"'` branch at ~line 190): while
   scanning a string, on `$` peek an identifier run; if none follows, error
   at that position. Emit `Token::Str(Vec<StringPart>)` where
   `enum StringPart { Lit(String), Var(Rc<str>) }` lives in `ast.rs`.
   Escape handling (`\n \t \" \\`) is unchanged and applies only inside
   literal segments.
2. **AST** (`src/ast.rs`): `Expr::Interpolated(Vec<Expr>)` — parts as
   expressions (`Expr::Str(lit)` / `Expr::Ident(name)`), mirroring the old
   `StringPart` but reusing `Expr` so there is one thing to evaluate.
3. **Parser**: nothing to do — the token arrives pre-split. (This is why
   splitting belongs in the lexer, not the parser: the quote-scanning loop
   already owns string internals.)
4. **Interpreter** (`src/interpreter.rs`): eval each part, join with
   `format!("{}", v)` via the existing `Display`.
5. **Codegen** (`src/codegen.rs`): declare
   `CodeValue *code_str_join(i32 n, ...)` in `runtime.c` — varargs of
   `const char *`, sums lengths, one allocation, copies, wraps with
   `code_str`. Each part compiles to a string: literals are constants;
   variables need a value→string conversion, so also declare
   `char *code_value_to_json(const CodeValue *)` returning a heap string
   the joiner does NOT free (caller frees) — or simpler, make join take
   `CodeValue *` varargs and render internally with the same JSON writer
   the runtime already has for errors. Pick the latter: one function, no
   lifetime choreography. Vendor-sync after.
6. **LSP** (`crates/code-lsp/src/tokens.rs`): interpolated names should
   classify as variables — check how `Token::Str` payloads surface to the
   tokenizer and extend; if the LSP only sees flat tokens, defer with a
   note rather than reshaping the token stream.

## Fixtures

`interp_basic.code` (`"hi $name"` round-trips through a handler or assert),
`interp_multi.code` (several vars, adjacent `$a$b`, leading/trailing),
`interp_empty_parts.code` (`""` and `"$x"` alone),
`fail_interp_unbound.code` (undefined variable still fails, located),
`fail_interp_bad_dollar.code` (`"a $ 1"` lexes as an error). Dual-mode as
always.

## Cost note

Cheapest of the big three: lexer branch + one AST variant + one runtime
helper. No precedence questions, no scoping questions, no ABI shape
decisions beyond one varargs function.
