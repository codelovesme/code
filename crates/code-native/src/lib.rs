//! `code-native` — Helper crate for writing Code language native modules in Rust.
//!
//! This crate eliminates the boilerplate required to create native `.so`
//! modules for the Code language.  It re-exports all C-ABI types, provides
//! safe value constructors and field-access helpers, and ships the
//! [`code_module!`] macro that generates the required `#[no_mangle]`
//! entry-points (`code_module_abi_version` + `code_module_init`).
//!
//! # Quick Start
//!
//! ```rust,ignore
//! use code_native::*;
//!
//! unsafe extern "C" fn handle_add(particle: CodeValue) -> CodeValue {
//!     let a = read_field_number(&particle, "a");
//!     let b = read_field_number(&particle, "b");
//!     code_object(vec![
//!         code_field("_class", code_string("AddResult")),
//!         code_field("result", code_number(a + b)),
//!     ])
//! }
//!
//! code_module! {
//!     vars: [
//!         "PI" => code_number(3.14159),
//!     ],
//!     types: [
//!         "Add" [("a", "Number"), ("b", "Number")],
//!     ],
//!     handlers: [
//!         "Add" => handle_add,
//!     ],
//!     emissions: [],
//! }
//! ```
//!
//! Compile with:
//! ```bash
//! cargo build -p code-native
//! rustc --edition 2021 --crate-type cdylib \
//!     --extern code_native=target/debug/libcode_native.rlib \
//!     -o mymodule.so mymodule.rs
//! ```

use std::ffi::{c_char, c_void, CStr, CString};
use std::ptr;
use std::sync::atomic::{AtomicPtr, Ordering};

// ===========================================================================
// ABI version
// ===========================================================================

/// Current ABI version. Native modules must return this from
/// `code_module_abi_version`.
pub const CODE_ABI_VERSION: u32 = 2;

// ===========================================================================
// Tag constants
// ===========================================================================

pub const CODE_TAG_NUMBER: u8 = 0;
pub const CODE_TAG_STRING: u8 = 1;
pub const CODE_TAG_BOOLEAN: u8 = 2;
pub const CODE_TAG_OBJECT: u8 = 3;
pub const CODE_TAG_NULL: u8 = 4;
pub const CODE_TAG_ARRAY: u8 = 5;

// ===========================================================================
// C-ABI struct definitions (repr(C) — must match the host runtime native_module.rs)
// ===========================================================================

/// A single Code runtime value in C-ABI representation.
///
/// Active fields depend on `tag`:
/// - `CODE_TAG_NUMBER`  → `number`
/// - `CODE_TAG_STRING`  → `string` (null-terminated UTF-8)
/// - `CODE_TAG_BOOLEAN` → `boolean` (0 = false, non-zero = true)
/// - `CODE_TAG_OBJECT`  → `fields` + `field_count`
/// - `CODE_TAG_NULL`    → (no data)
/// - `CODE_TAG_ARRAY`   → `elements` + `element_count`
#[repr(C)]
pub struct CodeValue {
    pub tag: u8,
    pub number: f64,
    pub string: *const c_char,
    pub boolean: u8,
    pub fields: *const CodeField,
    pub field_count: u32,
    pub elements: *const CodeValue,
    pub element_count: u32,
}

/// A key-value pair for object fields.
#[repr(C)]
pub struct CodeField {
    pub name: *const c_char,
    pub value: CodeValue,
}

// ---------------------------------------------------------------------------
// Function / handler signatures
// ---------------------------------------------------------------------------

/// Native handler signature: `fn(particle) -> CodeValue`.
pub type CodeNativeHandlerFn = unsafe extern "C" fn(particle: CodeValue) -> CodeValue;

// ---------------------------------------------------------------------------
// Export descriptor types
// ---------------------------------------------------------------------------

/// Exported variable: name + constant value.
#[repr(C)]
pub struct CodeExportVar {
    pub name: *const c_char,
    pub value: CodeValue,
}

/// Exported handler: class name + handler function.
#[repr(C)]
pub struct CodeExportHandler {
    pub class_name: *const c_char,
    pub handler: CodeNativeHandlerFn,
}

