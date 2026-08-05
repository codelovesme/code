# Backlog

Findings from the repo audit (native-module ABI, crate layout, docs) plus the
distribution roadmap, one file per ticket.

**Layout:** completed tickets live in [`done/`](done/); everything still
outstanding lives under its priority — [`high/`](high/), [`medium/`](medium/),
`low/` (created once something lands there). A ticket's priority is recorded
inside the file itself (`**Priority:**` line) even after it moves to `done/`,
so nothing is lost in the move — `done/` isn't priority-sorted because, once
finished, priority no longer matters for it. Numbers are stable identifiers
(never reused), not folder-relative — ticket 12 stays "12" whether it's in
`done/` or, hypothetically, moved back.

## Done

| # | Title |
|---|-------|
| [1](done/1-native-module-abi-docs.md) | Fix stale native-module ABI docs (`eug_*` → `code_*`, v1 → v2) |
| [2](done/2-abi-version-single-source.md) | Single source of truth for the ABI contract (`code-abi` crate) |
| [3](done/3-decouple-lsp-from-llvm.md) | Decouple `code-lsp` from LLVM/inkwell (feature-gate) |
| [4](done/4-purge-euglena-naming.md) | Purge leftover `euglena`/`eug` naming |
| [5](done/5-readme-completeness.md) | README completeness and polish |
| [6](done/6-test-suite-fixtures.md) | Restore dangling test fixtures + fix exe output name |
| [7](done/7-readme-semantic-mismatches.md) | README documents semantics the impl lacks (reassignment, `${}`) |
| [8](done/8-llvm-backend-and-test-isolation.md) | `+` link bug, default-target disagreement, llvm test isolation |
| [9](done/9-ast-spans-for-runtime-diagnostics.md) | Located errors via AST spans — `run` + `build`, single- & multi-file (expression-level dropped) |
| [10](done/10-negative-number-literal.md) | Negative number literals (`-5`) don't parse |
| [11](done/11-ditch-function-call-syntax-plan.md) | [PLANNING] Retire `name(args)` call syntax; move built-ins to handlers (decided) |
| [12](done/12-core-handlers-implementation.md) | Implement `to core` handler dispatch; remove `Expression::Call` |
| [13](done/13-release-workflow.md) | Release workflow: GitHub Releases on tag push (Linux x86_64) |
| [14](done/14-install-script.md) | Install script (`curl \| sh`) |
| [15](done/15-publish-code-native-crates-io.md) | Publish `code-native` to crates.io |
| [23](done/23-set-domain-and-possibility-enumeration.md) | First-class Set domain (superseded by T26 — see 26) |
| [25](done/25-partially-resolved-objects.md) | Objects with unresolved (constrained-only) fields (superseded by T26 — see 26) |
| [26](done/26-unified-set-based-semantics.md) | Unified set-based semantics: variables are constraint sets, types = sets, `=`/`∈` = `⊆`, universal `∩`/`∪`, discriminated unions, flow-sensitive narrowing (supersedes T23, T25) |
| [27](done/27-set-op-domain-materialization.md) | `∪`/`∩` materialize an unresolved-but-finite domain (scalar range, Schema, or Union) into a Set instead of demanding resolution first |

`cargo test --workspace` and the `.code` suite are fully green.

## Active — High priority

_(none right now)_

## Active — Medium priority

| # | Title |
|---|-------|
| [16](medium/16-vscode-extension-consolidation-and-publish.md) | Consolidate VS Code extension into this repo; publish to Marketplace |
| [17](medium/17-split-release-artifact-code-lsp.md) | Split release artifacts: `code` Runtime / `code` SDK / `code-lsp` |
| [18](medium/18-wasm-capable-core.md) | WASM-capable core: feature-gate LLVM and native-`.so` out of the default build |
| [20](medium/20-project-website-distribution-channel.md) | Project website: Downloads page, hosted install.sh, playground home |
| [22](medium/22-language-documentation-site.md) | Language documentation site: guide, tutorials, examples, reference (Phase 1 guide done; content, depends on T20) |
| [21](medium/21-native-backend-memory-management.md) | Native backend automatic memory management: compile-time-elided refcounting (Perceus/Lobster-style) |
| [24](medium/24-native-backend-constraint-narrowing.md) | Native backend constraint narrowing: silent-miscompile hole closed (Phase 1 done), full parity undecided (Phase 2) |

