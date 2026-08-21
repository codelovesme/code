//! Vendored copy of `code-abi`'s C-ABI contract.
//!
//! `code-abi` is the canonical, single source of truth for this ABI (used
//! directly by the host runtime: `native_module.rs`, `wasm_module.rs`), but it
//! stays unpublished (`publish = false`) on purpose. `code-native` — the
//! authoring SDK for native modules — needs to be publishable to crates.io on
//! its own, and crates.io rejects packages with a path dependency on an
//! unpublished crate.
//!
//! This file is therefore a literal, mechanically-kept-in-sync copy of
//! `crates/code-abi/src/lib.rs`. **Do not hand-edit it** — edit `code-abi`,
//! then copy its `src/lib.rs` here verbatim (only the header comment and the
//! crate-level `#![no_std]` attribute differ, since this is a module, not its
//! own crate). `tests/abi_in_sync.rs` fails the build if the two diverge.

use core::ffi::{c_char, c_void};

// ===========================================================================
// ABI version + target constants
// ===========================================================================

/// Current ABI version. Native modules must return this from
/// `code_module_abi_version`.
pub const CODE_ABI_VERSION: u32 = 2;

/// Emission target: dispatch to the linking module's base handlers.
pub const CODE_EMIT_TARGET_BASE: u32 = 0;

// ===========================================================================
// Value tag constants (must match the codegen tags)
// ===========================================================================

pub const CODE_TAG_NUMBER: u8 = 0;
pub const CODE_TAG_STRING: u8 = 1;
pub const CODE_TAG_BOOLEAN: u8 = 2;
pub const CODE_TAG_OBJECT: u8 = 3;
pub const CODE_TAG_NULL: u8 = 4;
pub const CODE_TAG_ARRAY: u8 = 5;

// ===========================================================================
// C-ABI struct definitions (repr(C) — mirrored in code_abi.h)
// ===========================================================================

/// A single Code value in C-ABI representation.
///
/// Active fields depend on `tag`:
/// - `CODE_TAG_NUMBER`  → `number`
/// - `CODE_TAG_STRING`  → `string` (null-terminated UTF-8 C string)
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

impl CodeValue {
    /// Create a null `CodeValue` with all fields zeroed.
    pub fn null() -> Self {
        CodeValue {
            tag: CODE_TAG_NULL,
            number: 0.0,
            string: core::ptr::null(),
            boolean: 0,
            fields: core::ptr::null(),
            field_count: 0,
            elements: core::ptr::null(),
            element_count: 0,
        }
    }
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

/// Callback signature for native-module emission.
pub type CodeEmitFn = unsafe extern "C" fn(context: *mut c_void, particle: CodeValue);

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

/// Emission declaration exported by a native module.
#[repr(C)]
pub struct CodeEmission {
    pub class_name: *const c_char,
    /// 0 = base (dispatch to linking module's handlers).
    pub target: u32,
}

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
    pub emissions: *const CodeEmission,
    pub emission_count: u32,
}

// ---------------------------------------------------------------------------
// Safety: these types contain raw pointers but are only used as immutable
// descriptors shared across threads. The module init function is called once
// and the data is read-only after that.
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
unsafe impl Sync for CodeEmission {}
unsafe impl Send for CodeEmission {}
unsafe impl Sync for CodeModuleDesc {}
unsafe impl Send for CodeModuleDesc {}
