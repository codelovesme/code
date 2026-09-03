//! Loads a native `.so` module and dispatches `emit ... to <alias>` calls
//! into it — the interpreter's side of `docs/todo/native-module-linking.md`.
//! See `code_abi.h` for the contract a module implements, and `runtime.c`'s
//! "Native modules" section for the compiled backend's equivalent
//! (`code_native_open`/`code_native_dispatch`) — this file exists because
//! the interpreter never links `runtime.c` at all, so it needs its own copy
//! of the same marshaling logic in plain Rust rather than reusing C code.
//!
//! Gated by the `native-modules` feature (see `Cargo.toml`) — wasm32 has no
//! `dlopen`, and `crates/code-wasm` never resolves a `link` to a native
//! module in the first place (`loader::NoModules` refuses every `link`), so
//! there is nothing for this file to do there.

use std::collections::VecDeque;
use std::ffi::{c_char, c_void, CStr, CString};
use std::mem::ManuallyDrop;
use std::rc::Rc;
use std::sync::Mutex;

use libloading::Library;

use crate::value::Value;

const CODE_ABI_VERSION: u32 = 1;

/// Must equal `code_abi.h`'s `CODE_INBOUND_CAPACITY`. The two runtimes have
/// to drop the *same* particles under overload or a fixture would assert
/// differently per output mode — the same lockstep rule `VALUE_SIZE` and
/// `CODE_VALUE_SLOT_SIZE` already live under.
const CODE_INBOUND_CAPACITY: usize = 256;
const CODE_VALUE_SLOT_SIZE: usize = 80; // must match code_abi.h / codegen.rs

const TAG_NUMBER: i32 = 0;
const TAG_STR: i32 = 1;
const TAG_BOOL: i32 = 2;
const TAG_NULL: i32 = 3;
const TAG_ARRAY: i32 = 4;
const TAG_OBJECT: i32 = 5;

/// Bit-for-bit the same layout as `code_abi.h`'s `CodeValue` — a native
/// module reads/writes this directly, so the field order and types here are
/// a wire format, not an implementation detail (see that header's doc
/// comment). `sizeof(CodeValueFfi)` is *not* `CODE_VALUE_SLOT_SIZE`: nested
/// arrays/objects are strided at the latter, never at the former — see
/// `write_slot`.
#[repr(C)]
struct CodeValueFfi {
    tag: i32,
    heap: i32,
    number: f64,
    str_: *const c_char,
    boolean: i32,
    items: *mut c_void,
    keys: *mut *const c_char,
    len: i64,
}

impl CodeValueFfi {
    const NULL: CodeValueFfi = CodeValueFfi {
        tag: TAG_NULL,
        heap: 0,
        number: 0.0,
        str_: std::ptr::null(),
        boolean: 0,
        items: std::ptr::null_mut(),
        keys: std::ptr::null_mut(),
        len: 0,
    };
}

/// Bit-for-bit the same layout as `code_abi.h`'s `CodeVarList` — what a
/// module's optional `code_module_vars` export returns. `values` is strided
/// at `CODE_VALUE_SLOT_SIZE` (address it through `slot_at`, never `[]`),
/// exactly like a `CodeValue`'s own `items` buffer.
#[repr(C)]
struct CodeVarListFfi {
    count: i64,
    names: *const *const c_char,
    values: *mut CodeValueFfi,
}

/// Writes `v` at slot `index` of a `CODE_VALUE_SLOT_SIZE`-strided buffer —
/// the same addressing convention `runtime.c`'s `slot_at` uses, needed here
/// because a module's `slot_at(items, i)` would otherwise silently read the
/// wrong offset the moment `sizeof(CodeValueFfi) != CODE_VALUE_SLOT_SIZE`.
/// `buf` must be a `u64`-backed allocation so every slot start is 8-byte
/// aligned (`CodeValueFfi`'s own alignment, from its `f64`/pointer fields).
fn write_slot(buf: &mut [u64], index: usize, v: CodeValueFfi) {
    let words_per_slot = CODE_VALUE_SLOT_SIZE / std::mem::size_of::<u64>();
    debug_assert!(std::mem::size_of::<CodeValueFfi>() <= CODE_VALUE_SLOT_SIZE);
    let ptr = buf[index * words_per_slot..].as_mut_ptr() as *mut CodeValueFfi;
    unsafe { ptr.write(v) };
}

fn slot_at(buf: *const c_void, index: i64) -> *const CodeValueFfi {
    (buf as *const u8).wrapping_add(index as usize * CODE_VALUE_SLOT_SIZE) as *const CodeValueFfi
}

