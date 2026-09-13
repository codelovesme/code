# `azure_blob` — Azure Blob Storage

`blob_storage`'s handlers, against Azure instead of S3. Azure has its own
REST API and its own signing and no S3 gateway of its own, so it needs its
own module — but it does not need its own vocabulary.

```code
link "azure_blob.so" as blobs

emit Config {
    bucket = "${AZURE_BLOB_CONTAINER}"
    connection_string = "${AZURE_BLOB_CONNECTION_STRING}"
    create = true
} to blobs get c
assert c.ok

emit Put { key = "notes/hello.txt", data = "hi", content_type = "text/plain" } to blobs get _
emit Get { key = "notes/hello.txt" } to blobs get r
assert r.found
assert r.data = "hi"
```

## Handlers

```
Config { bucket, connection_string? | account + key, endpoint?, create? }
                                                     → ConfigResult { ok }
Put    { key, data, content_type?, base64? }         → PutResult    { key }
Get    { key, base64? }                              → GetResult    { found, key, data, content_type }
Delete { key }                                       → DeleteResult { existed }
List   { prefix? }                                   → ListResult   { keys, count }
```

`Upload` is an alias for `Put`, `Download` for `Get`.

`Config` is the setup particle — everything else is an `Exception` until it
has run.

## Why the fields are spelled the S3 way

A container is configured as **`bucket`** and an object is a **`key`**. Azure
calls them a container and a blob, and this module still calls them what
`blob_storage` calls them, because the point of the module is that a program
moves between the two by changing the line it links and nothing else. One
vocabulary that is occasionally the wrong word beats two that are each right
half the time.

- **`connection_string`** — the Azure-native way to be handed the account,
  the key and the endpoint at once, and what Azurite prints on startup.
- **`account` + `key`** — the same thing spelled out. `key` is the account
  key, base64 as Azure gives it.
- **`endpoint`** — omit for real Azure, where the host is the account's own
  name. Azurite puts the account in the path instead
  (`http://127.0.0.1:10000/devstoreaccount1`), which the signature has to
  account for; it does.
- **`create = true`** — make the container if it isn't there (a 409 "already
  exists" counts as success). Off by default: "connect to storage" does not
  usually mean "and make it".

## Bytes

The language holds text, not arbitrary bytes. `Put { base64 = true }` decodes
`data` from base64 before storing, and `Get { base64 = true }` returns the
object base64-encoded. Without the flag, `Put` stores the string's UTF-8 and
`Get` decodes the object as UTF-8 (lossily).

## `Get` on a missing key

Returns `GetResult { found = false }`, not an `Exception` — "is it there?" is
a question, not an error. `Delete` on a missing key is `existed = false` for
the same reason. Every other failure (refused connection, bad credentials,
HTTP ≥ 300) is an `Exception` with `source = "azure_blob"`, carrying the
reason Azure gave rather than a bare status.

## SharedKey

Azure signs a fixed list of HTTP headers — most of them empty for what this
module sends — then every `x-ms-*` header, then the resource, and an error in
any of it is a 403 that says nothing about which line was wrong. Two things
that are easy to get wrong and are handled here:

- **The signature covers exactly the `x-ms-*` headers on the request.** An
  extra one in the string to sign is as wrong as a missing one, which is why
  `x-ms-blob-type` is signed for `Put` and for nothing else.
- **The canonical resource repeats the account against Azurite.** Azure's
  rule is the account name followed by the request's full path, and Azurite's
  path already starts with the account — so it appears twice. It looks wrong.
  It is what the SDKs compute.

## Testing

`tests/azure_blob_module.rs` drives the whole round trip against a real
endpoint, and is the same round trip `tests/blob_storage_module.rs` drives.
It runs when `AZURE_BLOB_CONNECTION_STRING` and `AZURE_BLOB_CONTAINER` are
set and skips cleanly when they are not. CI starts Azurite; locally:

```bash
docker run -d --name azurite -p 10000:10000 \
  mcr.microsoft.com/azure-storage/azurite:latest azurite-blob --blobHost 0.0.0.0
```
