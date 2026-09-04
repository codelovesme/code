# Answering a guest's own `link`

**Shipped 2026-09-04.** A program that opens another program while it runs can
now furnish that guest's organelles instead of letting it open its own. This
document is kept because the bug that held it up for two rewrites is worth
never repeating.

## What it is for

`ast::Stmt::LinkRuntime` lets a program open another program and stop it
again, and `code_abi.h` item 9 makes stopping give the memory back. What was
missing was *furnishing*: without it a hosted application binds its own port
and holds its own connections, and the host has no way to know what it took or
to take it back.

Three constraints, in the owner's words:

1. The guest's source does not change. The same file runs alone or hosted.
2. The guest shares the host's organelles rather than opening its own.
3. Stopping the guest reclaims everything, including whatever it was lent.

## The shape

`code_abi.h` item 10: two structs and one function. `code_module_set_host` is
defined in `runtime.c` itself, so every compiled `.so` exports it without
doing anything — which is also how a host tells a module built before this
from one that can be furnished. The opener calls it immediately after opening
and before anything else: a `.code` library runs its top level lazily and its
own `link`s run with it, so installing the host afterwards is too late for
exactly the statements this intercepts.

`code_native_open` then asks the host before it asks the filesystem, and a
`NativeHandle` gains `from_host` plus a copy of the supplied `CodeHostModule`;
dispatch, exported values and `serving` all route through it.

**The host's answers are its own handlers**, which is the point: nothing in
the runtime knows what an organelle is for. A guest's `link` becomes
`Offer { app, name }` and each `emit` to a stand-in becomes
`Organelle { app, name, particle }`, both asked of the hosting program's
dispatch chain — `code_set_program_dispatch` hands that chain to `runtime.c`
at startup, since only codegen knows its name. `app` is the path the host
linked the guest from; `name` is the organelle's stem, so a host handler reads
`if name = "net_server"` rather than matching whatever spelling was baked into
the guest. `native.rs` mirrors all of it for `code run`.

Three rules that are not obvious and are load-bearing:

- **A refusal is not a failure to resolve.** The ABI lets a host answer "I do
  not offer that", and the guest's `link` then fails — but a guest's top-level
  `link` failing ends the guest, and a fatal error inside a module ends the
  process it was loaded into. A host would be killed by its own policy, by a
  guest it deliberately said no to. So a refused organelle is handed over *as
  an organelle that refuses*: the guest links it and every particle it sends
  gets an `Exception`, which is the language's rule everywhere else.
- **The particle a guest sends is deep-copied**, not `code_copy`d, on the way
  into the host's handlers. It was built by the guest's own copy of the
  runtime with its own refcounts. Using the wrong one segfaults rather than
  failing politely.
- **Handles crossing the ABI are rows, never addresses.** Both hosting tables
  are appended to and emptied rather than removed, and `ctx` travels as
  `row + 1`. A guest outlives individual decisions the host makes about it; a
  stale handle then names an empty row and answers so.

## The bug that cost two rewrites

The symptom: a guest opened *after* another had been stopped inherited the
stopped one's world. It presented as a segfault at first, then — once handles
became rows — as a wrong answer. It moved when unrelated things moved: the
*source filenames* of the two guests changed whether it appeared, because they
change the location strings embedded in each `.so`.

Everything plausible was ruled out by measurement and none of it was the
cause: lifetimes (rows removed the crash but not the wrong answer), load
address reuse (skipping `dlclose` changed nothing), symbol interposition
(hiding every internal changed nothing — though it was worth doing and is
kept), and the host's own bookkeeping (correct at every step). Under
AddressSanitizer the whole scenario ran clean apart from one leaked handle.

The cause was three lines away from where anyone was looking. `NativeHandle`
has three construction paths. `from_host` was added to the struct and
initialised on two of them. On the third — the ordinary `dlopen` path — the
handle was `malloc`'d and every field assigned individually, so the new one
kept whatever the last freed handle had left in that byte. When the allocator
handed back a dirty block, a freshly opened module read as "supplied by a
host" and the program dispatched into a stand-in instead of into the module.
The module was never entered at all, which is why its initialisation appeared
never to run.

A backtrace found it in one step, once there was a debugger to take one:
`hosted_dispatch` called directly from the *host's* `code_native_dispatch`,
with no guest frame in between. Everything before that was reasoning without
data.

Two things follow, and both are now in the code:

- Every `NativeHandle` construction path starts with a whole-struct zero.
  Assigning every field is a promise that a later field will break; starting
  from zero cannot be forgotten.
- `--target shared` no longer exports its internals. Only `.a` had been
  hiding them, so every `.code` library was handing out its initialisation
  guard, its handler chain and one symbol per top-level slot — all generated
  identically in every library. It was not this bug, but two libraries loaded
  at once is exactly what a hosting program *is*.

## What is covered

`tests/hosted_app.rs`, in both output modes: a guest answering the same hosted
or alone, reaching the host's stand-in rather than its own module, a host
refusing without dying, per-guest answers, a guest started after another was
stopped, two guests held and stopped independently, memory reclaimed on stop,
and a guest still linked at exit released anyway.
`tests/library_targets.rs` covers the export surface and two libraries staying
independent.
