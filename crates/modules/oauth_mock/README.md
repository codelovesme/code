# `oauth_mock` — `oauth` without a provider

A drop-in for [`oauth`](../oauth/README.md): the same `Config`, `AuthUrl` /
`BuildAuthUrl` and `ExchangeCode` particles and the same result shapes, with
no HTTP anywhere.

```code
link "oauth_mock.so" as oauth

emit Config {
    client_id = "id", client_secret = "shh",
    redirect_uri = "https://app.example/callback",
    auth_url = "https://mock-provider.local/choose",
    token_url = "https://mock-provider.local/token"
} to oauth get c

emit AuthUrl { state = "csrf" } to oauth get a
| a.url points at auth_url — typically a local mock-provider page

emit ExchangeCode { code = "the-code" } to oauth get id
```

## How the identity is recovered

`ExchangeCode` never calls a token endpoint. The identity comes **from the
code**:

- If `code` is a base64 (URL-safe or standard) of a JSON object with a
  `sub` — `{ "sub": "u-1", "email": "u@x.com", "name": "…", "picture": "…" }`
  — those fields are returned, `email` empty if it has none. This is what a mock-provider page produces:
  it lets the tester pick an identity, then encodes it into the redirect.
- Otherwise the identity is synthesised from the code string:
  `sub = "mock|<code>"`, `email = "<code>@mock.test"`.

`access_token` / `refresh_token` are `"mock-access-<sub>"` /
`"mock-refresh-<sub>"`.

`claims` is the decoded object whole, or `{ sub, email }` for a plain
code, so a test signs in with whatever the real provider's id_token would
carry: Entra ID's `{ "sub": …, "preferred_username": …, "roles": ["hfc_admin"] }`
gives `claims.roles`, and an empty `email`, as Entra does. `id_token` is an
unsigned JWT (`alg: none`) of the same claims.

## Build

```sh
cargo build --release        # -> target/release/liboauth_mock.so
```
