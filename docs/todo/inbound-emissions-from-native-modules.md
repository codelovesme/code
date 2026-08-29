# Native modules cannot push particles back into the program

> **Phases A and B shipped 2026-08-25.** A module may export
> `code_module_set_inbound` and push particles; both output modes drain
> after every top-level statement and dispatch each one to the *program's*
> own handlers. `.a` static modules joined the story on 2026-08-28.
>
> **The keep-alive loop shipped 2026-08-29**, in two halves: a loop iteration
> became a statement boundary, and then a module gained the right to push
> from a thread of its own. Waiting is the *module's* job — the runtime does
> not sleep on a program's behalf. **`emit … to base` closed the last open
> piece the same day** — see its section near the end. What remains is the
> two modules/one class collision, accepted as a convention.

Dispatch is strictly request/response: `emit … to <module>` crosses into
native code and waits for one answer. The old implementation added the other
direction — a module could *initiate*, queuing particles that the program
handled later. That is how an event loop works (a `terminal` module pushing
`Key` particles as keys arrive, a server pushing requests), and without it
modules can only answer, never speak.

Old shape:

- `EmitQueue = Arc<Mutex<VecDeque<EmittedValue>>>`, one per loaded module
  (`old/src/native_module.rs:150`).
- Compiled programs got a C bridge: `__native_bridge_poll_emission(out,
  out_class)` drained one queued particle,
  `__native_bridge_is_keep_alive()` told the generated loop whether to keep
  polling (`old/src/runtime_native.c:265,279`; codegen declarations
  `old/src/codegen.rs:1147–1153`).
- The interpreter drained queues after every top-level statement and then
  spun a keep-alive loop: `while self.keep_alive { drain; sleep(10ms) }`
  (`old/src/interpreter.rs:171–175`), with public `drain_emissions()` /
  `set_keep_alive()` for hosts running their own loop (the cells organelle).
