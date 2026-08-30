# `cloud_drive_mock` — `cloud_drive` without Google

A drop-in for [`cloud_drive`](../cloud_drive/README.md): the same particles
and result shapes, with an in-memory file store and no HTTP.

```code
link "cloud_drive_mock.so" as drive

emit Config { client_id = "id", client_secret = "shh" } to drive get c

emit ExchangeCode { code = "user-1" } to drive get t     -- t.account_email = "user-1@drive.test"

emit UploadFile { access_token = t.access_token, file_name = "a.txt", data = "hi" } to drive get f
emit DownloadFile { access_token = t.access_token, file_id = f.file_id } to drive get d
assert d.data = "hi"
```

## Behaviour

- **`AuthUrl`** builds `{auth_url}?…` from the configured `auth_url` (default
  Google's), same as `oauth_mock`.
- **`ExchangeCode`** recovers `account_email` from a base64-JSON code
  (`{ "email": … }`) or synthesises `"<code>@drive.test"`. Tokens are
  `"mock-access"` / `"mock-refresh"`.
- **`GetQuota`** reports a fixed 16 GiB `total`; `used` is the bytes
  currently stored.
- **`UploadFile` / `ListFiles` / `DownloadFile` / `DeleteFile`** run against
  the in-memory store — file ids are `mock-file-<n>`.
- A `provider` other than `"google"` is an `Exception`, as in the real
  module.

## Build

```sh
cargo build --release        # -> target/release/libcloud_drive_mock.so
```
