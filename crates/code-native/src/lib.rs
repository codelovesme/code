//! Safe(r) Rust bindings for writing a native `.so` module for the [Code
//! programming language](https://github.com/codelovesme/code).
//!
//! `code_abi.h`'s contract needs two things from a module: agreement on the
//! `CodeValue` wire layout, and a `code_release` (plus friends) built from
//! the *real* `runtime.c` rather than a reimplementation that merely looks
//! compatible — getting refcounting subtly wrong is the kind of bug that
//! corrupts memory rather than crashing where you'd notice. This crate's
//! `build.rs` compiles the vendored `runtime.c` and links it into your
//! `cdylib` directly, so every function below calls the same code the host
//! runtime and every C module trust.
//!
//! # Quick start
//!
//! ```rust,ignore
//! use code_native::*;
//!
//! #[no_mangle]
//! pub extern "C" fn code_module_abi_version() -> u32 {
//!     CODE_ABI_VERSION
//! }
//!
//! #[no_mangle]
//! pub unsafe extern "C" fn code_module_dispatch(out: *mut CodeValue, particle: *const CodeValue) {
//!     let particle = &*particle;
//!     match read_field_str(particle, "_class") {
//!         Some("Double") => {
//!             let value = read_field_number(particle, "value").unwrap_or(0.0);
//!             make_result(&mut *out, "DoubleResult", |slot| code_number(slot, value * 2.0));
//!         }
//!         // A class this module does not handle answers null — see
//!         // docs/todo/errors-as-particles.md.
//!         _ => null(&mut *out),
//!     }
//! }
//! ```
//!
//! Build with `crate-type = ["cdylib"]`, then `link "libmymodule.so" as m`
//! from `.code` source. See this crate's README for the full walkthrough,
//! including `.a` static modules and `code_module_vars`.
//!
//! To *speak first* rather than only answer — pushing particles into the
//! program, which is what `Log`/`Exception`/`Tick`-shaped traffic needs —
//! add [`declare_inbound!`] and call [`emit_inbound`]:
//!
//! ```rust,ignore
//! code_native::declare_inbound!();
//!
//! fn report(message: &str) {
//!     let mut p = CodeValue::zeroed();
//!     // ... build a particle ...
//!     emit_inbound(&p);
//!     release(&mut p);
//! }
//! ```
//!
//! A pushed class the program has no handler for is dropped, so a module may
//! report without every program that links it having to listen.
//!
//! `code_module_dispatch` and `code_module_abi_version` are the two required
//! exports — there is no macro generating them here (unlike the *old*
//! language's `code-native`): the new ABI dropped the descriptor-table
//! design for one function a module dispatches through itself, so there is
//! no boilerplate left to generate. `code_release` needs no Rust code at
//! all — it comes from the linked `runtime.c` object automatically.

use std::ffi::{c_char, c_int, c_void, CStr};
use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

// ===========================================================================
// Wire layout — bit-for-bit `code_abi.h`. Only the pointer/int/float shapes
// matter for ABI compatibility (not what they're named), but names are kept
// identical to the header so the two are trivially diffable.
// ===========================================================================

/// Current ABI version. A module's `code_module_abi_version` must return
/// this.
pub const CODE_ABI_VERSION: u32 = 1;

/// Byte stride of an array/object element buffer — **not** `size_of::<CodeValue>()`.
/// This is a frozen ABI constant with headroom for `CodeValue` to grow
/// without breaking already-compiled modules; always address a buffer
/// through [`slot_at`], never by casting to `*mut CodeValue` and indexing.
pub const CODE_VALUE_SLOT_SIZE: usize = 80;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeTag {
    Number,
    Str,
    Bool,
    Null,
    Array,
    Object,
}

#[repr(C)]
pub struct CodeValue {
    pub tag: CodeTag,
    pub heap: c_int,
    pub number: f64,
    pub str: *const c_char,
    pub boolean: c_int,
    /// `CODE_ARRAY`: element buffer; `CODE_OBJECT`: value buffer — both
    /// strided at [`CODE_VALUE_SLOT_SIZE`], addressed via [`slot_at`].
    pub items: *mut c_void,
    /// `CODE_OBJECT` only, parallel to `items`.
    pub keys: *mut *const c_char,
    pub len: i64,
}

