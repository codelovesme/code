//! Native module ABI contract and loader.
//!
//! This module defines the C-ABI types that native libraries (.so) must export
//! to be usable as Code modules, and provides the runtime loader that
//! converts native exports into Code `Value`s.
//!
//! # ABI Contract
//!
//! A native module must export two C symbols:
//!
//! ```c
//! uint32_t code_module_abi_version(void);   // must return 2
//! const CodeModuleDesc* code_module_init(void);
//! ```
//!
//! The `CodeModuleDesc` struct enumerates all exported variables,
//! handlers, and type declarations.

use std::any::Any;
use std::collections::{HashMap, VecDeque};
#[cfg(feature = "native-so")]
use std::ffi::{c_char, c_void, CStr, CString};
use std::fmt;
#[cfg(feature = "native-so")]
use std::path::Path;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

#[cfg(feature = "native-so")]
use crate::ast::{ConstraintExpr, FieldConstraint, TypeExpr};
use crate::ast::TypeInfo;
use crate::runtime::Value;

// ---------------------------------------------------------------------------
// C-ABI contract — the single source of truth lives in the `code-abi` crate.
// Re-exported here so existing `native_module::Code*` paths keep resolving.
// ---------------------------------------------------------------------------

pub use code_abi::{
    CodeEmission, CodeEmitFn, CodeExportHandler, CodeExportType, CodeExportVar, CodeField,
    CodeModuleDesc, CodeNativeHandlerFn, CodeTypeField, CodeValue, CODE_ABI_VERSION,
    CODE_EMIT_TARGET_BASE, CODE_TAG_ARRAY, CODE_TAG_BOOLEAN, CODE_TAG_NULL, CODE_TAG_NUMBER,
    CODE_TAG_OBJECT, CODE_TAG_STRING,
};

// ---------------------------------------------------------------------------
// Rust-side wrapper types
// ---------------------------------------------------------------------------

/// A clonable, debuggable wrapper around a native function pointer.
///
/// The inner `Arc` keeps the originating `Library` alive through a captured
/// reference inside the closure.
#[derive(Clone)]
pub struct NativeFnPtr(pub Arc<dyn Fn(Vec<Rc<Value>>) -> Result<Rc<Value>, String>>);

impl fmt::Debug for NativeFnPtr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<native fn>")
    }
}

/// Information about a native handler (stored alongside AST handlers).
#[derive(Debug, Clone)]
pub struct NativeHandlerInfo {
    pub class_name: String,
    pub func: NativeFnPtr,
}

/// Declared emission from a native module.
#[derive(Debug, Clone)]
pub struct EmissionDecl {
    pub class_name: String,
    /// Target string: currently only `"base"`.
    pub target: String,
}

// ---------------------------------------------------------------------------
// Thread-safe emitted value (for cross-thread emission queues)
// ---------------------------------------------------------------------------

/// A thread-safe representation of a Code value, used to pass emitted
/// particles from native module threads to the interpreter thread.
#[derive(Debug, Clone)]
pub enum EmittedValue {
    Number(f64),
    String(String),
    Boolean(bool),
    Object(Vec<(String, EmittedValue)>),
    Array(Vec<EmittedValue>),
    Null,
}

impl EmittedValue {
    /// Convert a thread-safe `EmittedValue` to an `Rc<Value>` on the
    /// interpreter thread.
    pub fn to_value(&self) -> Rc<Value> {
        match self {
            EmittedValue::Number(n) => Value::number(*n),
            EmittedValue::String(s) => Value::string(s.clone()),
            EmittedValue::Boolean(b) => Value::boolean(*b),
            EmittedValue::Object(fields) => {
                let mut map = HashMap::new();
                for (name, val) in fields {
                    map.insert(name.clone(), val.to_value());
                }
                Value::object(map)
            }
            EmittedValue::Array(elements) => {
                Value::array(elements.iter().map(|e| e.to_value()).collect())
            }
            EmittedValue::Null => Value::null(),
        }
    }

    /// Convert an `Rc<Value>` to a thread-safe `EmittedValue` that can be
    /// sent across thread boundaries.
    pub fn from_value(val: &Value) -> EmittedValue {
        match val {
            Value::Number(n) => EmittedValue::Number(*n),
            Value::String(s) => EmittedValue::String(s.clone()),
            Value::Boolean(b) => EmittedValue::Boolean(*b),
            Value::Object(fields) => {
                let entries: Vec<(String, EmittedValue)> = fields.iter()
                    .map(|(k, v)| (k.clone(), EmittedValue::from_value(v)))
                    .collect();
                EmittedValue::Object(entries)
            }
            Value::Array(elements) | Value::Set(elements) => {
                // Sets flatten to an array crossing this boundary — the
                // native-module ABI has no Set tag yet (T26 is
                // interpreter-only for Phase 1); a deduplicated element
                // list is already a valid (if lossy-of-"setness") Array.
                EmittedValue::Array(elements.iter().map(|e| EmittedValue::from_value(e)).collect())
            }
            Value::Null => EmittedValue::Null,
        }
    }
}

