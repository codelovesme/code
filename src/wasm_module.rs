//! WebAssembly native module backend.
//!
//! Loads `.wasm` files and bridges their exports into `NativeModule` using the
//! `wasmi` embedded interpreter.  The WASM module must export the same two
//! symbols required by the `.so` ABI:
//!
//! ```c
//! uint32_t code_module_abi_version(void);
//! uint32_t code_module_init(void);   // returns offset into linear memory
//! ```
//!
//! Unlike the native (C) ABI, all pointer-like values are 32-bit offsets into
//! the module's linear memory (wasm32 address space).
//!
//! # Memory layout of CodeModuleDesc (wasm32, little-endian)
//!
//! ```text
//! Offset  Size  Field
//! 0       4     abi_version       (u32)
//! 4       4     vars_ptr          (u32 — offset to [CodeExportVar])
//! 8       4     var_count         (u32)
//! 12      4     reserved          (u32, must be 0)
//! 16      4     reserved          (u32, must be 0)
//! 20      4     handlers_ptr      (u32 — offset to [CodeExportHandler])
//! 24      4     handler_count     (u32)
//! 28      4     types_ptr         (u32 — offset to [CodeExportType])
//! 32      4     type_count        (u32)
//! ```
//!
//! Offsets 12/16 were originally a function-export slot; Code has no
//! function-call concept (see docs/tickets/T11-*.md, T12-*.md), so they are
//! kept as reserved/zeroed padding rather than reshuffling every subsequent
//! offset — a wire-format change that could break external `.wasm` modules
//! built against this layout, for no functional gain.
//!
//! CodeExportVar (16 bytes):
//! ```text
//! 0   4   name_ptr    (u32)
//! 4   12  value       (CodeValue)
//! ```
//!
//! CodeExportHandler (8 bytes):
//! ```text
//! 0   4   class_name_ptr  (u32)
//! 4   4   func_idx        (u32)
//! ```
//!
//! CodeExportType (12 bytes):
//! ```text
//! 0   4   name_ptr       (u32)
//! 4   4   fields_ptr     (u32 — offset to [CodeTypeField])
//! 8   4   field_count    (u32)
//! ```
//!
//! CodeTypeField (12 bytes):
//! ```text
//! 0   4   name_ptr        (u32)
//! 4   4   type_name_ptr   (u32)
//! 8   1   is_optional     (u8)
//! 9   3   _pad
//! ```
//!
//! CodeValue (32 bytes, mirrors the compiled layout):
//! ```text
//! 0   1   tag             (u8)
//! 1   7   _pad
//! 8   8   number          (f64)
//! 16  4   ptr             (u32 — string/object/array offset)
//! 20  4   ptr_count       (u32 — field/element count for object/array)
//! 24  1   boolean         (u8)
//! 25  7   _pad
//! ```
//! -- total 32 bytes --
//!
//! Strings: null-terminated UTF-8 bytes at `ptr`.
//! Objects: `ptr` points to array of `[ptr_count]` CodeField.
//! Arrays:  `ptr` points to array of `[ptr_count]` CodeValue.
//!
//! CodeField (8 bytes):
//! ```text
//! 0   4   name_ptr    (u32)
//! 4   4   value_off   (u32 — offset of CodeValue within the field block)
//! ```
//! Wait, for simplicity we use a flat repr:
//!
//! CodeField inline (36 bytes):
//! ```text
//! 0   4   name_ptr    (u32)
//! 4   32  value       (CodeValue, 32 bytes)
//! ```
//! Note: the C inline layout for CodeField {name: *const c_char, value: CodeValue}
//! with 8-byte alignment = 8 + 32 = 40 bytes.  But for wasm32, pointers are 4
//! bytes, so CodeField = name_ptr(4) + _pad(4) + CodeValue(32) = 40 bytes.

use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use wasmi::{Engine, ExternType, Instance, Linker, Module, Store, Val};

