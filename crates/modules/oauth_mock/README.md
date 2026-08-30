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
-- a.url points at auth_url — typically a local mock-provider page

emit ExchangeCode { code = "the-code" } to oauth get id
```

## How the identity is recovered

`ExchangeCode` never calls a token endpoint. The identity comes **from the
code**:

- If `code` is a base64 (URL-safe or standard) of JSON with `sub` and
  `email` — `{ "sub": "u-1", "email": "u@x.com", "name": "…", "picture": "…" }`
  — those fields are returned. This is what a mock-provider page produces:
  it lets the tester pick an identity, then encodes it into the redirect.
- Otherwise the identity is synthesised from the code string:
  `sub = "mock|<code>"`, `email = "<code>@mock.test"`.

`access_token` / `refresh_token` are `"mock-access-<sub>"` /
`"mock-refresh-<sub>"`.

## Build

```sh
cargo build --release        # -> target/release/liboauth_mock.so
```
