# `crypto` — password hashing and random codes

bcrypt, and a cryptographically-random string generator. Two jobs a web
service always needs and neither of which belongs in the language core.

```code
link "crypto.so" as crypto

emit Hash { password = "hunter2" } to crypto get h
emit Verify { password = "hunter2", hash = h.hash } to crypto get v
assert v.valid

emit RandomCode { length = 8 } to crypto get r   | r.code is 8 of [A-Za-z0-9]
```

## Handlers

```
Hash       { password, cost? }    → HashResult       { hash }
Verify     { password, hash }     → VerifyResult     { valid }
RandomCode { length? }            → RandomCodeResult { code }
```

**Stateless** — there is nothing to configure and no setup particle.

| Field | Kind | Default | Meaning |
|---|---|---|---|
| `Hash.password` | String | — | the password. `""` is a valid password to hash — the program is entitled to ask |
| `Hash.cost` | Number | `12` | the bcrypt work factor for this call. A value outside bcrypt's own `4..=31` is an `Exception`, not silently clamped |
| `Verify.password` / `Verify.hash` | String | — | a missing field, or a `hash` that isn't a bcrypt string, is an `Exception` |
| `RandomCode.length` | Number | `32` | clamped to `1..=512`; a fractional or negative value is an `Exception` |

## Verify: wrong password vs bad input

`Verify` answers `VerifyResult { valid }`. A **wrong password** is
`valid = false` — an answer, not an error. A **`hash` that is not a bcrypt
string** is an `Exception`: the program asked a question the input cannot
answer, and `false` would be a lie.

## Randomness

`RandomCode` draws from `rand::thread_rng()` — a ChaCha CSPRNG seeded from
the operating system — with rejection sampling over the 62-character
alphabet, so every position is uniform (no modulo bias). It is suitable for
verification codes, tokens, and salts; it is not a KDF.

## The decisions, and why

**Ported from `euglena-language`'s `crypto` organelle**, with three changes:

- **No `Sap`, no state.** The organelle had a `Sap { salt_rounds }` and
  refused every `Hash` until it arrived. A `cost` is one number — a per-call
  parameter with a default, not a configuration phase. `Sap` was euglena's
  manifest-delivery mechanism anyway, not a language concept.
- **Failure is an `Exception`, not an `Error`.** The organelle returned
  `Error { status_code, message }`; a module here returns
  `Exception { source, message }`, which the program receives as a value.
- **`RandomCode` refuses a bad `length`** rather than coercing it. A
  fractional length was a mistake somewhere; silently flooring it hides
  that.

**bcrypt only, no argon2/scrypt/pbkdf2.** One well-understood default beats
a menu. A program that needs a different KDF is a different module.

**No `cost` above 31 or below 4.** Those are bcrypt's own limits, not this
module's — the `Exception` names the range.

## Build

```sh
cargo build --release        # -> target/release/libcrypto.so
```