## Active — Low priority

| # | Title |
|---|-------|
| [19](low/19-browser-playground.md) | Browser playground: run `.code` in the browser via WASM (Phase 2; depends on T18) |

---

**Distribution roadmap (approved 2026-07-31):** the language had zero
distribution before this — no release binaries, no installer, no published
packages, no docs site, no playground. Tickets 13–16 are Phase 1 (release
binaries, installer, `code-native` on crates.io, VS Code Marketplace) of a
3-phase plan; Phase 3 (multi-platform binaries, package registry) is planned
but not yet ticketed. Tickets 13 (release workflow), 14 (install script), and
15 (`code-native` on crates.io) are done — pushing a `v*` tag produces a
GitHub Release with a standalone Linux x86_64 binary, `curl -sSf
.../install.sh | sh` installs it, and `cargo add code-native` pulls the
native-module authoring SDK straight from
[crates.io](https://crates.io/crates/code-native) — no repo clone needed.
Ticket 16 (VS Code extension) is not started. T16 also records two rejected
alternatives from the 2026-08-01 discussion: a separate repo for the
extension, and a `code lsp` wrapper subcommand — neither made sense at this
project's current scale.

T17 refines Phase 1 packaging: `code`/`code-lsp` split by audience (CLI vs.
editor), plus (absorbed from T18) `code` itself splitting into Runtime/SDK
tiers by capability — three release tarballs total, `code-runtime-*`,
`code-sdk-*`, `code-lsp-*`, all still binary-named `code` where applicable
(the `dotnet` Runtime/SDK model — one command name, capability gated by which
package you installed). T17 also adds `strip = true` to release builds.

Phase 2 (docs site, browser playground) is now ticketed: T18 (WASM-capable
core — the enabling refactor; also has standalone value as an LLVM-free
source build for contributors who don't touch `codegen`), T19 (browser
playground — visible output via a read-only bindings panel, no language
change; scope widened to a first-class npm-published embeddable package,
Pyodide-style, not just our own docs-site's internal implementation detail),
and T20 (the project website itself: Downloads page, hosted `install.sh`,
playground home). T20 explicitly does not remove `install.sh` — that's
gated on a still-undecided, separately-tracked choice of native package
manager (Homebrew/apt/winget/...) and multi-platform (macOS/Windows) support,
noted but deferred in T20.

**Foundational runtime work — T21 (native memory management):** the native
(`code build`) backend currently `malloc`s but never `free`s — a cosmetic
leak-until-exit for `exe`, but a real unbounded leak for `shared`/`static`
(linked into a long-lived host) and for the WASM `__code_dispatch` per-event
re-entry the playground depends on. The interpreter (`code run`) is already
correct via `Rc<Value>`. T21 makes the native backend reproduce those
`Rc` semantics with **compile-time-elided reference counting** (the
Perceus/Koka and Lobster family): a correct `dup`/`drop` refcount baseline,
then compile-time passes (last-use→move, borrow inference, non-escaping
stack promotion) that erase ~most of the count traffic — the common case pays
nothing. Crucially, Code **cannot form reference cycles** (no closures, no
back-references, immutable payloads), so **no cycle collector is ever needed** —
the hardest part of general refcounting doesn't apply. Both the naive
"bare `free`" and an arena/region-per-invocation were evaluated and rejected
(arena does nothing for `exe`, gives no mid-invocation reclamation, and forces
an awkward two-region split at the WASM event boundary that refcounting
dissolves). Phased plan; **Phase 1 (the prompt-free `dup`/`drop` baseline) is
implemented** — headered `code_alloc`, sentinel-static string literals,
recursive sentinel-aware `dup`/`drop`, and the reads-dup/stores-transfer/
consumers-drop discipline. Verified leak-free (alloc==free) on a 13-construct
stress fixture and a broad sweep, full suite green. Remaining Phase-1 polish
(inner-scope drops, non-core emit-particle drops) and the Phase 2 elision passes
are still open.

**T23 (Set domain and possibility enumeration):** a corpus audit while
answering "is Code really constraint-based?" (30 real apps, 15,521 lines)
found progressive domain narrowing is real but never actually used in
practice — everyone just writes `=`. That investigation surfaced two real
bugs (fixed directly on `main`, no ticket: `f1795c9` — a narrowed variable's
`=` pin didn't check prior constraints; `63fe8e3` — `Z/N/R` domains lost
their integer/natural/real distinction, and `∈`/`in` disagreed on them) and
then a longer design conversation about making narrowing into something
worth reaching for: a `⦃…⦄` set literal producing a genuine resolved
`Value::Set` via `=`, `∈` narrowing a scalar to one of a set's elements
(domain-borrowing: `a ∈ A`), and a new `loop a { }` form that enumerates a
scalar's own finite domain in place without resolving it. Interpreter-only
(native codegen never implemented narrowing beyond `Equals`/`IsType` either).
Not started.

**T24 (native backend constraint narrowing):** the owner's "interpreter and
compiler must never be able to diverge" principle, applied — and it found a
real one. `code build` used to silently accept every constraint form beyond
`Equals`/`IsType` (range narrowing, `∈`/`in Z/N/R`, set membership),
producing a binary whose behavior *silently contradicted* `code run` on the
identical source (`a > 3; a < 10; a = 15`: interpreter correctly rejects it
as a contradiction, native used to compile and run it to completion,
ignoring the narrowing entirely). Phase 1 — reject instead of silently
miscompiling — is done (`a59eb95`), two regression tests added. Phase 2 —
whether to actually implement narrowing in the native backend at all — is
open, and is likely to get revisited once T23 gives the feature real users.
Single-assignment/reassignment enforcement was checked and *does* already
match between the two backends — this was specifically the narrowing family.

**T25 (partially-resolved objects):** a natural follow-on question from T23
— can an object have a field that's only constrained (`{ k = 2, L ∈ Z }`),
not yet a concrete value? Verified: no, `Value::Object` requires every field
to already be a resolved `Value`, and the object-literal grammar only
accepts `name = expr` fields. Kept as its own ticket rather than folded into
T23 because it's a deeper mechanism (partial resolution *inside* a
structured value, not just a bare scalar) that touches field access,
structural type-checking, spread/construction, and host-boundary
serialization — none of which T23 needs to answer for scalars alone. Not
started; depends on T23 landing first.

**T26 (unified set-based semantics) — supersedes T23 and T25.** The design
conversation that produced T23/T25 kept going until it reached a single
unifying thesis: there is no "type" vs "value" — a variable *always* holds a
set of possible values (its domain), and a singleton set just reads as a
plain value. From that, everything collapses into set theory: `x = v` is
`domain(x) = {v}`, `x ∈ S` is `domain(x) ⊆ S` (so `=` is the singleton case
of `∈`); a `type` is a named set and construction yields a member (`abc ∈
ABC`, materialized as the `_class` tag — verified today); `∩`/`∪` stop being
type-only syntax and become universal set operators where `∩` *is*
inheritance and `∪` *is* union types. T26 also reverses two T23 leans: set
literals go back to `{ }` (not `⦃…⦄`), and sets become first-class *values*
(forced by nested sets like `{1, 2, {k, lm}}`). Much of this session's
shipped work (domain intersection, singleton=resolved, `=` via intersect,
`Z/N/R` domains, domain-entailed `assert b > 3`, `loop` over a finite
domain) turns out to be this model's primitives already. All three core
decisions (sets-as-values, `{ }` set/object disambiguation, open-vs-closed
schemas) were resolved the same day.

**Phase 1 (value-sets) shipped 2026-08-05**: `{ }` set literals as a genuine
`Value::Set`, uniform `=`/`∈` narrowing, `loop <var> { }` enumerating a
scalar's own finite domain, and universal `∩`/`∪` for set values. Caught and
fixed two real problems along the way: `Domain::intersect()` was missing
`ValueSet` arms entirely (the exact gap T23 had flagged — `x ∈ {1,2}; x = 5`
silently succeeded instead of contradicting), and adding `∩`/`∪` as new
parser precedence tiers blew the interpreter's 16MB parse-stack on *any*
input (fixed by folding them into the existing `*`/`+` tiers instead of
adding new ones — this parser's recursive `expr` is reused too many places
for new wrapping layers to be free). Also had to drop workspace dev builds
to `debug = "line-tables-only"` (`Cargo.toml`) after the pre-fold tiers hit
a separate `rust-lld` debug-info relocation limit linking the LLVM-embedding
`code` binary.

**Phase 2 (object-schemas) shipped 2026-08-05**, simpler than T25 originally
scoped: resolution stays per-*variable* (a new `Value::Schema`/
`Domain::Schema`, exactly parallel to Phase 1's Set), not per-*field* —
`Value::Object` itself is untouched. `mm ∈ K; mm = {...}` resolves or
contradicts by checking the object against K's field domains (open —
Decision 3, extra fields are fine); `∩` on two schemas merges field
constraints, which *is* inheritance. This made all four of T25's open
questions (field access, type-checking, spread, host-boundary) answer
themselves — each already routes through the ordinary "unresolved variable"
error path, no per-field tracking needed. Caught two real bugs doing it:
`Exact ∩ TypeDomain` never actually checked built-in type names (`m ∈
Number; m = "x"` was accepted anywhere, not just in a schema — now fixed
everywhere), and `∈`'s bare-capitalized-name-means-type-name rule from
Phase 1 was swallowing the schema variable itself (`mm ∈ K` parsed as "check
_class == 'K'", never reaching Schema logic) — fixed by checking for a bound
Set/Schema variable before falling back to type-name matching. One
deliberately deferred limitation: `KK = String` (aliasing a builtin type
name) doesn't work — `String` alone isn't yet a first-class value; writing
the schema with the literal name directly does. 6 new tests, full sweep
green (158/158 fixtures).

**Phase 3a (discriminated unions) shipped 2026-08-05.** New `Value::Union`/
`Domain::Union`, exactly parallel to Set (Phase 1) and Schema (Phase 2).
`∪` generalized: `Set ∪ Set` keeps its flat-merge fast path; anything
touching a `Schema` (open/predicate membership, can't collapse to a flat
Set) produces a `Union` instead — `Status = {"Success"} ∪ {tag = "Error",
code ∈ Number}`. `s ∈ Status; s = v` resolves if v satisfies *any*
alternative (each still enforcing its own constraints — a Schema branch's
field types still apply) and contradicts if it matches none. Discriminating
which branch a resolved value took needed no new mechanism — it's just an
ordinary `s ∈ Object` / `s ∈ String` check. Native codegen already rejected
`∪` entirely since Phase 1 (blanket, not operand-specific), so no codegen
work was needed here. 3 new tests, full sweep green (161/161 fixtures).
Flow-sensitive narrowing (narrowing a variable's domain inside an `if`
branch automatically) split off as Phase 3b, below.

**Phase 3b (flow-sensitive narrowing) shipped 2026-08-05 — T26 complete.**
Scope decision (owner): block-scoped only, no memory across separate `if`
statements — full TypeScript-style flow analysis (where a later `if` would
"remember" an earlier one ruled out a branch) was explicitly rejected as a
much bigger, separate mechanism nothing yet needs. `if <var> ∈/∉ TypeName`
on an *unresolved* variable now decides from its domain instead of forcing
resolution, and when genuinely mixed, runs the block with `<var>` shadowed
to the narrowed domain — the same block-scope-shadow pattern `loop <var>
{ }` already used in Phase 1, just triggered by `if` instead of `loop`. A
nice emergent behavior, not a special case: when narrowing collapses to
exactly one value, the shadowed variable is already a resolved singleton
the instant the block is entered (falls out of the existing one-element-
`ValueSet` rule for free). Verified with the full matrix: a value from the
*excluded* alternative still contradicts inside the narrowed block (proves
real narrowing, not a label); negation narrows to the complement; already-
decided conditions skip narrowing; the outer variable is provably untouched
after the block. No codegen work needed (same reason as 3a — the program
already fails to compile earlier, at the union/schema-establishing
statement). 2 new tests, full sweep green (163/163 fixtures). This closes
every phase of T26's plan; moved to `done/` alongside T23 and T25 (both now
superseded by it).

**Design decision on record:** Code has no user-defined functions and no
function value — reusable logic exists only as handlers (particle dispatch).
The README's former "Functions" section documented a feature that never
existed; it has been removed. Ticket 11 decided (full retirement, no
`name(args)` sugar survives): `timestamp`/`length` move to
`emit X to core get result`. Ticket 12 implemented it — `Expression::Call` is
fully removed from the language (`grep -rn 'Expression::Call' src/` is
empty). The `.wasm` ABI's dead function-export slot was investigated too:
shrinking it would be a breaking wire-format change for no gain (see ticket
12's resolution), so it stays as reserved/zeroed padding, honestly relabeled
instead of removed.
