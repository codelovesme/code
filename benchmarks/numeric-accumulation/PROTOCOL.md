# Numeric accumulation study (263) — fixed before elapsed sampling

Lane: Code runtime performance. Baseline source
`3c6cbae0d969596948760ec5df14ff6a3e844f0f` (2.25.0), freshly built in a
separate target directory; neither rejected 262 prototype is active. Candidate
only adds a checked Number+Number case to `code_add_into`, mirrored byte for
byte in code-native's vendor runtime. A valid Number owns no heap block. The
sum reads both payloads before writing the destination, with the same operand
order and IEEE addition as `code_add`. No syntax, ABI/layout, inference,
interpreter, release, website or app deployment changes.

## Diagnosis (local, untimed)

Preserve complete construction/full/direct-read probes from the original
threshold, weighted and nearest programs at size 32768. Inspect Callgrind call
arcs and native disassembly to establish actual helper use. Counts are dynamic
instructions/calls, not elapsed shares. Probes differ in validation and code
generation; subtracting totals is not an exact phase measurement.

## Fixed measurement and gates

Reuse all 25 cases of 262 unchanged: 17 established native controls, all six
compiled-workload cases, ASCII and Unicode character reads. Add five controls:
boxed numeric self/snapshot addition; arithmetic-temporary accumulation;
Number/String and Unicode-character addition; aliased list append; shared
object merge/field accumulation. Exact generated programs and binaries are
retained, including their result assertions.

30 programs x 2 source builds x 15 launches = **900 samples / 60 summaries**.
Prepare/validate every program before timing on a fresh ubuntu-24.04 hosted VM.
Build both compilers with identical flags/toolchains and distinct targetdirs.
Pin all serial launches to one allowed logical CPU; use a fixed shuffle seed
20261106. Include startup, construction, queries, checks and cleanup; exclude
native compilation. Retain every outcome, median, range, MAD and identity.
Reject dirty tracked trees and detected concurrent compilers/tests/profilers.
Host frequency/physical exclusivity is not guaranteed.

Keep only if **threshold_262144 median elapsed falls >=10%**. Reject if *any*
case increases >=10% and its absolute increase exceeds
`max(0.2 ms, 2 * (MAD_before + MAD_after))`. Never alter these gates after
sampling. `gate.py` applies them; `verify.py` independently recomputes all
CSV/JSON summaries and verifies source/native hashes in the artifact bundle.
This is a source before/after study, never a released Code/C/Rust ranking.

## Correctness / decision

Check native runtime directly against `code_add` for finite edge values,
negative zero, infinity, NaN, self-add and scratch operands; assert tags and
heap flags. Check mixed/string/list/object fallback, aliases/snapshots,
evaluation order and failure recovery through interpreter, native dev/release
and compiled WASM dev/release. Run leak/allocator checks, vendor sync, full
workspace, formatting, clippy, and applicable frozen app/site checks.

If gates fail, archive the unapplied patch and all evidence, restore production
sources, and state rejection. No release or comparison row follows from this
experiment. A website update needs separately verified released same-host
Code/C/Rust measurements and the owner's selected presentation.