impl CodeValue {
    /// An all-zero value — tag `Number`, `0.0`, not heap-owned. Bit-for-bit
    /// what `CodeValue x = {0};` produces in C, and the required starting
    /// state before passing `&mut` to any constructor below (each one calls
    /// `code_release` on `out` first, exactly like the C ABI expects).
    pub fn zeroed() -> Self {
        // SAFETY: an all-zero-bytes CodeValue is a valid Number(0.0), which
        // `code_release` (called by every constructor before overwriting
        // `out`) already treats as a safe no-op — the same invariant `{0}`
        // relies on in every C module.
        unsafe { std::mem::zeroed() }
    }
}

impl Default for CodeValue {
    fn default() -> Self {
        Self::zeroed()
    }
}

#[repr(C)]
pub struct CodeVarList {
    pub count: i64,
    pub names: *const *const c_char,
    /// `CODE_VALUE_SLOT_SIZE` stride, `count` slots — see [`slot_at`].
    pub values: *mut CodeValue,
}

// Both types carry raw pointers, so Rust doesn't derive Send/Sync for them
// automatically — but `code_module_vars` (see README) is exactly the case
// that needs a `static`/`OnceLock<CodeVarList>`, and the host only ever
// reads this data (once, at `link` time), never mutates it concurrently.
// Matches the old language's own `code-abi` crate, which needed the same
// impls for the same reason.
unsafe impl Send for CodeValue {}
unsafe impl Sync for CodeValue {}
unsafe impl Send for CodeVarList {}
unsafe impl Sync for CodeVarList {}

// ===========================================================================
// Raw bindings to `runtime.c`'s exported (non-`static`) functions — the same
// symbols `code_abi.h` declares for a C module. Calling into the actual
// compiled `runtime.c`, not a port of it, is what keeps this crate free of
// the layout-drift risk the *old* language's `code-native`/`code-abi` pair
// needed a dedicated test to guard against.
// ===========================================================================

extern "C" {
    fn code_number(out: *mut CodeValue, n: f64);
    fn code_str(out: *mut CodeValue, s: *const c_char);
    fn code_bool(out: *mut CodeValue, b: c_int);
    fn code_null(out: *mut CodeValue);
    fn code_array(out: *mut CodeValue, items: *mut c_void, len: i64);
    fn code_object(out: *mut CodeValue, keys: *mut *const c_char, values: *mut c_void, len: i64);
    fn code_copy(out: *mut CodeValue, src: *const CodeValue);
    fn code_field(out: *mut CodeValue, obj: *const CodeValue, field: *const c_char);
    fn code_index(out: *mut CodeValue, arr: *const CodeValue, index: *const CodeValue);
    fn code_retain(v: *const CodeValue);
    fn code_values_equal(a: *const CodeValue, b: *const CodeValue) -> c_int;
    fn code_bool_value(v: *const CodeValue, op: *const c_char) -> c_int;
    fn code_assert(v: *const CodeValue);
    fn code_runtime_error(message: *const c_char) -> !;

    // `build.rs` compiles `runtime.c` with `code_release` renamed to this at
    // the preprocessor level (`-D`), and this crate re-exports it below under
    // the real name from a function rustc actually treats as part of the
    // crate (not an archive) — see that `#[no_mangle]` fn's own doc comment
    // for why the rename is needed at all.
    fn code_native_vendored_release(v: *mut CodeValue);
}