/// Owns every buffer/string a marshaled particle's `CodeValueFfi` tree
/// points into, so they outlive the dispatch call that reads them. A native
/// module must treat everything it's handed as read-only and valid only for
/// the duration of that one call — the same convention any C callback API
/// uses for a borrowed argument — and every value built here has `heap = 0`
/// throughout, so even a module that (incorrectly) tried to `code_release`
/// a piece of it would find that a no-op rather than an attempt to free
/// Rust-owned memory.
#[derive(Default)]
struct Arena {
    buffers: Vec<Box<[u64]>>,
    keys: Vec<Box<[*const c_char]>>,
    strings: Vec<CString>,
}

impl Arena {
    fn cstr(&mut self, s: &str) -> *const c_char {
        // Interior NULs can't happen: this language's strings come from
        // source text or from `+`-concatenation of the same, and the lexer
        // never admits a literal NUL byte.
        let cs = CString::new(s).unwrap_or_else(|_| CString::new("").unwrap());
        self.strings.push(cs);
        self.strings.last().unwrap().as_ptr()
    }

    fn build(&mut self, value: &Value) -> CodeValueFfi {
        match value {
            Value::Number(n) => CodeValueFfi {
                tag: TAG_NUMBER,
                number: *n,
                ..CodeValueFfi::NULL
            },
            Value::Str(s) => CodeValueFfi {
                tag: TAG_STR,
                str_: self.cstr(s),
                ..CodeValueFfi::NULL
            },
            Value::Bool(b) => CodeValueFfi {
                tag: TAG_BOOL,
                boolean: if *b { 1 } else { 0 },
                ..CodeValueFfi::NULL
            },
            Value::Null => CodeValueFfi::NULL,
            Value::Array(items) => {
                let words_per_slot = CODE_VALUE_SLOT_SIZE / std::mem::size_of::<u64>();
                let mut buf = vec![0u64; items.len() * words_per_slot].into_boxed_slice();
                for (i, item) in items.iter().enumerate() {
                    let v = self.build(item);
                    write_slot(&mut buf, i, v);
                }
                let items_ptr = buf.as_mut_ptr() as *mut c_void;
                self.buffers.push(buf);
                CodeValueFfi {
                    tag: TAG_ARRAY,
                    items: items_ptr,
                    len: items.len() as i64,
                    ..CodeValueFfi::NULL
                }
            }
            Value::Object(fields) => {
                let words_per_slot = CODE_VALUE_SLOT_SIZE / std::mem::size_of::<u64>();
                let mut buf = vec![0u64; fields.len() * words_per_slot].into_boxed_slice();
                let mut keys: Vec<*const c_char> = Vec::with_capacity(fields.len());
                for (i, (key, val)) in fields.iter().enumerate() {
                    keys.push(self.cstr(key));
                    let v = self.build(val);
                    write_slot(&mut buf, i, v);
                }
                let items_ptr = buf.as_mut_ptr() as *mut c_void;
                self.buffers.push(buf);
                let mut keys = keys.into_boxed_slice();
                let keys_ptr = keys.as_mut_ptr();
                self.keys.push(keys);
                CodeValueFfi {
                    tag: TAG_OBJECT,
                    items: items_ptr,
                    keys: keys_ptr,
                    len: fields.len() as i64,
                    ..CodeValueFfi::NULL
                }
            }
        }
    }
}

/// Reads a module-produced `CodeValueFfi` tree into an owned `Value` — the
/// interpreter's equivalent of `runtime.c`'s `code_native_copy_in`, for the
/// same reason: the memory a module handed back belongs to *its* allocator,
/// not ours, and becomes invalid the moment its own `code_release` runs on
/// it, so every byte needed has to be copied out first.
///
/// # Safety
/// `v` must point at a validly-initialized `CodeValueFfi` built by a module
/// honoring `code_abi.h` — nested `items`/`keys` pointers are trusted for
/// exactly `len` slots.
unsafe fn ffi_to_value(v: *const CodeValueFfi) -> Value {
    let v = &*v;
    match v.tag {
        TAG_NUMBER => Value::Number(v.number),
        TAG_STR => {
            if v.str_.is_null() {
                Value::Str(Rc::from(""))
            } else {
                let s = CStr::from_ptr(v.str_).to_string_lossy().into_owned();
                Value::Str(Rc::from(s.as_str()))
            }
        }
        TAG_BOOL => Value::Bool(v.boolean != 0),
        TAG_ARRAY => {
            let mut items = Vec::with_capacity(v.len as usize);
            for i in 0..v.len {
                items.push(ffi_to_value(slot_at(v.items, i)));
            }
            Value::Array(Rc::new(items))
        }
        TAG_OBJECT => {
            let mut fields = Vec::with_capacity(v.len as usize);
            for i in 0..v.len {
                let key_ptr = *v.keys.add(i as usize);
                let key = CStr::from_ptr(key_ptr).to_string_lossy().into_owned();
                fields.push((key, ffi_to_value(slot_at(v.items, i))));
            }
            Value::Object(Rc::new(fields))
        }
        _ => Value::Null, // TAG_NULL or anything unrecognized
    }
}

