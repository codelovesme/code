# Working on `code`

Guidance for AI agents (Codex, Claude Code, anything else) working in this
repository. Read this before touching anything; it is the accumulated cost of
several long sessions, and most of it is not derivable from the source.

`README.md` documents the language. This file documents *working on* it.

---

## What this repo is

`code` — a small language: six JSON-shaped value kinds, one loop, no functions,
two output modes that must behave identically (`code run` interprets,
`code build` compiles through LLVM). Plus its first-party modules, its LSP, its
wasm/native embedding crates, and its VS Code extension.

**`old/` is an archive of a different, earlier language that shared the name.**
Constraints, `∈`-with-schemas, particles-with-declared-schemas. Nothing outside
`old/` refers to it. If a description of `code` involving constraints reaches
you, you were reading the wrong directory.

Layer rule, decided 2026-09-05 and enforced: **`code` is ignorant of euglena.**
No cells, no genes, and **no organelles** — those belong to the layer above. The
language says *module*. The runtime-link address is `{ _module }`; a host is
asked `Module`, never `Organelle`. Do not let euglena's vocabulary drift back
down into this repo.

---

## Build and test — read this part twice

```sh
cargo build --workspace
cargo test  --workspace
cargo clippy --workspace --all-targets
```

**Always `--workspace`.** The root manifest has both a `[package]` and a
`[workspace]`, so a plain `cargo build` / `cargo test` builds **only the root
package** and silently skips `code-lsp`, `code-native`, `code-wasm`, and
`code-launcher`. On
2026-09-04 a green local suite (31 suites) shipped a release whose editor
extension did not compile — a new reserved word was added to the lexer, and the
language server colours tokens with an exhaustive match. `--workspace` runs 35
suites and would have caught it. It cost a hotfix release.

The same reflex generalises: **when a change touches a shared type** (a token,
an AST node, an ABI struct), find every exhaustive match over it, not just the
ones in the crate you are editing.

Also worth knowing:

- **`tests/run_language_tests.rs` runs its fixtures in parallel** (2026-09-10),
  one thread per core. It was sequential and cost 56s of a 2-minute suite;
  it costs 10s now. Each fixture compiles to its own path, and
  `tests/concurrent_builds.rs` is the proof that concurrent `compile_file` is
  safe — that test exists because it once was not.
- `code format --check tests/` is part of green. It had been red for days once
  without anyone looking.
- `.code` fixtures must be `code format`-clean. The canonical empty particle is
  `{}`, not `{ }`.
- `src/runtime.c` and `crates/code-native/vendor/runtime.c` **must stay
  byte-identical** (`tests/native_crate_vendor_sync.rs` enforces it).
- Each `crates/modules/<name>/` is its own standalone workspace (empty
  `[workspace]` table). **Tests build them all into one shared
  `target/modules/`** (`tests/support/modules.rs` — use it, never a bare
  `cargo build` in a module dir from a test). Before 2026-09-24 each built
  into its own `target/`: 11 GB of duplicate dependencies, and a one-line
  `code-native` change took 210s to rebuild the modules; now 1.1 GB and 28s.
  The old `crates/modules/*/target/` folders are dead weight and can go.
- **The quick loop: `scripts/test-changed.sh`** (`-n` to only print the
  plan). It diffs against `origin/main`; if only modules and fixtures moved,
  it runs just their fixtures and tests (`ntfy`: ~28s). Anything shared —
  `src/`, `code-native`, the runtime — gets the full `--workspace` suite, for
  the reason above. `FIXTURES=console_,fail_emit cargo test --test
  run_language_tests` is the manual form: only those fixtures, and only the
  modules they link. CI always runs everything.
- `target/release/code` on this machine may be stale. Prefer `target/debug/code`
  when a test needs `EUGLENA_TEST_CODE_BIN`.

---

## Writing a native module (the `code-native` ABI)

Two `#[no_mangle]` exports:

- `code_module_abi_version() -> u32` returning `CODE_ABI_VERSION`
- `code_module_dispatch(out: *mut CodeValue, particle: *const CodeValue)`

Pattern: `guarded(&mut *out, "modname", |out| { ... })` catches panics. Route on
`read_field_str(particle, "_class")`. Unhandled `_class` → `null(out)`. Errors →
`exception(out, "modname", &msg)` — **never panic, never exit.**

Building results into `out`: `SlotBuffer::new(n)` + `borrowed_str(slot, c"...")`
+ `owned_str` / `number` / `boolean` / `null` / `copy` + `object(out, &[c"_class",
...], &mut buf)` + `buf.release_all()`. Helpers: `object_dyn`, `object_entries`,
`make_result(out, c"Class", |slot| ...)` for `{ _class, value }`, `require_str`,
`find_field`, `read_str`, `read_field_bool`.

**Every feature must behave identically in both output modes.** That is the
language suite's property to prove.

### Conventions settled during the organelle port

- Error model: the `Exception` value (`source = "<module>"`). Never the old
  `Error { status_code }` or `{ ok: false }`.
- Stateful modules take a `Config { ... } → ConfigResult { ok }` setup particle.
  Every other handler is an `Exception` until `Config` runs. `http_server` keeps
  `Config` for port/host and adds `Listen {}` as a separate action.
- `Sap` is removed everywhere — it was a euglena manifest-delivery mechanism.
- The `setup` field flows `module.json` → `.code/lock.json` → euglena codegen.
- Module dir names are single-word with a matching `[lib] name` (`json_store`,
  `blob_storage`, `mongodb`); the artifact is `lib<name>.so`.
- External-service modules are first-party here, tested env-gated against docker
  services plus `.code` error-path fixtures.

### Adding a module — the full wiring checklist

Miss one of these and it half-exists:

`src/module_install.rs` FIRST_PARTY · `.github/workflows/publish-modules.yml`
(dogfood case + handlers case + `setup` alternation — there are no module
lists to edit any more: every matrix comes from the `plan` job, which reads
`crates/modules/`, and a `page.mjs` is what sends a module to the wasm32
build; `tests/first_party_modules.rs` holds the workflow to that) ·
`tests/run_language_tests.rs` stem list + doc comment ·
`crates/modules/<name>/README.md` · `README.md` (2 spots) ·
`docs/todo/community-modules.md` · `.github/workflows/ci.yml` services if it
needs one · `tests/<name>_module.rs` integration test.

### Language gotchas that have bitten

