//! The `guest` native module — running another application inside this one.
//!
//! Handlers:
//!
//! - `Load { app, url, into }` — fetches a `.wasm`, starts it, and lets it
//!   draw inside `into`. Answers `LoadResult { ok, reason }`.
//! - `Unload { app }` — lets it go. Answers `UnloadResult { ok }`.
//! - `Tell { app, particle }` — hands a particle to the guest's own handlers,
//!   which is how a host says anything to what it is running.
//!
//! # What a guest is
//!
//! One `.wasm`, and nothing else. Its own code, its modules and the language
//! runtime are already inside it, so there is no manifest to read, no assets
//! to sequence and no modules to fetch. Its whole interface to whoever
//! runs it is four imports and five exports.
//!
//! It does not know it is a guest. The same file runs on its own page, and
//! the only way it could tell is by asking `session` who is signed in — which
//! an application that does not ask, cannot.
//!
//! # Who answers a guest
//!
//! The same two questions a host answers on a machine, in the same words.
//! `Offer` the first time a guest reaches for a module, `Module` for every
//! particle to one the host took:
//!
//! ```text
//! Offer { app, name } => {
//!     if name = "storage" { return Denied { } }
//!     if name = "router" { return Offered { } }
//!     | anything else: no answer, and the guest keeps its own
//! }
//!
//! Module { app, name, particle } => {
//!     return RouteResult { value = "/" + app }
//! }
//! ```
//!
//! `Denied` answers the guest with an `Exception` on first use, so a refusal
//! reaches it as an ordinary failure rather than as a silence. `Offered` puts
//! the host between the guest and that module for good: every particle
//! arrives at `Module`, to be answered in its place or forwarded to the
//! host's own copy. Answering neither leaves the guest with the page's own
//! half — a host that writes no handler hosts an application without taking
//! anything from it, which is what writing no `Offer` means on a machine too.
//!
//! What "its own" can mean is the one thing that differs here. A held `.so`
//! opens its own modules — its own file, its own settings — and a host that
//! says nothing never sees them. A page has no dlopen: every half a guest can
//! reach is this page's, out of the host's own build. So a guest's own module
//! is the same half given a world of its own, which is what the containment
//! below is.
//!
//! **The host decides; this module does.** It could have been the other way —
//! the host doing the drawing itself — but then the nodes would be the
//! host's, and a click on them would reach the host's handlers rather than
//! the guest's. What a guest draws has to stay the guest's.
//!
//! # What a guest cannot reach
//!
//! Its `dom` is given a document that stops at its container: `body` means
//! the container, a selector cannot match outside it, and its stylesheet is
//! rewritten so every rule is scoped to it. Two guests can be open at once
//! without either seeing the other, and neither can restyle the host.
//!
//! Its `router` sees the path after its name, so the host keeps the address
//! bar.
//!
//! Its **storage is not narrowed**. One origin is one store: applications
//! served from the same place already share it when they run alone, so a
//! guest that could not see what it wrote on its own page would be a
//! different application for being hosted — and the session a shell signed in
//! would be invisible to everything the shell runs. A shell that wants a
//! guest kept apart offers `storage` and namespaces it in its own handlers,
//! where that is a decision rather than a rule.
//!
//! None of that is a boundary — a guest shares this page's memory like
//! everything else here, and the page could read all of it. It is containment
//! of an honest application, which is what a shell of one's own applications
//! needs.
//!
//! # Where it works
//!
//! **A browser.** On a machine every handler answers an `Exception` saying
//! so: running another application on a machine is what `host` does, by
//! linking it, and this is not that.

#![cfg_attr(target_arch = "wasm32", no_std)]

#[cfg(not(target_arch = "wasm32"))]
mod machine {
    //! Where there is no page. A machine hosts an application by linking it
    //! (`code_abi.h` item 10), which is a different thing done a different
    //! way — so this says so rather than pretending to be it.
    use code_native::*;

    const NO_PAGE: &str =
        "there is no page here — `guest` runs one application inside another in a browser. On \
         a machine a program hosts another by linking it; see `code_abi.h`'s hosting section. \
         Ask `Linked` to find out which you are";

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
        guarded(&mut *out, "guest", |out| {
            match read_field_str(particle, "_class") {
                Some("Load") | Some("Unload") | Some("Tell") => exception(out, "guest", NO_PAGE),
                _ => null(out),
            }
        })
    }
}

#[cfg(target_arch = "wasm32")]
mod page {
    include!("../../browser_half.rs");

    browser_half!(
        "guest",
        guest_code_module_abi_version,
        guest_code_module_dispatch
    );
}