/// A loaded, ready-to-dispatch native module — what `link "x.so" as x`
/// produces at runtime.
pub struct NativeModule {
    /// `ManuallyDrop` because of `Drop for NativeModule` below: a module with
    /// a thread of its own must not be unloaded while that thread is running.
    lib: ManuallyDrop<Library>,
    path: String,
    /// Whether the module took the inbound channel — the one thing that can
    /// give it a life of its own past a dispatch call.
    has_inbound: bool,
    /// Whether it also wants to hear what the program answered.
    has_reply: bool,
    /// `code_module_serving`, if the module exports it — see `code_abi.h`.
    /// A module that answers non-zero holds the program open, the way a
    /// non-daemon thread holds a JVM open. Most modules export nothing here
    /// and so hold nothing.
    serving: Option<ServingFn>,
    /// Where this module's pushed particles land until the program drains
    /// them. Leaked at `open`, so the raw pointer the module keeps stays
    /// valid for as long as the module is loaded.
    inbound: &'static InboundQueue,
}

/// Unloads the module at the end of the run — unless it took the inbound
/// channel, in which case it is left mapped for the life of the process.
///
/// A module that can speak first is a module that may have spawned a thread,
/// and `dlclose` unmaps the code that thread is executing: the program would
/// die during its own cleanup, after its last statement succeeded. There is
/// no shutdown call in the ABI to avoid this with, deliberately — a module
/// that has to be asked politely before the program may exit is a module that
/// can hang it. So the mapping stays; the process is about to end anyway, and
/// `runtime.c`'s `code_native_close` keeps its half of this bargain the same
/// way.
impl Drop for NativeModule {
    fn drop(&mut self) {
        if !self.has_inbound {
            // SAFETY: `lib` is never used again — this is `drop`, and the
            // field is `ManuallyDrop` precisely so this is the only place it
            // can happen.
            unsafe { ManuallyDrop::drop(&mut self.lib) };
        }
    }
}

impl std::fmt::Debug for NativeModule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeModule")
            .field("path", &self.path)
            .finish()
    }
}

type DispatchFn = unsafe extern "C" fn(*mut CodeValueFfi, *const CodeValueFfi);
type ReleaseFn = unsafe extern "C" fn(*mut CodeValueFfi);
type VersionFn = unsafe extern "C" fn() -> u32;
type VarsFn = unsafe extern "C" fn() -> *const CodeVarListFfi;
type SetInboundFn = unsafe extern "C" fn(*mut c_void, EmitFn);
/// `code_module_inbound_reply` — see `code_abi.h`. Optional, and most
/// modules do not export it.
type InboundReplyFn = unsafe extern "C" fn(*const CodeValueFfi, *const CodeValueFfi);

/// `code_module_serving` — non-zero while the module still expects to speak.
type ServingFn = unsafe extern "C" fn() -> std::ffi::c_int;
/// The host function a module calls to speak first — see `code_abi.h`'s
/// `CodeEmitFn`. Handed across as a pointer because a `.so` has its own copy
/// of the runtime, so a direct call would reach the wrong queue.
type EmitFn = unsafe extern "C" fn(*mut c_void, *const CodeValueFfi);

/// What `queue` actually points at on the host side. Boxed and leaked for the
/// module's lifetime: the module keeps the raw pointer, and a `.so` is never
/// unloaded before the program ends.
///
/// A `Mutex`, not a `RefCell`: a module may push from a thread of its own,
/// which is what makes an event loop more than polling. `runtime.c`'s ring
/// takes a `pthread_mutex_t` for the same reason.
///
/// `Value` holds `Rc`s and so is not `Send`, and nothing here asserts that it
/// is — the queue is shared with the module through a raw pointer across FFI,
/// which the compiler never type-checks. What makes it sound is that a queued
/// value is never *shared* between threads, only handed over: the pushing
/// thread builds a fresh deep copy (`ffi_to_value` allocates every node), the
/// mutex publishes it, and the program owns it alone from `take` onwards. No
/// two threads ever touch one `Rc`'s count, and the lock provides the
/// happens-before that hand-off needs.
#[derive(Default)]
pub struct InboundQueue {
    pending: Mutex<VecDeque<Value>>,
    /// The environment to wake after a push. Attached at link time rather
    /// than at construction, because the queue is handed to the module before
    /// there is an environment to belong to — and it is per environment, so a
    /// host running several programs never wakes the wrong one.
    wakeup: Mutex<Option<std::sync::Arc<crate::interpreter::Wakeup>>>,
}

