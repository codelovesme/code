//! `test_strings` — Code native module example written in Rust.
//!
//! Exports:
//!   Variables: VERSION (String), MAX_LEN (Number)
//!   Types:     Message { text: String, urgent: Boolean }
//!   Handlers:  Message (returns Message with text uppercased if urgent)

use std::ffi::{c_char, CStr, CString};
use std::ptr;

// -----------------------------------------------------------------------
// ABI tag constants
// -----------------------------------------------------------------------
const CODE_TAG_NUMBER: u8 = 0;
const CODE_TAG_STRING: u8 = 1;
const CODE_TAG_BOOLEAN: u8 = 2;
const CODE_TAG_OBJECT: u8 = 3;
const CODE_TAG_NULL: u8 = 4;
#[allow(dead_code)]
const CODE_TAG_ARRAY: u8 = 5;

// -----------------------------------------------------------------------
// C-ABI struct definitions (must match code_abi.h / native_module.rs)
// -----------------------------------------------------------------------
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

#[repr(C)]
pub struct CodeField {
    pub name: *const c_char,
    pub value: CodeValue,
}

pub type CodeNativeHandlerFn = unsafe extern "C" fn(particle: CodeValue) -> CodeValue;

#[repr(C)]
pub struct CodeExportVar {
    pub name: *const c_char,
    pub value: CodeValue,
}

#[repr(C)]
pub struct CodeExportHandler {
    pub class_name: *const c_char,
    pub handler: CodeNativeHandlerFn,
}

#[repr(C)]
pub struct CodeTypeField {
    pub name: *const c_char,
    pub type_name: *const c_char,
    pub is_optional: u8,
}

#[repr(C)]
pub struct CodeExportType {
    pub name: *const c_char,
    pub fields: *const CodeTypeField,
    pub field_count: u32,
}

#[repr(C)]
pub struct CodeModuleDesc {
    pub abi_version: u32,
    pub vars: *const CodeExportVar,
    pub var_count: u32,
    pub handlers: *const CodeExportHandler,
    pub handler_count: u32,
    pub types: *const CodeExportType,
    pub type_count: u32,
    pub emissions: *const u8,
    pub emission_count: u32,
}