- `to` is a keyword — use `recipient` as a field name.
- `$FOO` interpolates inside `.code` string literals; escape as `\$FOO`.
- `∈` is the membership operator (`is` was removed 2026-08-29).
- **`∉` is its negation** (2026-09-09), parsed as `not (x ∈ Name)`.
- **Conditions are strict Bools.** A truthiness rule (`null`/`0`/`Exception`
  false) shipped and was reverted the same day, owner's call — it was a
  mistake. `if`, `assert`, `not`, `and`, `or` all require a Bool; check an
  answer with `r ∈ Exception` or `r ∉ Exception`.
- **Object equality is by field name, not position** (changed 2026-09-09).
  Arrays stay positional. `preserve_order` on a module's `serde_json::Map` is
  still worth having — `loop` and printing show the order — but it is no longer
  the difference between `assert v = { a, b }` passing and failing.
- **A block is an indented run of lines, from 2026-09-09.** There are no
  braces on it; `{ }` is an object and nothing else. The lexer emits
  `Indent`/`Dedent` **outside brackets only** — inside `{`, `[`, `(` the
  closer ends the construct, which is what keeps a multi-line literal free to
  lay itself out. A one-statement body goes on the header's line after a
  comma (`if x, return Y`); after `=>` it needs no comma. There is no bare
  block and no empty body. Four places count a block's depth and must stay in
  step: `src/lexer.rs`, `src/parser.rs`'s `block`, `src/format.rs`'s
  `push_token`, and `crates/code-lsp/src/tokens.rs`. The VS Code extension's
  `language-configuration.json` indents after a block header too, so its
  `increaseIndentPattern` is a fifth place — it has to know that `if x, …` on
  one line opens nothing.
- **`length` inside an index, and `xs[from, to]`** (2026-09-10). `length` is
  not a reserved word — it is a field name in a dozen module contracts
  (`RandomCode { length = 12 }`), and taking the token would have broken all
  of them — so it is only *substituted* where it stands inside an index, with
  the container being indexed as its operand (`parser::bind_length`). Binding
  a variable called `length` is refused, the way an uppercase name is.
  A range's brackets carry the **interval notation** — `[a, b]` closed,
  `[a, b)` and `(a, b)` leaving an end out — and the parser folds all four
  into one half-open pair, so the AST and both backends keep a single rule
  (the same trick as `∉` being built as `not (x ∈ …)`). A postfix `(` can only
  be a range: there are no calls, so nothing else follows an operand with one.
  Bounds clamp, since a single index past the end already answers null.
  **Strings index and range too** (2026-09-10), in characters — one character
  comes back as a one-character String, since there is no character kind. The
  UTF-8 offset walk is `runtime.c`'s `char_offset`; the interpreter uses
  `chars()`. An object answers `length` but cannot be ranged. A `Pop`
  core handler was written the same day and removed before committing — this
  covers it.
- **An uppercase-first name is a particle everywhere** (2026-09-09), brace or
  no brace — `return Checked` is `return Checked {}`. The other half of the
  rule is what makes it work: an uppercase name may not be **bound** (a
  variable, a `get`, a field list name, a loop variable all need lowercase).
- **A loop has no accumulator** (2026-09-09). `loop … get out = init` meant
  exactly a declaration on the line above, so it went; `get` is an emit's
  answer and nothing else. What survives a loop is an ordinary binding
  declared before it.
- **`let` is gone, and nothing may shadow** (2026-09-09). `name = value`
  assigns the visible binding or introduces one here; the two readings are
  never both available, so no keyword chooses. A **binder** — a handler's
  field list, a loop's variables, a `get` — is refused outright if the name is
  already in scope, and `as` is the way out (`Change { email as e }`). `get _`
  twice is therefore an error; `get` was always optional, so drop it.
  `verify.rs` is where the rule lives, so both backends refuse the same
  programs.
- **`export` is gone** (2026-09-09). A `.code` module's names are its own; a
  link reaches its *handlers*. `as` still names a link and binds an **empty
  object**. A `.code` library therefore emits no `code_module_vars` — a Rust
  native module still reports its own constants through that entry point,
  which is untouched.
- **A newline separates, a comma keeps two things on one line** (2026-09-09).
  Statements, object fields, array elements and field-list names all follow
  it, so a multi-line literal needs no commas at all. Trailing commas are
  still refused. `code format` leaves the commas an author wrote — whether it
  should strip the redundant ones is open.
- **The comment marker is `|`, from 2.0.0.** Hard change, no transitional `--`;
  `--` now lexes as two `Minus` tokens. Three places recover comments from
  inter-token gaps and must stay in step: `src/lexer.rs`, `src/format.rs`'s
  `gap()`, `crates/code-lsp/src/tokens.rs`, `crates/modules/syntax/src/lib.rs`
  (the terminal IDE's highlighter — its hand scanner is the fallback for a
  line that does not lex) — plus the VS Code extension's
  `syntaxes/code.tmLanguage.json` and `language-configuration.json`.
  The VS Code extension's `syntaxes/code.tmLanguage.json` is **fallback**
  highlighting — the LSP's semantic tokens override it wherever the server is
  running, which is why it went years out of step without anyone noticing (it
  was still matching the archived language: `yield`, `with`, `fold`, `by`,
  `private`, `is`, `Z|R|N`). Rewritten 2026-09-10 against `lexer.rs`, which is
  the only authority for the keyword list.
  **Never `sed` a comment-marker migration**: `--` also appears inside string
  literals (`assert "-" + -4 = "--4"`) and inside comment prose. Drive it from
  `code::lexer::tokenize` instead, one replacement per line, and hand-edit the
  `fail_interp_*` fixtures that deliberately fail to lex.

---

## Hosting: two models, and they differ in one dangerous way

### Native / machine hosting (`euglena-runtime-hosting`)