/// Field descriptor for type declarations.
#[repr(C)]
pub struct CodeTypeField {
    pub name: *const c_char,
    /// Type name as C string, e.g. "String", "Number".
    pub type_name: *const c_char,
    /// 0 = required, non-zero = optional.
    pub is_optional: u8,
}

/// Exported type declaration.
#[repr(C)]
pub struct CodeExportType {
    pub name: *const c_char,
    pub fields: *const CodeTypeField,
    pub field_count: u32,
}

/// Emission target constants.
pub const CODE_EMIT_TARGET_BASE: u32 = 0;

/// Emission declaration: the module may emit particles of this class to the
/// specified target.
#[repr(C)]
pub struct CodeEmission {
    pub class_name: *const c_char,
    /// Target for emission.  Currently only `CODE_EMIT_TARGET_BASE` (0) is
    /// supported — the particle is dispatched to the linking module's handlers.
    pub target: u32,
}

/// Callback signature for emitting particles from native modules.
///
/// The host provides this function via `code_module_set_emit`; the native
/// module calls it to push a particle into the host's dispatch queue.
///
/// # Safety
/// `context` must be the opaque pointer originally provided by the host.
/// `particle` must be a valid `CodeValue` with object tag and `_class` field.
pub type CodeEmitFn = unsafe extern "C" fn(context: *mut c_void, particle: CodeValue);

/// Top-level module descriptor returned by `code_module_init()`.
#[repr(C)]
pub struct CodeModuleDesc {
    pub abi_version: u32,
    pub vars: *const CodeExportVar,
    pub var_count: u32,
    pub handlers: *const CodeExportHandler,
    pub handler_count: u32,
    pub types: *const CodeExportType,
    pub type_count: u32,
    /// Emission declarations — particles this module may emit.
    pub emissions: *const CodeEmission,
    pub emission_count: u32,
}

// ---------------------------------------------------------------------------
// Safety: these types contain raw pointers but are only used as immutable
// descriptors shared across threads.  The module init function is called
// exactly once and the data is truly read-only after that.
// ---------------------------------------------------------------------------

unsafe impl Sync for CodeValue {}
unsafe impl Send for CodeValue {}
unsafe impl Sync for CodeField {}
unsafe impl Send for CodeField {}
unsafe impl Sync for CodeExportVar {}
unsafe impl Send for CodeExportVar {}
unsafe impl Sync for CodeExportHandler {}
unsafe impl Send for CodeExportHandler {}
unsafe impl Sync for CodeTypeField {}
unsafe impl Send for CodeTypeField {}
unsafe impl Sync for CodeExportType {}
unsafe impl Send for CodeExportType {}
unsafe impl Sync for CodeModuleDesc {}
unsafe impl Send for CodeModuleDesc {}
unsafe impl Sync for CodeEmission {}
unsafe impl Send for CodeEmission {}

// ===========================================================================
// Value builders
// ===========================================================================

/// Create a Number value.
pub fn code_number(n: f64) -> CodeValue {
    CodeValue {
        tag: CODE_TAG_NUMBER,
        number: n,
        string: ptr::null(),
        boolean: 0,
        fields: ptr::null(),
        field_count: 0,
        elements: ptr::null(),
        element_count: 0,
    }
}

/// Create a String value.  The string is leaked into a C-compatible pointer
/// that lives for the remainder of the process — fine for return values and
/// module descriptors.
pub fn code_string(s: &str) -> CodeValue {
    CodeValue {
        tag: CODE_TAG_STRING,
        number: 0.0,
        string: leak_str(s),
        boolean: 0,
        fields: ptr::null(),
        field_count: 0,
        elements: ptr::null(),
        element_count: 0,
    }
}

/// Create a String value from a raw `*const c_char` pointer.
///
/// Use this when you already hold a C string pointer (e.g. a `c"..."` literal
/// or a pointer received from the runtime).
pub fn code_string_raw(ptr: *const c_char) -> CodeValue {
    CodeValue {
        tag: CODE_TAG_STRING,
        number: 0.0,
        string: ptr,
        boolean: 0,
        fields: ptr::null(),
        field_count: 0,
        elements: ptr::null(),
        element_count: 0,
    }
}

