# `blob_fs` — blobs as plain files

`blob_storage`'s handlers, kept on a local disk as ordinary files. A program
moves here from `azure_blob`, `blob_storage` or `blob_storage_mock` by
changing the module its manifest names and the `Config` it sends.

```code
link "blob_fs.so" as blobs

emit Config { bucket = "keepsake-media", root = "/srv/blobs", create = true } to blobs get c
assert c.ok

emit Put { key = "ada/2026/11/beach.jpg", data = photo, base64 = true, content_type = "image/jpeg" } to blobs get _
emit Get { key = "ada/2026/11/beach.jpg", base64 = true } to blobs get r
assert r.found
```

## Handlers

```
Config { bucket, root, create? }                     → ConfigResult { ok }
Put    { key, data, content_type?, base64? }         → PutResult    { key }
Get    { key, base64? }                              → GetResult    { found, key, data, content_type }
                                                     | Archived     { key, disks }
Delete { key }                                       → DeleteResult { existed }
List   { prefix? }                                   → ListResult   { keys, count }
```

`Upload` is an alias for `Put`, `Download` for `Get`. `Config` is the setup
particle; everything else is an `Exception` until it has run.

## On the disk

```
<root>/data/<bucket>/<key>          the bytes, exactly as they were put
<root>/meta/<bucket>/<key>.json     { content_type, size, sha256, archived }
<root>/tmp/                         writes in progress
```

A person can open `data/` and find a file by its path; a backup is a copy.
A file copied into `data/` by hand is a blob too (served as
`application/octet-stream` until something puts it again).

- **`root`** must be absolute. **`create = true`** makes the bucket's folders;
  without it they must already exist.
- **Atomic writes.** `Put` writes into `tmp/`, syncs, then renames into place
  (bytes first, then the meta). A reader sees the old file or the whole new
  one; a write killed half-way leaves only a part file in `tmp/`, which is
  never a blob.
- **Keys are relative paths.** `/`-separated, no empty part, no `.` or `..`,
  no `\` or NUL, not absolute — otherwise an `Exception`. A key is never
  rewritten, so it cannot reach outside its bucket.
- **Archived.** When the meta says `"archived": true` and the file is not
  here, `Get` answers `Archived { key, disks }` — the file is on those archive
  disks, which is not the same as not found. `List` includes it.
- **Delete** removes the file and its meta, and folders left empty; the
  bucket's own folder stays. A missing key is `existed = false`.

## Bytes

As in `blob_storage`: `Put { base64 = true }` decodes `data` before storing
and `Get { base64 = true }` answers base64; without the flag, text in and
(lossy) UTF-8 out.