A host holds applications in memory: an app built `--target shared`, linked
while the host runs, and stopped so its memory comes back. Landed 2026-09-04:
`link <expr> as <name>` inside a handler, `unlink`, addresses as ordinary
values, `code_abi.h` item 9 (`code_module_release`) and item 10 (a host
furnishing its guest's modules).

Binding constraints the owner set:

- The guest's source must not change — same file standalone or hosted.
- The guest shares the host's modules; it must not bind its own port.
- Stopping a guest reclaims memory, not just routing.
- The host emits to **each guest separately**, not through one shared door.
- Strict: a module the host does not offer is refused.
- One instance per app name.

Rejected, with reasons: a `loader` module (runtime linking left it no job);
native stand-ins (they belong in genes); apps as separate processes.

`Linked` (**not** `Hosted`) answers "is this run a module somebody linked, or is
it the program?" — from the build target, so no runtime state and nobody has to
be present. `emit Linked to core get where`, and `link` inside an `if` turns
that into a choice of door. One build, two lives.

**A `.code` guest's top level is LAZY when hosted** — it runs on first dispatch
or first value read, not at `link` time. A host must poke it (`emit Anything {}
to opened get _`) right after linking. Forgetting this looks exactly like the
guest's `Config`/`Listen` never running, and cost a long dead-end chase.

### Browser hosting — the `guest` module

The web twin, landed 2026-09-06. A shell fetches another app's `.wasm`, gives it
a container, and stands behind the modules it reaches for. **Why a host on the
web at all** (owner's reasoning): not process economy — every client has its own
machine. It is **continuity**: the shell holds the session while apps come and
go.

The design is deliberately the machine's, in the same words: `Offer { app, name }`
asked once per module, `Module { app, name, particle }` per particle to one the
host took. `Offered` / `Denied` / **no answer = the guest keeps its own**. An
earlier draft with one `GuestAsks` per particle was replaced after the owner
asked whether the two layers agreed.

**Storage is deliberately NOT narrowed** (owner's decision, 2026-09-06). The
first cut prefixed a guest's keys with its name; the owner pushed back — the
session already lives in `localStorage`, shared per origin, which is how the
apps share a token today. Prefixing would make a hosted app behave differently
from the same file standalone. A shell that wants a guest kept apart *offers*
`storage` and namespaces it in its own handlers. This also killed the planned
`session` module. `dom` and `router` **do** stay contained — two apps cannot
both own the body, and only one can own the address bar.

### ⚠ The difference that will cost you a day

**In the browser model, a host must link every module any guest it hosts might
call — not just the modules the host's own code touches.**

A `--target wasm` build only bundles the browser-side JS glue (`PARTS` in the
generated `host.mjs`) for the modules **it itself** links. `guest`'s `Load`
creates a nested `createHost`, but from the *same* `PARTS` closure as the outer
shell — there is no separate bundle assembled from what the guest links. A
guest's call to a module the host never linked reaches `code_web_ask`, finds no
PART for that name, and returns `-1n` — the documented "nothing answered" case,
which the guest's runtime reads exactly like a module that chose not to answer.
**Silently. No error, no exception, no log.** A `Delay { then = X }` that is
never answered never schedules `X`.

This is *not* mirrored by native hosting, where furnishing a guest's modules is
an explicit, checked ABI item. Here it is an implicit, silent bundling
omission — and it cost real hours in `my-euglena-apps` (see that repo's
`AGENTS.md`, symptom 5).

Other `guest` gotchas:

- **`page.mjs` is `include_str!`d into the compiler.** After editing any browser
  half you must `cargo build` *before* rebuilding a wasm app, or the app carries
  the old half.
- A runtime error inside a handler is not silent — it answers an `Exception` to
  whoever asked. But `fire` throws the answer away, so a *told* particle's
  failure was invisible; `Tell` now asks instead and fires any Exception at the
  host with the guest's name on it.
- **`Tell` is deferred (`queueMicrotask`).** Running the guest inside the host's
  handler makes the guest's questions re-enter `code_event_ask`. It survives
  that today by construction, but `runtime.c` *assumes* one-at-a-time rather
  than enforcing it.
- One instance per app name, one app per container; both free on `Unload`.

Test: `tests/guest_module.rs` builds 6 wasm32 module archives plus a shell and
an app, then runs a node probe with a hand-written stand-in document. Skips
without node or the wasm32 target. **See the testing warning below about what
that stub does and does not prove.**

### Browser module instances and configured network clients (2026-09-08)

Each browser alias has its own JavaScript module state. Codegen exposes the
currently dispatched alias through `code_web_instance`; the page keys module
instances by that identity and restores the outer identity across nested calls.
Two aliases can link the same `.a`: their LLVM function declarations are shared,
but their browser configuration is not. Rebuild wasm and its `host.mjs` together.
This does not isolate native static-library globals; Linux `.so` links already
get their own images as described in `runtime.c::module_image`.

`net_client` now requires `Config { url }` before `Send { particle, timeout_ms? }`.
Do not put URLs back on `Send`: use one configured alias per destination.
The module's release metadata declares `setup: Config` so a manifest can supply
it. Tests cover two native clients and two browser aliases retaining different
destinations even when one is reconfigured.

---

## Keep-alive: settled, do not re-litigate

A bare `loop { }` pins a core (measured, both modes) because the host drain
returns instantly when nothing is queued. **Do NOT "fix" this in the runtime.**
`README.md` states as a design position that *waiting is the module's job, never
the runtime's*. A runtime backoff was added once and had to be reverted.
`Listen` joining its accept thread does not work either — measured: CPU 0% but
every request 504s, because the program parks inside `code_module_dispatch` and
the drain only runs between top-level statements.

The real fix: **the host keeps the program alive.** Optional ABI export (item 8,
no version bump) `int code_module_serving(void)`; while any linked module
answers non-zero the program does not end at its last statement. The host parks
on its own queue (condvar notified by every push, 1s re-check for shutdown),
drains, parks again — the Java non-daemon-thread / Node ref-count model.
`interpreter::keep_alive` + `codegen::gen_keep_alive` + `runtime.c`'s
`code_host_park` / `code_native_serving`. App source is now just `Config` +
`Listen` — no loop, no `Wait`, no keyword. `http_server` gained `Stop {} →
StopResult { ok }` (AtomicBool + self-connect to wake the blocking `accept()`).

The opt-in export is load-bearing: `test_events` / `test_timer` do not export
it, so the `tests/inbound_*.code` fixtures still terminate.

A held guest's own door was invisible to `keep_alive` until `834628f`:
`any_module_serving` asked only `env.inbound`, so a runtime-linked guest that
owns its door and never speaks first was never asked — a doorless host ended the
instant its top level finished. Now it also checks `env.runtime_modules`.

---

## Plain numbers in compiled code (ticket 113, 2026-10-11)

`codegen.rs` evaluates an expression to a `Val`: a boxed `CodeValue` slot, or
a plain `double` / `i1` where the operator can only answer one (`-`, `*`, `/`,
unary `-`, comparisons, `=`, `and`, `or`, `not`, `∈`; `+` of two numbers).
`src/kinds.rs` finds the variables that only ever hold a number (or a
boolean); those are stack `double`s, and the one IR pass we run, `mem2reg`,
puts them in registers. The benchmark loop is then C's five instructions.

What was measured and rejected, so nobody tries it again blind:

- **Tag checks with an in-line fast path on boxed operands.** Aquarium's wasm
  grew 27% and its build took 4× longer; even a lean version (slow side
  jumping straight to the unwind block) was +5% and a third slower to build,
  for no frame-time gain — Aquarium's profile has no time in the arithmetic
  calls. A boxed operand goes to the runtime function, as before.
- **Inline `code_number` writes** (release check + stores) at every
  assignment: +10% wasm for one call saved.

Two traps:

- **The wasm backend is slower than linear in a basic block's length.** A
  literal of hundreds of numbers is one block once nothing in it can fail;
  `StableShapes` went from 0.15 s to 0.68 s. `bound_block` ends a long block
  between literal elements with a failure check (a plain `br` is folded back
  by the backend and does nothing).
- **`kinds.rs` works by name, per function.** A top-level name any handler
  mentions stays boxed, and so does a handler name that is also a top-level
  one: a plain `double` in `main` cannot be reached from a handler.

## Growing a value in place (ticket 121, 2026-10-11)

`name += x` and `name = name + x` compile to `code_add_into(slot, x)`, which
grows the slot's own array, string or object block where it stands when its
count is 1 (`held_alone`), with room doubling (`room_for`, sized by
`malloc_usable_size` / the wasm shim's block size, so the header and module
ABI are unchanged). An object merge adds new fields on the end too
(`grow_object_in_place`): the block is `[count, keys][slots][key
pointers][characters]` (since C.2), `keys - items` is how many fields fit, and
characters run in key order, so the
last key's end is where the next goes; out of room, the object moves to a
block twice the size, its values moved, not retained. Anything shared, an
operand that is the value itself, or a merge naming one field twice falls
back to `code_add`. A value is never
seen to change: whoever else holds it has a count, and a count above 1 means
a copy. `tests/in_place_growth.code` holds the aliasing cases. It took
`benchmarks/workloads/` binary_trees from 500× C to 8×; building an 8,000-field
object went from 2.0 s to 0.22 s (what is left is `find_field`'s key search).
A string block's header keeps its byte length + 1 (`CodeHeader.text_length`,
the old padding word; 0 = not measured yet), filled by `text_length` and by
`join_in_place`, so an append no longer `strlen`s the whole text: string_build
12× → 3× C. Nothing else may change a string's bytes in place, or the kept
length goes stale.

`name = name[from, to)` compiles to `code_slice_into(slot, from, to)`: a list
held alone is cut where it stands (elements outside released, the rest
memmoved to the front; an empty result owns no block), anything else goes to
`code_slice` and is moved in — and on its failure `dst` is left alone.
`x = null` / `x ≠ null` compile to a tag read (`∈ Null`), never
`code_values_equal`. binary_trees 9× → 6× C.

## Setting a part (2026-10-11)

`xs[i] = v`, `fish.x = v`, `fishes[i].x += 1` — the owner's call, on the same
rule as ticket 121: one holder means change it where it stands, any other
holder means it never sees a change. The parser turns `name<path> = v` into
`name = Expr::SetAt { base: name, path: keys, value }` (a `.field` step is its
name as a `Str` key), so every assignment rule — verify, kinds, handler
scope — applies unchanged and nothing downstream needed a new statement.
Both backends special-case `SetAt` whose base is the assigned name:
the interpreter takes the binding out (`Environment::take`) and uses
`Rc::make_mut` per level (`set_at`); codegen calls `code_set_at(slot, keys,
n, value)`, which holds `value` first (so `xs[0] = xs` copies), then makes
each level the slot's own (`own_level`: copy only if its count is above 1).
A new last field goes through `code_add_into`. Messages are pinned in
`tests/message_parity.rs`; the aliasing cases are `tests/set_parts.code`,
also run on wasm.

Neither `code_set_at` nor `code_add_into` is in `code_abi.h`: both can fail,
and a fallible function a module can call is a silent hole
(`tests/module_abi_cannot_fail.rs`). Codegen declares them itself, like
`code_index`.

## Reading without copying (ticket 120, 2026-10-11)

`gen_operand` is `gen_val` for a use that only looks at the value — every
operand of `-`, `*`, `/`, `+`, `=`, ordering, `not`, `and`/`or`, `∈`, an `if`
condition, and the right side of an assignment. For a `Field`/`Index` chain it
calls `gen_peek`, which answers a pointer to the value where it is
(`code_field_ref`, `code_index_ref`; a string's character goes in a scratch
temp, a miss is a static null). **Never use it where the value is stored or
handed on** (`gen_expr`): only within one expression is nothing able to
release the container — `code_add_into` reallocs, a loop or an emitted
handler can reassign. The assignment case is safe because `code_copy` now
reads `src` before releasing `out` (`o = o.inner`); the fixture harness runs
compiled programs with `MALLOC_PERTURB_` and glibc's tcache off so a read of
freed memory fails a test (`tests/reads_in_place.code`).

`-`, `*`, `/` and ordering with any boxed operand take its number with
`code_operand_number` (one call, `code_mul`'s message) and do the arithmetic
in line, instead of boxing the plain side for `code_mul`. That retired
`code_compare` — and with it the last NaN difference (a NaN held in a slot
compared `≤`/`≥` true). spectral_norm 5× → 3× C; Aquarium's wasm +2%, build
time unchanged, frame rate unchanged (reads were ~4% of its busy time).

## Finding a field by hash (2026-10-11)

Every key an object holds is stored as `[u32 FNV-1a hash][chars\0]`, with
`keys[i]` still pointing at the characters (modules and `code-native` read
keys as C strings and see nothing new). The program's own lookups
(`field_position`, `own_field`, `own_field_of_key`) compare hashes first;
`.field` reads (`code_field_ref`) and part writes (`code_set_at`) get the
literal's hash from the compiler (`codegen.rs`'s `key_hash`, pinned to the
FNV vectors) and a private per-site global remembering the position found
last. Object literals with only literal keys use `code_object_literal` with
pre-hashed key constants (`hashed_key`); copying keys between objects
(`copy_own_key`) carries the hash along.

**`find_field` stays a plain `strcmp` scan, on purpose**: functions a module
can call reach it, and a module carries its own, possibly older, runtime copy
whose objects have no hashes (or an older host could hand a newer module
unhashed objects). The hashed paths are only ever given objects this
program's runtime built — module results and pushes are rebuilt by
`code_object` on the way in. If that ever stops being true, those lookups
read four bytes of whatever precedes a key and miss.

dictionary 22× → 5× C, points −30% instructions; Aquarium's wasm +2.5%,
frame rate unchanged.

## Kinds fixed by first assignment (ticket 122, 2026-10-11)

The owner's model: no named schemas; a variable, every object field and every
list element keeps the kind of its first non-null assignment. `src/infer.rs`
works it out per file with a union-find over type variables (shapes: Unknown,
Any, Scalar, Array(elem), Object{fields, map}) unified along every flow —
assignment, list literal and `+`, object merge, part writes, a literal
particle emitted `to this` (into that handler's *input* var), a handler's
`return`s (into its *result* var, which `get` reads). **A class name groups
nothing**: two `Result { … }` literals that never meet are two values — the
first version grouped by class and drowned in false alarms (`Store.value`).
A computed key makes a map; a module/core answer and a record walked by
`loop` are `Any`, never reported. Conflicts on label-derived vars are left to
`diagnostics.rs` (an error), and a kind-change on a statement that already has
a type-mismatch is dropped.

A.4 (2026-10-11): three false alarms gone — a list literal with mixed
kinds unifies its elements quietly to `Any` (`Infer::mixed`; it is no value
changing kind, and 123's union will name it); `s[0]` on an unknown value no
longer makes it a list (texts are indexed by number too); handler labels are
applied before any body runs, so a send written above a `∈ Any` field is not
reported. 185 warnings → 66; `∈ Array of Any` on the UI-tree list variables
and two real fixes took the apps to 13, all UI trees whose element objects
meet with `children` of different kinds (text in one, elements in another)
— those wait for 123's `Element`, and so does making the rule an error.

Measured on every app: ~215 warnings, ~75% from UI trees — `children` mixing
text and elements, canvas `fill` string-or-gradient, path arrays mixing
strings and numbers — the rest polymorphic helpers (`Folded.text` list or
string) and a handful of real variables. Labels: `{ x ∈ Number }`,
`Array of`, `Object of`, `Any` (`ast::Label`, parsed by `Parser::label`).

**A.3, the migration tool** (`src/write_types.rs`): `code check
--write-types [--observed <file>]` writes a label into every unlabelled
handler field, in the project's own files only (never `.code/`). Source 1:
`infer::suggested_labels` — the handler's input var, `UNKNOWN` (`_`) where the
program decides nothing. Source 2: run the fixtures with `CODE_OBSERVE=<file>`
(interpreter `observe` in `run_handler`, one `Class\tfield\tlabel` line per
field, `value::label_of_value`), merged per field by `Label::merge`;
`write_types::fill` completes the program's view with what was seen (unknown
parts, and fields the program never reads). Long labels lay the header out one
field per line; nothing else in the file is touched (no reformat — the
first run reformatted whole files and buried the labels in noise), and a file
that would not parse after is not written. Field names that are not words are quoted (`ast::field_name`).
`missing-field` is gone: a field an emit leaves out is null, which fits every
label. On every app: ~1,760 fields labelled, ~610 left (handlers only the
host/modules/other apps call and no fixture reaches), ~35% of new label parts
`Any`; zero new `code check` errors; every fixture still passes. Observation
cannot tell a map from a record (`house.temperature.bedroom` is user data) —
a person reviews.

## Shapes and in-line reads (ticket 122 stage B, 2026-10-11)

An object literal whose keys are all written down is built from one constant
key list per distinct ordered list (`Gen::shape_keys`, an array of
`hashed_key` pointers) via `code_object_literal`, which marks the block
header's spare word with that list's address: the object's **shape**
(`SHAPE_OF` in runtime.c; the same word holds a string's length). A copy
(`own_level`) keeps it; a merge adding no field keeps the left side's; adding
a field clears it. `infer::field_layouts` maps each `.field` read and each
named step of a part write to the first literal shape flowing into its
object's kind and the field's index (`Layouts::fields`, `Layouts::parts`,
keyed by expression address — codegen walks the same `Program`). Codegen then
emits `shaped_slot`: tag is Object, keys non-null, header word == shape →
the slot; else `code_field_ref` as before. `code_set_at` takes per-step
`shapes` (a pointer table — a `ptrtoint` in a static initializer does not
link on wasm) and `slots`. `number_index`: `xs[n]` with a plain number checks
Array, range and wholeness in line (`select`, so a poison `fptosi` decides
nothing) and boxes `n` only on the slow side. String literals are read-only
constant `CodeValue` slots (`str_constant`, laid out per target, padded to
80) — no `code_str`, no temporary; anything writing into a literal's slot
would now fault, which is the point.

points 30× → ~20× C, spectral_norm 0.48 → 0.42 s; Aquarium wasm +4%, build
time unchanged.

**Numbers read in line where the program only makes numbers.**
`Layouts::numbers` holds the `.field`/`[index]` reads whose kind is Number
(`read_sites` after unification). `gen_operand` marks their slots in
`likely_numbers`; `numbers_of` then checks the tag and loads the number in
line, calling `code_operand_number` only when it is not one (null, data from
outside), and `likely_add` adds two such operands in line, writing the sum
into a fresh temp, with `code_add` on the slow side so `+` still joins
strings. The earlier rejection of in-line tag checks was about doing it for
*every* boxed operand; here only reads the inference vouches for get it.
`clear_temps_from` looks at `heap` before calling `code_clear` — but only for
temps in `mostly_empty` (a slow path's scratch, a likely sum): doing it for
every temp made Aquarium's wasm +5% for nothing. spectral_norm 0.42 →
0.31 s (1.9× C); Aquarium wasm 947K → 978K, build time unchanged.

**Part writes in line** (`shaped_write`, called from `gen_set_at` before the
`code_set_at` call it falls back to): a number (or a boxed value whose tag is
Number) written along a path whose every step is a numeric list index or a
field with a known slot (`Layouts::parts`) — each level checked held alone
(heap, header count == 1, shape for objects, range and wholeness for
indexes) and the old value holding nothing on the heap — is stored as tag +
number. Variables the inference vouches are numbers get the same in-line tag
check as reads (`gen_operand`'s `Expr::Ident` arm), and assigning a likely
number into a slot holding nothing on the heap is in line
(`likely_number_assign`) — **both only inside a loop**: at every operand they
cost code, and Aquarium's straight-line handlers made the wasm +10% for no
gain. points 19× → ~10× C, spectral_norm 0.31 → 0.27 s (1.6× C); Aquarium
wasm 978K → 1043K. Later the same day every in-line read went loop-only
(`Gen::hot`: shaped field slots, numeric indexes, likely numbers): Aquarium
1,052K → 936K, the five workloads no slower. Counting the handlers a loop
emits to as hot too was tried and made it 1,102K — nearly every handler of a
frame-driven page is one. Running instcombine/simplifycfg/early-cse after
mem2reg saved only 3% for +2 s of build, so it is not done. `tests/shapes.code` holds the cases (shape kept, cleared,
other orders, fewer fields, non-objects), also on wasm.

## Loop versions and the release optimizer (ticket 122 stage C.1, 2026-10-11)

Points ran 300 instructions per iteration after stage B: every in-line check
had a slow path that **joined back** after a runtime call, so nothing a check
found survived to the next read — `pts[i]` was checked from scratch six
times. And release builds ran only `mem2reg`; the TargetMachine's `-O2` is
the backend alone.

- **Two versions of an innermost loop** (`gen_loop`, `contains_loop`): Fast
  (`Version::Fast`, `hot()` true) and Generic (`hot()` false: no fast paths,
  only calls). In Fast, a statement that is an assignment or an `if`
  (`redoable` in `gen_stmt`) gets a `Bail`: `bail()` makes its block on
  first ask; each fast path (`number_index`, `shaped_slot_or`,
  `likely_number`, `likely_number_of`/`likely_add` — which now answers a
  plain `Val::Num` —, `write_number_or`, `shaped_write` → `Written::Done`)
  branches there instead of building a slow path. At the statement's end the
  bail block clears the temporaries made since the fast body began
  (`emit_clears_from(fast_temps_mark)`) and jumps to the statement's
  `generic_entry`. Safe because a statement changes what it changes **last**
  (the write); nothing in an expression has an effect. The generic version's
  back edge goes to the *fast* head: one odd value costs one iteration.
- **One variable in both versions**: a binding's slot is kept by its
  statement's address (`binding_slots`, `binding_slot`), for assignments and
  `emit … get`; a loop's cursor is made once and shared.
- **`alias.rs`**: TBAA on every load/store it can place — `local.<field>`
  (a `_code_slot_*` global or an alloca), `heap.<field>` (through an
  list pointer read from a slot's payload; whole-slot `i * 16` offsets
  reduce to the field), `list.len`/`list.keys`, `header.-16/-8`. Untagged when unsure (call
  results, phis). Fields only at their own offset *and* width, so an `i64`
  over tag+heap is never tagged as one of them.
- **Release = `default<O2>`** after tagging. A fast field write checks the
  old value is a number and writes only the number — a tag store would be
  one every tag load has to look past.

Fast/generic doubles innermost loop bodies: Aquarium dev wasm 936K → 975K,
build time unchanged; release build 1.5 → 2.9 s, 944K. Points 7.3 → 3.7 ms
(60M → 17.7M instructions), spectral_norm 0.27 → 0.22 s. What is left is
memory: a point is ~500 bytes of 80-byte slots (a C model with the same
checks and layout: 4.7 ms; with 16-byte values: 2.65 ms) — stage C.2.
`tests/loop_versions.code` fails a check partway through loops; the language
harness now also runs every fixture as a `--release` build.

## 16-byte values (ticket 122 stage C.2, 2026-10-12)

A `CodeValue` was 80 bytes: tag, heap, and a field for every kind — number,
str, boolean, items, keys, len — side by side. A point `{ x, y, vx, vy }` cost
~500 bytes, and the C model said memory was what was left of points' gap.
Now (`code_abi.h`, **`CODE_ABI_VERSION` 2**, owner's go-ahead) it is `tag`,
`heap` and one 8-byte payload union (`number` / `str` / `boolean` / `list`),
`CODE_VALUE_SLOT_SIZE` 16 on both targets. A list's or object's count and
keys moved into its block: `CodeList { len, keys }`, then the slots, then (an
object's) the key pointers and characters — one block, one count. An empty
list or object has no block (`list` NULL).

- Runtime: `LEN`/`ITEMS`/`KEYS` (`code_list_len`/`_items`/`_keys` in
  `code_abi.h`, which answer 0/NULL for any other kind — code that read
  `v->len` of a number used to get 0, and still does); `list_alloc`. Hot
  loops read the count and keys once before calling `strcmp`.
- Codegen: `VALUE_SIZE` 16, `LIST_HEAD` 16, `load_list`/`list_items`; every
  in-line path checks the block pointer is non-null before reading through it
  (`number_index`'s `haslist`/`measured`). String constants are `{tag, heap,
  ptr}` (+4 bytes on wasm).
- `code-native`: `CodeValue`'s payload is private; modules read `number()`,
  `str_ptr()`, `boolean()`, `len()`, `items()`, `keys()`. `native.rs` builds
  the interpreter's marshaled lists with a header and a `CodeList` head.
- Every module, every `.so`/`.a` app and the euglena host must be rebuilt
  with this compiler; an old module is refused by its ABI version.

points 3.86 → 2.66 ms, binary_trees 20.3 → 15.7 ms, dictionary unchanged
(its time is the key search).

## Data types (ticket 123, 2026-10-12)

Owner's model: a type is a set of values; the language says **type**, not
"kind" (`kind-change` is now `type-change`). `type Name = <label>` is a
`Stmt::TypeDef`, recognised only as `type` + capitalised name + `=` at a
statement start (`Parser::at_type_definition`), top level only — `type` is a
field name in hundreds of places. Labels (`ast::Label`) gained `Union`
(`∪`, `Token::Union`), `Value` (`LabelValue`: a written string, number,
boolean — a set of one) and `Open` (`…`, `Token::Ellipsis`); `Fields` stays
closed. `label()` is parts joined by `∪`; `label_part()` is a name, `Array
of`/`Object of` *one part*, an object label, a value, or `( label )`.

`src/types.rs` (`Types`) is the program-wide table (linked files included):
`Particle = { _class ∈ String, … }` predefined; a capitalised name no `type`
declares is the particles of that class (`IsTest::Class`, so `r ∈
Exception` is unchanged). `Types::problem` — a name defined twice, or a type
reaching itself by names and unions alone (`type A = A ∪ String`, which no
check could ever finish) — is refused by `verify_defined`, so both backends
refuse it. `kind_of` gives the one basic type a label admits (a union of
several: none), which is what `diagnostics::static_type_from_annotation` and
`infer` use; `infer` gives each declared type one variable (`type_vars`) so
a recursive type terminates, and a union of several types is `Any`.

**A label that names `_class` needs the value to have one** — otherwise a
field left out is null, and null fits, so `{ x = 1 }` would have been a
`Particle`. That one exception is in all three: `label_mismatch`,
`Types::holds`, and `runtime.c`'s `type_holds`.

**`∈` takes a label at run time** (`IsTest::Label`, or `Class` naming a
declared type). The interpreter asks `Types::contains`; codegen emits the
type as a graph of constant `CodeTypeNode`s (`type_node`; a declared type is
its own node, made before its parts, so `Element`'s children point back at
it) and calls `code_is_type`. **Null at run time** is in `Null` and `Any`
only (as `x ∈ String` always answered); a *part* that is null fits. The node
struct is laid out by both LLVM and C (`{i32, i32, ptr, ptr, ptr, double}`),
which `wasm_asks_types_with_the_same_answers` checks where pointers are 4
bytes. `tests/types.code` holds the cases.

## A type change is an error (ticket 122 A.4, 2026-10-12)

Owner's rule: a value's type comes from its first value; one that will
hold another type later says so where it starts, with a label. `[]` and
`null` decide nothing — what first joins or is assigned does. So
`infer.rs` gained `Shape::Mixed(kinds)`: a list literal with several types
(`Infer::mixed` while its elements meet), a list written and joined in one
expression (`[name] + controls`, `"x" + [1, 2]` — `binary` sets `mixed`
when the left side is a list literal, or the right is and the left is not
a list), and an object literal's computed-key values. A later value of a
type in the set joins quietly; another type is a change. `xs + [x]` with
`xs` a list is `xs` growing, never mixed.

Also fixed: `list + "text"` appends at run time (`apply_binop`), but the
string rule came first in `binary`, so the result was a String — 13 false
alarms in console and ide. And a statement with no span (`assert`) no
longer borrows the previous statement's location.

`type-change` is `Severity::Error`, and `verify_defined` refuses the first
one (rendered with `span::render`), so `run` and `build` both refuse. Tests
that change type on purpose are labelled (`∈ Any`, `∈ Number ∪ String`);
`fail_reassignment_changes_type.code` pins the refusal and
`reassignment_labelled.code` the label. Apps: `type Element = { tag ∈
String, on ∈ Object of (String ∪ Particle), children ∈ Array of (String ∪
Element), … }` in each web app on my-euglena-apps `typed-labels`; console
and ide on `typed-labels`; euglena's diagram gene on `typed-diagram`. All of
those need this compiler, so they merge with its release — and the diagram
gene's tag after it (gems-web pins 0.4.3 until then).

## Wasm handler stack (root-caused 2026-09-07)

Lane A: `src/lib.rs::link_wasm`, with regressions in `tests/build_targets.rs`
and `tests/guest_module.rs`.

The auth-web crashes in `memset` were exhaustion of the **linear-memory
stack**, not binary size or a private dispatch stack in the shim. The linker
was left at its 64 KiB default. Generated handler temporaries live in stack
frames; nested `emit ... to this` keeps every caller's frame live. Large
handlers and deep chains therefore consume the same limited resource.

An isolated chain with a 32-number array in each handler passed at depths
1, 8, 10 and 11, then trapped in `memset` at depth 12. Disassembly showed a
5,696-byte frame for a non-leaf handler. Reserving **1 MiB** explicitly makes
the same probe pass through 128 handlers. `--stack-first` also makes the
layout explicit: the downward-growing stack sits below static data.

The permanent regression retains arrays across 32 nested calls, checks their
contents and the returned particle, and repeats the calls after `main` has
returned, in both debug and optimized wasm builds. Restoring 64 KiB was
confirmed to make it fail with `memory access out of bounds`.

The real-browser suite was also run with a temporary auth-web variant adding
one extra `emit` around `AccountPage`: **10/14 passed with 64 KiB, 14/14
with 1 MiB**. The failing cases included reload crashes. The application
source was not changed by this experiment.

The separately reported shape — a network callback reading `timer.Delay`'s
answer and continuing — has its own test using both real module archives
and browser halves. It passes; this small shape alone has not been shown to
be an independent defect. Missing host module halves remain a separate bug
class (see the hosting section above).

**This is still a finite stack.** 1 MiB gives ordinary application handlers
room; it does not promise arbitrary nesting or arbitrarily large handlers.
Rebuild wasm artifacts with the updated compiler to get the new reservation.
The app's existing backend-formatting workarounds have not been removed.

---

## A warm run without the driver (ticket 124, unreleased)

`src/run_inputs.rs` records loader inputs and resolution candidates, including
absent locks/files, then validates them before publishing a build.
`src/run_cache.rs` is the shared, parsing-free validator: size/time plus Unix
inode/ctime, canonical paths, compiler identity/version, target/tier, and module
resolution environment. `.inputs` records sit beside the run executable;
`run-index/` points at them. Missing/stale records or trimmed artifacts take the
full load/parse/build path. The full build key includes the input snapshot too,
so a changed lock cannot fall back to an old AST-only cache hit.

Unix `run_built` execs the program. The detached -O2 worker starts before exec;
non-Unix retains spawn-and-wait. `tests/run_builds_first.rs` covers invalidation,
exec/PID/streams/signals, fallback, and background optimisation.

The LLVM-linked full CLI costs about 28 MiB even for `--version`. Therefore the
release ships an independent `code-launcher` crate as `code`, with the original
full CLI renamed `code-driver` beside it. Both use the same cache validator;
the frontend has no LLVM or language-crate dependency. Keep the two release
files together. In the checkout, the binaries are `target/{debug,release}/code`
(full CLI) and `code-launcher` (fast frontend). Existing test/embedding paths
keep working; benchmark the frontend to measure the shipped warm path.
`crates/code-launcher/tests/cli.rs` exercises the installed two-file layout.
The release workflow includes both; no tag or release was made for this work.

## Utility modules (ticket 127, unreleased)

Numeric helpers belong in existing `math`, not core: rounding, floored Div,
divisor-sign Mod, Abs/Min/Max, strict decimal Parse and Sqrt/Pow/Sin/Cos/Ln/Exp.
`Double`/`Sum` remain; invalid/non-finite operands or results return Exception.
Round ties away from zero. Read `crates/modules/math/README.md` for contracts.

`math` and `strings` now have browser halves. Strings adds Unicode-scalar
Length; successful ASCII case/trim and single-scalar separators stay compatible.
New stateless `time` provides Timestamp (whole Unix seconds) and TimezoneOffset
(minutes east of UTC). Keep `timer` separate: Delay/Cancel own pending work.
At that stage core Timestamp/TimezoneOffset/Length remained compatible;
At that stage no handler was removed and no arrays module existed.
Ticket 130 below now removes Length. The generated wasm
host now floors its core timestamp too, correcting its former fractional-
second disagreement with native/interpreter and the documented contract.

These additions are on main/source, not in released module assets yet. Do not
claim that installing the current public pins supplies them. New apps can use
the shared helpers once their chosen module build carries them; browser shells
must link the same modules their guests call. App migration and live deployment
are separate work; fixture/browser parity is not a release. Keep future calendar
and text expansion work under ticket 099 rather than adding generic utilities
to core.


## Text utility continuation (ticket 128, unreleased)

Strings now also has Contains/StartsWith/EndsWith/IndexOf (value, search,
optional Bool ignore_case with ASCII folding), literal case-sensitive Replace
(value, search, replacement), scalar-indexed Slice (start/end optional signed
safe integers, end exclusive, negatives from end, clamped), and Pad (width,
optional fill/side, default space/left, width 0..1000000). IndexOf returns -1
when absent. Search accepts empty needles; Replace refuses them. Optional
fields default only when absent; explicit null is an error.

Split accepts any nonempty string separator; Join any string, including empty.
This intentionally broadens their old refusal contract; successful single-scalar
behavior and empty segments remain. Read `crates/modules/strings/README.md` for
full contracts. Native/dev/release/compiled-wasm fixtures and page/ABI edge tests
cover the changes. Tests run on generated hosts in Node, not a live browser UI.
App helpers, shell manifests and released pins have not been migrated. Keep
these source additions separate from claims about public module availability.


## Array utilities (ticket 129, unreleased)

New stateless arrays has Length, structural Contains/IndexOf/Unique, Reverse,
clamped Slice (same start/end rules as strings), stable Sort (homogeneous finite
numbers or Unicode-scalar string keys, optional by/descending), and numeric
Sum/Min/Max. Input is value; search item defaults to null like Code field
access. Empty Sum is 0, empty Min/Max null. The initial core Length compatibility
was superseded by ticket 130 below.
Native uses core values_equal; browser comparison is iterative and object
field-order independent. Read crates/modules/arrays/README.md for contracts,
quadratic Unique cost and existing JSON transport limits. All transformed
arrays preserve Code input snapshots. New install/release/browser wiring is
present, but no public module artifact, shell or application migration follows
from merging source. Tests instantiate real wasm archives/generated hosts in
Node; they do not constitute live browser deployment verification.


## Length leaves core (ticket 130, unreleased breaking change)

Core now handles Timestamp, TimezoneOffset and Linked. Length belongs to
arrays (lists) and strings (Unicode scalar count). Old Length particles to
core return null via the normal unhandled-class rule; there is no hidden
compatibility dispatcher. Index-context length and code_length_of remain
language/runtime primitives, including their optimized paths; ABI stays 2.

Source fixtures/examples, CLI scaffold, wasm smoke and consumers in Euglena/
apps are migrated. Trace fixtures observe container contents directly so
measuring a trace does not add module interactions; core-boundary tests use
Linked. The standalone string_build workload checks exact last/first-absent
indices instead of importing a module; historical timings predate that check.
Shared utility tests can bundle multiple real archives/generated host halves.
Old release assets still carry their old API: this work does not publish a tag.

## Releases

Cutting a release is the one act that is **not** pre-authorised — a tag
publishes to package registries and cannot be taken back. Say it out loud first.

A tag triggers `release.yml`, `publish-modules.yml`, `publish-crates-native.yml`
(crates.io) and `publish-npm-wasm.yml`. `one_version.rs` forces every manifest
(every module crate + code-wasm + code-native + npm + `editor/vscode/package.json`)
to the same version. **Bump with `scripts/bump-version.py X.Y.Z`** — about a
hundred files, our own `Cargo.lock` entries included and third-party crates
that share the number left alone; it refuses a major without `--major` and
runs `one_version` after.

**Every module is released at every tag, but only changed ones are compiled.**
`publish-modules.yml`'s `plan` job fingerprints each module
(`scripts/module-hashes.sh`: its files, `code-native`, and the compiler for
`interpreter`/`syntax`, version number blanked) and compares with the
`module-hashes.json` attached to the previous release. Unchanged modules are
downloaded from that release; changed ones are built. Every module — reused
or not — is still dogfooded against the new `code` and gets a fresh
`module.json`. A manual run with `rebuild_all` builds everything; so does a
previous release with no `module-hashes.json`.

**The local build is ahead of the last release carrying module assets.** To
exercise `code install` locally against real assets, point at a release that has
them via `CODE_MODULE_RELEASE`. `guest` in particular is unreleased — apps pin
it with `"source": "local"` in `.code/lock.json`, and `euglena install` will
report it as uninstallable while leaving that entry alone. That is expected.

---

## How we work here

- **Finish, commit, push.** Standing authorisation from the owner — do not ask
  each time. Releases are the exception (above).
- Verify the whole workspace before committing, not just the root package.
- Commit messages here are prose, lowercase-led, and say *why*, in the voice of
  the existing log (`git log` is the style guide). Ending trailers are fine.
- Reach for the debugger early. gdb is installed; both gdb and valgrind install
  without root (`apt-get download` + `dpkg-deb -x` into `~/.local`). A single
  backtrace once found in minutes what three theory-driven rewrites had missed.
- **Development crashes are investigated, not an automatic rollback** (owner,
  2026-10-15): keep trying while there is a credible fix; revert if the cause
  cannot be resolved. Never merge a crashing implementation. Record the cause
  and repair, and complete required checks. This supersedes earlier ticket
  instructions to abandon an optimization at its first crash.
- **Measure before theorising.** The longest session in this project's history
  was lost to successive plausible theories, each costing a rewrite.

### Two agents at once

Codex and Claude Code may be working these repos simultaneously. To stay out of
each other's way:

- **Announce your lane** in your first commit or in the working notes: which
  repo, which subsystem, which files.
- **Pull before you start and before you push** — all four repos are usually
  clean and pushed, so a conflict means the other agent is mid-flight.
- Prefer many small pushed commits over one long-running dirty tree. A dirty
  tree is invisible to the other agent; a pushed commit is not.
- If you must touch a shared file (`README.md`, `runtime.c`, the lexer), do it
  in its own commit, push immediately, and say so.
- `crates/modules/*` are naturally separable — two agents on two different
  modules will not collide.
