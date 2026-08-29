//! The `terminal` native module — printing to stdout for the Code
//! programming language, written in Rust on [`code-native`].
//!
//! Handlers:
//!
//! - `Print` — `{ value = … }`, renders one line to stdout and answers
//!   `TerminalResult` carrying how many characters landed on the wire, so a
//!   program can `assert` that the print happened.
//!
//! The language has no print statement of its own ("No core I/O" — reaching
//! the outside world goes through a module), so this module is where that
//! lands.
//!
//! Rewritten from C on 2026-08-28. It was the language's reference C module,
//! kept in that form so a reader could see the ABI with no framework in the
//! way. What changed the tradeoff is the guarantee a module now owes: it may
//! never end the application (`docs/todo/errors-as-particles.md`), and that
//! is real in Rust and unattainable in C — a forgotten NULL check segfaults
//! and an integer `100 / 0` raises SIGFPE, neither catchable from anywhere.
//! `terminal` ships to users, so it takes the path where the promise holds.
//! The `.a` doubles under `tests/native_modules/` keep the C header's
//! declarations exercised.

use code_native::*;

/// The ABI version this module speaks. Must equal `CODE_ABI_VERSION` or the
/// host refuses to load us.
#[no_mangle]
pub extern "C" fn code_module_abi_version() -> u32 {
    CODE_ABI_VERSION
}

/// The single dispatch point. A class this module does not handle answers
/// null; a `Print` it cannot render answers an `Exception`. Neither ends the
/// program.
///
/// # Safety
///
/// Both pointers must be valid for reads/writes respectively for the
/// duration of the call, and refer to values laid out per `code_abi.h` —
/// the host guarantees this on every dispatch (see `native.rs`).
#[no_mangle]
pub unsafe extern "C" fn code_module_dispatch(out: *mut CodeValue, particle: *const CodeValue) {
    let particle = &*particle;
    guarded(&mut *out, "terminal", |out| {
        if read_field_str(particle, "_class") != Some("Print") {
            null(out);
            return;
        }
        // A field the particle does not carry is null, and null renders as
        // "null" like any other value — `Print { }` is `Print { value =
        // null }`. This module used to refuse it, which was a validation gate
        // in front of a handler that has nothing to validate: there is no
        // value it cannot render (owner's rule, 2026-08-28).
        let line = match find_field(particle, "value") {
            Some(value) => render(value),
            None => "null".to_string(),
        };
        // Straight to stdout, newline-terminated, flushed so the line is
        // visible immediately even under pipe buffering. This is the whole
        // module.
        println!("{line}");
        use std::io::Write;
        let _ = std::io::stdout().flush();

        // The count is of *characters as rendered*, which is what the C
        // version reported too: `len` there was a byte count over output
        // that `render` keeps ASCII except for a string the caller supplied.
        make_result(out, c"TerminalResult", |slot| {
            number(slot, line.len() as f64)
        });
    })
}

/// Render a value the way a person expects to read it: strings bare (they
/// are already text — quoting would make `Print "hi"` show `"hi"`), numbers
/// integral when they have no fractional part (so `Print 5` shows `5`, not
/// `5.0`), everything else in a stable readable form.
///
/// Deliberately simple: this is a console, not a serializer. Arrays and
/// objects report their size rather than their contents, which keeps the
/// line honest without pulling in a JSON encoder — and keeps the reported
/// count meaningful, since a nested value could be any length.
fn render(v: &CodeValue) -> String {
    if let Some(s) = read_str(v) {
        return s.to_string();
    }
    if let Some(n) = read_number(v) {
        // The same integral test the C version used: exactly representable
        // as an i64, and inside the range where f64 counts by ones.
        if n == (n as i64) as f64 && (n as i64).abs() < (1i64 << 53) {
            return format!("{}", n as i64);
        }
        // `{}` on f64 is Rust's shortest-round-trip form, which the language
        // itself uses for number-to-text everywhere else (`src/runtime.c`'s
        // `text_push_number`), so `Print` agrees with interpolation.
        return format!("{n}");
    }
    if let Some(b) = read_bool(v) {
        return b.to_string();
    }
    match v.tag {
        CodeTag::Null => "null".to_string(),
        CodeTag::Array => format!("[{} items]", v.len),
        CodeTag::Object => format!("{{{} fields}}", v.len),
        // Unreachable: every tag is handled above. Rendered rather than
        // panicked on, because this module must not be the thing that ends
        // a program.
        _ => String::new(),
    }
}
