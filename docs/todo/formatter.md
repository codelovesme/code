# `code format`

> **Shipped 2026-08-28**, close to as planned. What follows is the original
> write-up; "What shipped" at the foot records the two style decisions the
> diff actually forced and the one prediction that was wrong.

One canonical layout for `.code` source, enforced in CI the way
`cargo fmt --all --check` already gates the Rust half of this repo
(`.github/workflows/ci.yml`). `code format <paths>` rewrites in place;
`code format --check <paths>` writes nothing and exits non-zero if anything
would change.

The corpus it has to serve already exists: 141 fixtures in `tests/`, plus
whatever `site/build.py` lifts into the playground. Those files *are* the
house style — the formatter's job is to state that style in code, not to
invent a new one. Which gives the acceptance criterion up front:

> **Running `code format tests/` should produce a near-empty diff.** Whatever
> it does change is the review of the style decisions below.

## The decision that shapes everything: format the token stream, not the AST

The obvious implementation — walk `ast::Program`, print it back out — cannot
work here, because this AST is a *desugared* one by design. What a
pretty-printer would silently destroy:

| written | what the AST holds | printed back as |
|---|---|---|
| `-- why this test exists` | nothing at all — `lexer.rs` skips comments | **deleted** |
| `n += 1` | `Stmt::Assign(n, n + 1)` (`parser.rs` rewrites it) | `n = n + 1` |
| `Timestamp {}` | `Expr::Object([("_class", "Timestamp")])` | `{ "_class": "Timestamp" }` |
| `5.0`, `1.50` | `f64` | `5`, `1.5` |
| `a; b` | one `Newline` token | two lines |
| a blank line | nothing (`tokenize` collapses separators) | gone |

The first row settles it on its own. Every fixture in `tests/` opens with a
`--` block explaining what it proves, and those comments are most of the
language's documentation right now (see
[no-language-documentation.md](no-language-documentation.md)). A formatter
that eats them is not shippable, and teaching the AST to carry comments is
exactly the "heavy and dependent everywhere" change the owner ruled out for
spans (see [runtime-error-locations.md](runtime-error-locations.md)).

So: **`format.rs` consumes `Lexed` + the original source text**, and emits
tokens by slicing them back out of that text. Literals keep the author's
spelling, escapes are never re-encoded, particle sugar survives because it
was never desugared, and the formatter has no opinion about semantics at all
— which is what makes the safety property below provable.

## The one change outside `format.rs`: `Lexed` gains `ends`

`lexer::Lexed` already records `starts[i]` (char offset, see `span.rs`). Add
`ends: Vec<u32>` — the offset just past each token — and everything else the
formatter needs falls out of slicing:

- **token text** — `src[starts[i]..ends[i]]`.
- **comments** — whatever sits in the gap `src[ends[i]..starts[i+1]]`. By
  construction that gap holds only whitespace and `--` comments: anything
  else would have become a token, and a `--` inside a string literal is
  inside the *token*, not the gap. No `Token::Comment`, no trivia list, no
  second lexer.
- **blank lines** — count `\n` in the same gap. A `Newline` token consumes
  one separator; a second newline in the gap after it means the author left
  a blank line.

Mechanically it is one `ends.push(...)` beside each existing `starts.push(...)`
(about eight sites), or one small `push_token` helper if that reads better.
Nothing else in the tree reads `Lexed` positionally, so the field is purely
additive.

## Layout rules — v1

**Hard line breaks are the author's.** The formatter never decides to split
or join a line; there is no max width and no re-flow. A `{ "x": 1 }` written
inline stays inline, and `multiline_literal.code`'s array stays multi-line.
This is the rule that keeps the whole thing small — it also dissolves the
one genuinely awkward question in the grammar, which is that `{` opens both
a block and an object literal and the token stream alone cannot always tell
which. Depth counting does not care.

Everything else is normalized:

1. **Indent** — four spaces per open `{` / `[` / `(` that was opened on an
   earlier line; a line beginning with the matching closer gets one less.
   Four is what 133 of the tree's 136 indented lines already use;
   `multiline_literal.code`'s two-space body is the diff v1 is expected to
   produce.
2. **Spacing** — exactly one space between tokens, except: none after an
   opener or before a closer for `[` `]` `(` `)`; none before `,` or `:`;
   none either side of `.`; one space inside inline braces (`{ "x": 1 }`,
   `Log { "x": 1 }` — the existing style); none after unary `-` or `not`'s
   operand side (`-3.5`, per `unary_negation.code`). Unary vs binary `-` is
   decided by the preceding token: an operator, opener, `,`, `:`, `=`,
   keyword, or line start means unary.
3. **Blank lines** — runs collapse to one; none at the start of a file, none
   immediately inside `{` or before `}`. File ends with exactly one newline.
4. **Comments** — kept verbatim, never re-wrapped. A comment alone on its
   line is emitted at the current indent; a trailing comment keeps its line,
   two spaces after the code. (The tree has no trailing comments today; the
   rule exists so the first one is not a surprise.)
5. **Separators** — `;` becomes a newline. It is a statement separator that
   nothing in `tests/` actually uses, so canonicalizing it costs nothing and
   removes a second way to write the same thing.

## CLI

