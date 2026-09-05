//! The `terminal` native module — printing a line to wherever this program's
//! output goes, written in Rust on [`code-native`].
//!
//! Handlers:
//!
//! - `Print` — `{ value = … }`, renders one line and answers
//!   `TerminalResult` carrying how many characters landed on the wire, so a
//!   program can `assert` that the print happened.
//!
//! **Where the line goes depends on where the program is running**, and that
//! is the whole of the difference between the two builds. On a machine it is
//! stdout. In a browser there is no such thing, so the line goes out through
//! one function the page supplies and lands in its console. The particle is
//! the same, the rendering is the same, and an application prints without
//! knowing which of the two it is — which is the point: a second module
//! called `console` would have made every program choose.
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

/// Writes one finished line.
///
/// The only part of this module that differs between the two targets. Both
/// halves take the same rendered text and neither can fail in a way the
/// caller could act on, so `Print` answers the same thing either way.
mod out {
    /// Straight to stdout, newline-terminated, flushed so the line is
    /// visible immediately even under pipe buffering.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn line(text: &str) {
        use std::io::Write;
        println!("{text}");
        let _ = std::io::stdout().flush();
    }

    /// Out through the page. `code_web_log` is an import the host fills in
    /// when it instantiates the module — one function, and the only thing
    /// this module needs from outside. A host that supplies nothing gets a
    /// link error naming it, which is better than a silent print into
    /// nowhere.
    #[cfg(target_arch = "wasm32")]
    pub fn line(text: &str) {
        extern "C" {
            fn code_web_log(ptr: *const u8, len: usize);
        }
        // SAFETY: the pointer and length describe `text`, which outlives the
        // call; the host only reads it.
        unsafe { code_web_log(text.as_ptr(), text.len()) }
    }
}

/// The ABI version this module speaks. Must equal `CODE_ABI_VERSION` or the
/// host refuses to load us.
///
/// Unprefixed for a `.so`, which has a symbol namespace of its own.
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle]
pub extern "C" fn code_module_abi_version() -> u32 {
    CODE_ABI_VERSION
}

/// The same, prefixed — every `.a` linked into one program shares a flat
/// symbol table, so the prefix is what tells two modules apart. `code build`
/// discovers it by reading the archive; nothing in the language names it.
#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn terminal_code_module_abi_version() -> u32 {
    CODE_ABI_VERSION
}

/// The wasm half of [`code_module_dispatch`], under the prefixed name.
///
/// # Safety
///
/// As [`code_module_dispatch`].
#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub unsafe extern "C" fn terminal_code_module_dispatch(
    out: *mut CodeValue,
    particle: *const CodeValue,
) {
    dispatch(out, particle)
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
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle]
pub unsafe extern "C" fn code_module_dispatch(out: *mut CodeValue, particle: *const CodeValue) {
    dispatch(out, particle)
}

/// # Safety
///
/// As [`code_module_dispatch`].
unsafe fn dispatch(out: *mut CodeValue, particle: *const CodeValue) {
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
        out::line(&line);

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