/// Thread-safe queue for particles emitted by a native module.
pub type EmitQueue = Arc<Mutex<VecDeque<EmittedValue>>>;

/// A fully loaded native module — ready to install into the Environment.
#[derive(Debug, Clone)]
pub struct NativeModule {
    /// Opaque handle kept alive via Arc — for .so this is Arc<libloading::Library>;
    /// for .wasm this is Arc<WasmLibraryHandle> (or any other Send+Sync+Any value).
    pub _library: Arc<dyn Any + Send + Sync>,
    /// Exported variables (name → value).
    pub vars: Vec<(String, Rc<Value>)>,
    /// Exported handlers (class_name → handler callable).
    pub handlers: Vec<NativeHandlerInfo>,
    /// Exported type declarations.
    pub types: Vec<TypeInfo>,
    /// Declared emissions (what particles this module may emit).
    pub emissions: Vec<EmissionDecl>,
    /// Thread-safe queue for receiving emitted particles from the module.
    pub emit_queue: EmitQueue,
}

// ---------------------------------------------------------------------------
// CodeValue helpers (null initializer)
// ---------------------------------------------------------------------------


// ---------------------------------------------------------------------------
// Conversion: CodeValue → Rc<Value>
// ---------------------------------------------------------------------------

/// Convert a C-ABI `CodeValue` into a heap-allocated Code `Value`.
///
/// # Safety
/// The caller must ensure that all pointers inside `c_value` are valid.
#[cfg(feature = "native-so")]
pub unsafe fn code_value_to_value(c_value: &CodeValue) -> Rc<Value> {
    match c_value.tag {
        CODE_TAG_NUMBER => Value::number(c_value.number),
        CODE_TAG_STRING => {
            if c_value.string.is_null() {
                Value::string("")
            } else {
                let s = CStr::from_ptr(c_value.string)
                    .to_str()
                    .unwrap_or("")
                    .to_string();
                Value::string(s)
            }
        }
        CODE_TAG_BOOLEAN => Value::boolean(c_value.boolean != 0),
        CODE_TAG_OBJECT => {
            let mut map = HashMap::new();
            for i in 0..c_value.field_count as usize {
                let field = &*c_value.fields.add(i);
                let name = CStr::from_ptr(field.name)
                    .to_str()
                    .unwrap_or("")
                    .to_string();
                let val = code_value_to_value(&field.value);
                map.insert(name, val);
            }
            Value::object(map)
        }
        CODE_TAG_ARRAY => {
            let mut elements = Vec::new();
            for i in 0..c_value.element_count as usize {
                let elem = &*c_value.elements.add(i);
                elements.push(code_value_to_value(elem));
            }
            Value::array(elements)
        }
        _ => Value::null(), // CODE_TAG_NULL or unknown
    }
}

// ---------------------------------------------------------------------------
// Conversion: Value → CodeValue (for passing values to native handlers)
// ---------------------------------------------------------------------------

/// Temporary storage that keeps C-compatible data alive for the duration of
/// a native function call.
#[cfg(feature = "native-so")]
pub struct CodeValueBacking {
    pub strings: Vec<CString>,
    pub fields: Vec<Vec<CodeField>>,
    pub elements: Vec<Vec<CodeValue>>,
    /// Nested backings for recursive values.
    pub children: Vec<CodeValueBacking>,
}

#[cfg(feature = "native-so")]
impl CodeValueBacking {
    pub fn new() -> Self {
        CodeValueBacking {
            strings: Vec::new(),
            fields: Vec::new(),
            elements: Vec::new(),
            children: Vec::new(),
        }
    }
}

