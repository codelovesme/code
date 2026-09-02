# `git_mock` — `git` without running git

A drop-in for [`git`](../git/README.md): the same ten particles and the same
result shapes, over a small in-memory model — no working tree, no
subprocess, no remote.

```code
link "git_mock.so" as git

emit Config { repo_path = "/anywhere", branch = "main" } to git get c
assert c.ok
assert c.dirty = false

emit Add { pattern = "." } to git get _
emit Commit { message = "first" } to git get cm     | cm ∈ CommitResult
emit Status { } to git get s
assert s.clean

emit Push { } to git get p                          | p ∈ PushResult, no network
```

## The model

A current branch, a commit count, a HEAD that moves on `Commit`, a "staged"
counter that `Add` bumps and `Commit` / `Stash` clear, and a stash flag.
Enough that a program's control flow behaves as it would against a real
repository:

- `Commit` with nothing staged is an `Exception` (pass `allow_empty` to
  override), same as git.
- `Status.clean` is `false` after `Add`, `true` after `Commit`.
- `Stash.changed` / `StashPop.changed` track whether there was work to move;
  `StashPop` with no stash is an `Exception`.
- `Clone`, `Push`, `SetRemote` succeed without touching a network;
  `SetRemote` masks `user:pass@` in the URL it echoes, like `git`.

## Build

```sh
cargo build --release        # -> target/release/libgit_mock.so
```