impl InboundQueue {
    /// Tell this queue which environment to wake after a push. Called once,
    /// when the module is linked into that environment.
    pub fn wake(&self, wakeup: std::sync::Arc<crate::interpreter::Wakeup>) {
        *self.wakeup.lock().unwrap_or_else(|e| e.into_inner()) = Some(wakeup);
    }

    /// Takes everything queued so far, leaving the queue empty. Draining
    /// rather than peeking so a handler that causes more pushes has them
    /// picked up by the next round rather than this one.
    pub fn take(&self) -> Vec<Value> {
        self.lock().drain(..).collect()
    }

    /// Poisoning is ignored on purpose: the only code that can panic while
    /// holding this lock is `ffi_to_value` on a malformed value, and a
    /// half-pushed particle leaves the `VecDeque` itself intact — refusing
    /// every later push would turn one bad particle into a dead program.
    fn lock(&self) -> std::sync::MutexGuard<'_, VecDeque<Value>> {
        self.pending.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Bounded, dropping the *oldest* past capacity — byte-for-byte the
    /// policy `runtime.c`'s ring follows, so both output modes lose exactly
    /// the same particles when a module outruns the program.
    fn push(&self, value: Value) {
        {
            let mut pending = self.lock();
            if pending.len() == CODE_INBOUND_CAPACITY {
                pending.pop_front();
            }
            pending.push_back(value);
        }
        // After the queue lock is released, never while holding it: the
        // waiter wakes into `take`, and waking it inside this critical
        // section only means it blocks again on the way out.
        let wakeup = self
            .wakeup
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if let Some(wakeup) = wakeup {
            wakeup.signal();
        }
    }
}

/// The `EmitFn` every module is handed. Deep-copies out of the module's heap
/// immediately — `ffi_to_value` does that — so the module is free to release
/// its own copy the moment this returns.
unsafe extern "C" fn push_inbound(queue: *mut c_void, value: *const CodeValueFfi) {
    if queue.is_null() || value.is_null() {
        return;
    }
    let queue = unsafe { &*(queue as *const InboundQueue) };
    let value = unsafe { ffi_to_value(&*value) };
    queue.push(value);
}

impl NativeModule {
    pub fn open(path: &str) -> Result<NativeModule, String> {
        let lib = unsafe { Library::new(path) }
            .map_err(|e| format!("cannot load native module '{path}': {e}"))?;

        let version = unsafe {
            lib.get::<VersionFn>(b"code_module_abi_version")
                .map_err(|_| format!("native module '{path}' missing 'code_module_abi_version'"))?
        };
        let version = unsafe { version() };
        if version != CODE_ABI_VERSION {
            return Err(format!(
                "native module '{path}' has ABI version {version} (expected {CODE_ABI_VERSION})"
            ));
        }

        // Fail fast on the other two required symbols too, rather than only
        // discovering a missing one on the first `emit` that reaches it.
        unsafe {
            lib.get::<DispatchFn>(b"code_module_dispatch")
                .map_err(|_| format!("native module '{path}' missing 'code_module_dispatch'"))?;
            lib.get::<ReleaseFn>(b"code_release")
                .map_err(|_| format!("native module '{path}' missing 'code_release'"))?;
        }

        // Optional, like `code_module_vars`: a module that never speaks
        // first simply doesn't export it. Leaked on purpose — the module
        // holds the raw pointer for as long as it is loaded, which is until
        // the program ends.
        let inbound: &'static InboundQueue = Box::leak(Box::new(InboundQueue::default()));
        let mut has_inbound = false;
        unsafe {
            if let Ok(set) = lib.get::<SetInboundFn>(b"code_module_set_inbound") {
                set(inbound as *const InboundQueue as *mut c_void, push_inbound);
                has_inbound = true;
            }
        }

        // Optional and additive: only a module that pushes *questions* wants
        // the answer. Looked up once here rather than per reply.
        let has_reply = unsafe { lib.get::<InboundReplyFn>(b"code_module_inbound_reply") }.is_ok();

        // Optional in the same way, and resolved once: a module that never
        // holds the program open simply doesn't export it.
        let serving = unsafe { lib.get::<ServingFn>(b"code_module_serving") }
            .ok()
            .map(|f| *f);

        Ok(NativeModule {
            lib: ManuallyDrop::new(lib),
            path: path.to_string(),
            has_inbound,
            has_reply,
            serving,
            inbound,
        })
    }