- Routing came from each module's declared emissions — `(class_name,
  target)` pairs read at load time (`old/src/native_module.rs:432–440`) —
  telling the drainer which handler a queued particle belonged to.

## Design for the new architecture

The new loader already records what each linked module exports (its
handlers), so the routing table falls out for free: **a queued particle is
routed to `<module_alias>.<its _class>`** — i.e. dispatched exactly like
`emit p to <alias>`. No separate emission-declaration ceremony needed; the
export list *is* the declaration. Anything whose class the module doesn't
export is a runtime error (loud, greppable).

### Phase A — interpreted mode

1. **ABI** (`src/runtime.c` + `src/code_abi.h`):
   `void code_emit_inbound(void *queue, const CodeValue *value)` — the
   module's C code pushes a value onto its queue. Queue is an opaque
   pointer handed to the module at link/load time. Vendor-sync the pair
   into `crates/code-native/vendor/` afterwards (lockstep rule).
2. **Loader** (`src/loader.rs`): each loaded module gets
   `Arc<Mutex<VecDeque<Value>>>`; the pointer is passed across at load.
3. **Interpreter**: after each top-level statement, drain every queue and
   dispatch each particle to its module's handler (reuse the existing
   `EmitTarget::Module` path). Expose `pub fn drain(&mut self)` and
   `pub fn set_keep_alive(bool)` on the library API for hosts that run
   their own loop, plus an optional keep-alive spin in `run` behind a
   flag — decide the flag shape when the first consumer (cells-style host)
   lands; the plumbing is identical either way.

### Phase B — compiled mode

`runtime.c` grows a bounded ring per module (fixed capacity, e.g. 256;
overflow drops oldest — a runaway module must not grow memory without
bound) with `code_emit_inbound` pushing and
`int code_poll_inbound(void *queue, CodeValue *out)` popping. Codegen emits
a tail loop in `main`: poll every registered queue, and for a hit call the
module's handler function for that class (codegen knows the mapping — it
already declares each module's handler functions up front, see the comment
at `src/codegen.rs:1210`). Keep-alive: the loop simply never exits while
any module was loaded with inbound capability; document that `code build`
output for such programs is a daemon, and provide the escape hatch (e.g. a
module-exported `Stop` class that breaks the loop) when the first real
consumer needs one rather than inventing one now.

### Phase C — fixtures

A tiny C test module exporting a handler plus emitting one particle at
load: `inbound_basic.code` (program handles the pushed particle, asserts on
it), dual-mode. Overflow/stress fixture once the ring lands. Node-side
coverage joins `tests/build_targets.rs` for the wasm target if inbound
ever applies there (it shouldn't — wasm modules are sandboxed libraries,
note that explicitly when it comes up).

## What was built, and where it differs from the plan above

**Unhandled pushes are dropped** (changed 2026-08-28, reversing the
original "loud rather than silently dropped"). The first rule was written
when the only pusher was `test_events` sending `Tick` — an *event*, which a
program that asked for it would always handle. `net`'s `Log`/`Exception` are
the opposite kind of traffic: the module speaks on its own initiative, so a
diagnostic nobody asked to hear is not a mistake by the program. Keeping it
fatal would have meant every program linking a module that *might* report
something had to handle it. `emit ... to this` was untouched at the time — the reasoning
being that the program addressing itself and finding no handler was still a
bug. That half was reversed later the same day (phase 1 of
`errors-as-particles.md`): all three emit targets now answer null for a class
nothing handles, so the outbound and inbound directions agree rather than
contrast. Cost accepted at the time of the decision: a module pushing a mistyped
class is now invisible rather than loud, and no test can catch that for you.
`tests/fail_inbound_unhandled.code` became
`tests/inbound_unhandled_dropped.code`, asserting the opposite.

Mechanically, one function serves both callers: `_code_dispatch_this` gained
a third parameter saying whether an unmatched class should error or return,
and the interpreter checks `env.handlers` before dispatching a drained
particle.

**Routing changed.** This document predates user-defined handlers, so it
routed a queued particle back to `<module_alias>.<its _class>` — into the
module that pushed it, the only place handling could live at the time. Now
that a program can write `Tick { value } => { … }`, queued particles go to
the *program's* handlers instead (`EmitTarget::This`). That is what an event
loop actually wants: the module supplies events, the program decides what
they mean. A class the program has no handler for is a runtime error, the
same answer `emit … to this` gives.

**The queue is handed over, not called into.** `code_module_set_inbound(void
*queue, CodeEmitFn emit)` is an optional module export; the host calls it at
link time with an opaque queue and *its own* pusher. The function pointer is
load-bearing: a `.so` carries its own copy of `runtime.c`, so a module
calling `code_emit_inbound` directly would push onto its own queue, which the
host never reads.

**Bounded, dropping oldest**, at `CODE_INBOUND_CAPACITY` (256) — mirrored as
a Rust constant in `native.rs` under the same lockstep rule as
`VALUE_SIZE`/`CODE_VALUE_SLOT_SIZE`, because both runtimes must lose exactly
the same particles under overload or a fixture would assert differently per
mode. `tests/inbound_overflow_drops_oldest.code` pins that down; catching it
is what turned the interpreter's originally-unbounded `VecDeque` into a
capped one.

**Draining is between top-level statements only**, in both modes — the
deterministic, testable half of the split this section originally proposed.
The compiled side gets one generated `_code_drain_inbound` called after each
top-level statement, which polls every module and loops until a full pass
finds nothing, so a handler that causes further pushes is still serviced
before the function returns.

## A push can be answered (2026-08-29)

The drain used to throw the handler's return value away. There was nowhere to
send it: a push arrives from the module's own thread, so there is no call site
and no `get` to bind it to. That made every pushed particle an announcement —
fine for `Log`, useless for a module that has a *question*, which is what an
HTTP server pushing a request has.

**What shipped:** one optional export, `code_module_inbound_reply(const
CodeValue *particle, const CodeValue *result)`. After dispatching a pushed
particle, the host calls it with the particle and whatever the handler
returned — null when nothing handled it. Additive, so `CODE_ABI_VERSION`
stays at 1 and the four already-published modules keep loading: a module that
does not export it hears nothing, exactly as before.

Deliberately *not* an id-and-callback pair. `code_emit_inbound` could have
returned a token for the host to hand back, but that changes `CodeEmitFn`'s
signature, which is an ABI break for every module built against the old
header — and hours after publishing four of them. Handing the particle back
costs nothing (it is a pointer the host already holds) and lets a module that
needs correlation carry its own key in what it pushed. `http_server` needs
none: its accept loop is serial, so there is exactly one request outstanding
when an answer arrives.

Where it landed: `runtime.c` (a `reply` pointer per `NativeHandle`, plus
`code_native_reply`), `native.rs` (`NativeModule::reply`), both drains, and
`code-native`'s `declare_inbound_reply!`. A `.a` is called directly by its
prefixed name, the same split its dispatch already has, which `loader.rs`
detects with the `nm` pass it was already doing.

Two things this cost, both worth keeping:

- **A zeroed slot is not null.** `CODE_NUMBER` is tag 0, so a program with no
  handlers at all handed every module the number zero as its answer. The
  compiled drain nulls `result` before dispatching now. Found by an
  `http_server` test expecting a 404 and getting 200.
- **A module can answer itself into a loop.** `http_server` logged a warning
  when an answer arrived with no request waiting — and a `Log` is a push,
  which gets an answer, which found no request waiting, which logged. The fix
  is one line (only replies to `Request` count), but the shape is worth
  naming: with answers, a module's own diagnostics are on the same channel as
  its questions.

Covered by `inbound_answer_reaches_the_module.code` (`.so`),
`buildonly_native_link_static_answer.code` (`.a`), and
`tests/http_server_module.rs`, which drives a real client against a program
in both output modes.

## What is still open

- **The keep-alive loop.** Two halves, and the first one shipped 2026-08-29.

  **How a program says "keep me up" is decided: `loop { }`.** Owner's call,
  from three options — a bare loop, a core `Wait` handler, or the module
  telling the host. The bare loop wins on adding no concept at all: it is
  already in the language, already means "until `break`", and a program that
  wants to stay up writes the thing that stays.

  **Shipped: a loop iteration is a statement boundary.** Queued particles had
  been handed over between *top-level statements* only, which was enough while
  a program was a straight line — and left everything a module pushed inside
  `loop { … }` sitting in the queue until the loop ended. So the one shape an
  event loop has to take was the one shape that did not work. Measured before
  fixing: `[1, 2, -1, -1]` where the ticks should have been `[1, -1, 2, -1]`.

  The drain stops at a handler's edge, in both backends
  (`interpreter.rs`'s `env.active`, codegen's `handler_frame`). A drain inside
  a handler's own loop would dispatch a particle *into* a handler while one is
  running, and re-entry is what the language forbids — the queue would quietly
  fill with `Exception`s that nothing looks at, since a pushed particle's
  result is discarded. `inbound_drains_each_loop_iteration.code` and
  `inbound_does_not_drain_inside_a_handler.code`.

  **Shipped: a module with a thread of its own** (2026-08-29). A module may
  now push from a thread the program knows nothing about, which is what a
  real `terminal` reading keys or a server accepting connections needs — and
  what makes an empty `loop { }` wait for something that can actually arrive,
  rather than only serving a *polling* loop
  (`loop { emit Poll {} to src get r … }`). The four items this section
  listed, and what each turned into:

  - **A lock around the ring.** `runtime.c`'s `NativeHandle` gained a
    `pthread_mutex_t`, held for the whole of a push — the ring's three fields
    *and* the deep copy that fills a slot, so a poll can never see a
    half-built value. `native.rs`'s `RefCell` became a `Mutex`. `cc_link`
    passes `-pthread` (a no-op on glibc 2.34+, where the mutex calls moved
    into libc; what makes an older one link).

    `Value` holds `Rc`s and is not `Send`, and nothing asserts that it is:
    the queue reaches the module as a raw pointer across FFI, which the
    compiler never type-checks. What makes it sound is that a queued value is
    never *shared*, only handed over — the pushing thread builds a fresh deep
    copy, the mutex publishes it, the program owns it alone from `take`
    onwards. No two threads ever touch one `Rc`'s count.

  - **`live_blocks` became atomic**, as this section predicted. Relaxed
    ordering: nothing is published through it, and it is read once at exit
    after the pushing thread has been shut out.

  - **`code_release`'s work stack had to become thread-local**, which this
    section did not predict and is the sharper half of the same problem. The
    push path reaches `code_release` — a full ring drops its oldest entry —
    and that walk uses a file-static buffer, deliberately, because the
    runtime was single-threaded. Two threads walking one buffer is heap
    corruption rather than a wrong answer. `__thread` on the two `dead`
    variables, a few KB per thread. `code_values_equal`'s stayed plain
    static: comparison is only reachable from program code.

  - **The sleep: built, then deleted the same day.** `code_idle_wait` (1ms)
    and its interpreter twin shipped exactly as designed here — chosen from
    the *empty body* at compile time, skipped on a round that handed
    something over. It was then removed at the owner's call, and the reason
    is worth keeping: **the runtime does not guess how long a program meant
    to wait.** Rust doesn't (`loop {}` burns a core; waiting is spelled
    `recv`, `park`, `Condvar::wait`, `epoll`), and neither should this.

    Two things settled it. The sleep was solving nothing measurable — the
    spinning loop cost 49 CPU ticks per half second and the sleeping one 0–1,
    but 0–1 was already what a *blocking module* delivered, with no host code
    at all. And the host's version could only be reached by an empty body,
    which made `loop { }` and `loop { x }` mean different things to the
    machine with nothing in the syntax to say so.

    What replaces it needs no language feature, because it already worked
    before any of this was written: a module blocks inside its own
    `code_module_dispatch` and returns when it has something. Measured on a
    throwaway condvar module: `loop { emit Wait { … } to src get w }` handled
    three pushes 200ms apart in 603ms of wall clock and 2ms of CPU, both
    output modes, no host involvement. The module owes two things in return —
    bound the block with a timeout (nothing in the ABI can stop one that
    blocks forever), and expect a backlog (another module's pushes queue
    behind yours, and past 256 the oldest are dropped).

    No check was added for an empty `loop { }`, deliberately: it spins, that
    is a program's own business, and `verify.rs` refusing it would be the
    same guessing in a different costume.

  - **A threaded module to prove it with**:
    `tests/native_modules/test_timer/`, which answers `Start` immediately and
    *then* pushes one `Tick` per millisecond from a spawned thread —
    `inbound_from_a_module_thread.code`, dual-mode. Deliberately a test
    double rather than a shipped `timer` module: what wanted proving is the
    host side, and a real timer module is a distribution question
    (`community-modules.md`), not this one.

  **What the threaded module forced, beyond the four:** a module that can
  speak first is never unloaded. `dlclose` (interpreter) and `free`ing the
  `NativeHandle` (compiled) both pull memory out from under a thread that may
  still be running, so the program would die during its own cleanup, after
  its last statement succeeded. Both sides now leave such a module mapped for
  the life of the process, and `code_native_close` sets a `closed` flag under
  the lock so a late push is dropped instead of allocating into a ring nobody
  will drain — which `CODE_CHECK_LEAKS` would otherwise report as a leak, as
  a flaky failure in whatever test happened to lose the race. There is no
  shutdown call in the ABI to do this politely, on purpose: a module that must
  be asked before the program may exit is a module that can hang it.

  **What went with the sleep.** `tests/idle_loop.rs` measured it by watching
  the process (half a second of wall clock, then `utime + stime` from
  `/proc/<pid>/stat`) because nothing a `.code` fixture can assert
  distinguishes a sleeping `loop { }` from a spinning one. It is deleted
  along with the thing it measured; the numbers it produced are quoted above.
  Checked before deleting: LLVM keeps an empty infinite loop at `-O0` and
  `--release` both — the IR carries no `mustprogress`, so the C rule that
  lets a compiler delete a side-effect-free loop does not apply here.
- **Two modules pushing the same class with different shapes** silently
  mismatch. Examined 2026-08-28 by building it: two modules both pushing
  `Log`, one as `{ source, level, message }` and one as `{ text, ts }`, into
  a program with one `Log` handler. The second one's fields all arrive as
  `null`, the handler runs, and **the program exits 0** — no error in either
  runtime, at any point, and no way to discover the data was lost.

  Note the inconsistency this sits in: two `.code` modules that both
  *define* `Log =>` are a hard error before the program runs (`duplicate
  handler for 'Log': only one handler per class`), while two native modules
  that both *push* `Log` are merged in silence. Definitions collide loudly,
  pushes collide silently — and the push path is the one with less
  information at the collision point.

  **Accepted for now rather than fixed**, and the reasoning is worth
  keeping. Two mechanisms were considered and rejected. Qualifying the
  handler by the pushing alias (`net.Log { … } => { … }`) makes a program's
  handler definitions depend on its own `link` structure, so a handler stops
  being a first-class definition — and a module author cannot know who will
  consume them anyway. A shared particle registry or definition file is a
  type system by another name, in a language whose README lists "no type
  keywords, annotations, or declarations of type" as a decision; it also
  designs for a scale that does not exist, with two common particles and
  four first-party modules.

  What shipped instead is a **convention, not a mechanism**: the "Common
  particles" section of the root README fixes the shape of `Log` and
  `Exception`, makes `source` the module's own data so one handler can serve
  every module without branching, and says that a module whose shape is not
  the common one must not use the common name either. A module that ignores
  that is a buggy module, not a gap in the language. Revisit if the
  vocabulary grows enough to need versioning — then there will be a concrete
  problem to design against.

- ~~**`.a` static modules** have no queue.~~ **Closed 2026-08-28.** It read as
  a decision and was not one: a `.a` links straight into the host binary and
  so has no `dlopen` handle — and the handle was where the queue lived. There
  was nowhere to queue *into*.

  `code_static_open` allocates one: the same `NativeHandle`, with only the
  ring live. The three function pointers stay NULL and are never read —
  dispatch goes direct to `<prefix>_code_module_dispatch`, there is no
  per-module `code_release` (one runtime, the host's), and exported variables
  come through `code_static_vars_object`. `code_native_close` frees it and
  drains what is still queued, unchanged.

  Everything else fell out of machinery that already existed. `loader.rs` ran
  `nm` to find the prefix and detect `<prefix>_code_module_vars`, so
  `has_inbound` is one more `names.contains` — and its presence in the archive
  is the whole signal that a module intends to speak. `gen_drain_body` polls a
  handle and does not care where it came from, so static links stopped being
  filtered out. `declare_inbound!` gained a form taking the export's full
  name, spelled out rather than pasted from a prefix because `macro_rules!`
  cannot concatenate identifiers — and because a static module already writes
  its other three exports that way.

  `tests/native_modules/test_events_static/` +
  `buildonly_native_link_static_inbound.code`, which pushes a second round
  too, so the queue is reusable rather than handed over once at link time.

  The two formats now differ only in how a module is *called*, not in what it
  can say.
- **Rust modules can now push too** — `code-native` gained `CodeEmitFn`,
  `declare_inbound!()` and `emit_inbound()` on 2026-08-28, `net` being the
  first consumer. The export has to be generated by a macro *in the module's
  own crate*: a `#[no_mangle]` symbol defined in a dependency is not
  reliably kept in the final `cdylib`.
