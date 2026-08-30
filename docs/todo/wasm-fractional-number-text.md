# Rendering a fractional number as text is unsupported on wasm

> **Shipped 2026-08-29 — by neither route below.** What follows is the
> original write-up, kept because its measurements are what ruled both routes
> out. "What shipped" at the foot records the third one.

String interpolation (`"$x"`) has to turn a number back into characters, and
that spelling must be byte-identical across every output mode or the same
program asserts differently depending on how it was built. Two of the three
modes manage it:

- the interpreter, through Rust's `Display` for `f64`;
- `code build` on a native target, through `runtime.c`'s `text_push_number`,
  which reproduces Rust's rule — the shortest decimal that reads back as the
  same double, laid out positionally — and is checked against Rust's own
  output over 205k values (`tests/interp_number_text.code` pins the
  interesting ones as a cross-mode fixture).

`code build --target wasm` is the exception. `src/wasm_shim.h` is a
freestanding stand-in for libc: its `snprintf` understands `%s`, `%u` and
`%lld` and nothing else, and there is no `strtod`. Both halves of the native
algorithm — ask printf for the exact decimal expansion, check a candidate
round-trips — are therefore unavailable.

So on wasm, `text_push_number` handles integral values exactly (the `%lld`
fast path, which the shim does support) and calls `code_runtime_error` for a
fractional one. Interpolating `2.5` under `--target wasm` stops the program
with a clear message instead of printing a number that disagrees with what
the other two modes print. That choice was deliberate: silently-wrong text is
the exact failure this whole feature existed to remove — a `"hi $name"` that
printed `hi $name` — so trading it back in for a different silent wrong
answer would defeat the point.

## Who this actually reaches (measured 2026-08-28)

**Not the playground.** `crates/code-wasm` calls `code::run_source` — the
*interpreter*, compiled to wasm — so it renders numbers through Rust's
`Display` and never reaches `runtime.c` at all. The only way to hit this is
`code build --target wasm`, which compiles a program to a `.wasm` module for
a host to run. That is a real path, but a narrow one, and nothing in the repo
currently depends on it. Worth knowing before budgeting the fix below.

## Fix direction

Write one shortest-round-trip formatter in plain C, with no libc dependency,
and use it on *every* target rather than only wasm — that way native and wasm
agree by construction instead of by two implementations happening to match.

Smaller than it first looks in one respect: **the positional layout is
already portable.** The last third of `text_push_number` — deciding where the
decimal point falls and writing the digits around it — is pure integer and
string work that needs nothing from libc, and it is the part that has to
match Rust's `Display` exactly. Only the two numeric primitives below are
missing, plus `memmove` and `strtol`, which are a few lines each (the shim
has `memcpy` and `strlen` but neither of those).

Not smaller in the respect that matters, though: the algorithm only *keeps*
41 significant digits, but reaching them means expanding the double exactly,
and a denormal's first significant digit sits around the 324th decimal place
— so the scratch expansion is still ~800 digits of bignum.

The two pieces the shim can't currently provide:

1. **Exact decimal expansion of a double.** A double's expansion is finite
   (up to ~767 digits), so this is bignum decimal arithmetic over the
   mantissa and a power of two — no floating point involved once the bits are
   unpacked.
2. **A round-trip check**, i.e. `strtod`. Same bignum machinery in reverse:
   parse the candidate digits, compare against the original bits.

Then the rounding loop already in `text_push_number` — shortest length whose
correctly-rounded form reads back identically, ties away from zero — sits on
top of both unchanged.

Budget it as a real numeric-code task rather than a cleanup: roughly 200+
lines of fiddly, high-risk arithmetic, and it wants the same 205k-value
differential test against Rust that the native path was verified with.

## Cheaper alternative — cheaper than the real fix, but not as cheap as this
## document claimed

Have the wasm host supply the formatting, the way it already supplies the
clock and the error sink (`code_host_now`, `code_host_error`). A
`code_host_number_text(double, char *out, unsigned len)` import would let
JavaScript do it.

~~JS's own `Number.prototype.toString` is shortest round-trip, the same rule,
so it would agree with Rust for free.~~ **Measured 2026-08-28: it does not.**
Both produce the same shortest-round-trip *digits*, but they lay them out
differently — Rust's `Display` is always positional, while JS switches to
exponential outside `[1e-6, 1e21)`:

| value | Rust `Display` | JS `toString()` |
|---|---|---|
| `1e21` | `1000000000000000000000` | `1e+21` |
| `1e-7` | `0.0000001` | `1e-7` |
| `5e-324` | `0.000…0005` (324 places) | `5e-324` |
| `f64::MAX` | `17976931348623157…` (309 digits) | `1.7976931348623157e+308` |

Four of seven probe values disagree. So the host would have to hand back
digits and an exponent and let something lay them out positionally — and
that layout is a third copy of logic the native path already has, in portable
C, which is an argument for moving it rather than duplicating it. This
alternative is still cheaper than the bignum work, but "agrees for free" was
wrong and the estimate should not rest on it.

## What shipped

Neither route, because both answered the wrong question. Each proposed a
*second* implementation of "how does a double become text" — one in portable
C, one split between JS and C — and then had to argue that it would agree
with the first. The agreement was the whole point, so building a second
implementation was the mistake.

The algorithm never moved. It reads as one function on every target, rounding
rule included. What moved is the two things it asks the outside world for:

| | native | wasm |
|---|---|---|
| the exact expansion, to 41 significant digits | `snprintf("%.40e")` | `code_host_number_exact` |
| reading a candidate back | `strtod` | `code_host_number_parse` |

Those two *are* the parts a freestanding build cannot compute — the
exact-arithmetic-over-hundreds-of-digits half, and the only reason the bignum
estimate above was as large as it was. Everything the modes have to agree on
stayed in one place, so they agree by construction rather than by inspection.
In JavaScript the two are `value.toExponential(40)` and `Number(text)`, one
call each; the exponent's zero-padding differs from C's and does not matter,
since it is read back as a number.

This is why the measurement that killed the cheap route does not touch this
one. That route asked JS for the *finished string* and inherited JS's layout
rule, exponential outside `[1e-6, 1e21)`. This asks JS only for digits that
the C side then rounds and lays out itself — and the tie-breaking rule that
made `printf`'s own rounding unusable in the first place (glibc to even, Rust
away from zero — and JS is to even as well) is the C side's, unchanged.

**Verified differentially**, at the scale the native path was: 205,000 random
double bit patterns rendered through both a native and a wasm build of
`runtime.c`, byte-identical, plus the fixture's own edge cases (denormals,
`f64::MAX`, `0.1 + 0.2`, an exact tie, `-0`). `tests/build_targets.rs` keeps
it honest from here by building `interp_number_text.code` — the same fixture
the other two modes are held to — for wasm and running it under Node.

The shim grew the three small things the algorithm needs and it did not have:
`memmove`, a base-10 `strtol`, and `%d` in its `snprintf`. A few lines each,
as the estimate above said.

**The cost, taken knowingly:** the wasm target asks its host for four
functions now rather than two. There is no making one optional — a weak
undefined symbol is dropped by the linker rather than imported, so it could
never be supplied at all — and a host that omits them fails at instantiation,
naming what is missing. Acceptable because the target already required a
clock and an error sink, and because nothing outside this repository hosts a
`code` wasm module yet. Worth a line in the release notes all the same.