/// Convert a Rust `Value` to a C-ABI `CodeValue`.
///
/// The `backing` struct must outlive the returned `CodeValue` so that
/// all C strings and arrays remain valid.
#[cfg(feature = "native-so")]
pub fn value_to_code_value(val: &Value, backing: &mut CodeValueBacking) -> CodeValue {
    match val {
        Value::Number(n) => CodeValue {
            tag: CODE_TAG_NUMBER,
            number: *n,
            ..CodeValue::null()
        },
        Value::String(s) => {
            let cs = CString::new(s.as_str()).unwrap_or_else(|_| CString::new("").unwrap());
            let ptr = cs.as_ptr();
            backing.strings.push(cs);
            CodeValue {
                tag: CODE_TAG_STRING,
                string: ptr,
                ..CodeValue::null()
            }
        }
        Value::Boolean(b) => CodeValue {
            tag: CODE_TAG_BOOLEAN,
            boolean: if *b { 1 } else { 0 },
            ..CodeValue::null()
        },
        Value::Object(map) => {
            let mut child = CodeValueBacking::new();
            let mut code_fields: Vec<CodeField> = Vec::new();
            for (name, val) in map {
                let name_cs = CString::new(name.as_str()).unwrap_or_else(|_| CString::new("").unwrap());
                let name_ptr = name_cs.as_ptr();
                child.strings.push(name_cs);
                let code_val = value_to_code_value(val, &mut child);
                code_fields.push(CodeField {
                    name: name_ptr,
                    value: code_val,
                });
            }
            let fields_ptr = code_fields.as_ptr();
            let field_count = code_fields.len() as u32;
            backing.fields.push(code_fields);
            backing.children.push(child);
            CodeValue {
                tag: CODE_TAG_OBJECT,
                fields: fields_ptr,
                field_count,
                ..CodeValue::null()
            }
        }
        // Sets flatten to an array crossing this boundary — the native-module
        // ABI has no Set tag yet (T26 is interpreter-only for Phase 1); a
        // deduplicated element list is already a valid (if lossy-of-
        // "setness") Array.
        Value::Array(elements) | Value::Set(elements) => {
            let mut child = CodeValueBacking::new();
            let mut code_elements: Vec<CodeValue> = Vec::new();
            for elem in elements {
                code_elements.push(value_to_code_value(elem, &mut child));
            }
            let elems_ptr = code_elements.as_ptr();
            let elem_count = code_elements.len() as u32;
            backing.elements.push(code_elements);
            backing.children.push(child);
            CodeValue {
                tag: CODE_TAG_ARRAY,
                elements: elems_ptr,
                element_count: elem_count,
                ..CodeValue::null()
            }
        }
        Value::Null => CodeValue::null(),
    }
}

// ---------------------------------------------------------------------------
// Loader: open .so, read descriptor, produce NativeModule
// ---------------------------------------------------------------------------