/// Create a Boolean value.
pub fn code_boolean(b: bool) -> CodeValue {
    CodeValue {
        tag: CODE_TAG_BOOLEAN,
        number: 0.0,
        string: ptr::null(),
        boolean: if b { 1 } else { 0 },
        fields: ptr::null(),
        field_count: 0,
        elements: ptr::null(),
        element_count: 0,
    }
}

/// Create a Null value.
pub fn code_null() -> CodeValue {
    CodeValue {
        tag: CODE_TAG_NULL,
        number: 0.0,
        string: ptr::null(),
        boolean: 0,
        fields: ptr::null(),
        field_count: 0,
        elements: ptr::null(),
        element_count: 0,
    }
}

/// Create an Object value from a `Vec` of fields.
///
/// The vector is leaked so the field pointer remains valid.
pub fn code_object(fields: Vec<CodeField>) -> CodeValue {
    let boxed = fields.into_boxed_slice();
    let count = boxed.len() as u32;
    let ptr = Box::leak(boxed).as_ptr();
    CodeValue {
        tag: CODE_TAG_OBJECT,
        number: 0.0,
        string: ptr::null(),
        boolean: 0,
        fields: ptr,
        field_count: count,
        elements: ptr::null(),
        element_count: 0,
    }
}

/// Create an Array value from a `Vec` of elements.
///
/// The vector is leaked so the element pointer remains valid.
pub fn code_array(elements: Vec<CodeValue>) -> CodeValue {
    let boxed = elements.into_boxed_slice();
    let count = boxed.len() as u32;
    let ptr = Box::leak(boxed).as_ptr();
    CodeValue {
        tag: CODE_TAG_ARRAY,
        number: 0.0,
        string: ptr::null(),
        boolean: 0,
        fields: ptr::null(),
        field_count: 0,
        elements: ptr,
        element_count: count,
    }
}

/// Create a single object field.  The name is leaked.
pub fn code_field(name: &str, value: CodeValue) -> CodeField {
    CodeField {
        name: leak_str(name),
        value,
    }
}

// ===========================================================================
// Reading helpers (for use in native function implementations)
// ===========================================================================

/// Read a string from an `CodeValue`, returning `""` if the tag is wrong or
/// the pointer is null.
///
/// # Safety
/// The `string` pointer inside `v` must be valid if `v.tag == CODE_TAG_STRING`.
pub unsafe fn read_str<'a>(v: &CodeValue) -> &'a str {
    if v.tag != CODE_TAG_STRING || v.string.is_null() {
        return "";
    }
    CStr::from_ptr(v.string).to_str().unwrap_or("")
}

/// Read a number from an `CodeValue`, returning `0.0` if the tag is wrong.
pub fn read_number(v: &CodeValue) -> f64 {
    if v.tag != CODE_TAG_NUMBER {
        return 0.0;
    }
    v.number
}

/// Read a boolean from an `CodeValue`, returning `false` if the tag is wrong.
pub fn read_boolean(v: &CodeValue) -> bool {
    if v.tag != CODE_TAG_BOOLEAN {
        return false;
    }
    v.boolean != 0
}

/// Look up a field by name inside an Object `CodeValue`.
///
/// Returns `None` if the value is not an object or the field is not found.
///
/// # Safety
/// The `fields` pointer and field name pointers must be valid.
pub unsafe fn read_field<'a>(v: &CodeValue, name: &str) -> Option<&'a CodeValue> {
    if v.tag != CODE_TAG_OBJECT || v.fields.is_null() {
        return None;
    }
    for i in 0..v.field_count as usize {
        let field = &*v.fields.add(i);
        if !field.name.is_null() {
            let field_name = CStr::from_ptr(field.name).to_str().unwrap_or("");
            if field_name == name {
                return Some(&field.value);
            }
        }
    }
    None
}

