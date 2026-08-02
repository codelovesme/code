# 21 — Native backend automatic memory management (compile-time-elided refcounting)

- **Priority:** Medium (foundational — a core building block of the language, not a peripheral fix)
- **Type:** Language runtime / codegen — automatic memory management
- **Area:** `src/codegen.rs` (primary), `src/runtime_native.c`, `src/native_module.rs` (ABI boundary), new escape/liveness analysis pass (module TBD)
- **Status:** **Phase 1 complete — zero known leaks.** Landed on
  `t21-native-refcounting` (merged, PR #1) and `t21-phase1-residuals`. Phase 2
  elision passes are the remaining design below.  Supersedes the original
  "codegen never frees" framing of this ticket (the bare-`free` idea and the
  arena approach were both evaluated and rejected — see *Rejected alternatives*).

  **Phase 1 landed (2026-08-02):** headered `code_alloc`, sentinel-static string
  literals, recursive sentinel-aware `code_dup`/`code_drop`, zero-initialised
  value slots (so scope drops are safe on early-return paths), and the
  reads-dup / stores-transfer / consumers-drop discipline wired through
  Identifier/property/index/equality/type-check reads, if/block/loop bodies
  (per-iteration), spread (object + particle, with child dup), array/object
  concat & merge (`+`, with child dup), string concat + interpolation (including
  `__value_to_cstr`'s own transient buffers, made headered so they're
  `code_drop`-able instead of leaking), computed object-literal field names
  (made immortal `strdup` copies, decoupled from the source variable's counted
  lifecycle), module import/link (`compile_import` now drops non-exported
  internal locals, not just skips exported ones), `assert`'s pass-through value,
  emit/handler dispatch (particle + fire-and-forget), core handlers, and the
  native ABI (copy-at-boundary, D2).

  **Verified: 98/98 buildable `.code` fixtures balance to `live=0`** (alloc==free
  via the `CODE_HEAP_REPORT` instrumentation) — **all four residuals from the
  first pass are closed, zero known leaks remain.** Includes array-of-objects
  concat, object-merge with nested heap fields, and a self-referential computed-key
  fixture (no double-free in any). Full `cargo test` + `.code` suite green
  throughout.

  Also fixed in passing (real bugs found while verifying, unrelated to memory):
  `__value_to_cstr` had no parameter for a Boolean's truth value at all (it
  lives in the value struct's dedicated 4th field, not `num`), so every
  `boolean + string` concat/interpolation silently stringified as `"false"`
  regardless of the actual value; and `compile_add`'s number/array dispatch
  checked only the left operand's tag before running number addition, so
  `Number + Array` (e.g. `0 + [1, 2]`) read the array's element-count field as
  a number instead of falling through to array-prepend.

---

## 0. Resolved design decisions (owner, 2026-08-02)

Three forks surfaced when reviewing the approach's downsides; all are now
decided and **not open** for the implementation to revisit:

- **D1 — One uniform strategy across *all* targets, including `exe`.** No
  per-target split, no "exe opts out of refcounting." Rationale: a single memory
  model is far simpler to build, test, and reason about than two, and keeps the
  native backend's behavior identical everywhere. **Accepted trade-off:** for a
  short-lived, allocation-heavy `exe`, refcounting adds `malloc`/`free` churn
  and count traffic that today's leak-until-exit avoids — i.e. this is a
  potential *throughput* cost on the one target with no correctness need. We
  accept that cost for uniformity; the §7 elision passes (especially non-escape
  stack promotion) are expected to claw most of it back. Any dev-time build flag
  (Phase 1) is a temporary de-risking scaffold, **removed before ship** — it is
  not a permanent exe opt-out.
- **D2 — Reference counts are non-atomic (`Rc`-style, not `Arc`), and the one
  cross-thread path is defined as copy-at-boundary.** Native modules can emit
  from background threads via `EmitQueue = Arc<Mutex<VecDeque<EmittedValue>>>`
  (`src/native_module.rs:138`). To keep counts non-atomic (atomic inc/dec is
  markedly more expensive), we pin the invariant: **a value polled from the emit
  queue is copied into a fresh Code-owned heap block at the poll boundary; no
  refcounted block is ever shared across threads.** The native thread never
  touches a Code-side count. This makes non-atomic counts sound. Enforced in the
  poll path (`src/native_module.rs` ~`:516`) — see §8.
- **D3 — Payload immutability is a permanent invariant this design depends on.**
  The share-freely-because-read-only model (`src/runtime.rs:190`) is now a
  *committed* language property, not an implementation detail. Introducing
  in-place mutation / growable buffers / mutable collections later would break
  shared-read safety and require revisiting memory management (COW or similar).
  Accepted: immutability will not be relaxed without re-opening this ticket.

---

## 1. What we are actually building, and why it matters

Code has **two execution backends** for the same source:

- **`code run x.code`** — the tree-walking interpreter (`src/interpreter.rs`,
  `src/environment.rs`). Values are `Rc<Value>` (`src/runtime.rs:191-199`).
  Memory is already fully managed: refcounting via `Rc`, freed deterministically
  when the last reference drops (a scope's `HashMap` dropping on `pop_scope`
  decrements every value only reachable from it). **This path is correct and
  needs no change.** It is the *reference semantics* the native backend must
  reproduce.
- **`code build x.code`** — the LLVM backend (`src/codegen.rs`) emitting native
  `exe`/`shared`/`static` and `wasm`. Objects, arrays, and dynamically-built
  strings are `malloc`'d directly into the generated code (`malloc_fn`,
  `src/codegen.rs:255-256`; ~15 call sites) and **never freed** — confirmed:
  `grep -n '"free"\|free_fn\|\bfree(' src/codegen.rs` is empty. Every allocation
  a compiled program makes lives until the process exits.

