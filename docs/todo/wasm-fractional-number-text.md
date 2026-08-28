# Rendering a fractional number as text is unsupported on wasm

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
