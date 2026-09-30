# A `Trace` handler: what one interaction did, handed to the program

> **Designed 2026-09-30 with the owner.** Stages A, B and C shipped the
> same day; D is open.

## The problem

A program that stays up — a server, a worker on a timer, a host holding
other programs — cannot say what one request actually *did*: which handlers
ran, which modules were asked what, how long each took, which one failed.
`code trace` records boundaries, but it is a test tool: one whole run, no
clock, written at exit. A program that runs for a week needs the same
information per interaction, while it runs, with timings.

## The shape the owner chose

A program that wants to know defines a handler:

```code
Trace { root, started, ms, ok, steps, dropped } =>
    emit Print { value = "$root.class took $ms ms" } to console
```

and the runtime hands it one `Trace` after each interaction has finished.
**No handler, nothing recorded.** There is no flag, no environment variable,
no module to link and no reserved word — a program opts in by answering the
particle, which is the one thing a program already knows how to do. Where the
trace goes (the console, a file, a log service, nowhere unless it was slow)
is the program's own business, written in its own handler.

Rejected on the way, with reasons:

- **The runtime pulls, or a host polls a buffer.** Anything that has to be
  asked in time loses what was not asked for. A finished interaction is
  pushed once, when it is finished.
- **A file named on the command line.** A compiled program is somebody's
  product; nobody should have to remember an argument to run it, and a file
  is only one of the places a trace might want to go.
- **A `trace` module.** A module in between adds nothing the handler cannot
  do in one line.

## What an interaction is

**Everything that happens because of one thing arriving at the top of the
program**: a top-level statement's `emit`, or a particle a module pushed
(a request at a door, a timer's tick, a message from a broker). Those are
the *roots*. Every boundary crossed while a root is running belongs to it.
When the root's handler returns, the interaction is over and its `Trace` is
delivered — at a statement boundary, with nothing else running, so the
`Trace` handler can never re-enter anything.

## The particle

```
Trace {
    root = { from, class, particle, program }
    started        | Unix time in milliseconds, when the root arrived
    ms             | how long the whole interaction took, milliseconds
    ok             | false when the root answered an Exception
    steps = [ { id, parent, target, class, at, ms, ok, program } ]
    dropped        | steps past the cap, counted and not kept
}
```

- `root.from` is `this` for a top-level statement, otherwise the alias of
  the module that pushed it (`net`, `clock`, `mqtt`). The runtime says where
  a root came from and nothing more: *human*, *timer* or *device* is the
  program's vocabulary, not the language's.
- `root.particle` is the root's particle, whole. It is the one value a
  program usually needs to make sense of a trace (which app, which user),
  and the program has already seen it. **Redacting it before it leaves is
  the handler's job** — a token in it is the program's own token.
- `steps` is a flat list in call order. `parent` is the `id` of the step it
  happened inside (`null` for the root, which is step `0`). That is the
  tree; indenting by it draws the call picture, and it maps one-to-one onto
  OpenTelemetry's spans should anyone want to send it there.
- `target` is `this`, `base` or `module:<alias>` — `code trace`'s spelling.
- `at` is milliseconds from `started`; `ms` is the step's own duration.
- `ok` is false when the step answered an Exception.
- `program` is `null` for the program's own steps, and the path a held
  library was linked from for its steps (stage C).

## Decisions

1. **Names and timings, not contents.** A step records its class, not its
   particle or its answer. Contents would cost a copy on every boundary and
   make every trace a place secrets can leak into. The root's particle is
   the single exception, above.
2. **`core` is not recorded.** `Length`, `Timestamp` and friends are
   everywhere and take no time; they would fill the cap with noise.
3. **`Trace` never traces itself.** While the handler runs, nothing is
   recorded, so writing a trace somewhere never produces another trace.
4. **A cap of 500 steps per interaction.** Past it, steps are counted in
   `dropped` and not kept, so a runaway loop cannot eat the memory.
5. **The runtime keeps nothing after delivery.** Only the interaction that
   is running is held. How long traces live is up to wherever the handler
   sends them.
6. **Held programs report into their host's trace.** A program linked into a
   host runs inside the host's interaction, so its steps are the host's
   steps: one tree, from the host's door down to the guest's database call.
   The guest needs no handler of its own. Run standalone, the same program
   gets its own `Trace` if it defines one.
7. **Cost.** No `Trace` handler: an interpreted program pays one branch per
   `emit`, a compiled executable has the recording left out entirely. A
   `--target shared` library cannot know whether its host will want its
   steps, so it carries the recording behind one flag its host switches on.

## Stages

- **A — interpreter. Shipped.** `code run`: roots at top-level emits and inbound
  pushes, steps with timings, cap, delivery, no self-tracing.
- **B — compiled executables. Shipped.** The same in `codegen.rs` and `runtime.c`,
  compiled in only when the program defines `Trace`. The language suite
  proves both modes agree on the shape (timings excepted):
  `tests/trace_handler.code`, `tests/trace_handler_inbound.code`. A step
  whose frame failed before it could close (a failed `emit` skips the
  compiled close) is closed by the next close above it, as not ok, in both
  modes.
- **C — held programs. Shipped.** A shared library records behind its
  host's switch and keeps each finished interaction for the host, which
  collects right after every call into it and every drain of it — never
  later, so nothing waits. Two runtime exports, both optional:
  `code_module_flows()` switches recording on (asked once, at `link`, only
  by a host that defines `Trace`), `code_module_flows_take(out)` hands over
  the finished traces as an array. A library built before this has neither
  and simply stays unrecorded.
  - A call from the host is a root `from` `host` inside the library
    (recorded in `code_dispatch_copied`, against the library's own copy of
    the particle — the host's belongs to the host's allocator; retaining it
    instead was a segfault). The host splices it under the innermost open
    step, which is the emit that made the call.
  - Spliced steps are placed from that step's `at`, not from the two
    `started` times: those are whole milliseconds, and a guest's steps are
    often a fraction of one (the first cut produced negative `at`s).
  - Anything else the library finished — its own timer, its lazy top level
    — is stamped with `program` and queued, then handed to `Trace` the
    moment the host has no interaction running.
  - `tests/trace_hosted.rs` runs an interpreted and a compiled host over the
    same guest, leak check on.

  **Known limitation.** While a guest runs, it may call a module its host
  furnished (the membrane). The host's handler for that runs inside the same
  interaction and its steps are recorded — but under the host's call into
  the guest, as siblings of the guest's steps, not under the guest step
  that reached the membrane. The host cannot know that step until the guest
  returns. Timings and contents are right; only that one parent link is
  coarser than it could be.
- **D — browser (`--target wasm`).** Not planned yet.

## Relation to `code trace`

`code trace` stays what it is: a deterministic, replayable record of a
whole run for tests. The two share the places they hook in and nothing
else — one is a fixture, the other is a program watching itself.
