# arrays — array utilities (unreleased)

Stateless native and browser implementations. No Config, callbacks or pending
work. Each answers `<Name>Result { value }`; errors are Exception with source
"arrays" and innerException null. Unknown classes answer null.

All handlers require `value` to be an array. Missing required typed fields and wrong types
return Exception. Optional fields default only when absent; null is an explicit
value and is not a request for a default.

| Handler | Other fields | Result value |
|---|---|---|
| Length | — | Number of elements |
| Contains | item (any, default null) | Bool |
| IndexOf | item (any, default null) | First zero-based index, or -1 |
| Reverse | — | Elements in reverse order |
| Slice | start? (default 0), end? (default length) | Elements in [start, end) |
| Unique | — | First occurrence of each distinct value, in source order |
| Sort | by? (string field name), descending? (Bool, default false) | Stable ascending/descending order |
| Sum | — | Sequential sum; 0 for empty input |
| Min / Max | — | Smallest/largest number; null for empty input |

A missing item defaults to null, matching Code field access.
Contains, IndexOf and Unique use structural equality: arrays compare in order,
objects by field name rather than insertion order. Types are distinct (false
and 0 differ); signed zeros compare equal. Unique preserves the first matching
value. Native results retain its field order; browser results use the existing
JavaScript/JSON enumeration rules (integer-like object keys enumerate numerically). Unique uses quadratic pairwise checks;
it does not hash/serialize values or invent a different equality rule.

Slice matches strings.Slice: start/end are finite safe integers (absolute value
<= 9007199254740991); negative positions count from the end, bounds clamp, and
end <= start gives []. For example, slicing [1, 2, 3] from -2 to -1 gives [2].

Sort accepts either finite numbers or strings, with one key type throughout.
With `by`, every item must be an object carrying that field; the key must have
one of those types. Missing, null, mixed, boolean or composite keys are errors,
not coerced or placed in an implicit ordering. Empty input returns [], after
validating options. An empty field name is valid. Strings sort by Unicode
scalar order, without locale or case folding (including astral characters,
where JavaScript's default UTF-16 ordering would differ). Descending reverses
key comparison only, so tied records retain their original order.

Sum/Min/Max require finite numbers. Sum adds left to right, like math.Sum,
with no compensated summation; a non-finite final sum is an Exception. A
non-finite element or non-numeric element is always an Exception. Numeric zero
results normalize to +0. Empty Min/Max return null because there is no extremum.

Operations return new arrays without mutating the input. Code copy-on-write
semantics preserve nested input/result snapshots when a caller later updates
one. Direct JavaScript factory calls follow the browser module convention:
array containers are new, with element references treated as immutable JSON.

Native equality delegates to the core ABI. Browser equality is iterative and
matches it for JSON values. Existing browser transport serializes particles as
JSON: duplicate object keys cannot be preserved and non-finite numeric payloads
cannot round-trip as numbers. This module does not change that boundary; parity
contracts apply to finite JSON values with unique object keys. Number validation
also rejects direct non-finite ABI/JavaScript sort keys and numeric operands.

Core Length remains compatible for arrays/strings; arrays.Length is the
array-specific entry point. Link arrays in both a guest and its browser shell,
then rebuild the compiler/generated host after changing page.mjs. These source
additions are not yet published module assets; no app migration is included.