/// Convenience: read a string field from an object by name.
/// Returns `""` if the field doesn't exist or isn't a string.
///
/// # Safety
/// All field pointers must be valid.
pub unsafe fn read_field_str<'a>(v: &CodeValue, name: &str) -> &'a str {
    match read_field(v, name) {
        Some(fv) => read_str(fv),
        None => "",
    }
}

/// Convenience: read a number field from an object by name.
/// Returns `0.0` if the field doesn't exist or isn't a number.
///
/// # Safety
/// All field pointers must be valid.
pub unsafe fn read_field_number(v: &CodeValue, name: &str) -> f64 {
    match read_field(v, name) {
        Some(fv) => read_number(fv),
        None => 0.0,
    }
}

/// Convenience: read a boolean field from an object by name.
/// Returns `false` if the field doesn't exist or isn't a boolean.
///
/// # Safety
/// All field pointers must be valid.
pub unsafe fn read_field_bool(v: &CodeValue, name: &str) -> bool {
    match read_field(v, name) {
        Some(fv) => read_boolean(fv),
        None => false,
    }
}

// ===========================================================================
// String helpers
// ===========================================================================

/// Leak a Rust `&str` into a `*const c_char` that lives forever.
///
/// This is the standard way to produce C strings for the ABI.  The memory is
/// never freed — acceptable for module descriptors and return values.
pub fn leak_str(s: &str) -> *const c_char {
    CString::new(s)
        .unwrap_or_else(|_| CString::new("").unwrap())
        .into_raw() as *const c_char
}

// ===========================================================================
// Emit callback (set by host, used by native modules)
// ===========================================================================

/// Global emit function pointer, set by the host via `code_module_set_emit`.
#[doc(hidden)]
pub static EMIT_FN_PTR: AtomicPtr<()> = AtomicPtr::new(ptr::null_mut());
/// Global emit context pointer, set by the host via `code_module_set_emit`.
#[doc(hidden)]
pub static EMIT_CTX_PTR: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

/// Emit a particle to the host runtime.
///
/// The particle must be an object with a `_class` string field.
/// If `code_module_set_emit` has not been called, this is a no-op.
///
/// Thread-safe: may be called from any thread.
pub fn code_emit(particle: CodeValue) {
    let fn_ptr = EMIT_FN_PTR.load(Ordering::Acquire);
    if fn_ptr.is_null() {
        return;
    }
    let ctx = EMIT_CTX_PTR.load(Ordering::Acquire);
    let func: CodeEmitFn = unsafe { std::mem::transmute(fn_ptr) };
    unsafe { func(ctx, particle) };
}

/// Build and emit a `Log` particle.
///
/// Convenience helper for native modules:
/// ```rust,ignore
/// code_emit_log("my-module", "Info", "Server started on port 3000");
/// ```
pub fn code_emit_log(source: &str, level: &str, message: &str) {
    code_emit(code_object(vec![
        code_field("_class", code_string("Log")),
        code_field("source", code_string(source)),
        code_field("level", code_string(level)),
        code_field("message", code_string(message)),
    ]));
}

/// Build and emit an `Exception` particle.
///
/// Convenience helper for native modules:
/// ```rust,ignore
/// code_emit_exception("my-module", "Something went wrong");
/// ```
pub fn code_emit_exception(source: &str, message: &str) {
    code_emit(code_object(vec![
        code_field("_class", code_string("Exception")),
        code_field("source", code_string(source)),
        code_field("message", code_string(message)),
    ]));
}

// ===========================================================================
// code_module! macro
// ===========================================================================