**This ticket makes the native backend manage memory automatically, matching
the interpreter's semantics, with (near-)zero runtime overhead.** Memory
management is one of the language's foundational building blocks: it must be
correct and uniform across both backends before the native path can be
considered production-quality, before `shared`/`static` embedding is safe, and
before the WASM playground (T19) runs non-trivial programs in a long-lived host
tab. It is deliberately scoped as a *language-runtime* concern, not a
per-target patch.

### Design goal in one line

> Reproduce the interpreter's `Rc<Value>` lifetime semantics in generated
> native code, but eliminate the reference-count traffic at compile time
> wherever the language's static structure proves it is unnecessary — so the
> common case pays nothing.

---

## 2. Where the leak actually bites (severity by target)

The fix's *value* differs sharply by target, and this must guide sequencing:

| Target | Leak impact today | Why |
|---|---|---|
| `exe` | **Cosmetic.** No user-visible bug. | One `main` run makes a statically-bounded number of allocations (no recursion, no user functions, only `loop … over <N-element array>` — see [[code-language-design-decisions]]), then exits; the OS reclaims everything. Freeing right before exit buys nothing on its own. |
| `shared` (`.so`) / `static` (`.a`) | **Real unbounded leak.** | Meant to be linked into / `dlopen`'d by a host process that **outlives** any single `.code` invocation. Every re-invocation leaks its allocations into the host's address space for the life of the host. |
| `wasm` | **Real, and the playground's problem.** | `__code_dispatch` (`src/codegen.rs:365-367,380`) is a re-entry point the JS host calls **per event after `main` returned**. A long-lived browser tab dispatching many events accumulates every event's transient allocations forever. |

**Consequence for design:** the mechanism cannot be an "at process exit" or
"free the world once" trick — it must reclaim per-logical-lifetime *while the
process keeps running*. That is exactly what refcounting gives and what an
exit-time sweep does not. (This is also why the arena approach failed; §*Rejected
alternatives*.)

---

## 3. The value & memory model the solution operates on (codegen facts)

Any solution must be written against the backend's *actual* representation, so
it is spelled out here. All line refs are `src/codegen.rs` unless noted.

**A Code value is a by-value tagged struct**, not a pointer
(`value_type`, `:237-240`):

```
value_type = { i8 tag, f64 num, i8* ptr, i1 bool }
tags (:19-24): NUMBER=0, STRING=1, BOOLEAN=2, OBJECT=3, NULL=4, ARRAY=5
```

- **Variable slots are stack `alloca`s** in `main`'s entry block
  (`create_entry_alloca`, `:2790-2798`) — always function-scoped, never on the
  heap. The codegen `push_scope`/`pop_scope` (`:2805-2821`) only maintain
  *name → alloca* maps for lexical resolution; **popping a scope frees nothing
  at runtime** and corresponds to no runtime frame.