/// The ABI's required `code_release` export. Defined here, as a real Rust
/// function, rather than left as whatever `runtime.c`'s own `code_release`
/// would otherwise be: `cdylib` targets get `--exclude-libs=ALL` from
/// rustc by default, which hides every symbol pulled in from a *linked
/// static archive* (exactly what `build.rs`'s `cc::Build::compile` produces
/// from `runtime.c`) out of the shared library's dynamic symbol table —
/// even though this crate's own code calls it just fine internally. A
/// symbol the crate defines directly (this function) isn't subject to that
/// exclusion, so renaming the archive's copy and re-exporting it from here
/// is what makes the host's `dlsym("code_release")` actually find it.
///
/// # Safety
/// `v` must point to a valid, initialized `CodeValue` — the same
/// requirement `runtime.c`'s own `code_release` has. The host only ever
/// calls this on values it deep-copied out of your `code_module_dispatch`
/// result, so you should never need to call it yourself except via
/// [`release`].
#[no_mangle]
pub unsafe extern "C" fn code_release(v: *mut CodeValue) {
    code_native_vendored_release(v)
}

/// Addresses slot `index` of a [`CODE_VALUE_SLOT_SIZE`]-strided buffer —
/// the Rust equivalent of `code_abi.h`'s `code_slot_at`. Pure pointer
/// arithmetic, safe to reimplement independently (no allocator/refcount
/// logic to drift from `runtime.c`).
pub fn slot_at(base: *mut c_void, index: i64) -> *mut CodeValue {
    (base as *mut u8).wrapping_offset(index as isize * CODE_VALUE_SLOT_SIZE as isize)
        as *mut CodeValue
}

fn cstr(s: &str) -> std::ffi::CString {
    std::ffi::CString::new(s).unwrap_or_else(|_| std::ffi::CString::new("<invalid-utf8>").unwrap())
}

// ===========================================================================
// Safe scalar constructors — thin wrappers: `code_release`s `out` first
// (matching every `runtime.c` constructor's own contract), then delegates.
// ===========================================================================

/// Write a Number into `out`.
pub fn number(out: &mut CodeValue, n: f64) {
    unsafe { code_number(out, n) }
}

/// Write a Str into `out`, borrowing `s` for `'static` (a string literal or
/// otherwise permanently-alive buffer) rather than copying it — matching
/// `code_str`'s own borrowing contract. Use [`owned_str`] for a value built
/// at runtime that needs its own heap block.
pub fn borrowed_str(out: &mut CodeValue, s: &'static CStr) {
    unsafe { code_str(out, s.as_ptr()) }
}

/// Write a Str into `out` from a freshly-built Rust string. Leaks the
/// `CString` — acceptable here because the value crosses into the host's
/// own heap the moment your `code_module_dispatch` returns (the host
/// deep-copies your result and then calls your module's `code_release` on
/// it, which only ever frees what `runtime.c`'s own allocator built, never
/// this leaked buffer).
pub fn owned_str(out: &mut CodeValue, s: &str) {
    let c = cstr(s);
    unsafe { code_str(out, c.as_ptr()) }
    std::mem::forget(c);
}

/// Write a Bool into `out`.
pub fn boolean(out: &mut CodeValue, b: bool) {
    unsafe { code_bool(out, b as c_int) }
}

/// Write Null into `out`.
pub fn null(out: &mut CodeValue) {
    unsafe { code_null(out) }
}

/// Release whatever `v` holds — call on every temporary [`CodeValue`] you
/// built and no longer need (matching `runtime.c`'s own refcounting rule:
/// every slot that ever named a heap block owns exactly one reference to
/// it).
pub fn release(v: &mut CodeValue) {
    unsafe { code_release(v) }
}

/// Deep-copy `src` into `out` — `out` ends up owning its own references to
/// everything `src` points at, and `src` is left untouched. This is how a
/// handler passes a value it did not build itself along (e.g. an `Echo`
/// returning its operand): the copy takes new references, so neither side's
/// lifetime constrains the other.
pub fn copy(out: &mut CodeValue, src: &CodeValue) {
    unsafe { code_copy(out, src) }
}

/// Increment `v`'s refcount — needed only if you're holding onto a
/// [`CodeValue`] you didn't just build yourself (e.g. a borrowed field from
/// [`find_field`]) somewhere that will outlive the call it came from.
/// Every retained value must be balanced by a [`release`].
pub fn retain(v: &CodeValue) {
    unsafe { code_retain(v) }
}

