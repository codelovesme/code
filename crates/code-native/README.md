# code-native

Write a native `.so` module for the [Code programming
language](https://github.com/codelovesme/code) in Rust — `link "x.so" as m`
+ `emit particle to m` from `.code` source, with the handler behind `m`
implemented here instead of C.

This is the Rust-specific path into `code`'s native-module ABI
(`src/code_abi.h` in the main repo). C — and anything else that can produce
a C-ABI shared library — still uses that header and `src/runtime.c`
directly; there's no package registry to publish a C-language bundle to, so
that story is unchanged. This crate exists so a Rust module doesn't need a
checkout of the `code` repo at all: `cargo add code-native` pulls in
everything required, including a real `runtime.c` compiled and linked in by
this crate's `build.rs` — not a Rust reimplementation of it, so there's no
risk of the refcounting subtly drifting from what the host trusts.

## Quick start

```toml
# Cargo.toml
[lib]
crate-type = ["cdylib"]

[dependencies]
code-native = "0.1"
```

```rust
// src/lib.rs
use code_native::*;

#[no_mangle]
pub extern "C" fn code_module_abi_version() -> u32 {
    CODE_ABI_VERSION
}

#[no_mangle]
pub unsafe extern "C" fn code_module_dispatch(out: *mut CodeValue, particle: *const CodeValue) {
    let particle = &*particle;
    match read_field_str(particle, "_class") {
        Some("Double") => {
            let value = read_field_number(particle, "value").unwrap_or(0.0);
            make_result(&mut *out, c"DoubleResult", |slot| number(slot, value * 2.0));
        }
        // A class this module does not handle answers null — see
        // docs/todo/errors-as-particles.md.
        _ => null(out),
    }
}
```

```bash
cargo build --release
# target/release/libmymodule.so
```

```
-- my_script.code
link "libmymodule.so" as m
emit Double { "_class": "Double", "value": 21 } to m get result
assert result.value = 42
```

Run it with `code run my_script.code`, or `code build my_script.code` to
compile a native binary that dlopen's the module at startup — both output
modes accept a `.so` the same way.

## Why only two required exports

`code_module_abi_version` and `code_module_dispatch` are it. There's no
`code_module!` macro generating boilerplate here (unlike the *old*
language's own `code-native`, which generated a whole descriptor table of
handlers/vars/types/emissions): the new ABI dropped that design for one
function a module dispatches through itself — see
[`docs/todo/native-module-linking.md`](https://github.com/codelovesme/code/blob/main/docs/todo/native-module-linking.md)
in the main repo for why. `code_release` needs no code from you at all — it
comes from the `runtime.c` this crate links in automatically.

## Exported variables

Optional third export, `code_module_vars`, is what makes `m.someConst` work
alongside `emit ... to m`:

```rust
use code_native::*;
use std::sync::OnceLock;

static VARS: OnceLock<CodeVarList> = OnceLock::new();

#[no_mangle]
pub extern "C" fn code_module_vars() -> *const CodeVarList {
    VARS.get_or_init(|| {
        let mut buf = SlotBuffer::new(1);
        number(buf.slot_mut(0), 3.14159);
        let values = buf.slot_mut(0) as *mut CodeValue;
        // Both leaked deliberately: the ABI requires this data to stay
        // valid for the module's whole lifetime, the same requirement a C
        // module meets with `static` storage.
        std::mem::forget(buf);
        let names: &'static [*const std::ffi::c_char] = Box::leak(Box::new([c"pi".as_ptr()]));
        CodeVarList { count: 1, names: names.as_ptr(), values }
    })
}
```

See `code_abi.h`'s own doc comment (vendored into this crate at
`vendor/code_abi.h`) for the full `CodeVarList` contract — this crate
mirrors it field-for-field rather than hiding it, since building the
`'static`-lifetime buffer correctly is easier to get right by following the
same shape a C module uses than behind a leaky abstraction.

## Failing without ending the program

**A module may never bring the application down.** Report a failure by
returning an `Exception` — the program receives it as an ordinary value,
tests it with `is Exception`, and may read `message` or ignore it entirely:

```rust
exception(out, "mymodule", "cannot open the door");
```

`code_runtime_error` is deprecated for module use and will leave this crate
once the C runtime has an error channel of its own.

### `guarded` — and why it cannot live in the host

Wrap your dispatch in `guarded` so a panic becomes an `Exception` too:

```rust
#[no_mangle]
pub unsafe extern "C" fn code_module_dispatch(
    out: *mut CodeValue,
    particle: *const CodeValue,
) {
    let particle = &*particle;
    guarded(&mut *out, "mymodule", |out| {
        match read_field_str(particle, "_class").unwrap_or("") {
            "Double" => { /* ... */ }
            _ => null(out),
        }
    })
}
```

This is not something the host could do for you. A panic escaping an
`extern "C"` function **aborts** the process rather than unwinding, so the
host's own `catch_unwind` never runs — the catch has to happen on this side
of the FFI boundary. `tests/native_modules/test_panics` exists to keep that
true.

It covers what "written wrong" usually means: `unwrap`/`expect`, index and
slice bounds, arithmetic overflow, explicit `panic!`/`assert!`, and panics
from inside dependencies. It cannot cover a deliberate `exit`, an infinite
loop, or undefined behaviour reached through `unsafe`.

**This is why Rust is the recommended path for a third-party module.** In C
the same guarantee does not exist: a module that forgets a NULL check
segfaults, and an integer `100 / 0` raises SIGFPE — neither is catchable by
anything, in any language, from anywhere. (Rust will not even compile the
latter.) The C path stays as the ABI's reference implementation; production
modules should take this one.

## Speaking first (inbound emissions)

A module normally only answers an `emit`. To push particles *into* the
program on its own initiative — an event source, or a module reporting what
went wrong — take the optional `code_module_set_inbound` export and push:

```rust
use code_native::*;

// Generates `code_module_set_inbound`. A macro rather than a function in
// this crate on purpose: a `#[no_mangle]` symbol defined in a *dependency*
// is not reliably kept in the final cdylib, so the export has to be emitted
// in your crate.
code_native::declare_inbound!();

fn report(message: &str) {
    let mut particle = CodeValue::zeroed();
    let mut buf = SlotBuffer::new(3);
    borrowed_str(buf.slot_mut(0), c"Exception");
    borrowed_str(buf.slot_mut(1), c"mymodule");
    owned_str(buf.slot_mut(2), message);
    object(&mut particle, &[c"_class", c"source", c"message"], &mut buf);
    buf.release_all();
    emit_inbound(&particle);
    release(&mut particle);
}
```

Pushed particles reach the **program's** handlers (not the module's own),
dispatched between top-level statements. Two things worth knowing:

- `emit_inbound` returns `false` when the host never took an inbound
  channel. Pushing is always best-effort, and a module has to stay correct
  when nobody is listening.
- A pushed class the program has no handler for is **dropped**. That is what
  lets a module report something without every program that links it having
  to handle it. Since 2026-08-28 the outbound direction agrees: `emit ... to
  <anything>` with no matching handler is null, not an error.

The queue is bounded at `CODE_INBOUND_CAPACITY` (256) per module, dropping
the oldest, so a module that outruns the program costs bounded memory.

## `.a` static modules

Everything above is the `.so` path — `code_abi.h`'s "primary format", the
one artifact both `code run` and `code build` accept. A `.a` uses a
different, simpler contract: it links straight into the host binary, so
there is no deep-copy boundary, no per-module `code_release`, and exactly
one runtime — the host's. In exchange it needs a symbol prefix, since every
`.a` linked into one program shares a flat symbol table.

Two changes to your `Cargo.toml`:

```toml
[lib]
crate-type = ["staticlib"]

[dependencies]
code-native = { version = "1", default-features = false, features = ["static-module"] }
```

`static-module` is what makes this work: without it this crate compiles the
vendored `runtime.c` into your archive, and linking that against a host that
already has one is `multiple definition of 'code_release'` — forty-one
symbols over. With it, the crate brings no runtime and calls the host's.

Then prefix your exports with a name unique among every `.a` the program
will link alongside:

```rust
#[no_mangle]
pub extern "C" fn mymath_code_module_abi_version() -> u32 { CODE_ABI_VERSION }

#[no_mangle]
pub unsafe extern "C" fn mymath_code_module_dispatch(
    out: *mut CodeValue,
    particle: *const CodeValue,
) { /* ... */ }
```

Nothing in the language names the prefix — `code build` finds it by running
`nm` on the archive, so it only has to be unique. `code_module_vars` takes
the prefix too if you export it. A working example is
`tests/native_modules/test_math_static/` in the main repo.

`code run` refuses a `.a` outright (there is no `dlopen` for an archive), so
fixtures that link one are `buildonly_*`.

## Safety

Most of this crate's surface is safe Rust, but `code_module_dispatch` itself
is necessarily `unsafe extern "C" fn` — it's called across an FFI boundary
with a raw pointer the host guarantees is valid, which Rust has no way to
express short of `unsafe`. Everything you do *inside* the handler
(`read_field_str`, `number`, `make_result`, …) is safe.

`code-native` on crates.io already has `0.2.0` published under this same
repo + account, from the *old* language — a completely different API (the
macro/descriptor-table design this README's "Why only two required
exports" section explains is gone) and a different license (that one's
MIT; this one is GPL-3.0, since it links `runtime.c` from the main repo
directly rather than reimplementing it). That's why this package starts at
**`1.0.0`** rather than continuing the `0.2.x` line: a real break — API and
license both — deserves a major version, not one a `^0.2` pin would
silently accept. (Same call `crates/code-wasm` made for its own npm
package, for the same reason.)

## Releasing (maintainers)

Published via **crates.io Trusted Publishing (OIDC)** from GitHub Actions
(`.github/workflows/publish-crates-native.yml`) — no `CARGO_REGISTRY_TOKEN`
stored anywhere, mirroring `crates/code-wasm`'s npm Trusted Publishing setup.

**One-time setup** (crates.io account configuration, can't be done from
CI) — normally the very first publish has to be manual, since crates.io
has no equivalent to npm's *Staged Packages* for configuring a trusted
publisher before a crate exists at all. Not needed here: the crate already
exists (see above), owned by the same account doing this setup, so go
straight to its **Settings → Trusted Publishing** page and add:

- Repository owner: `codelovesme`
- Repository name: `code`
- Workflow filename: `publish-crates-native.yml`
- Environment: leave blank

**Every release after that** is the repository's own release tag — there is
no separate one. Everything published from this repo shares one version, so
`code v1.1.0`, `code-native 1.1.0` and every module at 1.1.0 ship together
and a consumer never has to check whether they match (`tests/one_version.rs`
holds every manifest to it):

```bash
git tag v1.1.0   # whatever the new version is
git push origin v1.1.0
```

The workflow sets `Cargo.toml`'s version from the tag itself, verifies the
package builds standalone (`cargo publish --dry-run`), and publishes.
`workflow_dispatch` (the "Run workflow" button in the Actions tab) does
everything except the actual publish — a real dry run against the exact
package that would ship.

## License

GPL-3.0 — see [LICENSE](./LICENSE). This crate links `runtime.c` from the
main `code` repo directly (vendored, not reimplemented), so it carries the
same license.