- **Only the `ptr` field is heap.** It is non-null and owned only for three
  value kinds:
  - **String** (`tag=1`): `ptr` → a C string. **Two sub-cases, and they differ
    critically:** string *literals* are LLVM **global constants**
    (`build_global_string_ptr`, e.g. `:1914`) — **must never be freed**;
    *dynamically built* strings (concat/interpolation, `strcat_buf` `:4330`,
    `cc_mem` `:4596`, `app_mem` `:4659`, `pre_mem` `:4713`) are `malloc`'d and
    **must** be freed.
  - **Object** (`tag=3`): `ptr` → `malloc`'d array of `field_type`
    (`:1899-1904`). `field_type = { i8* name, value_type value }` (`:278-281`).
    Field **names are global constants** (`:1914`, never free); field **values
    are `value_type` by value**, whose own `ptr` may be heap → **recursive
    ownership**. Freeing an object = drop each field value (recursive), then
    free the field-array block. (Also `mod_obj_mem` `:740`, `nmod_obj_mem`
    `:949`, `cobj_mem` `:1837`, `spread_mem` `:1982`, `pspread_mem` `:2194`.)
  - **Array** (`tag=5`): `ptr` → `malloc`'d array of `value_type` (`arr_mem`
    `:3989`; also `linf_mem` `:3078`, `lc_mem` `:3845` for loop yield
    collectors). Elements are `value_type` by value → **recursive ownership**.
    Freeing = drop each element (recursive), then free the block.
  - **Number / Boolean / Null**: entirely inline in the struct, **no heap, no
    drop.**
- **Assignment copies the struct by value**, which copies the `ptr` field →
  **aliasing.** `x = y` (store into an alloca) makes two slots point at one
  heap payload. This is the crux (§4).
