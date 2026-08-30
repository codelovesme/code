# `blob_storage` — S3-compatible object storage

Put, get, list and delete objects in any store that speaks S3 — AWS S3,
MinIO, Cloudflare R2, Backblaze B2, DigitalOcean Spaces.

```code
link "blob_storage.so" as blobs

emit Config {
    bucket = "${S3_BUCKET}", access_key = "${S3_ACCESS_KEY}", secret_key = "${S3_SECRET_KEY}",
    endpoint = "${S3_ENDPOINT}"          -- omit for AWS
} to blobs get c
assert c.ok

emit Put { key = "notes/hello.txt", data = "hi", content_type = "text/plain" } to blobs get _
emit Get { key = "notes/hello.txt" } to blobs get r
assert r.found
assert r.data = "hi"
```

## Handlers

```
Config { bucket, access_key, secret_key, endpoint?, region?, path_style?, create? }
                                                     → ConfigResult { ok }
Put    { key, data, content_type?, base64? }         → PutResult    { key }
Get    { key, base64? }                              → GetResult    { found, key, data, content_type }
Delete { key }                                       → DeleteResult { existed }
List   { prefix? }                                   → ListResult   { keys, count }
```

`Upload` is an alias for `Put`, `Download` for `Get`.

`Config` is the setup particle — everything else is an `Exception` until it
has run.

- **`endpoint`** — set it for anything that isn't AWS. Trailing slash is
  trimmed.
- **`region`** — defaults to `us-east-1`; for AWS it must be the bucket's
  real region.
- **`path_style`** — `https://host/bucket/key` rather than
  `https://bucket.host/key`. Defaults **on** when `endpoint` is set (MinIO
  and most self-hosted stores need it), off for AWS.
- **`create = true`** — make the bucket if it isn't there (a 409 "already
  exists" counts as success). Off by default: "connect to storage" does not
  usually mean "and make it".

## Bytes

The language holds text, not arbitrary bytes. `Put { base64 = true }`
decodes `data` from base64 before storing, and `Get { base64 = true }`
returns the object base64-encoded. Without the flag, `Put` stores the
string's UTF-8 and `Get` decodes the object as UTF-8 (lossily).

## `Get` on a missing key

Returns `GetResult { found = false }`, not an `Exception` — "is it there?"
is a question, not an error. Every other failure (refused connection, bad
credentials, HTTP ≥ 300) is an `Exception` with `source = "blob_storage"`.

## The decisions, and why

**Ported from `euglena-language`'s `blob-storage` organelle**, with these
changes:

- **S3, not Azure Blob.** The organelle spoke Azure Blob's SharedKey REST
  API directly. S3 is the interface every object store — Azure included, via
  its S3 gateway — now exposes, so one module reaches all of them.
- **`Sap` became `Config`**, and a failed operation is an `Exception` rather
  than `{ ok: false }`.
- **`List` returns a real array** of keys; the old ABI could only hand back
  a JSON string.

**No presigned URLs, no multipart, no bucket policies, no per-object ACLs.**
Each is a real feature to add when asked; the euglena apps needed
put/get/list/delete.

**The bucket is per link.** `Config` replaces it. An app talking to two
buckets links `blob_storage` twice.

## Build

```sh
cargo build --release        # -> target/release/libblob_storage.so
```