```
code format <path>...            rewrite in place
code format --check <path>...    write nothing; exit 1 if anything differs
```

A path may be a directory, walked for `*.code`. Notes worth fixing now:

- **Format never calls the loader.** It lexes and parses *one file's text*.
  A file that `link`s a module that isn't installed still formats — the
  formatter has no business resolving anything.
- **It must parse before it writes.** Lexing alone is enough to lay a file
  out, but an unbalanced brace would silently reindent the rest of the file
  into nonsense. So: refuse to touch a file that doesn't parse, report it,
  and (this is the point) *skip* it under `--check` rather than failing —
  the `fail_*` fixtures that are deliberate parse errors are unformattable
  by construction and must not break the CI gate.
- A file that doesn't lex is likewise reported and left alone. The formatter
  never writes a file it could not read completely.

## Why this is safe — the property to test

Because the formatter only moves whitespace between tokens it copied out of
the source, it can be *checked* rather than trusted:

- **Token equality.** `tokenize(format(src)).tokens == tokenize(src).tokens`,
  for every fixture. If that holds, the formatted file cannot mean anything
  different — the parser sees the identical sequence. (`;`-to-newline is
  already invisible here: both lex to `Newline`.)
- **Comment preservation.** The ordered list of comment texts is unchanged.
  Token equality says nothing about comments, so this is its companion.
- **Idempotence.** `format(format(src)) == format(src)`.
- **The corpus.** All three properties, over all 141 `tests/*.code` files,
  as one `tests/format_fixtures.rs` — the same "the fixtures are the tests"
  shape `run_language_tests.rs` uses.

Then in `ci.yml`, beside the existing `cargo fmt --all --check` step:

```yaml
- name: Code formatting check
  run: cargo run --quiet -- format --check tests/
```

## Phases

1. `Lexed::ends`, and `format.rs` behind `pub fn format(src: &str) -> Result<String, Located>`.
2. `tests/format_fixtures.rs` — the four properties above. Written *before*
   the tree is reformatted, so the properties are proven against the tree as
   it stands.
3. `code format [--check] <paths>` in `main.rs`, plus `USAGE`.
4. Run it over `tests/`, review the diff as the style review, commit.
5. The CI step, which from then on is what keeps the tree canonical.

Budget: ~117 lines for `format.rs` — a spacing table, a gap/trivia walker,
and an emit loop. If it starts growing past that, the cause will be rule 1
having quietly turned into re-flow.

## Deliberately not in v1

- **Re-flow / max width.** The alternative design: the formatter owns line
  breaks too (prettier-style), splitting anything over ~80 columns and
  joining what fits. Strictly more canonical, and strictly bigger — it needs
  the block-vs-object-literal distinction that v1 avoids, which means
  formatting from the parse tree rather than the token stream. Worth
  revisiting only if hand-layout drift turns out to be a real problem; the
  tree's longest line today is 87 characters.
- ~~**stdin/stdout mode** (`code format -`), for editor integration.~~ Not
  needed: `crates/code-lsp` serves `textDocument/formatting` directly from
  `code::format::format`, so an editor gets format-on-save without shelling
  out to anything (shipped 2026-08-28, alongside this).
- **Anything configurable.** No width knob, no indent knob — a formatter
  with options is a style argument with extra steps.

## What shipped

All five phases, in order. `Lexed::ends` turned out to exist already (added
for `crates/code-lsp`, whose doc comment names this document), so phase 1 was
`format.rs` alone — 250 lines rather than the 117 budgeted, the extra being
doc comments and the two spacing cases below, not re-flow.

**The properties held against the tree as its authors left it**, which was
the point of writing `tests/format_fixtures.rs` before reformatting anything:
token equality, comment preservation and idempotence all passed over the
corpus *unchanged*. Two more tests guard the guards — that most of the corpus
is actually formatted (so the three cannot pass vacuously by refusing
everything), and that every file refused is a `fail_*` one.

Then the whole suite passed after reformatting all 213 fixtures, in both
output modes, which is the real evidence: a formatter that changed meaning
would have shown up as a failing program, not just a failing property.

### The two rules the corpus decided, not this document

- **`[` after a value is a subscript, and takes no space.** The spacing table
  here only said "none after an opener", which turned `data.items[2]` into
  `data.items [2]`. Told apart by whether the preceding token could end an
  operand.
- **Empty braces close up: `Ping {}`, not `Ping { }`.** The corpus was
  genuinely split (28 to 37 the other way), so this was decided on the diff:
  `{}` leaves 172 files untouched against 164 for `{ }`. The inner space
  exists to hold content off the braces and there is none.

### The prediction that was wrong

"The tree has no trailing comments today; the rule exists so the first one is
not a surprise." It has three, in `is_basic.code`, and they were *column
aligned*. Rule 4 collapses them to two spaces, which loses the alignment —
kept as written, because a formatter that preserves hand alignment is not
canonical, and this is the same trade `cargo fmt` makes next door.

### The diff, which was the style review

22 files, 69 lines, in four groups: `{ }` to `{}` (32), `{"a": 1}` to
`{ "a": 1 }` (the minority spelling, 51 occurrences against 229), the three
aligned comments above, and `multiline_literal.code`'s two-space body going
to four — the one diff this document predicted by name.
