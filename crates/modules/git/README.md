# `git` — version control over the system `git`

A thin adapter: every handler shells out to `git`, and authentication is
whatever the host's SSH agent or credential helper already provides. `git`
must be on `PATH`.

```code
link "git.so" as git

emit Config { repo_path = "/var/lib/app/repo", remote_url = "git@github.com:me/app.git" } to git get c
assert c.ok

emit Add    { pattern = "." } to git get _
emit Commit { message = "nightly snapshot", allow_empty = true } to git get _
emit Push   { branch = "main" } to git get _
```

## Handlers

```
Config    { repo_path, remote_url?, branch?, on_dirty? }        → ConfigResult    { ok, dirty, stashed, branch, head }
Stash     { }                                                   → StashResult     { changed }
StashPop  { }                                                   → StashResult     { changed }
Init      { path? }                                             → InitResult      { path }
Clone     { url, path? }                                        → CloneResult     { path }
Add       { pattern? }                                          → AddResult       { pattern }
Commit    { message, author_name?, author_email?, allow_empty? } → CommitResult    { message, output }
Push      { remote?, branch? }                                  → PushResult      { remote, branch, output }
SetRemote { name, url }                                         → SetRemoteResult { name, url }
Status    { }                                                   → StatusResult    { output, clean }
```

## `Config` checks the ground first

`Config` is the setup particle, and it does not just remember a path — it
decides whether it's safe to work there:

| the folder… | `Config` does |
|---|---|
| is inside another repo's tree | **`Exception`** — `'…' is inside the git repository at '…'` |
| is a checkout whose `origin` isn't the `remote_url` you passed | **`Exception`** — `'…' already tracks X, not Y` |
| isn't a repo yet | `git init` (and `git remote add origin` if `remote_url` given) |
| `branch` was given | `git checkout <branch>` (an `Exception` if that fails) |
| is a **pre-existing** repo with a **dirty** working tree | `on_dirty` decides — see below |

`ConfigResult` reports the state it settled on: `dirty`, `stashed`,
`branch`, and `head` (short SHA, `""` before the first commit).

### `on_dirty`

Only a pre-existing repo's dirt is protected — a fresh `git init` has no
history to lose, so its untracked files just come along (`dirty = true`,
never an error).

| `on_dirty` | dirty pre-existing repo |
|---|---|
| `"error"` (default) | `Exception` — nothing runs |
| `"stash"` | `git stash --include-untracked`, then `stashed = true` |
| `"ignore"` | proceed, `dirty = true` |

An app that wants finer control passes `on_dirty = "ignore"`, reads
`cfg.dirty`, and drives `Stash` / `Commit` / anything else itself.

## `Stash` / `StashPop`

`Stash {}` runs `git stash --include-untracked` and answers
`changed = false` if there was nothing to stash. `StashPop {}` runs
`git stash pop` — an `Exception` if the stack is empty.

## Errors

A `git` command that exits non-zero is an `Exception` carrying its stderr.
`Status` on a clean tree is `clean = true` with an empty `output`; a
non-zero exit anywhere else (bad ref, no upstream, network down for `Push`)
is the failure path.

**Credentials are masked.** A `scheme://user:pass@host` anywhere in a URL —
in `remote_url`, in a `SetRemoteResult`, in an `Exception` message from a
failed clone or push — is rewritten to `scheme://****@host`.

## The decisions, and why

**Ported from `euglena-language`'s `git` organelle**, with these changes:

- **`Sap` became `Config`**, and `Config` gained the safety checks above.
  The organelle's `Sap` just stored `repo_path` and best-effort init'd —
  it would happily point at someone else's checkout.
- **`on_dirty` and `Stash`/`StashPop`.** The organelle had no notion of a
  dirty tree; a `Commit` on top of unrelated local changes was a real way
  to lose work.
- **Failures are `Exception`s**, not `Error { status_code }`.
- **`Commit` no longer forces `--allow-empty`** — it's opt-in, and the
  author defaults (`code` / `code@localhost`, via `-c` so the repo config
  is never written) are there only so a commit works in a fresh checkout.

**No merge, rebase, log, diff, branch management.** Each is its own surface;
the euglena apps needed a backup/deploy loop, which is `Add` + `Commit` +
`Push`, plus knowing what state the repo was in.

**No libgit2.** The system `git` is what a developer's credential helper,
SSH config, and hooks are already wired to. A linked C library would be a
second, subtly different git.

## Build

```sh
cargo build --release        # -> target/release/libgit.so
```