- **wasm** needs nothing — `reject_wasm_native_links` refuses a native link
  at compile time, so no queue can exist.

## `emit … to base` (shipped 2026-08-29)

Found 2026-08-25 while porting user-defined handlers. `docs/todo`'s
now-removed `user-defined-handlers.md` described `emit ... to base` as
"handlers visible outside the innermost enclosing handler
(override-and-fallback)". That was a misreading of the old tree.

`old/tests/base_target_single.code` states what it actually meant:
*"dispatch to the module that linked this module"*. A child module emits
`to base` and the **parent module's** handler answers
(`get_handlers_outside_current_scope`, keyed on module scope, not handler
scope) — and `base_target_multi.code` shows one shared child reaching several
different linking parents.

So `base` is an *upward* edge in the module graph, which put it here rather
than with handler mechanics: it is the same "a module needs to talk back to
whoever loaded it" problem this document already covers, for `.code` modules
instead of native ones.

**What shipped:** the old semantics, directly. A linked module's
`emit p to base` crosses up exactly one level of the module graph and is
answered by the *direct* parent's own top-level handlers — never skipping a
level, never falling through to a grandparent. A class the parent does not
handle answers null, the same answer `to this` gives, since sending a
particle is not a demand. The old "several different linking parents" case
falls out for free: each `link` folds the child inline into its own parent,
so every copy of a shared child has exactly one parent to send to.

