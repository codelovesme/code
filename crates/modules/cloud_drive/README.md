# `cloud_drive` — Google Drive

The server-side OAuth 2.0 authorization-code flow, plus quota, upload,
download, list and delete against Google Drive v3.

```code
link "cloud_drive.so" as drive

emit Config {
    client_id = "${GOOGLE_CLIENT_ID}", client_secret = "${GOOGLE_CLIENT_SECRET}",
    redirect_uri = "https://myapp.example/oauth/callback"
} to drive get c
assert c.ok

| 1. send the user here
emit AuthUrl { state = "csrf-token" } to drive get a
| redirect to a.url …

| 2. they come back with ?code=…
emit ExchangeCode { code = "the-code" } to drive get t
assert t.access_token ≠ ""

| 3. use the token
emit UploadFile {
    access_token = t.access_token, file_name = "notes.txt",
    data = "remember the milk", content_type = "text/plain"
} to drive get f
assert f.file_id ≠ ""
```

## Handlers

```
Config       { client_id, client_secret, redirect_uri?, scope?,
               auth_url?, token_url?, api_base? }          → ConfigResult { ok }
AuthUrl      { state, redirect_uri?, extra? }              → AuthUrlResult { url }
ExchangeCode { code, redirect_uri? }                       → Tokens { account_email, access_token,
                                                                       refresh_token, expires_in }
RefreshToken { refresh_token }                             → Tokens { … }   (no account_email)
GetQuota     { access_token }                              → Quota { account_email, total, used, available }
ListFiles    { access_token, query?, page_size? }          → FileList { files, count }
UploadFile   { access_token, file_name, data,
               content_type?, base64? }                    → RemoteFile { file_id, file_name,
                                                                          content_type, size, web_view_url }
DownloadFile { access_token, file_id, base64? }            → FileContent { file_id, file_name,
                                                                           content_type, data }
DeleteFile   { access_token, file_id }                     → DeleteResult { existed }
```

`BuildAuthUrl` is an alias for `AuthUrl`. `Config` is the setup particle —
everything else is an `Exception` until it has run.

## Config

- **`redirect_uri`** — required by `AuthUrl` and `ExchangeCode`; set it here
  or pass it on the particle. The two must match, as OAuth demands.
- **`scope`** — defaults to
  `openid email profile https://www.googleapis.com/auth/drive.file` (the
  app sees only files it created). Widen it if the app needs to.
- **`auth_url` / `token_url` / `api_base`** — default to Google's real
  endpoints (`accounts.google.com`, `oauth2.googleapis.com`,
  `www.googleapis.com`). Override for a Google-compatible gateway, a proxy,
  or a test double; every other Drive URL is derived from `api_base`.

## Tokens

The app holds the tokens and passes `access_token` on every call — this
module keeps no session. When an access token expires, `RefreshToken` trades
the refresh token for a new one (Google doesn't re-issue the refresh token
on a refresh, so `Tokens.refresh_token` echoes the one you sent).

## Bytes

The language holds text, not arbitrary bytes. `UploadFile { base64 = true }`
decodes `data` from base64 before uploading; `DownloadFile { base64 = true }`
returns the content base64-encoded. Without the flag, `data` is the content's
UTF-8 (decoded lossily on the way out). A `DownloadFile` over 64 MiB is an
`Exception` rather than an unbounded read.

## `DeleteFile` on a missing file

Returns `DeleteResult { existed = false }`, not an `Exception` — "is it
there?" is a question. Every other failure (refused connection, bad token,
Drive error) is an `Exception` with `source = "cloud_drive"`; the message
carries Drive's own `error.message` where there is one.

## The decisions, and why

**Ported from `euglena-language`'s `cloud-drive` organelle**, with these
changes:

- **Google Drive only, and it says so.** The organelle carried OneDrive and
  Yandex branches that returned `ProviderUnavailable` for every call. A
  `provider` field is still accepted for a gentle migration, but anything
  other than `"google"` (or absent) is an `Exception`.
- **`Sap` became `Config`**, a failed call is an `Exception` rather than
  `{ ok: false }`, and the URL endpoints are overridable (they were
  hard-coded constants).
- **`ListFiles` returns a real array** of `RemoteFile`; the old ABI could
  only hand back a JSON string.
- **`RefreshToken` is new** — the organelle left token refresh entirely to
  the app.

**No resumable/chunked upload, no shared-drive support, no folder
management, no change tracking.** Each is a real feature to add when asked;
the euglena aggregator needed the flow above.

## Build

```sh
cargo build --release        # -> target/release/libcloud_drive.so
```