/// `obj.field` field access, exactly like `.code` source's own semantics:
/// writes Null into `out` on a non-Object or missing field rather than
/// erroring — see `code_field`'s doc comment in `code_abi.h`.
pub fn field(out: &mut CodeValue, obj: &CodeValue, name: &str) {
    let c = cstr(name);
    unsafe { code_field(out, obj, c.as_ptr()) }
}

/// `arr[index]` element access, exactly like `.code` source's own
/// semantics: writes Null on a non-Array or out-of-bounds index.
pub fn index(out: &mut CodeValue, arr: &CodeValue, i: &CodeValue) {
    unsafe { code_index(out, arr, i) }
}

/// Structural equality, matching `.code` source's `=` operator.
pub fn values_equal(a: &CodeValue, b: &CodeValue) -> bool {
    unsafe { code_values_equal(a, b) != 0 }
}

/// Coerce `v` to a `bool` the way a boolean operator does, raising the same
/// fatal error a type mismatch would in `.code` source itself (`op` is the
/// operator name, used only for that error message — e.g. `"&&"`).
pub fn bool_value(v: &CodeValue, op: &str) -> bool {
    let c = cstr(op);
    unsafe { code_bool_value(v, c.as_ptr()) != 0 }
}

/// `assert v` semantics: fatal error (never returns) if `v` isn't `true`.
pub fn assert_value(v: &CodeValue) {
    unsafe { code_assert(v) }
}

/// Raise a fatal module error, taking the whole host process down.
///
/// **Deprecated as of 2026-08-28, and not for modules to call.** A module
/// may never end the application — see
/// `docs/todo/errors-as-particles.md`. Report a failure by returning an
/// [`exception`] instead, which the program receives as an ordinary value
/// and may examine or ignore.
///
/// Kept only because `runtime.c` itself still uses it internally for
/// conditions with no frame to return to (out of memory). It will leave
/// this crate's API entirely once the C runtime has an error channel.
#[deprecated(
    since = "1.1.0",
    note = "a module may not end the application; return `exception(out, source, message)` instead"
)]
pub fn runtime_error(message: &str) -> ! {
    let c = cstr(message);
    unsafe { code_runtime_error(c.as_ptr()) }
}

// ===========================================================================
// Slot buffers — for Array/Object construction, which `runtime.c` expects
// as a `CODE_VALUE_SLOT_SIZE`-strided scratch buffer of already-built
// elements (see `code_array`/`code_object`'s doc comments in `runtime.c`;
// `tests/native_modules/test_math.c`'s `factors`/`meta` exported vars are
// the C-side version of the same pattern).
// ===========================================================================

/// A scratch buffer of `count` [`CodeValue`] slots, zero-initialized (so
/// each slot starts in the same safe state [`CodeValue::zeroed`] documents).
/// Build each element in place with [`SlotBuffer::slot_mut`], then hand the
/// buffer to [`array`] or [`object`] — matching `runtime.c`'s "elements are
/// retained and copied out of this buffer, never adopted by reference"
/// contract, after which every slot you wrote must still be [`release`]d
/// (the copy took its own reference; yours is still live until you drop it).
pub struct SlotBuffer {
    buf: Vec<u8>,
    len: i64,
}

impl SlotBuffer {
    pub fn new(count: usize) -> Self {
        Self {
            buf: vec![0u8; count * CODE_VALUE_SLOT_SIZE],
            len: count as i64,
        }
    }

    /// Slot `index` — write a value into it with [`number`]/[`owned_str`]/etc.
    pub fn slot_mut(&mut self, index: i64) -> &mut CodeValue {
        debug_assert!(index >= 0 && index < self.len);
        unsafe { &mut *slot_at(self.buf.as_mut_ptr() as *mut c_void, index) }
    }

    fn as_items_ptr(&mut self) -> *mut c_void {
        self.buf.as_mut_ptr() as *mut c_void
    }