- **Payloads are immutable after creation** (`src/runtime.rs:190`: "Values are
  immutable after creation — reassignment creates a new heap value"). A shared
  payload is therefore always safe *read*-sharing; a slot overwrite installs a
  *new* payload and may orphan the old one (a free opportunity), never mutates
  the pointed-to block.

---

## 4. Why naive static `free` is unsound: the aliasing catalogue

Because a struct copy duplicates `ptr`, the same heap block is reachable from
multiple slots. "Free at end of the block where it was created" is therefore
**wrong** — another live slot may still point at it. Every construct that
duplicates a `ptr` is an aliasing source the discipline must account for:

1. **Assign / reassign** — `x = y` copies the struct (`build_store` into an
   alloca). Also the reassign path that stores into an existing slot
   (`HandlerInvokeAssign`, `:550-551`).
2. **`loop var over arr`** — each element is `load`ed into the loop var slot
   (`:3887-3893`); the var now aliases the element's payload still owned by the
   array.
3. **Property / index access** — `obj.field`, `arr[i]` extract a child
   `value_type`, sharing its `ptr` with the parent.
4. **Object/array construction consuming sub-expressions** — a field/element
   expression that is a variable copies that variable's payload into the new
   aggregate (parent now co-owns).
5. **Spread** — `{ ...source, … }` copies `source`'s fields into a new object
   (`spread_mem` `:1982`, `pspread_mem` `:2194`), sharing child payloads.
6. **Handler invoke** — the particle expression is passed into the (inlined)
   handler body; the returned particle is stored into the `get` slot.
7. **Loop `get` / `yield`** — yielded values are copied into the collector
   array (`:3844-3854`), sharing payloads with wherever they came from.
8. **Native ABI boundary** — `value_to_code_value` / `code_value_to_value`
   (`src/native_module.rs:172,244`) move payloads across FFI (§8).

The only sound ways to reclaim in the presence of (1)–(8) are (a) a runtime
count of how many slots point at a block — **reference counting** — or (b)
never freeing individually and dropping a whole region at once — **arena**
(rejected, §*Rejected alternatives*). We take (a), then erase most of its cost
statically (§6–7).

---

## 5. Why *this* language makes automatic management uniquely tractable

Code's deliberate restrictions (see [[code-language-design-decisions]]) remove
the hard parts of automatic memory management that general languages fight:

- **No cycles can ever form.** No user functions/closures, no mutable
  back-references, payloads immutable. A refcount can never be kept alive by a
  cycle → **no cycle collector is ever needed** (the single hardest, most
  invasive part of Perceus/Koka's general implementation simply does not apply
  to us). This is a permanent, structural simplification, worth stating up
  front so nobody ever adds a mark-sweep fallback "just in case."
- **No recursion, no user functions; handlers are inlined** at the emit site
  (`compile_handler_invoke` inlines the body into `main_fn` — `HandlerReturn`,
  break, etc. all `append_basic_block(self.main_fn, …)`, `:589,599`). So the
  whole program (top-level + every emitted handler body) is **one function**
  whose call graph is a fully-inlined DAG. Whole-"function" dataflow/liveness
  analysis is trivially available — there are no interprocedural edges to model.
- **Bounded iteration only** (`loop … over <N-element array>`, no
  `while`/counter/recursion). Total allocation per run is statically bounded;
  the only *repeated* allocation is a loop body over a known-shape array, which
  is exactly where prompt per-iteration reclamation matters and where refcount
  drop-at-last-use shines.
- **Single-assignment-style dataflow + immutable payloads.** The dataflow is
  near-SSA already; combined with immutability, last-use is statically
  determinable for the overwhelming majority of values, so the *move* (transfer
  ownership, no count bump) optimization applies broadly.

These are the same properties that let **Lobster** remove ~95% of refcount ops
at compile time and **Perceus/Koka** compile to C with no GC. Code is a
*stricter* language than either, so the elision ceiling is at least as high.

References:
- Perceus (Koka): <https://www.microsoft.com/en-us/research/publication/perceus-garbage-free-reference-counting-with-reuse/>, paper <https://xnning.github.io/papers/perceus.pdf>
- Lobster memory management: <https://aardappel.github.io/lobster/memory_management.html>

---

## 6. Chosen approach: compile-time-elided reference counting

**Runtime model (the correctness baseline):** every heap-owned payload carries a
reference count. Two primitives, emitted by codegen:

- `dup(v)` — if `v` is heap-owned, increment its count; return `v`. No-op for
  Number/Boolean/Null and for **static payloads** (§6.1).
- `drop(v)` — if `v` is heap-owned, decrement; **at zero**, recursively `drop`
  every child value (object field values, array elements), then `free` the
  block. No-op for inline and static payloads.

This alone (emitted maximally — dup on every duplication in §4, drop on every
slot overwrite and at every lexical scope exit for slots that will not be read
again) is **correct and complete** for all of §4, needs **no cycle handling**,
and reproduces the interpreter's semantics exactly. Everything in §7 is pure
optimization layered on top of this always-correct baseline.

### 6.1 Static vs heap payloads (a real, must-handle detail)

A `String`/`Object` `ptr` can point at an **LLVM global constant** (string
literals, object field-name strings — `:1914`) that must **never** be freed, or
at a `malloc`'d block that must. `dup`/`drop` must distinguish them. Recommended:
a **refcount header word** prepended to every *heap* block, with static blocks
represented by a **sentinel/saturating count** (e.g. `SIZE_MAX`) so `dup`/`drop`
are naturally no-ops on them without a branch on "is this global?" at every site.
(Perceus uses exactly this "static reference count" trick.) The header layout and
the exact "how do we know a literal's block carries the sentinel" mechanic is the
first implementation sub-task — literals are emitted by codegen, so codegen can
allocate them with a leading sentinel header just like it will `malloc` heap
blocks with a leading `1` header.

### 6.2 Per-case ownership discipline (the "which case, which way" spec)

This is the normative table the implementation follows. "Owned" = this site is
responsible for a `drop`; "Borrowed" = it must not drop (someone else owns).

| Construct | Ownership rule |
|---|---|
| **Number / Boolean / Null literal** | Inline, no payload. `dup`/`drop` are no-ops. |
| **String literal, object field-name** | Static payload (sentinel count). Never freed. |
| **Object/array/dynamic-string construction** | Produces a fresh owned payload (count = 1). Sub-expressions that are *consumed* into it transfer ownership (move); if a consumed sub-expression is also read elsewhere later, `dup` it instead (see §7 last-use). |
| **`x = <expr>` into a fresh slot** | Slot takes ownership of the expr's payload (move if expr is a temporary/last-use; else `dup`). |
| **`x = <expr>` overwriting an existing slot** | `drop` the slot's *old* payload first (it may be orphaned), then install the new one (move/`dup` as above). Mirrors interpreter replacing the slot's `Rc`. |
| **Read a variable (`Identifier`)** | If this read is the variable's *last* use on all paths → **move** (no bump; slot becomes dead). Otherwise → **borrow** if the consumer only reads transiently, or **`dup`** if the consumer stores it. §7 decides which. |
| **`obj.field` / `arr[i]`** | Extraction shares a child `ptr`. If the result escapes (stored/returned) → `dup` the child. If only read transiently → borrow. Parent keeps ownership of the aggregate. |
| **Spread `{ ...src, … }`** | New object co-owns `src`'s child payloads → `dup` each carried-over child (unless `src` is itself last-use and can be dismantled — a later reuse optimization, §7). |
| **`loop var over arr`** | The loop var **borrows** each element (array retains ownership); no per-iteration `dup`/`drop` unless the body stores the element somewhere that outlives the iteration (then `dup` at that store). Payloads created *inside* the body and not yielded are `drop`ped at iteration end. |
| **`yield` / loop `get`** | The collector array takes ownership of each yielded value (move if last-use, else `dup`). The finished collector array is owned by the `get` slot. |
| **Handler invoke `emit p to t get r`** | Inlined. The handler body **borrows** the particle argument by default (Perceus-style borrow of parameters); `return <expr>` transfers ownership of the returned particle to the caller's `r` slot. If the body stores the particle beyond its own lifetime, `dup` there. |
| **Scope exit** | `drop` every slot that owns a payload and is not moved-out/returned. Because codegen scopes are compile-time only, this is a set of `drop`s emitted at the lexical block's end on the not-escaping owners. |
| **Program/handler boundary (`main`, `__code_dispatch`) return** | The return value (if any) is moved out to the caller/host and **not** dropped; everything else reachable only within the invocation is dropped before return. This is what makes `.so` re-invocation and per-event `__code_dispatch` reclaim correctly *without* a separate arena (§8). |

---

## 7. The elision passes (turning correct into cheap)

Emitting `dup`/`drop` maximally (§6) is correct but slow. These compile-time
passes remove the vast majority, exploiting §5. Each is *pure optimization* over
the §6 baseline — implementable and shippable independently, each measurable.

1. **Last-use → move.** If a value's read is provably its last use on every
   path (trivial on the inlined single-function DAG with near-SSA dataflow),
   transfer ownership instead of `dup`+later-`drop`. Eliminates the majority of
   pairs outright.
2. **Borrow inference.** A value passed to a consumer that only *reads* it
   (comparisons, type checks, field reads that don't escape, the particle
   argument of an inlined handler that doesn't store it) is **borrowed** — no
   `dup`/`drop` at all. Directly mirrors Perceus borrow inference / Lobster
   ownership specialization.
3. **Non-escaping stack promotion.** An object/array/dynamic-string whose payload
   provably never escapes its lexical scope (never stored into an outer slot,
   returned, yielded, or passed to a store) can be **`alloca`'d instead of
   `malloc`'d** and needs no refcount at all — freed for free at function exit.
   Given how many aggregates are transient temporaries, this is expected to be a
   large win and also lowers peak memory even for `exe`.
4. **Drop specialization / reuse (optional, later — the Perceus "FBIP" tier).**
   When an owned aggregate is dismantled at its last use to build a same-shaped
   one (e.g. spread `{ ...src, extra=… }` where `src` is last-use), reuse
   `src`'s block in place instead of free+malloc. Highest-effort, lowest-priority;
   list as a stretch goal, not required for correctness or the core win.

**Expectation:** after passes 1–3, `exe`/`shared`/`wasm` hot paths carry close to
zero refcount traffic; residual `dup`/`drop` remain only where genuine dynamic
sharing is unprovable — which, given §5, is rare.

---

## 8. Targets, entry boundaries, and the native ABI

- **`exe`.** Correctness comes for free; the win is lower peak memory (pass 3)
  and uniformity. No special-casing — the same discipline runs; process exit
  still backstops anything the passes leave.
- **`shared` / `static`.** The §6.2 "program boundary" rule is the fix: the
  entry function drops everything not returned before it returns, so each
  host invocation is self-cleaning. This is the target that turns the real leak
  into no leak.
- **`wasm` / `__code_dispatch`.** Same boundary rule at the dispatch function:
  each event dispatch drops its transients on return; anything that must persist
  across events (module-global state established during `main`) is reachable
  from the persistent global slots and simply retains a nonzero count — it is
  *not* in a per-dispatch region, so refcounting keeps it alive naturally with
  **no separate global-vs-per-dispatch arena bookkeeping** (this is precisely
  the two-region complication the arena approach would have forced; refcounting
  dissolves it).
- **Native module ABI boundary** (`src/native_module.rs`, `src/runtime_native.c`).
  This is the one place ownership crosses out of codegen's control and needs an
  explicit contract, to be pinned down as part of this ticket:
  - Values **passed to** a native handler: **borrowed** by default (Code side
    retains ownership; the C side copies into its own `CodeValueBacking`
    `src/native_module.rs:219-238` if it needs to retain).
  - Values **returned from** a native handler and particles **polled from the
    emit queue**: **owned** by the Code side → must be `drop`-tracked once
    converted in. **Per D2, the poll path copies the emitted value into a fresh
    Code-owned heap block** (count = 1) rather than adopting a block the native
    thread might still reference — this is what keeps refcounts non-atomic and
    thread-safe. No Code-side count is ever incremented/decremented from a native
    thread.
  - `CODE_ABI_VERSION` (`src/native_module.rs`): decide whether the ownership
    contract needs an ABI-version bump or is purely a Code-side accounting
    change (leaning: Code-side only, since the C ABI structs are unchanged and
    D2's copy-at-poll keeps all counting on the Code side — but verify against
    `code-abi`).

---

## 9. Illustrative examples (NOT implementation — for reviewer intuition)

Annotated source showing where the discipline lands after §7 elision. `‹…›`
marks a compiler-inserted operation; most are elided to nothing.

**(a) Transient temporary — fully stack-promoted, zero refcount:**
```
p is { x = 1, y = 2 }            ‹alloca, no malloc; never escapes›
emit Log{ msg = "hi" } to core   ‹"hi" is a static string, no dup/drop›
                                 ‹p dropped = no-op (stack), at scope end›
```

**(b) Move on last use — no count traffic:**
```
name is "user-" + id             ‹dynamic string, owned, count=1›
greeting is "hello " + name      ‹name is last-used here → MOVE into concat;
                                   no dup, no later drop of name›
```

**(c) Genuine sharing — one real `dup`, one real `drop`:**
```
shared is { role = "admin" }         ‹owned, count=1›
a is shared                          ‹not last use → dup; count=2›
emit Audit{ who = shared } to core   ‹last use of `shared` → move; count stays 2›
                                     ‹scope end: drop a → count=1;
                                      Audit's ref dropped later → free›
```

**(d) Loop borrows elements (no per-iteration alloc churn):**
```
loop item over items {               ‹item BORROWS each element; no dup/drop›
    emit Print{ v = item } to core   ‹borrowed read; still no dup›
}                                    ‹items retains ownership throughout›
```

**(e) Pseudo-IR shape of the baseline primitives (before elision):**
```
; drop(v):  (emitted inline or as a helper)
;   if tag ∈ {OBJECT,ARRAY,STRING} and header != SENTINEL:
;     if --header.count == 0:
;       for each child value c: drop(c)      ; recursive, objects/arrays only
;       free(block)
```

---

## 10. Staging / implementation plan (each phase independently correct)

- **Phase 0 — scoping & docs.** Land this ticket. Add a one-line note in the
  README/roadmap that `exe` output is currently leak-until-exit *by design*
  pending this work, and `shared`/`static`/`wasm` re-entry are known-leaking.
  No code.
- **Phase 1 — refcount runtime + header.** Add the refcount header to every
  `malloc` site in `codegen.rs`; emit the sentinel header for string/field-name
  literals; implement `dup`/`drop` (recursive) as emitted IR or C-bridge
  helpers in `runtime_native.c`. **No elision yet** — dup on every §4 alias,
  drop on every overwrite + scope exit + entry-boundary. Correct but slow.
  May sit behind a **temporary** dev build flag to de-risk during bring-up, but
  per D1 the flag is removed before ship — the final behavior is uniform across
  all targets, `exe` included. Verify with ASan/valgrind: zero leaks, zero
  double-frees on the `.code` suite.
- **Phase 2 — last-use/move + borrow inference (§7.1–7.2).** The near-SSA
  last-use analysis over the inlined single-function DAG; convert dup→move and
  drop-elimination where borrowed. This is where the ~95% removal lands.
- **Phase 3 — non-escaping stack promotion (§7.3).** Escape analysis; malloc→
  alloca for non-escaping aggregates. Lowers peak memory including `exe`.
- **Phase 4 — native ABI ownership contract (§8).** Pin down borrow/own rules
  across the C bridge; adjust `runtime_native.c` and the poll/return paths.
- **Phase 5 (stretch) — reuse/FBIP (§7.4).** Optional in-place reuse.

Phases 1–4 are the ticket; Phase 5 is a follow-up.

---

## 11. Testing & verification

- **Leak/double-free:** run the full `.code` suite (`code test`) and the LLVM
  backend tests under **valgrind** and **ASan** — assert zero leaks and zero
  invalid frees. Add fixtures that specifically stress each §4 aliasing case.
- **Long-lived host:** a harness that `dlopen`s a `.so` build and invokes it in
  a loop N×10⁶, asserting bounded RSS (proves the `shared` fix).
- **Per-event dispatch:** a WASM harness dispatching many events into
  `__code_dispatch`, asserting bounded linear-memory growth, **and** that
  cross-event persistent state survives (proves refcount, not arena, semantics).
- **Semantic parity:** every existing test must produce identical output under
  interpreter and native — memory management must be invisible to program
  behavior.
- **Elision doesn't break correctness:** Phase 2/3 must keep the Phase 1
  ASan/valgrind guarantees green (the passes are optimizations, never
  correctness changes).

---

## 12. Rejected alternatives

- **Bare `free` at end of allocating block.** Unsound — aliasing (§4) means
  another live slot may still point at the block. This was the original naive
  framing of this ticket; discarded.
- **Arena / region per invocation.** Frees the whole region at the entry
  boundary. Rejected because: (a) for `exe` it does nothing the OS doesn't
  already do at exit; (b) it provides **no mid-invocation reclamation**, so a
  bounded-but-large loop holds peak memory to invocation end; (c) for
  `wasm`/`__code_dispatch` it forces an awkward **two-region** split (persistent
  module-global region vs. per-dispatch region) with a manual escape decision at
  that boundary — refcounting handles cross-event persistence for free. Its only
  edge (dead-simple per-dispatch cleanup) is subsumed by §6.2's boundary rule.
  **Note:** arena is rejected only as a *foundational/whole* memory model. A
  per-event transient region as a **complement to** refcounting — for provably
  non-escaping allocations only — remains a legitimate deferred optimization; see
  §15.
- **Tracing / mark-sweep GC.** Overkill and mis-fit: the language **cannot form
  cycles** (§5), so the one thing tracing buys over refcounting (cycle
  collection) is worthless here, while it adds a runtime, pause behavior, and
  root-scanning complexity — the opposite of the deterministic, prompt, no-
  runtime freeing we already have in the interpreter and want in native output.
- **Rust-style compile-time ownership only (no runtime count).** Can't express
  Code's free *aliasing-by-copy* of immutable payloads without either forbidding
  the aliasing (a language change) or a whole-program alias analysis that
  degenerates to refcounting anyway at the unprovable sites. Refcounting-with-
  elision is that analysis, done pragmatically with a correct runtime fallback.

---

## 13. Acceptance criteria

- Native output (`exe`/`shared`/`static`/`wasm`) frees heap allocations such
  that ASan/valgrind report **zero leaks and zero invalid frees** across the
  `.code` suite and the aliasing-stress fixtures.
- A `.so` invoked N×10⁶ from a host harness holds **bounded RSS** (no per-call
  growth).
- `__code_dispatch` driven by many events holds **bounded** WASM memory **and**
  preserves cross-event persistent state.
- Interpreter and native backends produce **identical program output** on every
  test (memory management is behavior-invisible).
- **No cycle collector exists** in the implementation (documented as
  structurally unnecessary).
- Elision passes measurably reduce emitted `dup`/`drop` count (report before/
  after on a representative program) without regressing any correctness check.

---

## 14. Effort & risk

- **Phase 1 (baseline refcounting):** Medium. Touches every `malloc` site in
  `codegen.rs` (~15) + `dup`/`drop` helpers + literal-header emission. Mechanical
  but broad; ASan-gated so regressions are caught hard.
- **Phase 2 (last-use/borrow):** Medium. The analysis is genuinely easier here
  than in a normal compiler (fully-inlined single function, no interprocedural
  edges, near-SSA, no cycles) — the §5 restrictions are the enabling asset.
- **Phase 3 (stack promotion):** Small–Medium. Escape analysis over the same DAG.
- **Phase 4 (ABI contract):** Small, but needs care and possibly a `code-abi`
  review re: whether ownership rules are Code-side-only (likely) or ABI-visible.
- **Phase 5 (reuse/FBIP):** Medium–Large, optional, deferrable indefinitely.
- **Primary risk:** double-free from an under-counted alias (a `dup` the
  discipline misses). Mitigation: Phase 1 emits maximally (over-dup rather than
  under-dup is a leak, not a crash — the *safe* direction), and every later
  elision is proven against the ASan baseline before it can remove an op.

---

## 15. Future optimization — deferred / batched reclamation (profiling-gated, NOT in initial scope)

Once the prompt-free refcount core (Phases 1–4) is correct and measured, one
coherent optimization theme is worth pursuing **if** profiling a real workload
shows a bottleneck. It is a pure add-on: it never changes *what* is freed, only
*when*. **Do not build it before the baseline exists and a profile justifies
it.**

### 15.1 The idea

The language is oriented at **web services / async event processing**, and the
compiled program is one inlined body driven by an event/dispatch/request loop
(`__code_dispatch` per event; the native drain loop for emissions). Each event
is a natural **epoch**. Instead of freeing a block synchronously the instant its
count hits 0 (which triggers the recursive drop cascade right there, in the
handler's hot path), route the zeroed block onto a **to-free list** and reclaim
the list in a **batch at the epoch boundary** — classic *deferred reference
counting* (Deutsch–Bobrow) / epoch-batched reclamation. For provably
non-escaping allocations, go further: bump-allocate them into a **per-event
region** and reset it wholesale at event end (subsuming individual frees) — the
"complement" arena from §12, scoped strictly to non-escaping transients.

### 15.2 Why it fits *this* language especially well

- **Free timing is semantically unobservable in Code.** There are no
  destructors, no RAII side effects, no finalizers — freeing is pure memory
  hygiene with zero effect the program can observe (payloads are immutable, D3).
  In most RC languages deferring free changes observable behavior; here it
  changes nothing, so deferral is semantically free. This is the crux that makes
  the idea sound.
- **The event loop is a built-in, frequent epoch** — a well-defined, cheap
  moment to flush that always recurs in a long-running service.
- It **smooths the one runtime cost we flagged**: the synchronous O(tree) drop
  cascade moves out of the handler hot path (relevant to p99 latency SLOs), and
  batched frees are more cache-friendly than scattered ones.
- It **reconciles the arena/refcount split**: refcount *tracks liveness* (so
  cross-event survivors — anything ref>0 at epoch end — are correctly retained,
  the thing a pure arena cannot do), while batch/region reclamation does the
  actual freeing cheaply. Each mechanism does what it is best at.

### 15.3 The cost, and the knob

Deferral **raises peak memory** — dead blocks linger until the flush, so peak =
`live + dead-since-last-flush`. This partially trades back the very property
refcounting was chosen for, so the deferral window must be bounded:

| Flush at… | Peak memory | Verdict |
|---|---|---|
| Each ref=0 (no deferral) | Lowest | The Phase-1 baseline — always correct |
| **Event / request boundary** | + one event's garbage | **Sweet spot** — bounded, matches the epoch |
| Across many events | Approaches arena bloat | ✗ reintroduces the leak — do not |

- **Safety valve:** for a pathologically long single event (a huge `loop`),
  flush the to-free list when it exceeds a size threshold, so in-event peak stays
  bounded regardless of event length.
- **Target-sensitive:** on a memory-rich server, defer freely to the request
  boundary; on **WASM in a browser tab** (scarce linear memory), keep the
  threshold tight or defer less — peak is the scarce resource there.

### 15.4 Precisions & hard safety conditions

- A block at ref=0 is owned by **no** slot, so scope/region teardown will **not**
  reclaim it automatically — it must be *explicitly routed* to the to-free list
  (or live in a region that resets). "Freed by scope anyway" is only true if we
  wire it that way; it is not free-by-accident.
- The per-event **region** variant carries the same corruption-class risk as
  §12's rejected arena, now at region granularity: escape analysis must be
  **airtight** — no surviving/refcounted object may ever hold a pointer into
  region memory, or the reset is an instant use-after-free in the next event. In
  a 24/7 process that is the worst possible failure.
- **Overlap with §7.3 (stack promotion):** the native call frame of
  `__code_dispatch` already acts as a zero-cost per-event region for
  *bounded* non-escaping allocations. The heap region here earns its keep **only**
  for the *dynamic/unbounded* non-escaping case (a loop building a variable,
  large number of transients that would overflow the stack). Its incremental
  value over stack promotion is therefore narrow — another reason to gate it on
  evidence.

### 15.5 Recommendation

Ship the prompt-free baseline (Phases 1–4) first. Treat 15.1–15.4 as a single
**deferred, profiling-gated** follow-up, tuned to the **event boundary** with a
size safety-valve. It is layered strictly on top of a correct baseline and, by
construction, cannot change program behavior — only free timing, which Code
cannot observe.
