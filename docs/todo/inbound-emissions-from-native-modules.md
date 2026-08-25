# Native modules cannot push particles back into the program

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

## Open question

Whether draining happens *only* between top-level statements (deterministic,
testable) or continuously (needed for interactive daemons). Old did both —
between-statement drain plus keep-alive spin. Recommend keeping exactly
that split; it costs nothing extra once the queue exists.
