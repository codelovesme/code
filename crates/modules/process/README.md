# `process` — run other programs

Two shapes: **`Run`** for a command you wait on and want the output of,
**`Spawn`** for a long-running child you track and later stop.

```code
link "process.so" as proc

emit Run { command = "git", args = ["rev-parse", "HEAD"] } to proc get r
assert r.success
-- r.stdout is the commit hash

emit Spawn { id = "server", command = "./myserver", args = ["--port", "8080"] } to proc get s
-- ... later ...
emit Kill { id = "server" } to proc get _
```

## Handlers

```
Run    { command, args?, cwd?, env?, stdin? }   → RunResult    { code, success, stdout, stderr }
Spawn  { id, command, args?, cwd?, env? }        → SpawnResult  { id, pid }
Status { id }                                    → StatusResult { id, alive, code, status }
Wait   { id }                                    → StatusResult { id, alive, code, status }
Kill   { id }                                    → KillResult   { id, killed }
List   { }                                       → ProcessList  { processes, count }
```

`WaitFor` is an alias for `Wait`. No configuration and no setup particle —
the process table starts empty and fills as you `Spawn`.

| Field | Meaning |
|---|---|
| `command` | the executable — looked up on `PATH` if it has no `/`. Missing or empty is an `Exception` |
| `args` | an array of strings. A non-string element is an `Exception` |
| `cwd` | working directory for the child |
| `env` | `{ KEY = "value", … }` — **added to** the inherited environment, not a replacement. Non-string values are an `Exception` |
| `stdin` | (`Run` only) a string written to the child's stdin, which is then closed |

`RunResult.code` is the exit code, or `-1` when the child was killed by a
signal. `stdout`/`stderr` are captured in full and decoded lossily (invalid
UTF-8 becomes `�` rather than failing). `StatusResult.status` is
`"running"`, `"exited"` (code 0), or `"failed"` (non-zero / killed).

## `Run` vs `Spawn`

**`Run`** pipes the child's stdout and stderr, waits for it to finish, and
hands both back. It blocks the whole program while the child runs — the same
way `http_client` blocks for a round trip, and fine for the same reason (the
program is single-threaded and has nothing else to do meanwhile).

**`Spawn`** inherits stdio — the child writes to *this* program's stdout and
stderr — and returns immediately with a `pid`. The child is tracked under
the `id` you chose; `Status` and `List` reap it without blocking, `Wait`
blocks until it exits, `Kill` stops it. A second `Spawn` on an `id` whose
child is still running is an `Exception`; once that child has exited, the
`id` is free to reuse.

## Errors

A **non-zero exit** is a normal result — `RunResult { success = false }` or
`StatusResult { status = "failed" }`. An **`Exception`** means the program
got the call wrong or the OS refused: no such command, a bad `args` element,
an unknown `id` for `Status`/`Wait`/`Kill`.

`Kill` on an already-dead process is `killed = false`, not an `Exception` —
stopping something that is already stopped is not a failure.

## The decisions, and why

**Ported from `euglena-language`'s `process` organelle**, with two changes:

- **No `Sap`.** The organelle's `Sap` allocated an empty `HashMap` and
  refused every `Spawn` until it had run. There is nothing to configure; the
  table is lazy.
- **`Run` is new.** The organelle only did spawn-and-track, so a program
  that just wanted a command's output had to `Spawn`, `Wait`, and then had
  no way to read stdout anyway (it was inherited). `Run` is what most
  subprocess use actually is.

**No shell.** `command` is executed directly, not through `sh -c`. A caller
that wants a pipeline or a glob passes `sh` as the command and `-c "…"` as
an arg — explicitly, so the shell is never a surprise.

**No output streaming for `Spawn`.** Capturing a long-running child's output
means draining two pipes forever or risking a full buffer that blocks the
child. Inheriting is the honest default; a program that needs a daemon's
logs redirects them itself (`sh -c "./d > d.log 2>&1"`).

## Build

```sh
cargo build --release        # -> target/release/libprocess.so
```
