# A misspelled particle is silent, while a misspelled variable is not

The language is strict about names it can check and silent about the names
that carry the most weight. Both halves are deliberate; together they put the
strictness on the wrong side of the risk.

```
| a variable typo is caught
assert nope = 1
→ error: undefined variable 'nope'

| a particle class typo is not
Grade { score } => {
    return G { letter = "A" }
}
emit Grdae { score = 88 } to this get g
→ runs. g is null.

| a field typo is not either
emit Grade { scoer = 88 } to this get g
→ runs. the handler executes with score = null.
```

Neither of the last two prints anything. The program carries on with a `null`
it was never meant to have, and the first symptom appears somewhere else.

## Both behaviours are load-bearing

This is not a bug to be reverted. `emit` returning null for an unhandled
class is what makes a module's vocabulary optional: `README.md`'s "Common
particles" section rests on a program being free to ignore `Log` or a class
it does not care about, and `docs/todo/inbound-emissions-from-native-modules.md`
records the same for pushed particles — an unhandled push is dropped, on
purpose. A missing field reading as null is the same rule that makes
`Greet { }` and `Greet { name = null }` the same particle, which the module
template's own fixture asserts.

So the fix cannot be "error on unknown class" or "error on unknown field".
What is missing is a way to tell a deliberate no-answer from a typo.

## Why it bites harder than an ordinary dynamic-language typo

A particle is this language's call boundary. With no functions, `emit Name
{ ... } to this get r` *is* the function call — so a typo there is a typo in
a call, and every language that has calls diagnoses those. The asymmetry is
that the language checks the thing it made cheap to check (variables, which
must be declared with `let`) and skips the thing it made central.

The blast radius is also larger than it looks, because `get` always declares
a fresh binding. A misspelled class does not leave the previous value in
place or fail to bind — it binds `null`, so the next line reads a field off
null rather than reading a stale-but-plausible value.

## Options

1. **A `--strict` / `--check` mode on `run` and `build`** that refuses an
   `emit ... to this` whose class no handler in the program defines. `to this`
   is the tractable case: the handler table is known statically, unlike `to
   <module alias>`, where the class list belongs to a `.so`. Would have caught
   both examples above; would not touch module vocabulary at all. `verify.rs`
   already walks the program for the `emit ... to base` check and is the
   natural home.

2. **A warning, on by default, for `to this` only** — same analysis, printed
   rather than fatal, so nothing that runs today stops running. Cheaper to
   adopt, easier to ignore.

3. **A `module.json`-declared handler list**, so `to <alias>` gets the same
   treatment as `to this`. Much larger; only worth it if 1 or 2 proves its
   worth first.

4. **Nothing, and document it.** The README's `emit` section could state
   plainly that an unknown class answers null, so at least the behaviour is
   discoverable before it is experienced. This is the floor, not a fix.

Option 1 or 2, limited to `to this`, is the recommendation: it is where the
information exists, and it is where the typo happens.

Field names are the harder half and are deliberately left out of the options
above. A handler declares its fields (`Grade { score } => ...`), so an
`emit Grade { scoer = ... }` could be checked against that list — but a
particle is an ordinary object and the field list is documentation rather
than a schema (`README.md`: nothing a declaration says about a kind is
checked), so tightening it is a language decision, not a diagnostic one.

## Where it came from

Found by using the language cold, then deliberately mistyping. The variable
case erroring while the particle case did not was the surprise — the
expectation, having just been told there are no functions and handlers are
the substitute, was that handlers would get *at least* as much checking as a
variable name.
