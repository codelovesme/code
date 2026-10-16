# math — numeric utilities

Stateless first-party module, in the existing Code repository. Native and
browser implementations share the same handlers and error contracts. The
expanded API and browser half are unreleased; existing Double/Sum remain.

```code
link "math" as math
emit Floor { value = -1.2 } to math get floor
assert floor.value = -2
emit Mod { value = -7, by = 3 } to math get remainder
assert remainder.value = 2
```

| Handler | Inputs | Meaning |
|---|---|---|
| Double | value | Multiply by two |
| Sum | value (array of numbers) | Ordered sum; empty array is 0 |
| Floor / Ceil | value | Round toward negative / positive infinity |
| Round | value | Nearest integer, ties away from zero |
| Trunc | value | Discard fractional part toward zero |
| Abs | value | Absolute value |
| Min / Max | value, other | Smaller / larger of two numbers |
| Div | value, by | Floor of the quotient |
| Mod | value, by | Remainder with the divisor's sign |
| Parse | text | Strict decimal text to Number |
| Sqrt | value | Square root, value >= 0 |
| Pow | value, exponent | Power |
| Sin / Cos | value | Trigonometry, radians |
| Ln | value | Natural logarithm, value > 0 |
| Exp | value | Natural exponential |

Each succeeds with `<Name>Result { value }`. Unknown classes answer null.
Bad/missing operands, non-finite inputs/results, zero divisors and domain
errors answer `Exception { source = "math", message, innerException = null }`.
Existing Double/Sum type-error messages are retained. Their previously unchecked
non-finite inputs/overflow now also return Exception. Finite underflow to zero
is allowed. All zero results are normalized to positive zero.

Parse accepts ASCII digits, an optional leading sign, an optional fractional
part and optional `e`/`E` exponent (for example `-.5e2`). It consumes the whole
string: whitespace, separators, hexadecimal, NaN and Infinity are rejected.
Values use the language's f64 precision, including rounding beyond exact
integers; this is not decimal or arbitrary-precision arithmetic.

Div and Mod include negative divisors: Div(-7,3)=-3, Mod(-7,3)=2,
Div(7,-3)=-3, Mod(7,-3)=-2. Mod starts with the floating remainder and adds
the divisor when signs differ. Extreme inputs retain ordinary f64 rounding;
it is not an exact-arithmetic identity guarantee.

Browser uses JavaScript Math; native uses Rust f64 operations. Basic exact
cases and error contracts agree; transcendental results may differ by a few
last bits between platform math libraries, so numerical tests use tolerances.
The browser half is included in generated host.mjs when the module is linked.
A hosted guest needs its shell to include the same module, as with other
browser modules. No extra numeric handlers are added to core.
