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

| redirect the browser here
emit AuthUrl { state = "${csrf_token}", extra = { access_type = "offline" } } to oauth get a

| ...user comes back to redirect_uri with ?code=...&state=...
emit ExchangeCode { code = "${code_from_callback}" } to oauth get id
assert id.email = "…"
```

## Handlers

```
Config       { client_id, client_secret, redirect_uri, auth_url, token_url, userinfo_url?, scope? } → ConfigResult { ok }
AuthUrl      { state, extra? }   → AuthUrlResult { url }
ExchangeCode { code }            → Identity { sub, email, name, picture, access_token, refresh_token, id_token, claims }
```

`BuildAuthUrl` is an alias for `AuthUrl`. `Config` is the setup particle —
`AuthUrl` and `ExchangeCode` are an `Exception` until it has run.

| Field | Meaning |
|---|---|
| `AuthUrl.state` | your CSRF token, echoed back to `redirect_uri`. Required — you must verify it |
| `AuthUrl.extra` | `{ key = "value" }` of provider-specific query parameters, appended in order. Google wants `access_type = "offline"` for a refresh token; some providers want `prompt = "consent"` |
| `ExchangeCode.code` | the authorization code from the callback |

`Identity.sub`/`email`/`name`/`picture` come from the userinfo endpoint.
Without `userinfo_url` they come from the id_token's claims, and are empty
strings when the provider sent no id_token; `access_token` and
`refresh_token` are returned either way.

`Identity.id_token` is the id_token as the token endpoint returned it
(`null` if it sent none), and `Identity.claims` its decoded payload, whole.
The module reads nothing in it beyond those four fields: `roles`, `tid`,
`oid`, `preferred_username` are the program's.

### Microsoft Entra ID

Entra puts a user's **app roles** in the id_token and nowhere else (its
userinfo has none), and leaves `email` empty for some accounts, with the
address in `preferred_username`. So leave `userinfo_url` out and read the
claims:

```code
emit Config {
    client_id = "${ENTRA_CLIENT_ID}"
    client_secret = "${ENTRA_CLIENT_SECRET}"
    redirect_uri = "https://api.example.org/auth/callback"
    auth_url = "https://login.microsoftonline.com/${TENANT_ID}/oauth2/v2.0/authorize"
    token_url = "https://login.microsoftonline.com/${TENANT_ID}/oauth2/v2.0/token"
    scope = "openid email profile"
} to oauth get _

emit ExchangeCode { code = code } to oauth get id
address = id.email
if address = "", address = id.claims.preferred_username
admin = false
loop role over id.claims.roles
    if role = "hfc_admin", admin = true
```

Check `claims.tid` against your own tenant too: a multi-tenant app
registration signs in accounts from anywhere.

### When not checking the signature is safe

**The id_token's signature is not verified.** It does not need to be here:
the token arrived straight from the token endpoint, over TLS, in answer to
a request authenticated by this client's own secret, which is the case
OpenID Connect Core §3.1.3.7 lets a client skip validation for. That is the
only case. An id_token that reached the program any other way (from a
browser, a header, a query string, another service) must be verified against
the provider's published keys before a single claim in it is believed, and
this module is not the way to do that.

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