use crate::ast::{ConstraintExpr, FieldConstraint, TypeExpr, TypeInfo};
use crate::native_module::{EmitQueue, EmissionDecl, NativeFnPtr, NativeHandlerInfo, NativeModule};
use crate::runtime::Value;

/// ABI version constant — shared with the `.so` contract via the `code-abi` crate.
use code_abi::CODE_ABI_VERSION;

// ------------------------------------------------------------------
// Memory layout constants (wasm32 little-endian)
// ------------------------------------------------------------------

// CodeValue layout (32 bytes, same as compiled LLVM layout on wasm32):
//   [0]  tag         u8
//   [1..7] padding
//   [8]  number      f64
//   [16] ptr         u32   (string/fields/elements offset)
//   [20] count       u32   (field_count or element_count)
//   [24] boolean     u8
//   [25..31] padding
const CODE_VAL_SIZE: u32 = 32;
const CODE_VAL_TAG: u32 = 0;
const CODE_VAL_NUM: u32 = 8;
const CODE_VAL_PTR: u32 = 16;
const CODE_VAL_COUNT: u32 = 20;
const CODE_VAL_BOOL: u32 = 24;

// CodeField layout (40 bytes, wasm32):
//   [0]  name_ptr    u32
//   [4]  padding u32
//   [8]  value       CodeValue (32 bytes)
const CODE_FIELD_SIZE: u32 = 40;
const CODE_FIELD_NAME: u32 = 0;
const CODE_FIELD_VALUE: u32 = 8;

// CodeModuleDesc layout (44 bytes), wasm32 linear-memory offsets:
//   [0]  abi_version    u32
//   [4]  vars_ptr       u32
//   [8]  var_count      u32
//   [12] reserved       u32  (must be 0 — see the module doc comment above)
//   [16] reserved       u32  (must be 0)
//   [20] handlers_ptr   u32
//   [24] handler_count  u32
//   [28] types_ptr      u32
//   [32] type_count     u32
//   [36] emissions_ptr  u32
//   [40] emission_count u32
const DESC_ABI_VERSION: u32 = 0;
const DESC_VARS_PTR: u32 = 4;
const DESC_VAR_COUNT: u32 = 8;
// Offsets 12/16 are reserved/zeroed padding (see module doc comment).
const DESC_HANDLERS_PTR: u32 = 20;
const DESC_HANDLER_COUNT: u32 = 24;
const DESC_TYPES_PTR: u32 = 28;
const DESC_TYPE_COUNT: u32 = 32;
const DESC_EMISSIONS_PTR: u32 = 36;
const DESC_EMISSION_COUNT: u32 = 40;

// CodeExportVar (16 bytes):
//   [0]  name_ptr  u32
//   [4]  pad       u32
//   [8]  value     CodeValue? No — CodeValue is 32 bytes, so:
//   [0]  name_ptr  u32
//   [4]  pad       u32 (to align CodeValue to 8)
//   [8]  value     CodeValue (32 bytes)
//   total 40 bytes
const CODE_EXPORT_VAR_SIZE: u32 = 40;
const CODE_EXPORT_VAR_NAME: u32 = 0;
const CODE_EXPORT_VAR_VALUE: u32 = 8;

// CodeExportHandler (8 bytes):
//   [0]  class_name_ptr  u32
//   [4]  func_idx        u32
const CODE_EXPORT_HANDLER_SIZE: u32 = 8;
const CODE_EXPORT_HANDLER_NAME: u32 = 0;
const CODE_EXPORT_HANDLER_IDX: u32 = 4;

// CodeExportType (12 bytes):
//   [0]  name_ptr     u32
//   [4]  fields_ptr   u32
//   [8]  field_count  u32
const CODE_EXPORT_TYPE_SIZE: u32 = 12;
const CODE_EXPORT_TYPE_NAME: u32 = 0;
const CODE_EXPORT_TYPE_FIELDS: u32 = 4;
const CODE_EXPORT_TYPE_FIELD_COUNT: u32 = 8;