    /// Release every slot. Call after handing the buffer to [`array`] or
    /// [`object`] — they copy elements out, they don't take ownership of
    /// this buffer's own references.
    pub fn release_all(&mut self) {
        for i in 0..self.len {
            unsafe { code_release(slot_at(self.buf.as_mut_ptr() as *mut c_void, i)) }
        }
    }
}

/// Write an Array into `out`, copying (and retaining) `elems`'s slots.
/// `elems` still owns its own references afterwards — release it once
/// you're done (see [`SlotBuffer::release_all`]).
pub fn array(out: &mut CodeValue, elems: &mut SlotBuffer) {
    unsafe { code_array(out, elems.as_items_ptr(), elems.len) }
}

/// Write an Object into `out` from parallel `keys` and `values` (a
/// [`SlotBuffer`] built the same way [`array`] expects). `keys` must
/// outlive nothing in particular — `code_object` copies the pointers, and
/// C-string field names are expected to be `'static` (string literals),
/// matching `code_abi.h`'s own "key pointers are read-only data" note.
pub fn object(out: &mut CodeValue, keys: &[&'static CStr], values: &mut SlotBuffer) {
    debug_assert_eq!(keys.len() as i64, values.len);
    let mut key_ptrs: Vec<*const c_char> = keys.iter().map(|k| k.as_ptr()).collect();
    unsafe {
        code_object(
            out,
            key_ptrs.as_mut_ptr(),
            values.as_items_ptr(),
            values.len,
        )
    }
}

// ===========================================================================
// Reading helpers — for use inside `code_module_dispatch`.
// ===========================================================================

/// Read a field by name off an Object value. `None` if `v` isn't an
/// Object or the field doesn't exist — mirrors `code_field`'s own
/// permissive-null behavior, but as an `Option` instead of writing Null.
pub fn find_field<'a>(v: &'a CodeValue, name: &str) -> Option<&'a CodeValue> {
    if v.tag != CodeTag::Object || v.keys.is_null() {
        return None;
    }
    for i in 0..v.len {
        let key = unsafe { *v.keys.offset(i as isize) };
        if key.is_null() {
            continue;
        }
        let key_str = unsafe { CStr::from_ptr(key) };
        if key_str.to_bytes() == name.as_bytes() {
            return Some(unsafe { &*slot_at(v.items, i) });
        }
    }
    None
}

/// Read `v` as a `&str`, if it's a Str with a valid UTF-8 payload.
pub fn read_str(v: &CodeValue) -> Option<&str> {
    if v.tag != CodeTag::Str || v.str.is_null() {
        return None;
    }
    unsafe { CStr::from_ptr(v.str) }.to_str().ok()
}

/// Read `v` as an `f64`, if it's a Number.
pub fn read_number(v: &CodeValue) -> Option<f64> {
    (v.tag == CodeTag::Number).then_some(v.number)
}

/// Read `v` as a `bool`, if it's a Bool.
pub fn read_bool(v: &CodeValue) -> Option<bool> {
    (v.tag == CodeTag::Bool).then_some(v.boolean != 0)
}

/// Convenience: [`find_field`] + [`read_str`].
pub fn read_field_str<'a>(v: &'a CodeValue, name: &str) -> Option<&'a str> {
    read_str(find_field(v, name)?)
}

/// Convenience: [`find_field`] + [`read_number`].
pub fn read_field_number(v: &CodeValue, name: &str) -> Option<f64> {
    read_number(find_field(v, name)?)
}

/// Convenience: [`find_field`] + [`read_bool`].
pub fn read_field_bool(v: &CodeValue, name: &str) -> Option<bool> {
    read_bool(find_field(v, name)?)
}

/// Iterate an Array's elements.
pub fn array_elems(v: &CodeValue) -> impl Iterator<Item = &CodeValue> {
    let (items, len) = if v.tag == CodeTag::Array {
        (v.items, v.len)
    } else {
        (std::ptr::null_mut(), 0)
    };
    (0..len).map(move |i| unsafe { &*slot_at(items, i) })
}