    /// The queue itself, usable after this module has been moved into a
    /// dispatch closure — `'static` because it was leaked at `open`.
    pub fn inbound_handle(&self) -> &'static InboundQueue {
        self.inbound
    }

    /// Whether this module still expects to speak — a listening socket, a
    /// running timer. While any linked module says so, the program stays up
    /// after its last statement instead of exiting (see
    /// `interpreter::keep_alive`).
    ///
    /// A module that exports no `code_module_serving` holds nothing open,
    /// which is what keeps every existing program ending exactly when it
    /// used to.
    pub fn serving(&self) -> bool {
        // SAFETY: the symbol was resolved from a library that is still
        // loaded — a module with an inbound channel is never unloaded, and
        // one without has no thread to be serving from anyway.
        self.serving.map(|f| unsafe { f() != 0 }).unwrap_or(false)
    }

    /// Tell the module what the program's handler answered to a particle it
    /// pushed — `result` is null when nothing handled it. A no-op for a
    /// module that exports no `code_module_inbound_reply`.
    ///
    /// Both values are built into a fresh arena and freed when it drops: the
    /// module reads what it needs during the call and copies it out, which is
    /// the same boundary rule `dispatch` follows in the other direction.
    pub fn reply(&self, particle: &Value, result: &Value) {
        if !self.has_reply {
            return;
        }
        let mut arena = Arena::default();
        let particle_ffi = arena.build(particle);
        let result_ffi = arena.build(result);
        unsafe {
            let Ok(reply) = self.lib.get::<InboundReplyFn>(b"code_module_inbound_reply") else {
                return;
            };
            reply(&particle_ffi, &result_ffi);
        }
    }

    /// Dispatch `particle` and return the module's (deep-copied, host-owned)
    /// result — mirrors `runtime.c`'s `code_native_dispatch` exactly.
    pub fn dispatch(&self, particle: &Value) -> Result<Value, String> {
        let mut arena = Arena::default();
        let particle_ffi = arena.build(particle);

        let mut result = CodeValueFfi::NULL;
        unsafe {
            let dispatch = self
                .lib
                .get::<DispatchFn>(b"code_module_dispatch")
                .map_err(|_| {
                    format!(
                        "native module '{}' missing 'code_module_dispatch'",
                        self.path
                    )
                })?;
            dispatch(&mut result, &particle_ffi);
        }

        let value = unsafe { ffi_to_value(&result) };

        unsafe {
            let release = self
                .lib
                .get::<ReleaseFn>(b"code_release")
                .map_err(|_| format!("native module '{}' missing 'code_release'", self.path))?;
            release(&mut result);
        }

        Ok(value)
    }

    /// The module's exported variables (constants), as host-owned
    /// `(name, value)` pairs in the module's own order — the interpreter's
    /// equivalent of `runtime.c`'s `code_native_get_var`, for the same
    /// reason: each value a module hands back belongs to *its* allocator and
    /// becomes invalid the moment its own `code_release` runs on it, so
    /// every byte is copied out first. A module with no `code_module_vars`
    /// export (a Phase 1, handlers-only module) yields an empty list.
    pub fn vars(&self) -> Result<Vec<(String, Value)>, String> {
        let vars_ptr = unsafe {
            self.lib
                .get::<VarsFn>(b"code_module_vars")
                .ok()
                .map(|vars| vars())
                .filter(|p| !p.is_null())
        };
        let Some(vars_ptr) = vars_ptr else {
            return Ok(Vec::new());
        };
        let list = unsafe { &*vars_ptr };
        if list.count < 0 {
            return Err(format!(
                "native module '{}' reports a negative variable count",
                self.path
            ));
        }
        let count = list.count as usize;
        let mut out = Vec::with_capacity(count);
        for i in 0..count {
            let name = unsafe { CStr::from_ptr(*list.names.add(i)) }
                .to_string_lossy()
                .into_owned();
            let value = unsafe { ffi_to_value(slot_at(list.values as *const c_void, i as i64)) };
            out.push((name, value));
        }
        Ok(out)
    }
}