Where the legality check had to go: the parser never sees a `link` (every
file is lexed and parsed on its own, and the loader builds the import tree
afterwards), so "is this statement inside a linked module?" is unanswerable
at parse time. `verify.rs` asks it once, against the resolved tree, and both
backends share the refusal: `to base` at the top level of the entry file is
an error before anything runs.

The interpreter keeps one handler table per level of the module graph plus a
counter for how many levels deep execution currently is; a `to base` looks
up exactly one level down. Handlers additionally remember the level they
were *defined* at, and restore it around their body: a handler runs whenever
someone emits to it, possibly from inside another handler's body, and its
`to base` must still mean its own parent. Codegen bakes the same fact into
IR — one small dispatch function per level
(`_code_dispatch_base_<depth>`, none where a level defines nothing),
generated next to the handlers, and a handler's body is emitted with its
defining depth restored.

Covered by `emit_to_base_basic.code`, `emit_to_base_three_levels.code`
(each hop lands exactly one level up),
`emit_to_base_unknown_class_is_null.code`,
`emit_to_base_from_a_handler.code`,
`emit_to_base_from_nested_handler.code` (the handler-invoked-from-a-handler
case), `fail_emit_to_base_outside_module.code`, and the five
`modules/base_*.code` files they link.

One cost worth naming: `base` is now a reserved word, like `this` and
`core`. It collided with one existing fixture (`object_merge.code` used it
as a variable name); the rename is the whole migration.