/// Build a `{ "_class": <class_name>, "value": <fill's result> }` particle
/// into `out` — the shape `emit ... to <alias> get x` expects a handler's
/// result to have. Mirrors `runtime.c`'s own `code_make_result`, which a
/// C module reaches via `#include "runtime.c"` but isn't exported for a
/// separately-linked module to call directly, so this is a small
/// reimplementation rather than an FFI binding.
pub fn make_result(
    out: &mut CodeValue,
    class_name: &'static CStr,
    fill: impl FnOnce(&mut CodeValue),
) {
    let mut value = CodeValue::zeroed();
    fill(&mut value);
    let mut buf = SlotBuffer::new(2);
    borrowed_str(buf.slot_mut(0), class_name);
    unsafe { code_copy(buf.slot_mut(1), &value) };
    object(out, &[c"_class", c"value"], &mut buf);
    buf.release_all();
    release(&mut value);
}

// ===========================================================================
// Inbound emissions — speaking first, rather than only answering.
// ===========================================================================

/// The host's pusher, handed over by `code_module_set_inbound`. `queue` is
/// opaque — a module only ever passes it straight back. Mirrors
/// `code_abi.h`'s `CodeEmitFn`.
pub type CodeEmitFn = unsafe extern "C" fn(queue: *mut c_void, value: *const CodeValue);

/// Where [`declare_inbound!`] parks what the host handed over. Two atomics
/// rather than a `static mut`: the host sets these once at link time, and a
/// module with a thread of its own would read them from that thread, so the
/// access wants to be well-defined even though nothing does that yet.
pub static INBOUND_QUEUE: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
/// The `CodeEmitFn` as a raw address — `AtomicPtr` cannot hold a `fn`
/// pointer directly, and this is only ever written by [`store_inbound`] and
/// read back by [`emit_inbound`].
pub static INBOUND_EMIT: AtomicUsize = AtomicUsize::new(0);

/// Record what the host handed over. Called by the export
/// [`declare_inbound!`] generates; not useful on its own.
pub fn store_inbound(queue: *mut c_void, emit: CodeEmitFn) {
    INBOUND_QUEUE.store(queue, Ordering::Release);
    INBOUND_EMIT.store(emit as usize, Ordering::Release);
}

/// Generate the optional `code_module_set_inbound` export.
///
/// A macro rather than a plain function in this crate, and that is
/// load-bearing: `#[no_mangle]` symbols defined in a dependency are not
/// reliably kept in the final `cdylib`, so the export has to be emitted in
/// *your* crate. One invocation at the top level is all it takes:
///
/// ```rust,ignore
/// code_native::declare_inbound!();
/// ```
///
/// A module that never speaks first simply doesn't invoke it — the export is
/// optional, and the host checks for it rather than requiring it.
#[macro_export]
macro_rules! declare_inbound {
    () => {
        /// Handed the host's queue and pusher once, at link time.
        ///
        /// # Safety
        ///
        /// Called by the host with its own queue pointer and pusher; both
        /// stay valid for as long as the module is loaded.
        #[no_mangle]
        pub unsafe extern "C" fn code_module_set_inbound(
            queue: *mut ::std::ffi::c_void,
            emit: $crate::CodeEmitFn,
        ) {
            $crate::store_inbound(queue, emit);
        }
    };
}

/// Push a particle into the program, to be dispatched to *its* handlers the
/// next time the host drains (between top-level statements).
///
/// Returns `false` when the host never called `code_module_set_inbound` —
/// which happens whenever the module was loaded by something that does not
/// support inbound emissions. Pushing is therefore always best-effort from
/// the module's side, and a module must stay correct when nobody is
/// listening.
///
/// The particle is deep-copied into the host's heap by the host's own
/// pusher, so `value` may be released as soon as this returns.
///
/// **A pushed class the program has no handler for is a runtime error**, not
/// a silent drop (`tests/fail_inbound_unhandled.code` pins that). Push only
/// what the program has agreed to receive.
pub fn emit_inbound(value: &CodeValue) -> bool {
    let emit = INBOUND_EMIT.load(Ordering::Acquire);
    if emit == 0 {
        return false;
    }
    let queue = INBOUND_QUEUE.load(Ordering::Acquire);
    // SAFETY: `emit` is non-zero only because `store_inbound` wrote a real
    // `CodeEmitFn` there, and `queue` is whatever the host paired with it.
    let emit: CodeEmitFn = unsafe { std::mem::transmute::<usize, CodeEmitFn>(emit) };
    unsafe { emit(queue, value) };
    true
}

