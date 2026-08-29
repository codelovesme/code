# greet — a native module for [Code](https://github.com/codelovesme/code)

A starting point for publishing your own module. Everything here works as it
stands: build it, run its fixture, and you have a module a program can `link`.
Then rename `greet` and replace the handler.

> ## Licence: GPL-3.0, and not a free choice
>
> **Every native module embeds Code's `runtime.c`.** That is how the ABI's
> value-lifetime contract works — the reference counting, the deep-copy
> boundary, the `CodeValue` layout — and `code-native` links it into your
> `cdylib` for you. A module is therefore a derivative work of a GPL-3.0
> project, and must be GPL-3.0 itself.
>
> That is fine for most people, but it is not a detail to discover after
> writing something. If it does not suit your situation, decide now.

## Build and test it

```sh
cargo build --release
cp target/release/libgreet.so tests/greet.so

code run   tests/greet.code
code build tests/greet.code -o /tmp/greet && CODE_CHECK_LEAKS=1 /tmp/greet
```

Run **both**. `code run` interprets and `code build` compiles through LLVM,
and the one invariant the language holds itself to is that every feature
behaves identically in both — a module is not exempt. A module that works
under only one of them is broken, and the difference usually shows up as a
lifetime bug rather than a compile error. `CODE_CHECK_LEAKS=1` makes the
compiled binary abort at exit if anything your handler allocated outlived the
program, which is free proof and worth keeping in the loop.

## What to keep when you replace the handler

`src/lib.rs` is short, and each part of its shape is load-bearing:

- **`guarded` wraps every dispatch.** A module may never end the host
  program. This is a hard rule of the language rather than a courtesy, and it
  is not something you can uphold by being careful: a panic escaping an
  `extern "C"` function *aborts* rather than unwinding, so the host cannot
  catch it. `guarded` catches it on your side and turns it into an
  `Exception`. Without it, an `unwrap` on a `None` kills someone else's
  program.
- **A class you do not handle answers null**, not a complaint. Sending a
  particle is not a demand, and whether to act on one is the recipient's
  business — a program may link six modules and emit the same particle to all
  of them.
- **Failures come back as values.** `exception(out, "greet", "…")` returns an
  `Exception` particle the caller can test with `is Exception`, or ignore.
  Nothing you return can end their program.
- **Do not validate the particle's shape before running.** A field that is
  not there reads as null, exactly as `.field` does in the language, so
  `Greet {}` is the same particle as `Greet { "name": null }`. Ask one
  question about the value, not two about whether it was supplied.

Rust is the recommended language for a module precisely because of the first
point: `guarded` makes the promise keepable. C modules are possible — the ABI
is C (`code_abi.h`) — but there a forgotten NULL check segfaults and an
integer `100 / 0` raises SIGFPE, and nothing can catch either.

## Publishing

`.github/workflows/publish.yml` does it:

```sh
git tag v1.0.0
git push --tags
```

It builds the artifact, proves it loads through both output modes, writes the
`module.json` manifest `code install` reads, and attaches everything to a
GitHub Release. Then share the release URL — a consumer installs it with

```sh
code install https://github.com/YOUR-NAME/code-module-greet/releases/download/v1.0.0/greet.json
```

There is nothing central to register with, and nothing to ask permission for.

## Renaming

`greet` appears in four places: `Cargo.toml` (`name`), `src/lib.rs` (the
`_class` match, the two `"greet"` source strings), `tests/greet.code`, and
`.github/workflows/publish.yml` (`MODULE`, and the `handlers` list in the
manifest step).

## Reading

- [The language README](https://github.com/codelovesme/code#readme) — the
  particle model, `emit`, and the error rules the points above come from.
- [`code_abi.h`](https://github.com/codelovesme/code/blob/main/src/code_abi.h)
  — the contract every module implements, and the authority when this README
  and the code disagree.
- [`code-native`](https://docs.rs/code-native) — the Rust API used here.
- The first-party modules —
  [`terminal`, `math`, `strings`, `http_client`](https://github.com/codelovesme/code/tree/main/crates/modules)
  — are the worked examples. `http_client` is the one to read for a module that does
  something genuinely fallible.
