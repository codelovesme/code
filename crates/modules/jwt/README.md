# `jwt` — sign and verify HS256 and EdDSA tokens

A JSON Web Token is three base64url segments joined by dots: a fixed header,
the claims, and a signature over the first two. Both algorithms here are
small enough to implement directly (`hmac`/`sha2` and `ed25519-dalek`)
rather than pull `jsonwebtoken` and a crypto backend.

**Two algorithms, because signing and verifying are not always the same
power.** With HS256 one secret does both: everyone who can check a token can
also make one. That is right for a program that signs its own tokens and
reads them back, and wrong for a service that only ever needs to check what
somebody else issued — it means every such service could mint a token as
anybody, and nothing downstream could tell. With EdDSA the issuer holds a
private key and every verifier holds only the public one.

```code
link "jwt.so" as jwt

emit Config { secret = "${JWT_SECRET}" } to jwt get _
emit Sign   { sub = "user-42", role = "admin" } to jwt get s

emit Decode { token = s.token } to jwt get d
assert d.valid
assert d.sub = "user-42"
```

## Handlers

```
Config { secret, expires_in? }        → ConfigResult { ok }     HS256, both powers
Config { private_key, expires_in? }   → ConfigResult { ok }     EdDSA, signs and verifies
Config { public_key, expires_in? }    → ConfigResult { ok }     EdDSA, verifies only
Sign   { sub, role?, expires_in? }    → SignResult   { token }
Decode { token }                      → DecodeResult { valid, sub, role, exp }
GenerateKeypair                       → Keypair      { private_key, public_key }
```

Exactly one key per `Config`. None is an `Exception`, and so is more than
one — a deployment naming two keys means something the module would have to
guess at. `GenerateKeypair` needs no `Config` and changes nothing: it is
there so an operator never assembles a pair out of two commands' output.

`Config` is the setup particle — a stateful module. `Sign` and `Decode` are
an `Exception` until it has run.

| Field | Kind | Default | Meaning |
|---|---|---|---|
| `Config.secret` | String | — | the HMAC key, for HS256 |
| `Config.private_key` | String | — | base64 of 32 bytes, for EdDSA. Signs and verifies |
| `Config.public_key` | String | — | base64 of 32 bytes, for EdDSA. Verifies; `Sign` is then an `Exception` |
| `Config.expires_in` | Number | `86400` | default token lifetime, in seconds. Must be a positive whole number |
| `Sign.sub` | String | — | the subject claim. Missing or empty is an `Exception` |
| `Sign.role` | String | `""` | the role claim |
| `Sign.expires_in` | Number | the Config default | overrides the lifetime for this one token |
| `Decode.token` | String | — | the token to check. Missing is an `Exception` |

`Sign` writes `{ sub, role, iat, exp }`. `Decode` reads them back — `exp` is
the expiry as unix seconds, `0` when the token was invalid.

## `Decode` answers, it doesn't refuse

`Decode` returns `DecodeResult { valid, ... }`. A token that is **garbled,
tampered with, signed by a different secret, or past its `exp`** comes back
`valid = false` — that is the question `Decode` exists to answer, and an
`Exception` would make a caller wrap every check in error handling.

An `Exception` is reserved for the program getting the call itself wrong:
no secret configured, or no `token` field.

## A verify-only deployment cannot sign

`Config { public_key }` leaves `Sign` an `Exception`: this jwt can check
tokens, not make them. It fails at the call rather than producing a token
nothing would accept, because the mistake is the deployment's, not the
caller's.

## The key is not a per-call parameter

`Sign` and `Decode` both take the key from `Config`, never from the
particle. A signing key is deployment configuration — it belongs in a
manifest and an environment variable, not in the body of a request a handler
is processing. euglena delivers `Config` from the manifest at cell startup.

## Header pinning

`Config` decides the algorithm; the token never does. `Decode` compares the
incoming header against the one its own configuration writes — `HS256` or
`EdDSA` — *before* any signature check, and refuses anything else outright.
`{"alg":"none"}` has nothing to work with, and neither does the sharper
version: an `HS256` token signed with a verifier's **public key** as the
HMAC secret. A verifier that believed the header would check that token
happily, and a public key is public. Both directions are covered by tests.

## The decisions, and why

**Ported from `euglena-language`'s `jwt` organelle**, with three changes:

- **`Sap` became `Config`.** `Sap` is euglena's manifest-delivery
  mechanism, not a language concept — a `code` module's setup is a particle
  the program sends, named for what it does.
- **Failure is an `Exception`, not an `Error`** — the organelle returned
  `Error { status_code, message }` for a missing secret or field.
- **`jsonwebtoken` is gone.** HS256 is `hmac`/`sha2` over two strings; the
  dependency and its `ring`/`aws-lc` backend bought nothing for the one
  algorithm this needs.

**EdDSA rather than RS256** for the asymmetric half. Same property — the
verifier cannot forge — at 32-byte keys, no key-size or padding parameters
to choose wrongly, and a pure-Rust dependency a fraction of the weight.
RS256 is what an outside system is most likely to expect; nothing here talks
to one, and it can be added beside these two if that changes.

**No `nbf`, `aud`, `iss` validation.** `sub`, `role`, `iat`, `exp` are what
the euglena apps use. A claim set that needs more is a fork of this file, and
a small one.

## Build

```sh
cargo build --release        # -> target/release/libjwt.so
```
