# `blob_storage_mock` — `blob_storage` without an object store

A drop-in for [`blob_storage`](../blob_storage/README.md): the same
`Config` / `Put` / `Get` / `Delete` / `List` particles (plus the
`Upload` / `Download` aliases) and the same result shapes, over an in-memory
map that lives for the process.

```code
link "blob_storage_mock.so" as blobs

emit Config { bucket = "b", access_key = "k", secret_key = "s" } to blobs get c

emit Put { key = "notes/a.txt", data = "hello", content_type = "text/plain" } to blobs get _
emit Get { key = "notes/a.txt" } to blobs get g
assert g.found
assert g.data = "hello"

emit List { prefix = "notes/" } to blobs get l   | l.keys, l.count
```

Every S3 field on `Config` (`endpoint`, `region`, `path_style`, `create`) is
accepted and ignored. `base64` flags on `Put` / `Get` work exactly as in the
real module; `Get` on a missing key is `GetResult { found = false }`.

## Build

```sh
cargo build --release        # -> target/release/libblob_storage_mock.so
```