// -----------------------------------------------------------------------
// Helper constructors
// -----------------------------------------------------------------------
fn code_number(n: f64) -> CodeValue {
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

fn code_string(s: *const c_char) -> CodeValue {
    CodeValue {
        tag: CODE_TAG_STRING,
        number: 0.0,
        string: s,
        boolean: 0,
        fields: ptr::null(),
        field_count: 0,
        elements: ptr::null(),
        element_count: 0,
    }
}

fn code_boolean(b: bool) -> CodeValue {
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

fn code_null() -> CodeValue {
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

fn code_object(fields: *const CodeField, count: u32) -> CodeValue {
    CodeValue {
        tag: CODE_TAG_OBJECT,
        number: 0.0,
        string: ptr::null(),
        boolean: 0,
        fields,
        field_count: count,
        elements: ptr::null(),
        element_count: 0,
    }
}

/// Read a C string from an CodeValue, returning "" if null or wrong tag.
unsafe fn read_str<'a>(v: &CodeValue) -> &'a str {
    if v.tag != CODE_TAG_STRING || v.string.is_null() {
        return "";
    }
    CStr::from_ptr(v.string).to_str().unwrap_or("")
}

/// Leak a Rust String into a `*const c_char` that lives forever.
/// Acceptable for test/example modules.
fn leak_cstring(s: String) -> *const c_char {
    CString::new(s).unwrap().into_raw() as *const c_char
}

// -----------------------------------------------------------------------
// Handler
// -----------------------------------------------------------------------

/// Handler for Message particles.
/// If `urgent` is true, uppercases the `text` field.
unsafe extern "C" fn handle_message(particle: CodeValue) -> CodeValue {
    let mut text = String::new();
    let mut urgent = false;

    for i in 0..particle.field_count as usize {
        let field = &*particle.fields.add(i);
        let name = CStr::from_ptr(field.name).to_str().unwrap_or("");
        match name {
            "text" => text = read_str(&field.value).to_string(),
            "urgent" if field.value.tag == CODE_TAG_BOOLEAN => {
                urgent = field.value.boolean != 0;
            }
            _ => {}
        }
    }

    let processed = if urgent {
        text.to_uppercase()
    } else {
        text
    };

    // Build result particle: Message { _class, text, urgent, processed }
    // We leak the boxes so the pointers survive the return.
    let class_name = c"_class".as_ptr();
    let text_key = c"text".as_ptr();
    let urgent_key = c"urgent".as_ptr();
    let processed_key = c"processed".as_ptr();

    let fields = Box::leak(Box::new([
        CodeField { name: class_name, value: code_string(c"Message".as_ptr()) },
        CodeField { name: text_key, value: code_string(leak_cstring(processed.clone())) },
        CodeField { name: urgent_key, value: code_boolean(urgent) },
        CodeField { name: processed_key, value: code_boolean(true) },
    ]));

    code_object(fields.as_ptr(), fields.len() as u32)
}

// -----------------------------------------------------------------------
// Static module descriptor
// -----------------------------------------------------------------------

// Raw pointers are not Sync, but our statics are truly immutable constant
// data, so it is safe to share them across threads.
unsafe impl Sync for CodeValue {}
unsafe impl Sync for CodeField {}
unsafe impl Sync for CodeExportVar {}
unsafe impl Sync for CodeExportHandler {}
unsafe impl Sync for CodeTypeField {}
unsafe impl Sync for CodeExportType {}
unsafe impl Sync for CodeModuleDesc {}

// We use `c"..."` literal syntax (Rust 1.77+) for static C strings.

static MODULE_VARS: [CodeExportVar; 2] = [
    CodeExportVar {
        name: c"VERSION".as_ptr(),
        value: CodeValue {
            tag: CODE_TAG_STRING,
            number: 0.0,
            string: c"1.0.0".as_ptr(),
            boolean: 0,
            fields: ptr::null(),
            field_count: 0,
            elements: ptr::null(),
            element_count: 0,
        },
    },
    CodeExportVar {
        name: c"MAX_LEN".as_ptr(),
        value: CodeValue {
            tag: CODE_TAG_NUMBER,
            number: 1024.0,
            string: ptr::null(),
            boolean: 0,
            fields: ptr::null(),
            field_count: 0,
            elements: ptr::null(),
            element_count: 0,
        },
    },
];

static MESSAGE_FIELDS: [CodeTypeField; 2] = [
    CodeTypeField { name: c"text".as_ptr(),   type_name: c"String".as_ptr(),  is_optional: 0 },
    CodeTypeField { name: c"urgent".as_ptr(), type_name: c"Boolean".as_ptr(), is_optional: 0 },
];

static MODULE_TYPES: [CodeExportType; 1] = [
    CodeExportType {
        name: c"Message".as_ptr(),
        fields: MESSAGE_FIELDS.as_ptr(),
        field_count: 2,
    },
];

static MODULE_HANDLERS: [CodeExportHandler; 1] = [
    CodeExportHandler { class_name: c"Message".as_ptr(), handler: handle_message },
];

static MODULE_DESC: CodeModuleDesc = CodeModuleDesc {
    abi_version: 2,
    vars: MODULE_VARS.as_ptr(),
    var_count: 2,
    handlers: MODULE_HANDLERS.as_ptr(),
    handler_count: 1,
    types: MODULE_TYPES.as_ptr(),
    type_count: 1,
    emissions: ptr::null(),
    emission_count: 0,
};

// -----------------------------------------------------------------------
// Exported ABI entry points
// -----------------------------------------------------------------------

#[no_mangle]
pub extern "C" fn code_module_abi_version() -> u32 {
    2
}

#[no_mangle]
pub extern "C" fn code_module_init() -> *const CodeModuleDesc {
    &MODULE_DESC
}
