# Read-only loop-local element bindings (262)

Investigate additional compiled native execution work, following 261. Baseline
is source `f18461d` (2.25.0, not the old public 2.24.1 binary). No release,
website update, new syntax, ABI, value layout or interpreter change in this lane.

## Diagnosis and candidate

Local cumulative instruction probes separate construction from 16 queries and
compare a diagnostic direct-field-read source variant. They are complete
programs with different checks/code generation: do not subtract their counts
as exact phase timings or report them as CPU-time percentages. All source and
binary identities are retained. In weighted/nearest, the original local binding
costs many code_copy instructions; scalar threshold already has a guarded path.

Candidate in codegen.rs: a freshly declared, once-assigned `v = values[i]` in an
innermost loop can borrow an array element while the root remains unchanged. Roots must be inferred as Array; this only
selects candidates and does not remove runtime checks.
Only a simple number/known-numeric-name index qualifies. No native links,
runtime links, either tracing mode, handler frame, emits or unknown control flow.
Browser programs with handlers are also excluded: number-format imports can
synchronously fire an event and change the source between statements. Existing
bindings, loop binders, source mutation or read-binding mutation are excluded.

Both loop versions share a pointer cell for generic bailouts; the fast version
keeps its direct array GEP visible so alias tagging can retain the heap proof. A successful guarded fast array read
publishes the borrowed slot. The generic version owns a normal snapshot,
including string-character scratch/missing-index values. A later statement
bailing from fast to generic reads the same cell. A subsequent fast read drops
any owned fallback first; the common loop exit clears it. Escaped values still
copy/retain normally. Source evaluation, bounds/type/shape guards and diagnostic
statement boundaries stay in their original places. No reference is borrowed
from scratch; language-level snapshots and normal fallbacks remain.

## Fixed acceptance protocol — before timing

`run.py` keeps 17 established controls plus all six 261 cases and two protected ASCII/Unicode
character-iteration controls: 25 programs, 50 before/after groups, 15 shuffled
serial launches each (750 samples). Same
fresh hosted VM, one allowed CPU, separately built source compilers with the
same toolchain/flags and distinct target directories. Record exact revisions,
compiler/source/native/wrapper hashes, commands, environment and every sample.
Native compilation is excluded; program startup, construction, queries,
validation and cleanup are included. Physical isolation/frequency isn't assured.
This is a matched source study, not a new public Code/C/Rust ranking.

Keep the candidate only with >=10% elapsed reduction in at least one large
weighted/nearest target. Reject any material regression across all 25 cases:
an increase >=10% AND larger than max(0.2 ms, twice the sum of before/after MADs).
Retain all small regressions and unchanged scalar results. Independent audit
recomputes all statistics, source coverage and artifact identities. No samples
are discarded. The two added string controls guard the scratch-owning fallback
whose source read cannot borrow an array slot. If rejected, leave implementation unmerged and preserve evidence.

Local `profile.py --code /source/compiler --output-dir /new/path` is untimed.
Local `run.py --before /source/before --after /source/after --output-dir /new/path
--smoke-only` validates all 50 outputs without publishing machine timing.
Full workspace, mutation/alias/bailout/scratch regressions in interpreter,
native dev/release and compiled-WASM dev/release, leak/allocator checks and
applicable frozen app/site checks must pass before retaining the change.

## First candidate rejected

[The first hosted trial](results/2026-11-06-first-candidate/README.md) retains all
750 samples, including +46.05% ASCII regression and insufficient large-target
gains. The revised candidate selects inferred Array roots, preserves direct
fast SSA pointers for alias tagging, and clears earlier generic ownership
before establishing new array facts. The fixed 25-case protocol and gates are
unchanged. Browser callback snapshot regression additionally guards against
reentrant mutation from a number-format host import.
