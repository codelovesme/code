//! A native module for the Code programming language.
//!
//! Rename `greet` throughout — here, in `Cargo.toml`, in `tests/greet.code`
//! and in `.github/workflows/publish.yml` — and replace the handler below
//! with your own. What is worth keeping is the *shape*: it is the shape every
//! first-party module has, and each part of it is load-bearing.
//!
//! Handlers:
//!
//! - `Greet { name }` — answers `GreetResult { value }`
//!
//! `code_release` needs no code here: `code-native` links the vendored
//! `runtime.c` into the cdylib and re-exports it.

use code_native::*;

/// The ABI version this module speaks. Must equal `CODE_ABI_VERSION` or the
/// host refuses to load it — which is the point: a module built against an
/// older ABI is refused at link time rather than misreading values later.
#[no_mangle]
pub extern "C" fn code_module_abi_version() -> u32 {
    CODE_ABI_VERSION
}

/// The single entry point. Read the particle's `_class`, route to a handler.
///
/// Three rules are worth copying exactly:
///
/// 1. **`guarded` wraps everything.** A module may never end the host
///    program — that is a hard rule of the language, not a courtesy. A panic
///    escaping an `extern "C"` function *aborts* rather than unwinding, so
///    the host cannot catch it; `guarded` catches it on this side of the
///    boundary and turns it into an `Exception`. Without it, an `unwrap` on
///    a `None` kills someone else's program.
/// 2. **A class you do not handle answers null**, not an error. Sending a
///    particle is not a demand, and whether to act on one is the recipient's
///    business. A program may link several modules and emit to all of them.
/// 3. **Failures come back as values.** `exception(out, …)` returns an
///    `Exception` particle the program can inspect with `is Exception` —
///    or ignore, which is its right.
///
/// # Safety
///
/// Both pointers must be valid for reads/writes respectively for the
/// duration of the call, and refer to values laid out per `code_abi.h`. The
/// host guarantees this on every dispatch.
#[no_mangle]
pub unsafe extern "C" fn code_module_dispatch(out: *mut CodeValue, particle: *const CodeValue) {
    let particle = &*particle;
    guarded(&mut *out, "greet", |out| {
        let outcome = match read_field_str(particle, "_class").unwrap_or("") {
            "Greet" => greet(out, particle),
            _ => {
                null(out);
                Ok(())
            }
        };
        if let Err(message) = outcome {
            exception(out, "greet", &message);
        }
    })
}

/// `Greet { name }` -> `GreetResult { value: "hello, <name>" }`.
///
/// Note what is *not* here: a check that `name` was supplied. A field the
/// particle does not carry reads as null, exactly as `.field` does in the
/// language itself, and emitting is not filling in a form — a handler is
/// never refused over the fields it happens to declare. So there is one
/// question to ask (is it a string?) rather than two, and `Greet {}` gets the
/// same answer as `Greet { "name": null }`.
fn greet(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let name = find_field(particle, "name")
        .and_then(read_str)
        .ok_or("Greet requires a string 'name'")?;
    let text = format!("hello, {name}");
    make_result(out, c"GreetResult", |slot| owned_str(slot, &text));
    Ok(())
}
