# `oauth` — the authorization-code flow, one provider

Server-side OAuth 2.0: build the URL you redirect the user to, take the
`code` they come back with, trade it for tokens and their identity. The
client secret lives in `Config`, delivered from a manifest — a gene never
holds it.

```code
link "oauth.so" as oauth

emit Config {
    client_id     = "${GOOGLE_CLIENT_ID}",
    client_secret = "${GOOGLE_CLIENT_SECRET}",
    redirect_uri  = "https://myapp.com/oauth/callback",
    auth_url      = "https://accounts.google.com/o/oauth2/v2/auth",
    token_url     = "https://oauth2.googleapis.com/token",
    userinfo_url  = "https://openidconnect.googleapis.com/v1/userinfo",
    scope         = "openid email profile"
} to oauth get _

-- redirect the browser here
emit AuthUrl { state = "${csrf_token}", extra = { access_type = "offline" } } to oauth get a

-- ...user comes back to redirect_uri with ?code=...&state=...
emit ExchangeCode { code = "${code_from_callback}" } to oauth get id
assert id.email = "…"
```

## Handlers

```
Config       { client_id, client_secret, redirect_uri, auth_url, token_url, userinfo_url?, scope? } → ConfigResult { ok }
AuthUrl      { state, extra? }   → AuthUrlResult { url }
ExchangeCode { code }            → Identity { sub, email, name, picture, access_token, refresh_token }
```

`BuildAuthUrl` is an alias for `AuthUrl`. `Config` is the setup particle —
`AuthUrl` and `ExchangeCode` are an `Exception` until it has run.

| Field | Meaning |
|---|---|
| `AuthUrl.state` | your CSRF token, echoed back to `redirect_uri`. Required — you must verify it |
| `AuthUrl.extra` | `{ key = "value" }` of provider-specific query parameters, appended in order. Google wants `access_type = "offline"` for a refresh token; some providers want `prompt = "consent"` |
| `ExchangeCode.code` | the authorization code from the callback |

`Identity.sub`/`email`/`name`/`picture` come from the userinfo endpoint —
empty strings if `userinfo_url` is not configured, in which case
`ExchangeCode` still returns the `access_token` and `refresh_token`.

## Errors

A provider rejecting the exchange — a used or expired `code`, a bad
`redirect_uri`, wrong credentials — is an `Exception` carrying the
`error_description` from its response. So is a token or userinfo endpoint
that won't respond, or a `Config` missing a required field. `AuthUrl` never
touches the network.

## The decisions, and why

**Ported from `euglena-language`'s `oauth` organelle**, with three changes:

- **Provider parameters are not hard-coded.** The organelle always appended
  Google's `access_type=offline&prompt=select_account`. Those belong to the
  caller now, as `extra`.
- **Tokens are surfaced.** The organelle exchanged the code, used the access
  token for one userinfo call, and threw it away. `Identity` carries the
  `access_token` and `refresh_token` so a program can call the provider's
  API or refresh later.
- **`userinfo_url` is optional.** An OIDC provider returns an `id_token` a
  program may prefer to decode itself (with the `jwt` module); the userinfo
  round trip is a convenience, not the only path.

**One provider per link.** `Config` replaces the whole provider. An app that
offers "sign in with Google *or* GitHub" links `oauth` twice under two
aliases — which is also how it keeps two client secrets apart.

**No PKCE, no implicit flow, no device flow.** Authorization code with a
client secret is the server-side flow, and it is what the euglena apps use.
PKCE is the natural next addition if a public client ever needs this.

## Build

```sh
cargo build --release        # -> target/release/liboauth.so
```