/// Declare an Code native module.
///
/// Generates the required `#[no_mangle]` C symbols:
/// - `code_module_abi_version() -> u32`
/// - `code_module_init() -> *const CodeModuleDesc`
/// - `code_module_set_emit(fn, ctx)` — called by the host to provide the emit callback.
///
/// # Syntax
///
/// ```rust,ignore
/// code_module! {
///     vars: [
///         "NAME" => value_expr,
///     ],
///     types: [
///         "TypeName" [
///             ("field", "FieldType"),
///         ],
///     ],
///     handlers: [
///         "ClassName" => handler_fn_ident,
///     ],
///     emissions: [
///         "Log" => "base",
///         "Exception" => "base",
///     ],
/// }
/// ```
///
/// All four sections are required but may be empty (`[]`).
#[macro_export]
macro_rules! code_module {
    (
        vars: [ $( $var_name:literal => $var_value:expr ),* $(,)? ],
        types: [ $( $type_name:literal [ $( ( $field_name:literal , $field_type:literal $( , $field_optional:literal )? ) ),* $(,)? ] ),* $(,)? ],
        handlers: [ $( $handler_class:literal => $handler_fn:expr ),* $(,)? ],
        emissions: [ $( $emit_class:literal => $emit_target:literal ),* $(,)? ]
        $(,)?
    ) => {
        #[no_mangle]
        pub extern "C" fn code_module_abi_version() -> u32 {
            $crate::CODE_ABI_VERSION
        }

        #[no_mangle]
        pub unsafe extern "C" fn code_module_set_emit(
            emit_fn: $crate::CodeEmitFn,
            context: *mut std::ffi::c_void,
        ) {
            $crate::EMIT_FN_PTR.store(emit_fn as *mut (), std::sync::atomic::Ordering::Release);
            $crate::EMIT_CTX_PTR.store(context, std::sync::atomic::Ordering::Release);
        }

        #[no_mangle]
        pub extern "C" fn code_module_init() -> *const $crate::CodeModuleDesc {
            // Build exported variables
            let vars: Vec<$crate::CodeExportVar> = vec![
                $( $crate::CodeExportVar {
                    name: $crate::leak_str($var_name),
                    value: $var_value,
                }, )*
            ];

            // Build exported types (each type has its own field array)
            let types: Vec<$crate::CodeExportType> = vec![
                $( {
                    let fields: Vec<$crate::CodeTypeField> = vec![
                        $( $crate::CodeTypeField {
                            name: $crate::leak_str($field_name),
                            type_name: $crate::leak_str($field_type),
                            is_optional: { 0u8 $( + ($field_optional as u8) )? },
                        }, )*
                    ];
                    let fields_leaked = Box::leak(fields.into_boxed_slice());
                    $crate::CodeExportType {
                        name: $crate::leak_str($type_name),
                        fields: fields_leaked.as_ptr(),
                        field_count: fields_leaked.len() as u32,
                    }
                }, )*
            ];

            // Build exported handlers
            let handlers: Vec<$crate::CodeExportHandler> = vec![
                $( $crate::CodeExportHandler {
                    class_name: $crate::leak_str($handler_class),
                    handler: $handler_fn,
                }, )*
            ];

            // Build emission declarations
            let emissions: Vec<$crate::CodeEmission> = vec![
                $( $crate::CodeEmission {
                    class_name: $crate::leak_str($emit_class),
                    target: {
                        // Map target string to constant
                        match $emit_target {
                            "base" => $crate::CODE_EMIT_TARGET_BASE,
                            _ => $crate::CODE_EMIT_TARGET_BASE, // default
                        }
                    },
                }, )*
            ];

            // Leak all slices so pointers survive the return
            let vars_leaked = Box::leak(vars.into_boxed_slice());
            let types_leaked = Box::leak(types.into_boxed_slice());
            let handlers_leaked = Box::leak(handlers.into_boxed_slice());
            let emissions_leaked = Box::leak(emissions.into_boxed_slice());

            let desc = Box::new($crate::CodeModuleDesc {
                abi_version: $crate::CODE_ABI_VERSION,
                vars: vars_leaked.as_ptr(),
                var_count: vars_leaked.len() as u32,
                handlers: handlers_leaked.as_ptr(),
                handler_count: handlers_leaked.len() as u32,
                types: types_leaked.as_ptr(),
                type_count: types_leaked.len() as u32,
                emissions: emissions_leaked.as_ptr(),
                emission_count: emissions_leaked.len() as u32,
            });

            Box::leak(desc)
        }
    };
}
