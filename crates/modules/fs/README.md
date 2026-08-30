# `fs` — files under a sandboxed base

Read, write, list and remove files and directories — all of them **inside**
one base directory that `Config` establishes. There is no way to name a path
outside it.

```code
link "fs.so" as fs

emit Config { base_path = "/var/lib/myapp" } to fs get _

emit WriteFile { path = "notes/today.md", content = "# hi" } to fs get w
emit ReadFile  { path = "notes/today.md" } to fs get r
assert r.content = "# hi"
```

## Handlers

```
Config     { base_path }        → ConfigResult     { ok, base_path }
ReadFile   { path }             → FileContent      { path, content }
WriteFile  { path, content }    → WriteResult      { path, bytes }
DeleteFile { path }             → DeleteResult     { path, existed }
CreateDir  { path }             → CreateDirResult  { path }
RemoveDir  { path }             → RemoveDirResult  { path, existed }
ListDir    { path }             → DirListing       { path, entries }
Exists     { path }             → ExistsResult     { path, exists, is_file, is_dir }
```

- `Config` is the setup particle — a stateful module. `Config.base_path` is
  created if it doesn't exist, and every other handler is an `Exception`
  until `Config` has run (a filesystem module with an implicit writable root
  is a footgun).
- `DirListing.entries` is `[{ name, is_dir }]`, sorted by name.
- `DeleteFile` / `RemoveDir` are idempotent: an absent target answers
  `existed = false`, not an `Exception`. Both are recursive for `RemoveDir`.
- `WriteFile` creates missing parent directories, then writes atomically —
  to a hidden sibling temp file, then `rename` — so a reader never sees a
  half-written file. It is a full overwrite, not an append.

## The sandbox

Every `path` is resolved *relative to* `base_path`, always:

| given | resolves to |
|---|---|
| `notes/x.md` | `<base>/notes/x.md` |
| `/notes/x.md` | `<base>/notes/x.md` — a leading `/` is dropped, not honored |
| `a/../b` | `<base>/b` |
| `../secret` | **`Exception`** — climbs above the base |
| `""` | `<base>` itself (list it, check it) |

`..` is allowed as long as the result stays inside; it is only refused when
it would leave. This is a real change from the euglena `fs` organelle, whose
path resolution returned an absolute path unchanged and had no `..` check —
`ReadFile { path = "/etc/passwd" }` read `/etc/passwd`.

## Text only

`ReadFile` and `WriteFile` deal in UTF-8 strings, because the value model
has strings and no byte type. A file whose bytes are not valid UTF-8 is a
`ReadFile` `Exception`. Binary files are out of scope.

## The decisions, and why

**Ported from `euglena-language`'s `fs` organelle**, with four changes:

- **The sandbox actually contains.** See above — this is the headline.
- **`Sap` became `Config`.** `Sap` is euglena's manifest-delivery
  mechanism; a `code` module's setup is a particle the program sends.
- **Failures are `Exception`s**, not the organelle's `Error { status_code }`.
  A missing file in `ReadFile` is an `Exception` (use `Exists` to branch on
  presence); an absent target in `Delete`/`RemoveDir` is `existed = false`.
- **No `status_code` on results.** The organelle put HTTP status codes on
  every answer; nothing here speaks HTTP.

**No `stat`, no permissions, no symlink control, no watching.** Each is its
own feature and none of the euglena apps needed them. `Exists` covers "is it
there, and is it a file or a directory".

## Build

```sh
cargo build --release        # -> target/release/libfs.so
```