/// Load a native module from a shared library (.so).
///
/// The library must export `code_module_abi_version` and `code_module_init`.
#[cfg(feature = "native-so")]
pub fn load_native_module(path: &Path) -> Result<NativeModule, String> {
    unsafe {
        let lib = libloading::Library::new(path)
            .map_err(|e| format!("Failed to load native module '{}': {}", path.display(), e))?;
        let lib = Arc::new(lib);

        // --- ABI version check ---
        let version_fn: libloading::Symbol<unsafe extern "C" fn() -> u32> = lib
            .get(b"code_module_abi_version")
            .map_err(|e| format!(
                "Native module '{}' missing symbol 'code_module_abi_version': {}",
                path.display(), e
            ))?;
        let version = version_fn();
        if version != CODE_ABI_VERSION {
            return Err(format!(
                "Native module '{}' has ABI version {} (expected {})",
                path.display(), version, CODE_ABI_VERSION
            ));
        }

        // --- Module descriptor ---
        let init_fn: libloading::Symbol<unsafe extern "C" fn() -> *const CodeModuleDesc> = lib
            .get(b"code_module_init")
            .map_err(|e| format!(
                "Native module '{}' missing symbol 'code_module_init': {}",
                path.display(), e
            ))?;
        let desc_ptr = init_fn();
        if desc_ptr.is_null() {
            return Err(format!(
                "Native module '{}': code_module_init() returned null",
                path.display()
            ));
        }
        let desc = &*desc_ptr;

        // --- Convert exported variables ---
        let mut vars: Vec<(String, Rc<Value>)> = Vec::new();
        for i in 0..desc.var_count as usize {
            let export = &*desc.vars.add(i);
            let name = c_str_to_string(export.name);
            let value = code_value_to_value(&export.value);
            vars.push((name, value));
        }

        // --- Convert exported handlers ---
        let mut handlers: Vec<NativeHandlerInfo> = Vec::new();
        for i in 0..desc.handler_count as usize {
            let export = &*desc.handlers.add(i);
            let class_name = c_str_to_string(export.class_name);
            let handler_ptr = export.handler;

            let lib_arc = Arc::clone(&lib);
            let handler_wrapper = NativeFnPtr(Arc::new(move |args: Vec<Rc<Value>>| {
                let _keep_alive = &lib_arc;
                if args.is_empty() {
                    return Err("Native handler called with no arguments".to_string());
                }
                let mut backing = CodeValueBacking::new();
                let particle_code = value_to_code_value(&args[0], &mut backing);
                let result = handler_ptr(particle_code);
                Ok(code_value_to_value(&result))
            }));
            handlers.push(NativeHandlerInfo {
                class_name,
                func: handler_wrapper,
            });
        }

        // --- Convert exported types ---
        let mut types: Vec<TypeInfo> = Vec::new();
        for i in 0..desc.type_count as usize {
            let export = &*desc.types.add(i);
            let name = c_str_to_string(export.name);
            let mut fields = Vec::new();
            for j in 0..export.field_count as usize {
                let tf = &*export.fields.add(j);
                let field_name = c_str_to_string(tf.name);
                let type_name = c_str_to_string(tf.type_name);
                let is_optional = tf.is_optional != 0;
                fields.push(FieldConstraint {
                    name: field_name,
                    constraints: vec![ConstraintExpr::IsType(TypeExpr::Named(type_name))],
                    optional: is_optional,
                });
            }
            types.push(TypeInfo { name, fields });
        }

        // --- Read emission declarations ---
        let mut emissions: Vec<EmissionDecl> = Vec::new();
        for i in 0..desc.emission_count as usize {
            let em = &*desc.emissions.add(i);
            let class_name = c_str_to_string(em.class_name);
            let target = match em.target {
                CODE_EMIT_TARGET_BASE => "base".to_string(),
                _ => "base".to_string(),
            };
            emissions.push(EmissionDecl { class_name, target });
        }

        // --- Set up emit callback queue ---
        let emit_queue: EmitQueue = Arc::new(Mutex::new(VecDeque::new()));

        // Look for `code_module_set_emit` symbol and call it.
        let set_emit_fn: Result<libloading::Symbol<unsafe extern "C" fn(CodeEmitFn, *mut c_void)>, _> =
            lib.get(b"code_module_set_emit");
        if let Ok(set_emit) = set_emit_fn {
            // Leak an Arc clone so the queue stays alive as long as the module.
            let queue_ptr = Arc::into_raw(Arc::clone(&emit_queue)) as *mut c_void;
            set_emit(host_emit_callback, queue_ptr);
        }

        Ok(NativeModule {
            _library: lib as Arc<dyn Any + Send + Sync>,
            vars,
            handlers,
            types,
            emissions,
            emit_queue,
        })
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

#[cfg(feature = "native-so")]
unsafe fn c_str_to_string(ptr: *const c_char) -> String {
    if ptr.is_null() {
        String::new()
    } else {
        CStr::from_ptr(ptr)
            .to_str()
            .unwrap_or("")
            .to_string()
    }
}

// ---------------------------------------------------------------------------
// Emit callback (called by native modules, bridges to EmitQueue)
// ---------------------------------------------------------------------------

/// Convert a C-ABI `CodeValue` into a thread-safe `EmittedValue`.
///
/// # Safety
/// All pointers inside `c_value` must be valid.
#[cfg(feature = "native-so")]
unsafe fn code_value_to_emitted(c_value: &CodeValue) -> EmittedValue {
    match c_value.tag {
        CODE_TAG_NUMBER => EmittedValue::Number(c_value.number),
        CODE_TAG_STRING => {
            if c_value.string.is_null() {
                EmittedValue::String(String::new())
            } else {
                EmittedValue::String(c_str_to_string(c_value.string))
            }
        }
        CODE_TAG_BOOLEAN => EmittedValue::Boolean(c_value.boolean != 0),
        CODE_TAG_OBJECT => {
            let mut fields = Vec::new();
            for i in 0..c_value.field_count as usize {
                let field = &*c_value.fields.add(i);
                let name = c_str_to_string(field.name);
                let val = code_value_to_emitted(&field.value);
                fields.push((name, val));
            }
            EmittedValue::Object(fields)
        }
        CODE_TAG_ARRAY => {
            let mut elements = Vec::new();
            for i in 0..c_value.element_count as usize {
                let elem = &*c_value.elements.add(i);
                elements.push(code_value_to_emitted(elem));
            }
            EmittedValue::Array(elements)
        }
        _ => EmittedValue::Null,
    }
}

/// Host-side emit callback — called from native module threads.
///
/// `context` is a raw pointer to an `Arc<Mutex<VecDeque<EmittedValue>>>` that
/// was leaked via `Arc::into_raw`.
///
/// # Safety
/// * `context` must have been produced by `Arc::into_raw` on an `EmitQueue`.
/// * `particle` must be a valid `CodeValue`.
#[cfg(feature = "native-so")]
unsafe extern "C" fn host_emit_callback(context: *mut c_void, particle: CodeValue) {
    if context.is_null() {
        return;
    }
    // Reconstruct a temporary Arc reference without taking ownership.
    let queue = Arc::from_raw(context as *const Mutex<VecDeque<EmittedValue>>);
    let emitted = code_value_to_emitted(&particle);
    if let Ok(mut q) = queue.lock() {
        q.push_back(emitted);
    }
    // Don't drop the Arc — it was leaked intentionally. Re-leak it.
    let _ = Arc::into_raw(queue);
}
