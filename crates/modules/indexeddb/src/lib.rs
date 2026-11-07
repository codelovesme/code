//! The `indexeddb` module — what a page keeps that is too large for
//! `storage`: photos, recordings, a whole offline copy of an application's
//! records.
//!
//! `storage` is the browser's `localStorage`: text, a few megabytes shared by
//! every application on the origin, answered at once. This is the browser's
//! IndexedDB: any value the language has (objects, lists, long text), as
//! much as the browser allows the origin, kept until the application or the
//! person removes it — and answered **later**, because IndexedDB is
//! asynchronous and waiting for it would block the page.
//!
//! Handlers (each answers `<Name>Result { ok, id }` at once; `id` is the
//! number the later answer carries):
//!
//! - `Put { key, value }` → later `DbStored { id, key }`
//! - `Get { key }` → later `DbFound { id, key, value }` (`value = null` when
//!   nothing is kept under the key)
//! - `Delete { key }` → later `DbDeleted { id, key }`
//! - `Keys { prefix? }` → later `DbKeys { id, prefix, keys }`, sorted
//! - `Space {}` → later `DbSpace { id, usage, quota, persisted }` — bytes,
//!   as the browser estimates them
//! - `Persist {}` → later `DbSpace { … }` after asking the browser to keep
//!   this origin's data through storage pressure
//!
//! Anything that fails arrives as `DbFailed { id, key, reason, full }`,
//! `full = true` when the browser said the origin is out of space — a page
//! that cannot keep something must say so, never claim it did.
//!
//! One store per origin, as `storage` is one per origin: an application
//! keeps its keys under its own prefix (`keepsake:<who>:…`).
//!
//! # Where it works
//!
//! **A browser.** On a machine every handler answers an `Exception`: a
//! program with no page has a disk, and `fs` or a database is the way to it.
//!
//! For wasm it is built as an archive linked into the program:
//!
//! ```bash
//! cargo rustc --target wasm32-unknown-unknown --release --crate-type staticlib
//! ```

#![cfg_attr(target_arch = "wasm32", no_std)]

#[cfg(not(target_arch = "wasm32"))]
mod machine {
    use code_native::*;

    const NO_BROWSER: &str =
        "there is no browser here — `indexeddb` is the page's own store, and this program is \
         running on a machine. A machine keeps things with `fs` or a database";

    #[no_mangle]
    pub extern "C" fn code_module_abi_version() -> u32 {
        CODE_ABI_VERSION
    }

    /// # Safety
    ///
    /// Both pointers must be valid for the duration of the call and laid out
    /// per `code_abi.h` — the host guarantees this on every dispatch.
    #[no_mangle]
    pub unsafe extern "C" fn code_module_dispatch(out: *mut CodeValue, particle: *const CodeValue) {
        let particle = &*particle;
        guarded(&mut *out, "indexeddb", |out| match read_field_str(particle, "_class") {
            Some("Put") | Some("Get") | Some("Delete") | Some("Keys") | Some("Space")
            | Some("Persist") => exception(out, "indexeddb", NO_BROWSER),
            _ => null(out),
        })
    }
}

#[cfg(target_arch = "wasm32")]
mod page {
    include!("../../browser_half.rs");

    browser_half!(
        "indexeddb",
        indexeddb_code_module_abi_version,
        indexeddb_code_module_dispatch
    );
}
