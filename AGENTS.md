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
package** and silently skips `code-lsp`, `code-native`, `code-wasm`. On
2026-09-04 a green local suite (31 suites) shipped a release whose editor
extension did not compile — a new reserved word was added to the lexer, and the
language server colours tokens with an exhaustive match. `--workspace` runs 35
suites and would have caught it. It cost a hotfix release.

The same reflex generalises: **when a change touches a shared type** (a token,
an AST node, an ABI struct), find every exhaustive match over it, not just the
ones in the crate you are editing.

Also worth knowing:

- `code format --check tests/` is part of green. It had been red for days once
  without anyone looking.
- `.code` fixtures must be `code format`-clean. The canonical empty particle is
  `{}`, not `{ }`.
- `src/runtime.c` and `crates/code-native/vendor/runtime.c` **must stay
  byte-identical** (`tests/native_crate_vendor_sync.rs` enforces it).
- Each `crates/modules/<name>/` is its own standalone workspace (empty
  `[workspace]` table).
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
(2 build matrices + dogfood case + handlers case + `setup` alternation) ·
`tests/run_language_tests.rs` stem list + doc comment ·
`crates/modules/<name>/README.md` · `README.md` (2 spots) ·
`docs/todo/community-modules.md` · `.github/workflows/ci.yml` services if it
needs one · `tests/<name>_module.rs` integration test.

### Language gotchas that have bitten

- `to` is a keyword — use `recipient` as a field name.
- `$FOO` interpolates inside `.code` string literals; escape as `\$FOO`.
- `∈` is the membership operator (`is` was removed 2026-08-29).
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
  `push_token`, and `crates/code-lsp/src/tokens.rs`.
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
  `gap()`, `crates/code-lsp/src/tokens.rs` — plus the VS Code extension's
  `syntaxes/code.tmLanguage.json` and `language-configuration.json`.
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

## Releases

Cutting a release is the one act that is **not** pre-authorised — a tag
publishes to package registries and cannot be taken back. Say it out loud first.

A tag triggers `release.yml`, `publish-modules.yml`, `publish-crates-native.yml`
(crates.io) and `publish-npm-wasm.yml`. `one_version.rs` forces all 32 manifests
(every module crate + code-wasm + code-native + npm + `editor/vscode/package.json`)
to the same version — bump them together.

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