// ===========================================================================
// Failing without ending the program.
// ===========================================================================

/// Build `Exception { source, message, innerException }` into `out` — how a
/// module reports that it could not do the work.
///
/// This is the *only* way a module should fail. A module may never end the
/// application (`docs/todo/errors-as-particles.md`): the program receives
/// this as an ordinary value through `get`, and may test it with
/// `is Exception`, read `message`, or ignore it entirely.
///
/// `source` names the module, which a returned value cannot otherwise be
/// asked — the caller knows what it emitted to, but an `Exception` stored,
/// passed on, or wrapped as another's `innerException` has lost that.
///
/// `innerException` is null here; use [`exception_wrapping`] to carry the
/// failure underneath this one.
pub fn exception(out: &mut CodeValue, source: &str, message: &str) {
    let mut inner = CodeValue::zeroed();
    null(&mut inner);
    exception_wrapping(out, source, message, &inner);
    release(&mut inner);
}

/// [`exception`], carrying the failure that caused it as `innerException`.
pub fn exception_wrapping(out: &mut CodeValue, source: &str, message: &str, inner: &CodeValue) {
    let mut buf = SlotBuffer::new(4);
    borrowed_str(buf.slot_mut(0), c"Exception");
    owned_str(buf.slot_mut(1), source);
    owned_str(buf.slot_mut(2), message);
    copy(buf.slot_mut(3), inner);
    object(
        out,
        &[c"_class", c"source", c"message", c"innerException"],
        &mut buf,
    );
    buf.release_all();
}

/// Run a module's dispatch body so that a panic inside it becomes an
/// [`exception`] rather than killing the host.
///
/// **Wrap every `code_module_dispatch` in this.** The guarantee it provides
/// cannot be provided by the host: a panic escaping an `extern "C"` function
/// aborts the process rather than unwinding, so the host's own
/// `catch_unwind` never runs — the catch has to happen on this side of the
/// FFI boundary, which is here.
///
/// What it covers is most of what "a badly written module" means in
/// practice: `unwrap`/`expect` on `None` or `Err`, slice and index bounds,
/// arithmetic overflow, explicit `panic!`/`assert!`, and panics raised
/// inside dependencies. What it cannot cover is a deliberate `exit`, an
/// infinite loop, or undefined behaviour reached through `unsafe`.
///
/// ```rust,ignore
/// #[no_mangle]
/// pub unsafe extern "C" fn code_module_dispatch(
///     out: *mut CodeValue,
///     particle: *const CodeValue,
/// ) {
///     guarded(&mut *out, "mymodule", |out| match read_field_str(&*particle, "_class") {
///         Some("Double") => { /* ... */ }
///         _ => null(out),
///     })
/// }
/// ```
pub fn guarded(out: &mut CodeValue, source: &str, body: impl FnOnce(&mut CodeValue)) {
    let slot: *mut CodeValue = out;
    // `AssertUnwindSafe` over the whole closure: `out` is a slot the host
    // owns, there is no invariant of ours for a panic to leave half-broken,
    // and whatever the body managed to write is released by the constructor
    // `exception` runs next.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // SAFETY: `slot` came from the `&mut` above and outlives this call.
        body(unsafe { &mut *slot })
    }));
    let Err(payload) = result else {
        return;
    };
    // Rust's panic payload is a string for `panic!("...")` and `unwrap`
    // alike; anything else is reported without a message rather than
    // guessed at.
    let detail = payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "panicked".to_string());
    // SAFETY: as above — `catch_unwind` returning `Err` means the body
    // stopped early, not that the slot went away.
    exception(
        unsafe { &mut *slot },
        source,
        &format!("module panicked: {detail}"),
    );
}
