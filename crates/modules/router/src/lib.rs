//! The `router` native module — where in the application the reader is.
//!
//! Handlers:
//!
//! - `Route {}` — answers `RouteResult { value }`, the path shown now.
//! - `Navigate { path }` — goes there, and answers `NavigateResult { ok }`.
//!   The page's history gains an entry, so Back means what a reader expects.
//! - `Watch { then }` — answers `WatchResult { ok }`, and from then on every
//!   change of the path — a link, Back, Forward, an address typed by hand —
//!   arrives as a particle of class `then`, carrying the new path as
//!   `path`.
//! - `Where {}` — answers `WhereResult { origin, protocol, hostname, port }`:
//!   what this page was served from. `origin` is null where there is none
//!   (a file opened straight off disk, `file://`).
//!
//! # Where answers what `Route` cannot
//!
//! `Route` and `Navigate` are the hash — the part of the address this
//! module's own application controls, hosted or alone. `Where` is
//! everything before it: the scheme, the host, the port. An application
//! that talks to a service of its own needs this and nothing else, because
//! there is no module that knows a deployment's address for it — the page
//! it is loaded from already is that address.
//!
//! A guest's own `router` never narrows it — `Route` is scoped there, but
//! `Where` reads the page directly rather than through that per-guest
//! slice. A shell that takes the whole module over for a guest can answer
//! `Where` however it likes, same as for anything else it took; that is an
//! ordinary hosting decision, not something this module arranges.
//!
//! # Watching is the whole of it
//!
//! ```text
//! emit Watch { then = "Went" } to router get w
//!
//! Went { path } => {
//!     ...draw the page for `path`
//! }
//! ```
//!
//! The application names the class it wants back, in advance — the same rule
//! `dom` follows for a click. What that buys is a fixed *shape*: what arrives
//! is always a class and at most one piece of text, so a page cannot invent a
//! particle with fields of its own choosing.
//!
//! It is not a boundary, and it is worth saying so plainly. A page and the
//! module it loaded share one memory, and nothing stops a page from naming a
//! class the application never offered it. Anything that could is already
//! able to write to that memory directly. What actually protects an
//! application from the page it runs in is on the other side of the network,
//! where the two really are separate.
//!
//! `Navigate` fires it too. An application that draws in one place — the
//! handler — does not then have to draw again at every call site, and the
//! two ways a path can change stop being two paths through the code.
//!
//! # Where it works
//!
//! **A browser.** On a machine every handler answers an `Exception` saying
//! so. The module is still linkable there, so one application can be built
//! both ways and ask [`Linked`](../../../README.md#linked) which it is.
//!
//! For wasm it is built as an archive linked into the program:
//!
//! ```bash
//! cargo rustc --target wasm32-unknown-unknown --release --crate-type staticlib
//! ```
//!
//! # What the page has to supply
//!
//! A half that answers this module's particles, in
//! [`web/host.mjs`](../../../web/README.md) — which `code build --target
//! wasm` writes beside the module it built, holding the halves of exactly
//! the modules the program linked.
//!
//! Nothing crosses as a pointer or a length: the particle goes over as JSON
//! and the answer comes back the same way, through one door shared by every
//! browser module. So this module's wasm half has nothing to do but hand the
//! particle over — see `crates/modules/browser_half.rs`, which is that half,
//! written once.
//!
//! Its wasm half is `no_std` and hand-written against `code_abi.h` — see
//! `dom`'s file for why a module meant for the browser brings no standard
//! library.

#![cfg_attr(target_arch = "wasm32", no_std)]

#[cfg(not(target_arch = "wasm32"))]
mod machine {
    //! Where there is no address bar. Every handler answers an `Exception`
    //! saying so rather than inventing a path: a program that thought it
    //! knew where it was would draw the wrong page and never find out why.
    use code_native::*;

    const NO_BROWSER: &str =
        "there is no address bar here — `router` is where a page is, and this program is \
         running on a machine. Ask `Linked` to find out which you are";

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
        guarded(&mut *out, "router", |out| {
            match read_field_str(particle, "_class") {
                // A class this module does not handle answers null and does
                // not end the program: it may have been meant for something
                // else entirely.
                Some("Route") | Some("Navigate") | Some("Watch") | Some("Where") => {
                    exception(out, "router", NO_BROWSER)
                }
                _ => null(out),
            }
        })
    }
}

#[cfg(target_arch = "wasm32")]
mod page {
    include!("../../browser_half.rs");

    browser_half!(
        "router",
        router_code_module_abi_version,
        router_code_module_dispatch
    );
}
