//! The `dom` native module — putting a tree of nodes on the page.
//!
//! Handlers:
//!
//! - `Render { into?, styles?, tree }` — replaces the contents of `into` (a
//!   CSS selector, `"body"` by default) with `tree`, sets `styles` as the
//!   page's stylesheet, and answers `RenderResult { ok }`. `ok` is false when
//!   the selector matched nothing.
//!
//! # The tree is a value, not markup
//!
//! A node is `{ tag, attrs?, on?, children? }` and a string is a text node:
//!
//! ```text
//! { tag = "ul", children = [
//!     { tag = "li", attrs = { class = "row" }, children = ["kahve"] }
//! ] }
//! ```
//!
//! That is the whole vocabulary. There is **no raw HTML and no property
//! assignment** — a tree can describe elements, attributes, events and text,
//! and nothing else. So a tree built out of someone's name or a translated
//! string is data all the way to the page and cannot become code on the way.
//! The page's half is held to the same rule.
//!
//! # Events
//!
//! `on` maps an event name to **the particle it means**:
//!
//! ```text
//! { tag = "button", on = { click = { _class = "Remove", id = 7 } } }
//! { tag = "input",  on = { input = "Typed" } }
//! ```
//!
//! A whole particle, written where the node is: fields and all, exactly as
//! the handler will receive them. `on = { click = "Add" }` is the short way
//! of writing `{ _class = "Add" }`, for an event with nothing else to say.
//!
//! When it happens the page sends that back, adding what the element holds —
//! the text a reader typed, or whatever the application wrote on the node —
//! as `value`, unless the particle already names one. So `Remove { id = 7 }`
//! arrives with its `id`, and `Typed { value = "ne yazdiysa" }` arrives with
//! what was typed, and the application answers both with an ordinary handler
//! written in a gene like any other.
//!
//! **A listener is never a function, and nothing is held between renders.**
//! `on` is data like every other field: this module serialises it and forgets
//! it. There is no table of live listeners to grow, go stale, or be swept,
//! and a page redrawn a thousand times costs exactly what one render costs.
//!
//! `tree` may also be a **string**, which is taken as JSON that is already in
//! this shape and passed through untouched. That is for an application that
//! built the text itself; the ordinary case is to hand over a value and let
//! this module serialise it.
//!
//! # Where appearance lives
//!
//! In the same particle, and **not on the nodes**:
//!
//! ```text
//! emit Render {
//!     styles = {
//!         ".cart"  = { "max-width" = "24rem", padding = "1rem" },
//!         ".total" = { "font-weight" = "600" }
//!     },
//!     tree = { tag = "p", attrs = { class = "total" }, children = ["57 TL"] }
//! } to dom
//! ```
//!
//! A node says what it *is* — `class = "total"` — and the rules say what that
//! looks like, once, in one place. Both travel in the same JSON, so there is
//! no stylesheet file to keep in step with the application and nothing to
//! serve beside it.
//!
//! That split is the whole point. Colours and positions written onto every
//! node would make the code that builds a page *be* the page's design, which
//! is exactly what a gene should not turn into. A genuinely per-node value —
//! a bar's width computed from data — is an ordinary attribute
//! (`attrs = { style = "width: 40%" }`) and needs nothing from this module.
//!
//! `styles` is a value, not CSS text: selector to properties to values. So
//! there is no stylesheet to parse, and nothing that could end a rule early
//! and start a different one.
//!
//! # Where it works
//!
//! Only in a wasm build, because a page is the only thing this module talks
//! to. On a machine every handler answers an `Exception` saying so, rather
//! than pretending to draw — the module is still linkable there so that one
//! application can be built for both and find out at runtime (`Linked`)
//! which it is.
//!
//! The page supplies one function, and it is the only thing this module can
//! reach: it takes the JSON and the selector, and answers whether the
//! selector matched.
//!
//! # Where the drawing actually happens
//!
//! In the page's half, which `code build --target wasm` writes beside the
//! module it built. **A module speaks particles in both directions** — toward
//! the language and toward the page — so this module's wasm half hands the
//! `Render` over as JSON and reads back the `RenderResult`, and does nothing
//! else. That half is written once for every browser module
//! (`crates/modules/browser_half.rs`) and included here.
//!
//! It used to reach the page through an import of its own devising, taking
//! the particle apart to do it — a JSON writer, a node walker and a fixed
//! buffer, per module. All of that is gone: the interesting half, what a
//! `Render` actually *means*, is written once in the only language that can
//! reach a page.
//!
//! The wasm half brings no standard library, which is now almost nothing to
//! bring. See `web/README.md` for the rest.
//!
#![cfg_attr(target_arch = "wasm32", no_std)]

#[cfg(not(target_arch = "wasm32"))]
mod machine {
    //! Where there is no page. Every handler answers an `Exception` saying
    //! so, rather than pretending to draw — an application drawing into
    //! nothing is a mistake worth naming, not a page that happened to be
    //! empty. The module is still linkable here so one application can be
    //! built for both and ask `Linked` which it is.
    use code_native::*;

    const NO_PAGE: &str =
        "there is no page here — `dom` draws in a browser, and this program is running on a \
         machine. Ask `Linked` to find out which you are";

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
        guarded(&mut *out, "dom", |out| {
            match read_field_str(particle, "_class") {
                // A class this module does not handle answers null and does
                // not end the program: it may have been meant for something
                // else entirely.
                Some("Render") => exception(out, "dom", NO_PAGE),
                _ => null(out),
            }
        })
    }
}

#[cfg(target_arch = "wasm32")]
mod page {
    include!("../../browser_half.rs");

    browser_half!("dom", dom_code_module_abi_version, dom_code_module_dispatch);
}