// CodeTypeField (12 bytes):
//   [0]  name_ptr       u32
//   [4]  type_name_ptr  u32
//   [8]  is_optional    u8
const CODE_TYPE_FIELD_SIZE: u32 = 12;
const CODE_TYPE_FIELD_NAME: u32 = 0;
const CODE_TYPE_FIELD_TYPE: u32 = 4;
const CODE_TYPE_FIELD_OPT: u32 = 8;

// CodeEmission (8 bytes):
//   [0]  class_name_ptr   u32
//   [4]  target           u32
const CODE_EMISSION_SIZE: u32 = 8;
const CODE_EMISSION_NAME: u32 = 0;
const CODE_EMISSION_TARGET: u32 = 4;

// Tag constants matching the C ABI and codegen.
const TAG_NUMBER: u8 = 0;
const TAG_STRING: u8 = 1;
const TAG_BOOLEAN: u8 = 2;
const TAG_OBJECT: u8 = 3;
const TAG_NULL: u8 = 4;
const TAG_ARRAY: u8 = 5;

// ------------------------------------------------------------------
// Memory helpers
// ------------------------------------------------------------------

fn read_u8(mem: &[u8], off: u32) -> u8 {
    mem[off as usize]
}

fn read_u32(mem: &[u8], off: u32) -> u32 {
    let b = &mem[off as usize..off as usize + 4];
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

fn read_f64(mem: &[u8], off: u32) -> f64 {
    let b = &mem[off as usize..off as usize + 8];
    f64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
}

/// Read a null-terminated string at the given offset.
fn read_cstr(mem: &[u8], off: u32) -> String {
    let start = off as usize;
    let end = mem[start..].iter().position(|&b| b == 0).unwrap_or(0);
    std::str::from_utf8(&mem[start..start + end])
        .unwrap_or("")
        .to_string()
}

/// Read a CodeValue from memory at the given offset.
/// Public so that the WasmCell executor can deserialise particle arguments
/// from linear memory without re-implementing the layout.
pub fn read_code_value(mem: &[u8], off: u32) -> Value {
    let tag = read_u8(mem, off + CODE_VAL_TAG);
    match tag {
        TAG_NUMBER => {
            let n = read_f64(mem, off + CODE_VAL_NUM);
            Value::Number(n)
        }
        TAG_STRING => {
            let ptr = read_u32(mem, off + CODE_VAL_PTR);
            if ptr == 0 {
                Value::String(String::new())
            } else {
                Value::String(read_cstr(mem, ptr))
            }
        }
        TAG_BOOLEAN => {
            let b = read_u8(mem, off + CODE_VAL_BOOL);
            Value::Boolean(b != 0)
        }
        TAG_OBJECT => {
            let fields_ptr = read_u32(mem, off + CODE_VAL_PTR);
            let field_count = read_u32(mem, off + CODE_VAL_COUNT);
            let mut map = HashMap::new();
            for i in 0..field_count {
                let field_off = fields_ptr + i * CODE_FIELD_SIZE;
                let name_ptr = read_u32(mem, field_off + CODE_FIELD_NAME);
                let name = read_cstr(mem, name_ptr);
                let val = read_code_value(mem, field_off + CODE_FIELD_VALUE);
                map.insert(name, Rc::new(val));
            }
            Value::Object(map)
        }
        TAG_ARRAY => {
            let elems_ptr = read_u32(mem, off + CODE_VAL_PTR);
            let elem_count = read_u32(mem, off + CODE_VAL_COUNT);
            let mut elems = Vec::new();
            for i in 0..elem_count {
                let elem_off = elems_ptr + i * CODE_VAL_SIZE;
                elems.push(Rc::new(read_code_value(mem, elem_off)));
            }
            Value::Array(elems)
        }
        _ => Value::Null,
    }
}

// ------------------------------------------------------------------
// Public loader
// ------------------------------------------------------------------

/// Load a WebAssembly module from a `.wasm` file, instantiate it via `wasmi`,
/// and return a `NativeModule` with bridged Rust wrappers for the WASM exports.
///
/// The module must export `code_module_abi_version` and `code_module_init`
/// following the Code WASM module ABI (documented above).
/// Functions that accept arguments must also export `code_alloc(i32) -> i32`.
pub fn load_wasm_module(path: &Path) -> Result<NativeModule, String> {
    let bytes = std::fs::read(path)
        .map_err(|e| format!("Failed to read WASM module '{}': {}", path.display(), e))?;

    let engine = Engine::default();
    let wasm_module = Module::new(&engine, &bytes)
        .map_err(|e| format!("Failed to parse WASM module '{}': {}", path.display(), e))?;

    let linker: Linker<()> = Linker::new(&engine);
    let mut store = Store::new(&engine, ());

    // Register stub functions for any unsatisfied imports.
    // WASM modules (e.g. browser organelles) may import host functions that are
    // not available at compile time.  We provide no-op stubs so that wasmi can
    // instantiate the module for type introspection.
    let mut linker = linker;
    for import in wasm_module.imports() {
        if let ExternType::Func(func_ty) = import.ty() {
            let results_len = func_ty.results().len();
            let stub_ty = func_ty.clone();
            let _ = linker.func_new(
                import.module(),
                import.name(),
                stub_ty,
                move |_caller, _params, results| {
                    for r in results.iter_mut().take(results_len) {
                        *r = Val::I32(0);
                    }
                    Ok(())
                },
            );
        }
    }

    let instance = linker
        .instantiate_and_start(&mut store, &wasm_module)
        .map_err(|e| format!("Failed to instantiate WASM module '{}': {}", path.display(), e))?;

    // --- ABI version check ---
    let version_fn = instance
        .get_func(&store, "code_module_abi_version")
        .ok_or_else(|| format!(
            "WASM module '{}' missing 'code_module_abi_version'",
            path.display()
        ))?;
    let mut ver_result = [Val::I32(0)];
    version_fn.call(&mut store, &[], &mut ver_result)
        .map_err(|e| format!("code_module_abi_version call failed for '{}': {}", path.display(), e))?;
    let version = ver_result[0].i32().unwrap_or(0) as u32;
    if version != CODE_ABI_VERSION {
        return Err(format!(
            "WASM module '{}' has ABI version {} (expected {})",
            path.display(), version, CODE_ABI_VERSION
        ));
    }

    // --- Module descriptor ---
    let init_fn = instance
        .get_func(&store, "code_module_init")
        .ok_or_else(|| format!(
            "WASM module '{}' missing 'code_module_init'",
            path.display()
        ))?;
    let mut init_result = [Val::I32(0)];
    init_fn.call(&mut store, &[], &mut init_result)
        .map_err(|e| format!("code_module_init call failed for '{}': {}", path.display(), e))?;
    let desc_off = init_result[0].i32().unwrap_or(0) as u32;

    // Get memory name (try "memory", fall through).
    let memory_name = "memory";

    // Helper closure to get memory bytes.
    let get_mem = |store: &Store<()>, instance: &Instance| -> Vec<u8> {
        instance
            .get_memory(store, memory_name)
            .map(|m| m.data(store).to_vec())
            .unwrap_or_default()
    };

    let mem = get_mem(&store, &instance);
    if (desc_off as usize + 44) > mem.len() {
        return Err(format!(
            "WASM module '{}': code_module_init() returned invalid offset {}",
            path.display(), desc_off
        ));
    }

    let abi_ver = read_u32(&mem, desc_off + DESC_ABI_VERSION);
    if abi_ver != CODE_ABI_VERSION {
        return Err(format!(
            "WASM module '{}': descriptor abi_version {} (expected {})",
            path.display(), abi_ver, CODE_ABI_VERSION
        ));
    }

    let vars_ptr = read_u32(&mem, desc_off + DESC_VARS_PTR);
    let var_count = read_u32(&mem, desc_off + DESC_VAR_COUNT);
    let handlers_ptr = read_u32(&mem, desc_off + DESC_HANDLERS_PTR);
    let handler_count = read_u32(&mem, desc_off + DESC_HANDLER_COUNT);
    let types_ptr = read_u32(&mem, desc_off + DESC_TYPES_PTR);
    let type_count = read_u32(&mem, desc_off + DESC_TYPE_COUNT);
    let emissions_ptr = read_u32(&mem, desc_off + DESC_EMISSIONS_PTR);
    let emission_count = read_u32(&mem, desc_off + DESC_EMISSION_COUNT);

    // --- Exported variables ---
    let mut vars: Vec<(String, Rc<Value>)> = Vec::new();
    for i in 0..var_count {
        let off = vars_ptr + i * CODE_EXPORT_VAR_SIZE;
        let name_ptr = read_u32(&mem, off + CODE_EXPORT_VAR_NAME);
        let name = read_cstr(&mem, name_ptr);
        let val = read_code_value(&mem, off + CODE_EXPORT_VAR_VALUE);
        vars.push((name, Rc::new(val)));
    }

    // Shared state for handler closures.
    use std::sync::Mutex;
    let state = Arc::new(Mutex::new(WasmStateShared {
        store,
        instance: instance.clone(),
        _engine: engine,
    }));

    // --- Exported handlers ---
    let mut handlers: Vec<NativeHandlerInfo> = Vec::new();
    for i in 0..handler_count {
        let off = handlers_ptr + i * CODE_EXPORT_HANDLER_SIZE;
        let name_ptr = read_u32(&mem, off + CODE_EXPORT_HANDLER_NAME);
        let class_name = read_cstr(&mem, name_ptr);
        let handler_idx = read_u32(&mem, off + CODE_EXPORT_HANDLER_IDX);

        let state_clone = Arc::clone(&state);
        let handler_class = class_name.clone();
        let module_display = path.display().to_string();

        let handler_wrapper = NativeFnPtr(Arc::new(move |args: Vec<Rc<Value>>| {
            if args.is_empty() {
                return Err("WASM handler called with no arguments".to_string());
            }
            let mut guard = state_clone.lock()
                .map_err(|e| format!("WASM lock error for handler '{}': {}", handler_class, e))?;
            let WasmStateShared { ref mut store, ref instance, .. } = *guard;

            // Look up handler and alloc functions (immutable borrows for lookup).
            let wasm_handler = instance
                .get_func(&*store, &format!("code_handler_{}", handler_idx))
                .or_else(|| instance.get_func(&*store, &format!("code_handler_{}", handler_class)))
                .ok_or_else(|| {
                    format!(
                        "WASM module '{}': handler '{}' (idx {}) not found",
                        module_display, handler_class, handler_idx
                    )
                })?;

            let alloc_fn = instance.get_func(&*store, "code_alloc")
                .ok_or_else(|| "WASM module missing 'code_alloc'".to_string())?;

            // Allocate particle slot, write the particle value.
            let mut alloc_result = [Val::I32(0)];
            alloc_fn.call(&mut *store, &[Val::I32(CODE_VAL_SIZE as i32)], &mut alloc_result)
                .map_err(|e| format!("code_alloc failed: {}", e))?;
            let particle_off = alloc_result[0].i32().unwrap_or(0) as u32;

            write_value_to_mem(&mut *store, instance, particle_off, &args[0])?;

            // Allocate result slot.
            alloc_fn.call(&mut *store, &[Val::I32(CODE_VAL_SIZE as i32)], &mut alloc_result)
                .map_err(|e| format!("code_alloc (result) failed: {}", e))?;
            let result_off = alloc_result[0].i32().unwrap_or(0) as u32;

            // Call: fn(particle_ptr: i32, result_ptr: i32).
            wasm_handler.call(
                &mut *store,
                &[Val::I32(particle_off as i32), Val::I32(result_off as i32)],
                &mut [],
            ).map_err(|e| format!("WASM handler call failed: {}", e))?;

            let mem = instance.get_memory(&*store, "memory")
                .ok_or_else(|| "WASM module has no memory".to_string())?;
            let data = mem.data(&*store).to_vec();
            let val = read_code_value(&data, result_off);
            Ok(Rc::new(val))
        }));

        handlers.push(NativeHandlerInfo { class_name, func: handler_wrapper });
    }

    // --- Exported types ---
    let mut types: Vec<TypeInfo> = Vec::new();
    for i in 0..type_count {
        let off = types_ptr + i * CODE_EXPORT_TYPE_SIZE;
        let name_ptr = read_u32(&mem, off + CODE_EXPORT_TYPE_NAME);
        let name = read_cstr(&mem, name_ptr);
        let fields_ptr_off = read_u32(&mem, off + CODE_EXPORT_TYPE_FIELDS);
        let field_count = read_u32(&mem, off + CODE_EXPORT_TYPE_FIELD_COUNT);
        let mut fields = Vec::new();
        for j in 0..field_count {
            let tf_off = fields_ptr_off + j * CODE_TYPE_FIELD_SIZE;
            let f_name = read_cstr(&mem, read_u32(&mem, tf_off + CODE_TYPE_FIELD_NAME));
            let f_type = read_cstr(&mem, read_u32(&mem, tf_off + CODE_TYPE_FIELD_TYPE));
            let f_opt = read_u8(&mem, tf_off + CODE_TYPE_FIELD_OPT) != 0;
            fields.push(FieldConstraint {
                name: f_name,
                constraints: vec![ConstraintExpr::IsType(TypeExpr::Named(f_type))],
                optional: f_opt,
            });
        }
        types.push(TypeInfo { name, fields });
    }

    // --- Read emission declarations ---
    let mut emissions: Vec<EmissionDecl> = Vec::new();
    for i in 0..emission_count {
        let off = emissions_ptr + i * CODE_EMISSION_SIZE;
        let name_ptr = read_u32(&mem, off + CODE_EMISSION_NAME);
        let class_name = read_cstr(&mem, name_ptr);
        let target_val = read_u32(&mem, off + CODE_EMISSION_TARGET);
        let target = if target_val == 0 { "base".to_string() } else { "base".to_string() };
        emissions.push(EmissionDecl { class_name, target });
    }

    let emit_queue: EmitQueue = std::sync::Arc::new(std::sync::Mutex::new(std::collections::VecDeque::new()));

    // Build a NativeModule.  The _library field holds the wasm state Arc so
    // all closures (and the state) are kept alive as long as the NativeModule is.
    Ok(NativeModule {
        _library: Arc::clone(&state) as Arc<dyn std::any::Any + Send + Sync>,
        vars,
        handlers,
        types,
        emissions,
        emit_queue,
    })
}

// ------------------------------------------------------------------
// Shared state and helpers for closure captures
// ------------------------------------------------------------------

/// Shared wasmi state across all function/handler closures.
struct WasmStateShared {
    _engine: Engine,  // kept alive so the module remains valid
    store: Store<()>,
    instance: Instance,
}

/// Write a Rust Value into wasmi linear memory at `off`.
/// Fetches the memory handle from the instance as needed.
fn write_value_to_mem(
    store: &mut Store<()>,
    instance: &Instance,
    off: u32,
    val: &Value,
) -> Result<(), String> {
    /// Write raw bytes at `off` into wasm memory.
    fn write_raw(store: &mut Store<()>, instance: &Instance, off: u32, buf: &[u8]) -> Result<(), String> {
        let mem = instance
            .get_memory(&*store, "memory")
            .ok_or_else(|| "WASM module has no memory".to_string())?;
        mem.data_mut(store)[off as usize..off as usize + buf.len()].copy_from_slice(buf);
        Ok(())
    }

    match val {
        Value::Number(n) => {
            let mut buf = [0u8; CODE_VAL_SIZE as usize];
            buf[CODE_VAL_TAG as usize] = TAG_NUMBER;
            buf[CODE_VAL_NUM as usize..CODE_VAL_NUM as usize + 8]
                .copy_from_slice(&n.to_le_bytes());
            write_raw(store, instance, off, &buf)?;
        }
        Value::String(s) => {
            let str_off = alloc_in_wasm(store, instance, (s.len() + 1) as u32)?;
            let bytes: Vec<u8> = s.bytes().chain(std::iter::once(0)).collect();
            write_raw(store, instance, str_off, &bytes)?;
            let mut buf = [0u8; CODE_VAL_SIZE as usize];
            buf[CODE_VAL_TAG as usize] = TAG_STRING;
            buf[CODE_VAL_PTR as usize..CODE_VAL_PTR as usize + 4]
                .copy_from_slice(&str_off.to_le_bytes());
            write_raw(store, instance, off, &buf)?;
        }
        Value::Boolean(b) => {
            let mut buf = [0u8; CODE_VAL_SIZE as usize];
            buf[CODE_VAL_TAG as usize] = TAG_BOOLEAN;
            buf[CODE_VAL_BOOL as usize] = if *b { 1 } else { 0 };
            write_raw(store, instance, off, &buf)?;
        }
        Value::Object(map) => {
            let count = map.len() as u32;
            let fields_off = if count > 0 {
                alloc_in_wasm(store, instance, count * CODE_FIELD_SIZE)?
            } else {
                0
            };
            for (i, (name, v)) in map.iter().enumerate() {
                let field_off = fields_off + i as u32 * CODE_FIELD_SIZE;
                let name_off = alloc_in_wasm(store, instance, (name.len() + 1) as u32)?;
                let name_bytes: Vec<u8> = name.bytes().chain(std::iter::once(0)).collect();
                write_raw(store, instance, name_off, &name_bytes)?;
                write_raw(store, instance, field_off + CODE_FIELD_NAME, &name_off.to_le_bytes())?;
                write_value_to_mem(store, instance, field_off + CODE_FIELD_VALUE, v)?;
            }
            let mut buf = [0u8; CODE_VAL_SIZE as usize];
            buf[CODE_VAL_TAG as usize] = TAG_OBJECT;
            buf[CODE_VAL_PTR as usize..CODE_VAL_PTR as usize + 4]
                .copy_from_slice(&fields_off.to_le_bytes());
            buf[CODE_VAL_COUNT as usize..CODE_VAL_COUNT as usize + 4]
                .copy_from_slice(&count.to_le_bytes());
            write_raw(store, instance, off, &buf)?;
        }
        Value::Array(elems) => {
            let count = elems.len() as u32;
            let arr_off = if count > 0 {
                alloc_in_wasm(store, instance, count * CODE_VAL_SIZE)?
            } else {
                0
            };
            for (i, v) in elems.iter().enumerate() {
                let elem_off = arr_off + i as u32 * CODE_VAL_SIZE;
                write_value_to_mem(store, instance, elem_off, v)?;
            }
            let mut buf = [0u8; CODE_VAL_SIZE as usize];
            buf[CODE_VAL_TAG as usize] = TAG_ARRAY;
            buf[CODE_VAL_PTR as usize..CODE_VAL_PTR as usize + 4]
                .copy_from_slice(&arr_off.to_le_bytes());
            buf[CODE_VAL_COUNT as usize..CODE_VAL_COUNT as usize + 4]
                .copy_from_slice(&count.to_le_bytes());
            write_raw(store, instance, off, &buf)?;
        }
        Value::Null => {
            let mut buf = [0u8; CODE_VAL_SIZE as usize];
            buf[CODE_VAL_TAG as usize] = TAG_NULL;
            write_raw(store, instance, off, &buf)?;
        }
    }
    Ok(())
}

fn alloc_in_wasm(store: &mut Store<()>, instance: &Instance, size: u32) -> Result<u32, String> {
    let alloc_fn = instance
        .get_func(&*store, "code_alloc")
        .ok_or_else(|| "WASM module missing 'code_alloc'".to_string())?;
    let mut result = [Val::I32(0)];
    alloc_fn
        .call(store, &[Val::I32(size as i32)], &mut result)
        .map_err(|e| format!("code_alloc failed: {}", e))?;
    Ok(result[0].i32().unwrap_or(0) as u32)
}
