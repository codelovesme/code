# `jwt` — sign and verify HS256 tokens

A JSON Web Token is three base64url segments joined by dots: a fixed header,
the claims, and `HMAC-SHA256(secret, header.claims)`. HS256 is the only
algorithm here — it is small enough to implement directly (`hmac` + `sha2` +
`base64`) rather than pull `jsonwebtoken` and a crypto backend.

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
Config { secret, expires_in? }        → ConfigResult { ok }
Sign   { sub, role?, expires_in? }    → SignResult   { token }
Decode { token }                      → DecodeResult { valid, sub, role, exp }
```

`Config` is the setup particle — a stateful module. `Sign` and `Decode` are
an `Exception` until it has run.

| Field | Kind | Default | Meaning |
|---|---|---|---|
| `Config.secret` | String | — | the HMAC key. Missing or empty is an `Exception` — nothing here works without it |
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

## The secret is not a per-call parameter

`Sign` and `Decode` both take the secret from `Config`, never from the
particle. A signing key is deployment configuration — it belongs in a
manifest and an environment variable, not in the body of a request a handler
is processing. euglena delivers `Config` from the manifest at cell startup.

## Header pinning

The header is fixed to `{"alg":"HS256","typ":"JWT"}` and `Decode` checks the
incoming header against it *before* the MAC check. A token presenting
`{"alg":"none"}` or `{"alg":"RS256"}` is rejected outright — the classic
algorithm-confusion downgrade has nothing to work with.

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

**HS256 only.** RS256/ES256 need a keypair and a place to publish the public
half — a different shape of configuration and a different module.

**No `nbf`, `aud`, `iss` validation.** `sub`, `role`, `iat`, `exp` are what
the euglena apps use. A claim set that needs more is a fork of this file, and
a small one.

## Build

```sh
cargo build --release        # -> target/release/libjwt.so
```
