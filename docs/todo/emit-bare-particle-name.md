# A no-field particle still has to be written `{}`

Every particle in an `emit` must be written with a brace body, even when it
has no fields to carry:

```
emit Timestamp {} to core get t
```

The `{}` is pure ceremony. `Timestamp` takes no operands — the handler reads
the clock and ignores the particle beyond its `"_class"` — so the braces say
nothing, and every `Timestamp` site in the tree pays them (see
`tests/emit_timestamp.code`). The owner wants

```
emit Timestamp to core get t
```

to mean exactly the same thing: **at an emit site, a bare uppercase name is
the empty particle of that class.**

`Timestamp` is the only one today, but it is not a one-off: the tier list in
[community-modules.md](community-modules.md) has more no-operand particles
coming (`terminal`'s `Read`, a `console` `GroupEnd`), so this is the shape of
a whole small family, not a single fixture's wart.

## Why it is safe to say

The lexical rule that decides what a name means already exists and already
belongs to particles. `parser.rs`'s `primary` treats an uppercase-first name
followed by `{` as particle construction, desugaring it into the ordinary
`Expr::Object` a brace literal would produce with `"_class"` prepended
(`src/parser.rs:592`). Uppercase-first is therefore already the language's
mark for "class name, not variable" — this proposal only lets it stand
without the braces, and only where a particle is the one thing that can
appear.

The cost is stated plainly: after this, `emit Foo to core` can never mean
"emit whatever the variable `Foo` holds". That is already true of
`Foo { ... }` anywhere in the grammar, so it costs nothing new in practice —
a particle held in a variable is conventionally lowercase
(`tests/emit_particle_from_variable.code`), and one that isn't can still be
emitted through any expression that isn't a bare name (`(Foo)` is not special
— it parses to the same `Expr::Ident`; `[Foo][0]` is the escape hatch nobody
will need).

## Fix direction

Do it as a desugar in the `Stmt::Emit` arm of `parser::statement`
(`src/parser.rs:145`), *not* in `primary`: `primary` must keep returning
`Expr::Ident` for an uppercase name, or every uppercase variable read in the
program turns into a particle.

The emit arm already parses the particle with `self.expr()` and then expects
`to`. After that `to` is confirmed, rewrite the parsed expression:

```rust
let particle = match particle {
    Expr::Ident(name) if starts_uppercase(&name) => {
        Expr::Object(vec![("_class".to_string(), Expr::Str(name))])
    }
    other => other,
};
```

No two-token lookahead, and the rewrite is exactly what `Name {}` produces
today — same node, same field order. Everything downstream is untouched: the
interpreter, codegen, and both module hosts see an `Expr::Object` with a
`"_class"` and nothing else, which is the case they already handle. Three
lines, one file, no ripple — the kind of change this repo prefers (compare
the reasoning in [runtime-error-locations.md](runtime-error-locations.md)).

Note it only fires on a *bare* name. `emit stored[0] to core` and
`emit p.inner to core` still evaluate normally, since neither is
`Expr::Ident`.

## What to test

- `tests/emit_bare_particle.code` — the new form against a core handler,
  asserting the same `TimestampResult` shape `emit_timestamp.code` does, so
  the two forms are proven identical rather than merely both accepted.
- The braced form must keep working: leave `tests/emit_timestamp.code`
  exactly as it is.
- A lowercase name is still a variable read: `emit p to core` where `p` holds
  a particle already has coverage in `emit_particle_from_variable.code`;
  worth a `fail_` fixture for a lowercase *unbound* name so the error stays
  `undefined variable`, not `unknown handler`.
- Dispatch is still runtime: `emit Foo to core` (no such handler) should fail
  with the same message `fail_emit_unknown_handler.code` expects.

Both output modes come for free — the change is in the parser, above the
point where `code run` and `code build` diverge.
