# `env` — the environment, as particles

Everything a program needs from outside itself and cannot write down: the
port to listen on, the database it talks to, the secret it signs with. A
program that hardcodes those runs in exactly one place, and a repository that
contains them has leaked them.

```code
link "env.so" as env
link "http_server.so" as srv

emit Get { "name": "PORT", "default": 8080 } to env get p
emit Listen { "port": p.value } to srv get l
assert l.ok
```

## Handlers

```
Get     { name, default? } → EnvResult { name, value, found }
Require { name }           → EnvResult, or an Exception when it is unset
```

| Case | `value` | `found` |
|---|---|---|
| set | the variable, read as the default's kind | `true` |
| unset, with a default | the default, verbatim | `false` |
| unset, no default | null | `false` |
| set but unreadable as the default's kind | — | an `Exception` |

`found` is reported even when a default filled the value in: whether the
variable was *set* is the one fact this module has that the program cannot
get any other way.

## The default says how to read it

There are no type keywords in this language and this module invents none. The
kind of the `default` decides how the variable is read:

| `default` | read as |
|---|---|
| a Number | a number — `"9090"` becomes `9090` |
| a Bool | `true`/`1`/`yes` or `false`/`0`/`no` |
| anything else, or absent | the string as it stands |

That is what makes the example above one emit rather than two. Without it the
program would get `"8080"` and have to turn it into a number — and the
language has no way to. (A general string→number parse belongs in a `strings`
or `json` module and would be welcome; it is not a reason for this module to
hand back the wrong kind in the meantime.)

**An unreadable value is an `Exception`, not a fallback.** `PORT=banana` with
a numeric default is a deployment mistake; quietly listening on 8080 instead
would hide it until someone wondered why nothing was reaching the service.
The program can still decide what that means — an `Exception` is a value, and
`is Exception` is the whole check.

## `Require`

`Get` is for what might not be there. `Require` is for what the program
cannot run without:

```code
emit Require { "name": "DATABASE_URL" } to env get db
if db is Exception {
    -- say so and stop, rather than starting half-configured
}
```

## Deliberately not here

- **No `Set`.** A program setting a variable in its own process affects only
  itself and whatever it spawns — which is nothing, since there is no process
  module. It would be a way to write something nobody can read.
- **No `List`.** Enumerating the environment is how a secret ends up in a log.
  Ask for what you need by name.
- **No `.env` file loading.** That is a file-format decision and a filesystem
  capability, neither of which is this module's business. The shell, systemd,
  Docker and every CI runner already put variables in the environment; this
  reads what they put there.
- **No CLI arguments.** Same idea, different source — `Args` wants its own
  module, and the CLI has no way to pass arguments through to a program yet.

## A note on capability

A linked `env` module can read every variable in the process, including the
ones a program never asks for. That is the same trust a `link` already
implies — a native module runs in the program's own address space — but it is
worth saying out loud in the module that exists to touch secrets.

## Build

```sh
cargo build --release        # -> target/release/libenv.so
```
