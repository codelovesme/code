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

use std::ffi::{c_char, c_void, CStr, CString};
use std::rc::Rc;

use libloading::Library;

use crate::value::Value;

const CODE_ABI_VERSION: u32 = 1;
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
    lib: Library,
    path: String,
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

        Ok(NativeModule {
            lib,
            path: path.to_string(),
        })
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
}
