use std::collections::HashMap;
use std::path::Path;

use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::targets::{
    CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine, TargetTriple,
};
use inkwell::types::StructType;
use inkwell::values::{BasicValueEnum, FunctionValue, GlobalValue, PointerValue, StructValue};
use inkwell::AddressSpace;
use inkwell::FloatPredicate;
use inkwell::IntPredicate;
use inkwell::OptimizationLevel;

use crate::ast::{BinaryOp, ConstraintExpr, Expression, FieldConstraint, HandlerTarget, ObjectField, Program, Spanned, Statement, TypeExpr, TypeInfo, UnaryOp};

const TAG_NUMBER: u8 = 0;
const TAG_STRING: u8 = 1;
const TAG_BOOLEAN: u8 = 2;
const TAG_OBJECT: u8 = 3;
const TAG_NULL: u8 = 4;
const TAG_ARRAY: u8 = 5;

/// Build target kind — passed into codegen for target-aware decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildTarget {
    Ir,
    Exe,
    Shared,
    Static,
    Wasm,
}

pub fn emit_llvm_ir(program: &Program, module_name: &str) -> Result<String, String> {
    emit_llvm_ir_with_target(program, module_name, BuildTarget::Ir)
}

pub fn emit_llvm_ir_with_target(program: &Program, module_name: &str, target: BuildTarget) -> Result<String, String> {
    let context = Context::create();
    let mut codegen = Codegen::new(&context, module_name, target);
    codegen.compile_program(program)?;
    Ok(codegen.module.print_to_string().to_string())
}

/// Emit a native object file (.o) for the host target.
/// Returns `Ok(has_native_imports)` — true if the program links native .so modules.
/// When `release` is true, LLVM uses -O2; otherwise -O0 for fast dev builds.
pub fn emit_object_file(
    program: &Program,
    module_name: &str,
    output_path: &Path,
    target: BuildTarget,
    release: bool,
) -> Result<bool, String> {
    let context = Context::create();
    let mut codegen = Codegen::new(&context, module_name, target);
    codegen.compile_program(program)?;
    let has_native = codegen.has_native_imports;
    let machine = host_target_machine(release)?;
    machine
        .write_to_file(&codegen.module, FileType::Object, output_path)
        .map_err(|e| format!("Failed to write object file: {}", e))?;
    Ok(has_native)
}

/// Emit a WASM object file (.o) targeting wasm32-unknown-unknown.
pub fn emit_wasm_object(
    program: &Program,
    module_name: &str,
    output_path: &Path,
    release: bool,
) -> Result<(), String> {
    let context = Context::create();
    let mut codegen = Codegen::new(&context, module_name, BuildTarget::Wasm);
    codegen.compile_program(program)?;

    Target::initialize_webassembly(&InitializationConfig::default());
    let triple = TargetTriple::create("wasm32-unknown-unknown");
    codegen.module.set_triple(&triple);

    let opt = if release {
        OptimizationLevel::Default
    } else {
        OptimizationLevel::None
    };

    let target = Target::from_triple(&triple)
        .map_err(|e| format!("WASM target error: {}", e))?;
    let machine = target
        .create_target_machine(
            &triple,
            "generic",
            "",
            opt,
            RelocMode::PIC,
            CodeModel::Default,
        )
        .ok_or_else(|| "Failed to create WASM target machine".to_string())?;

    machine
        .write_to_file(&codegen.module, FileType::Object, output_path)
        .map_err(|e| format!("Failed to write WASM object file: {}", e))
}

fn host_target_machine(release: bool) -> Result<TargetMachine, String> {
    Target::initialize_native(&InitializationConfig::default())
        .map_err(|e| format!("Failed to initialize native target: {}", e))?;

    let triple = TargetMachine::get_default_triple();
    let target = Target::from_triple(&triple)
        .map_err(|e| format!("Target error: {}", e))?;

    let opt = if release {
        OptimizationLevel::Default
    } else {
        OptimizationLevel::None
    };

    target
        .create_target_machine(
            &triple,
            "generic",
            "",
            opt,
            RelocMode::PIC,
            CodeModel::Default,
        )
        .ok_or_else(|| "Failed to create target machine".to_string())
}

struct Codegen<'ctx> {
    context: &'ctx Context,
    module: Module<'ctx>,
    builder: Builder<'ctx>,
    value_type: StructType<'ctx>,
    main_fn: FunctionValue<'ctx>,
    scopes: Vec<HashMap<String, PointerValue<'ctx>>>,
    strcmp_fn: FunctionValue<'ctx>,
    abort_fn: FunctionValue<'ctx>,
    malloc_fn: FunctionValue<'ctx>,
    field_type: StructType<'ctx>,
    values_equal_fn: FunctionValue<'ctx>,
    string_count: u64,
    /// Type annotations: scope-level -> name -> TypeExpr.
    type_annotations: Vec<HashMap<String, TypeExpr>>,
    /// Type definitions (particle schemas): scope-level -> name -> fields (name, TypeExpr, optional).
    type_registry: Vec<HashMap<String, Vec<(String, TypeExpr, bool)>>>,
    /// Handler definitions: scope-level -> class_name -> handler body.
    handler_registry: Vec<HashMap<String, Vec<Spanned<Statement>>>>,
    /// Handler return alloca (set when compiling a handler body).
    handler_return_alloca: Option<PointerValue<'ctx>>,
    /// Handler exit block (set when compiling a handler body).
    handler_exit_block: Option<inkwell::basic_block::BasicBlock<'ctx>>,
    /// Break exit block (set when compiling a loop body).
    break_exit_block: Option<inkwell::basic_block::BasicBlock<'ctx>>,
    strlen_fn: FunctionValue<'ctx>,
    memcpy_fn: FunctionValue<'ctx>,
    /// C time(NULL) — returns current Unix timestamp.
    time_fn: FunctionValue<'ctx>,
    /// Runtime helper: converts any value (tag, num, ptr) to a C string pointer.
    value_to_cstr_fn: FunctionValue<'ctx>,
    /// Tracks nesting depth inside handler bodies (0 = not in handler).
    in_handler_depth: usize,
    /// Type alias definitions: scope-level -> alias name -> TypeExpr.
    type_alias_registry: Vec<HashMap<String, TypeExpr>>,
    /// Build target kind (ir/exe/shared/static/wasm).
    target: BuildTarget,
    /// Whether the program contains any NativeImport statements.
    has_native_imports: bool,
    /// Native handler function pointers: scope-level -> class_name -> PointerValue (i8*).
    native_handler_ptrs: Vec<HashMap<String, PointerValue<'ctx>>>,
    /// Bridge function: __native_bridge_open(path) -> desc*
    native_bridge_open_fn: Option<FunctionValue<'ctx>>,
    /// Bridge function: __native_bridge_get_var(desc, idx, out)
    native_bridge_get_var_fn: Option<FunctionValue<'ctx>>,
    /// Bridge function: __native_bridge_handler_ptr(desc, idx) -> handler_ptr
    native_bridge_handler_ptr_fn: Option<FunctionValue<'ctx>>,
    /// Bridge function: __native_bridge_call_handler(handler_ptr, particle, out)
    native_bridge_call_handler_fn: Option<FunctionValue<'ctx>>,
    /// Bridge function: __native_bridge_poll_emission(out_cval*, out_class_str**) -> i32
    native_bridge_poll_emission_fn: Option<FunctionValue<'ctx>>,
    /// Bridge function: __native_bridge_is_keep_alive() -> i32
    native_bridge_is_keep_alive_fn: Option<FunctionValue<'ctx>>,
    /// Whether any NativeImport in the program declares emissions.
    has_native_emissions: bool,
    /// Class names emitted by native modules (for drain loop dispatch).
    emission_handler_classes: Vec<String>,
    /// LLVM global variables backing each native handler pointer.
    /// Key matches the key used in native_handler_ptrs (alias-prefixed or plain).
    /// These are module-level globals so they can be loaded from any function,
    /// including __code_dispatch which runs after main() has returned.
    native_handler_globals: HashMap<String, GlobalValue<'ctx>>,
    /// Yield collector: pointer to array pointer (for loop with `get`).
    yield_arr_ptr: Option<PointerValue<'ctx>>,
    /// Yield collector: pointer to current count (for loop with `get`).
    yield_count_ptr: Option<PointerValue<'ctx>>,
}

impl<'ctx> Codegen<'ctx> {
    fn new(context: &'ctx Context, module_name: &str, target: BuildTarget) -> Self {
        let module = context.create_module(module_name);
        let builder = context.create_builder();

        let i8_type = context.i8_type();
        let f64_type = context.f64_type();
        let i1_type = context.bool_type();
        let i8_ptr_type = i8_type.ptr_type(AddressSpace::default());

        let value_type = context.struct_type(
            &[i8_type.into(), f64_type.into(), i8_ptr_type.into(), i1_type.into()],
            false,
        );

        let i32_type = context.i32_type();
        let main_fn_type = i32_type.fn_type(&[], false);
        let main_fn = module.add_function("main", main_fn_type, None);
        let entry = context.append_basic_block(main_fn, "entry");
        builder.position_at_end(entry);

        let strcmp_type = i32_type.fn_type(&[i8_ptr_type.into(), i8_ptr_type.into()], false);
        let strcmp_fn = module.add_function("strcmp", strcmp_type, None);

        let abort_type = context.void_type().fn_type(&[], false);
        let abort_fn = module.add_function("abort", abort_type, None);

        let i64_type = context.i64_type();
        let malloc_type = i8_ptr_type.fn_type(&[i64_type.into()], false);
        let malloc_fn = module.add_function("malloc", malloc_type, None);

        let strlen_type = i64_type.fn_type(&[i8_ptr_type.into()], false);
        let strlen_fn = module.add_function("strlen", strlen_type, None);

        let time_type = i64_type.fn_type(&[i8_ptr_type.into()], false);
        let time_fn = module.add_function("time", time_type, None);

        let memcpy_type = i8_ptr_type.fn_type(
            &[i8_ptr_type.into(), i8_ptr_type.into(), i64_type.into()],
            false,
        );
        let memcpy_fn = module.add_function("memcpy", memcpy_type, None);

        // __value_to_cstr(tag: i32, num: f64, ptr: i8*) -> i8*
        // Converts any Code value to its C-string representation.
        let value_to_cstr_type = i8_ptr_type.fn_type(
            &[i32_type.into(), f64_type.into(), i8_ptr_type.into()],
            false,
        );
        let value_to_cstr_fn = module.add_function("__value_to_cstr", value_to_cstr_type, None);

        let field_type = context.struct_type(
            &[i8_ptr_type.into(), value_type.into()],
            false,
        );

        let val_ptr_type = value_type.ptr_type(AddressSpace::default());

        let values_equal_type = context.bool_type().fn_type(
            &[val_ptr_type.into(), val_ptr_type.into()],
            false,
        );
        let values_equal_fn = module.add_function(
            "__code_values_equal",
            values_equal_type,
            None,
        );

        // Pre-register built-in Exception type.
        let mut initial_types = HashMap::new();
        initial_types.insert("Exception".to_string(), vec![
            ("message".to_string(), TypeExpr::Named("String".to_string()), false),
            ("innerException".to_string(), TypeExpr::Named("Exception".to_string()), true),
        ]);

        Self {
            context,
            module,
            builder,
            value_type,
            main_fn,
            scopes: vec![HashMap::new()],
            strcmp_fn,
            abort_fn,
            malloc_fn,
            field_type,
            values_equal_fn,
            string_count: 0,
            type_annotations: vec![HashMap::new()],
            type_registry: vec![initial_types],
            handler_registry: vec![HashMap::new()],
            handler_return_alloca: None,
            handler_exit_block: None,
            break_exit_block: None,
            strlen_fn,
            memcpy_fn,
            time_fn,
            value_to_cstr_fn,
            in_handler_depth: 0,
            type_alias_registry: vec![HashMap::new()],
            target,
            has_native_imports: false,
            native_handler_ptrs: vec![HashMap::new()],
            native_bridge_open_fn: None,
            native_bridge_get_var_fn: None,
            native_bridge_handler_ptr_fn: None,
            native_bridge_call_handler_fn: None,
            native_bridge_poll_emission_fn: None,
            native_bridge_is_keep_alive_fn: None,
            has_native_emissions: false,
            emission_handler_classes: Vec::new(),
            native_handler_globals: HashMap::new(),
            yield_arr_ptr: None,
            yield_count_ptr: None,
        }
    }

    fn compile_program(&mut self, program: &Program) -> Result<(), String> {
        self.build_values_equal_fn();
        for stmt in &program.statements {
            self.compile_statement(&stmt.node)?;
        }

        // If any native module emitted __KeepAlive, run the end-of-program
        // drain loop so emissions from background threads are processed.
        if self.has_native_emissions {
            self.compile_native_drain_loop()?;
        }

        let i32_type = self.context.i32_type();
        self.builder.build_return(Some(&i32_type.const_int(0, false))).unwrap();

        // For WASM targets: export a re-entry dispatch function so that JS can
        // invoke compiled gene handlers after main() has returned.
        // Always generate for WASM since the JS runtime dispatches emissions
        // (e.g. user actions, timer events) to gene handlers via this function.
        // For native targets with emissions, the drain loop handles dispatch.
        if matches!(self.target, BuildTarget::Wasm) {
            self.compile_code_dispatch_fn()?;
        }

        Ok(())
    }

    /// Compile an exported WASM re-entry function `__code_dispatch`.
    ///
    /// This function is callable from JavaScript after `main()` has returned.
    /// It accepts a class-name C-string pointer and a `value_type` pointer
    /// (pointing to a particle in WASM linear memory) and dispatches to the
    /// appropriate Code gene handler.
    ///
    /// Signature: `fn __code_dispatch(class_ptr: i8*, particle_ptr: value_type*)`
    fn compile_code_dispatch_fn(&mut self) -> Result<(), String> {
        let i32_type  = self.context.i32_type();
        let i8_ptr    = self.context.i8_type().ptr_type(AddressSpace::default());
        let val_ptr   = self.value_type.ptr_type(AddressSpace::default());

        let fn_type   = self.context.void_type().fn_type(
            &[i8_ptr.into(), val_ptr.into()],
            false,
        );
        let dispatch_fn = self.module.add_function(
            "__code_dispatch",
            fn_type,
            Some(inkwell::module::Linkage::External),
        );

        let entry_block = self.context.append_basic_block(dispatch_fn, "entry");
        self.builder.position_at_end(entry_block);

        // Swap main_fn so that create_entry_alloca and append_basic_block
        // target this new function rather than the real `main`.
        let saved_main_fn = self.main_fn;
        self.main_fn = dispatch_fn;

        // Reload native handler pointers from their LLVM global backing stores.
        // The values stored in native_handler_ptrs are instruction results scoped
        // to main(), so they cannot be referenced here.  Loading from globals
        // produces fresh i8* values valid in this function's context.
        let saved_handler_ptrs = self.native_handler_ptrs.clone();
        let mut dispatch_scope: HashMap<String, PointerValue<'ctx>> = HashMap::new();
        let globals_snapshot: Vec<(String, inkwell::values::GlobalValue<'ctx>)> =
            self.native_handler_globals.iter().map(|(k, g)| (k.clone(), *g)).collect();
        for (key, global) in &globals_snapshot {
            let safe_key = key.replace('.', "_");
            let loaded = self.builder.build_load(
                i8_ptr, global.as_pointer_value(), &format!("disp_nhp_{}", safe_key),
            ).unwrap().into_pointer_value();
            dispatch_scope.insert(key.clone(), loaded);
        }
        self.native_handler_ptrs = vec![dispatch_scope];

        let class_ptr_param    = dispatch_fn.get_nth_param(0).unwrap().into_pointer_value();
        let particle_ptr_param = dispatch_fn.get_nth_param(1).unwrap().into_pointer_value();

        // Load the particle value from the pointer.
        let particle_val = self.builder.build_load(self.value_type, particle_ptr_param, "disp_pv")
            .unwrap().into_struct_value();

        let emission_classes = self.emission_handler_classes.clone();
        let mut all_dispatch_classes = emission_classes.clone();
        let gene_handler_classes: Vec<String> = self
            .handler_registry
            .iter()
            .flat_map(|scope| scope.keys().cloned())
            .filter(|name| !name.contains('.') && !all_dispatch_classes.contains(name))
            .collect();
        all_dispatch_classes.extend(gene_handler_classes);

        let ret_block = self.context.append_basic_block(dispatch_fn, "disp_ret");

        for (i, class_name) in all_dispatch_classes.iter().enumerate() {
            let class_global = self.builder.build_global_string_ptr(
                class_name, &format!("disp_cls_{}", i),
            ).unwrap();

            let cmp = self.builder.build_call(
                self.strcmp_fn,
                &[class_ptr_param.into(), class_global.as_pointer_value().into()],
                &format!("disp_cmp_{}", i),
            ).unwrap().try_as_basic_value().left().unwrap().into_int_value();

            let is_match = self.builder.build_int_compare(
                IntPredicate::EQ, cmp, i32_type.const_int(0, false),
                &format!("disp_match_{}", i),
            ).unwrap();

            let match_block    = self.context.append_basic_block(dispatch_fn, &format!("disp_h_{}", class_name));
            let no_match_block = self.context.append_basic_block(dispatch_fn, &format!("disp_nm_{}", i));

            self.builder.build_conditional_branch(is_match, match_block, no_match_block).unwrap();

            self.builder.position_at_end(match_block);
            self.compile_handler_invoke_with_val(particle_val, class_name, &HandlerTarget::This)?;
            if self.builder.get_insert_block().unwrap().get_terminator().is_none() {
                self.builder.build_unconditional_branch(ret_block).unwrap();
            }

            self.builder.position_at_end(no_match_block);
        }

        // Fell through all comparisons — unknown class, just return.
        if self.builder.get_insert_block().unwrap().get_terminator().is_none() {
            self.builder.build_unconditional_branch(ret_block).unwrap();
        }

        self.builder.position_at_end(ret_block);
        self.builder.build_return(None).unwrap();

        // Restore main_fn and native handler pointer scope.
        self.main_fn = saved_main_fn;
        self.native_handler_ptrs = saved_handler_ptrs;

        Ok(())
    }

    fn compile_statement(&mut self, stmt: &Statement) -> Result<(), String> {
        match stmt {
            Statement::Link { module_ref, .. } => Err(format!(
                "Unresolved module link '{}': links must be resolved before codegen",
                module_ref
            )),
            Statement::Constraint { variable, constraint, private: _ } => {
                match constraint {
                    ConstraintExpr::Equals(value) => {
                        // Exact constraint → compile like old assignment
                        if self.exists_in_any_scope(variable) {
                            if let Some(ann) = self.get_type_annotation(variable) {
                                let ann = ann.clone();
                                match self.infer_expr_type(value) {
                                    Ok(actual) if !self.inferred_matches_type_expr(&actual, &ann) => {
                                        return Err(format!(
                                            "Type mismatch for '{}': expected {}, got {}",
                                            variable, ann, actual
                                        ));
                                    }
                                    _ => {}
                                }
                            }
                        }
                        self.compile_assignment(variable, value)?;
                        if self.exists_in_any_scope(variable) {
                            if let Some(ann) = self.get_type_annotation(variable) {
                                let ann = ann.clone();
                                if self.infer_expr_type(value).is_err() {
                                    self.emit_runtime_type_check_expr(variable, &ann)?;
                                }
                            }
                        }
                        Ok(())
                    }
                    ConstraintExpr::IsType(type_expr) => {
                        // Type constraint → store as annotation only (no value generated)
                        self.set_type_annotation(variable.clone(), type_expr.clone());
                        Ok(())
                    }
                    _ => {
                        // Other constraint forms (LessThan, GreaterThan, etc.) are not
                        // directly representable in compiled output yet; silently accept.
                        Ok(())
                    }
                }
            }
            Statement::TypeDeclaration { name, fields } => {
                self.define_type(name.clone(), fields.clone());
                Ok(())
            }
            Statement::HandlerDefinition { class_name, inline_type, body } => {
                if let Some(fields) = inline_type {
                    self.define_type(class_name.clone(), fields.clone());
                }
                self.define_handler(class_name.clone(), body.clone());
                Ok(())
            }
            Statement::HandlerInvoke { particle, target } => {
                // Fire-and-forget: discard return value.
                self.compile_handler_invoke(particle, target)?;
                Ok(())
            }
            Statement::HandlerInvokeAssign { particle, target, result_name } => {
                let ret_val = self.compile_handler_invoke(particle, target)?;
                // Store the return value in the result variable.
                if let Some(existing_ptr) = self.get_var_ptr(result_name) {
                    self.builder.build_store(existing_ptr, ret_val).unwrap();
                } else {
                    let ptr = self.create_entry_alloca(result_name);
                    self.builder.build_store(ptr, ret_val).unwrap();
                    self.scopes
                        .last_mut()
                        .expect("No active scope")
                        .insert(result_name.clone(), ptr);
                }
                Ok(())
            }
            Statement::HandlerReturn { value } => {
                let (ret_alloca, exit_block) = match (&self.handler_return_alloca, &self.handler_exit_block) {
                    (Some(a), Some(b)) => (*a, *b),
                    _ => return Err("Return statement 'return' used outside of a handler".to_string()),
                };
                // Enforce: handlers must return Particles.
                if self.in_handler_depth > 0 {
                    if let Ok(ty) = self.infer_expr_type(value) {
                        match ty.as_str() {
                            "Object" => {} // Particles are Objects at compile time
                            other => return Err(format!(
                                "Handler return must be a Particle, got {}", other
                            )),
                        }
                    }
                    // For dynamic expressions we can't infer, add a runtime tag check.
                }
                let val = self.compile_expr(value)?;
                // Runtime check: if in handler, verify the value is an object (TAG_OBJECT).
                if self.in_handler_depth > 0 {
                    let struct_val = val.into_struct_value();
                    let tag = self.builder.build_extract_value(struct_val, 0, "hret_tag")
                        .unwrap().into_int_value();
                    let expected = self.context.i8_type().const_int(TAG_OBJECT as u64, false);
                    let is_obj = self.builder.build_int_compare(
                        inkwell::IntPredicate::EQ, tag, expected, "hret_is_obj",
                    ).unwrap();
                    let ok_bb = self.context.append_basic_block(self.main_fn, "hret_ok");
                    let fail_bb = self.context.append_basic_block(self.main_fn, "hret_fail");
                    self.builder.build_conditional_branch(is_obj, ok_bb, fail_bb).unwrap();
                    self.builder.position_at_end(fail_bb);
                    self.emit_trap();
                    self.builder.position_at_end(ok_bb);
                }
                self.builder.build_store(ret_alloca, val).unwrap();
                self.builder.build_unconditional_branch(exit_block).unwrap();
                // Create unreachable block for any subsequent statements.
                let after = self.context.append_basic_block(self.main_fn, "after_return");
                self.builder.position_at_end(after);
                Ok(())
            }
            Statement::Import { alias, body, public_names, public_types, public_handlers } => {
                self.compile_import(alias.as_deref(), body, public_names, public_types, public_handlers)
            }
            Statement::NativeImport { alias, native_path, is_wasm, vars, handlers, types, emissions, emit_queue: _ } => {
                if self.target == BuildTarget::Wasm && !is_wasm {
                    return Err("Native module linking (.so) is not supported for the wasm target. Use a .wasm module instead.".to_string());
                }
                if self.target == BuildTarget::Wasm && *is_wasm {
                    // For WASM target: the module descriptor was already loaded by the host
                    // at program load time (same as the interpreter path).  At codegen time
                    // we simply install the values into scope using the same mechanism as
                    // a regular NativeImport — the bridge functions route through wasmi.
                    // The WASM-compiled program itself cannot do dlopen, but the host
                    // (wasmi / wasmtime) that runs the compiled wasm is responsible for
                    // providing the linked module's symbols through host imports.
                    // We fall through to compile_native_import which uses the values
                    // already embedded in the AST (loaded at parse time by wasmi).
                }
                self.compile_native_import(alias.as_deref(), native_path, vars, handlers, types, emissions)
            }
            Statement::Assert(expr) => self.compile_assert(expr),
            Statement::Block(stmts) => {
                self.push_scope();
                for inner in stmts {
                    let name_to_check = match &inner.node {
                        Statement::Constraint { variable, constraint: ConstraintExpr::Equals(_), .. } => Some(variable.clone()),
                        _ => None,
                    };
                    if let Some(name) = name_to_check {
                        // Inside handler bodies, allow updating outer-scope handler
                        // variables (imperative semantics). At global scope, prevent
                        // shadowing completely.
                        if self.in_handler_depth == 0
                            && self.exists_in_any_scope(&name)
                            && !self.current_scope_has(&name)
                        {
                            self.pop_scope();
                            return Err(format!(
                                "Cannot redefine '{}' inside block: shadowing is not allowed",
                                name
                            ));
                        }
                    }
                    self.compile_statement(&inner.node)?;
                }
                self.pop_scope();
                Ok(())
            }
            Statement::If { condition, body } => {
                self.compile_if(condition, body)
            }
            Statement::LoopOver { variable, index, iterable, result, body } => {
                self.compile_loop_over(variable, index.as_deref(), iterable, result.as_deref(), body)
            }
            Statement::LoopInfinite { result, body } => {
                self.compile_loop_infinite(result.as_deref(), body)
            }
            Statement::Break => {
                let exit_block = self.break_exit_block
                    .ok_or_else(|| "Break statement used outside of a loop".to_string())?;
                self.builder.build_unconditional_branch(exit_block).unwrap();
                // Create unreachable block for subsequent statements.
                let after = self.context.append_basic_block(self.main_fn, "after_break");
                self.builder.position_at_end(after);
                Ok(())
            }
            Statement::Yield(expr) => {
                // Yield stores a value into the collector array set up by a loop with `get`.
                let val = self.compile_expr(&expr)?;
                if let (Some(arr_ptr), Some(count_ptr)) = (self.yield_arr_ptr, self.yield_count_ptr) {
                    let i32_type = self.context.i32_type();
                    let _i64_type = self.context.i64_type();
                    let cur_count = self.builder.build_load(i32_type, count_ptr, "yld_cur")
                        .unwrap().into_int_value();
                    let cur_arr = self.builder.build_load(
                        self.context.i8_type().ptr_type(AddressSpace::default()),
                        arr_ptr, "yld_arr",
                    ).unwrap().into_pointer_value();

                    // Store element at current index.
                    let elem_ptr = unsafe { self.builder.build_in_bounds_gep(
                        self.value_type, cur_arr, &[cur_count], "yld_elem_ptr",
                    ) }.unwrap();
                    self.builder.build_store(elem_ptr, val).unwrap();

                    // Increment count.
                    let new_count = self.builder.build_int_add(
                        cur_count, i32_type.const_int(1, false), "yld_new_count",
                    ).unwrap();
                    self.builder.build_store(count_ptr, new_count).unwrap();

                    Ok(())
                } else {
                    Err("Yield requires a loop with 'get'".to_string())
                }
            }
        }
    }

    /// Compile a module import.
    fn compile_import(
        &mut self,
        alias: Option<&str>,
        body: &[Spanned<Statement>],
        public_names: &[String],
        public_types: &[TypeInfo],
        public_handlers: &[crate::ast::HandlerInfo],
    ) -> Result<(), String> {
        self.push_scope();

        for stmt in body {
            self.compile_statement(&stmt.node)?;
        }

        let mut pub_ptrs: Vec<(String, PointerValue<'ctx>)> = Vec::new();
        for name in public_names {
            let ptr = self.get_var_ptr(name).ok_or_else(|| {
                format!("Module declares public name '{}' but it was never defined", name)
            })?;
            pub_ptrs.push((name.clone(), ptr));
        }

        self.pop_scope();

        match alias {
            Some(alias_name) => {
                let count = pub_ptrs.len();
                let i64_type = self.context.i64_type();
                let i32_type = self.context.i32_type();

                let field_size = self.field_type.size_of().unwrap();
                let count_val = i64_type.const_int(count as u64, false);
                let total_size = self.builder.build_int_mul(
                    field_size, count_val, "mod_obj_size",
                ).unwrap();
                let raw_ptr = self.builder.build_call(
                    self.malloc_fn, &[total_size.into()], "mod_obj_mem",
                ).unwrap()
                    .try_as_basic_value().left()
                    .ok_or_else(|| "malloc returned no value".to_string())?
                    .into_pointer_value();

                for (i, (name, var_ptr)) in pub_ptrs.iter().enumerate() {
                    let val = self.builder.build_load(self.value_type, *var_ptr, "mod_load")
                        .unwrap();
                    let idx = i32_type.const_int(i as u64, false);
                    let field_ptr = unsafe { self.builder.build_in_bounds_gep(
                        self.field_type, raw_ptr, &[idx], &format!("mod_field_{}", i),
                    ) }.unwrap();

                    let name_global = self.builder.build_global_string_ptr(
                        name, &format!("mod_fname_{}", self.string_count),
                    ).unwrap();
                    self.string_count += 1;
                    let name_slot = self.builder.build_struct_gep(
                        self.field_type, field_ptr, 0, "mod_name_slot",
                    ).unwrap();
                    self.builder.build_store(name_slot, name_global.as_pointer_value()).unwrap();

                    let val_slot = self.builder.build_struct_gep(
                        self.field_type, field_ptr, 1, "mod_val_slot",
                    ).unwrap();
                    self.builder.build_store(val_slot, val).unwrap();
                }

                let tag = self.context.i8_type().const_int(TAG_OBJECT as u64, false);
                let num = self.context.f64_type().const_float(count as f64);
                let bool_val = self.context.bool_type().const_int(0, false);
                let obj_val = self.build_value(tag, num, raw_ptr, bool_val);

                let ptr = self.create_entry_alloca(alias_name);
                self.builder.build_store(ptr, obj_val).unwrap();
                self.scopes
                    .last_mut()
                    .expect("No active scope")
                    .insert(alias_name.to_string(), ptr);

                for t in public_types {
                    let key = format!("{}.{}", alias_name, t.name);
                    self.define_type(key, t.fields.clone());
                }
                for h in public_handlers {
                    let key = format!("{}.{}", alias_name, h.class_name);
                    self.define_handler(key, h.body.clone());
                }
            }
            None => {
                for (name, var_ptr) in &pub_ptrs {
                    if self.exists_in_any_scope(name) {
                        return Err(format!(
                            "Name conflict: linked module defines '{}' which already exists in the current scope",
                            name
                        ));
                    }
                    let val = self.builder.build_load(self.value_type, *var_ptr, "flat_load")
                        .unwrap();
                    let new_ptr = self.create_entry_alloca(name);
                    self.builder.build_store(new_ptr, val).unwrap();
                    self.scopes
                        .last_mut()
                        .expect("No active scope")
                        .insert(name.clone(), new_ptr);
                }

                for t in public_types {
                    self.define_type(t.name.clone(), t.fields.clone());
                }
                for h in public_handlers {
                    self.define_handler(h.class_name.clone(), h.body.clone());
                }
            }
        }

        Ok(())
    }

    /// Ensure the C bridge functions for native module loading are declared.
    fn ensure_native_bridge_fns(&mut self) {
        if self.native_bridge_open_fn.is_some() {
            return; // Already declared.
        }
        let i8_ptr = self.context.i8_type().ptr_type(AddressSpace::default());
        let i32_type = self.context.i32_type();
        let void_type = self.context.void_type();

        // void* __native_bridge_open(const char* path)
        let open_ty = i8_ptr.fn_type(&[i8_ptr.into()], false);
        self.native_bridge_open_fn = Some(self.module.add_function("__native_bridge_open", open_ty, None));

        // void __native_bridge_get_var(void* desc, uint32_t idx, void* out)
        let get_var_ty = void_type.fn_type(&[i8_ptr.into(), i32_type.into(), i8_ptr.into()], false);
        self.native_bridge_get_var_fn = Some(self.module.add_function("__native_bridge_get_var", get_var_ty, None));

        // void* __native_bridge_handler_ptr(void* desc, uint32_t idx)
        let handler_ptr_ty = i8_ptr.fn_type(&[i8_ptr.into(), i32_type.into()], false);
        self.native_bridge_handler_ptr_fn = Some(self.module.add_function("__native_bridge_handler_ptr", handler_ptr_ty, None));

        // void __native_bridge_call_handler(void* handler_ptr, void* particle, void* out)
        let call_handler_ty = void_type.fn_type(&[i8_ptr.into(), i8_ptr.into(), i8_ptr.into()], false);
        self.native_bridge_call_handler_fn = Some(self.module.add_function("__native_bridge_call_handler", call_handler_ty, None));

        // int __native_bridge_poll_emission(void* out_cval, void** out_class_str) -> i32
        let poll_ty = i32_type.fn_type(&[i8_ptr.into(), i8_ptr.into()], false);
        self.native_bridge_poll_emission_fn = Some(self.module.add_function("__native_bridge_poll_emission", poll_ty, None));

        // int __native_bridge_is_keep_alive() -> i32
        let ka_ty = i32_type.fn_type(&[], false);
        self.native_bridge_is_keep_alive_fn = Some(self.module.add_function("__native_bridge_is_keep_alive", ka_ty, None));
    }

    /// Compile a native module import (.so).
    /// At compile time: registers types/handlers metadata.
    /// At runtime: calls dlopen via C bridge, extracts vars/handler-ptrs.
    fn compile_native_import(
        &mut self,
        alias: Option<&str>,
        native_path: &str,
        vars: &[(String, std::rc::Rc<crate::runtime::Value>)],
        handlers: &[crate::native_module::NativeHandlerInfo],
        types: &[TypeInfo],
        emissions: &[crate::ast::EmissionDecl],
    ) -> Result<(), String> {
        // Record emission class names for the inline drain step.
        if !emissions.is_empty() {
            self.has_native_emissions = true;
            for em in emissions {
                if !self.emission_handler_classes.contains(&em.class_name) {
                    self.emission_handler_classes.push(em.class_name.clone());
                }
            }
        }
        self.has_native_imports = true;
        self.ensure_native_bridge_fns();

        let i8_ptr = self.context.i8_type().ptr_type(AddressSpace::default());
        let i32_type = self.context.i32_type();

        // For WASM targets, strip the absolute filesystem path and keep only
        // the filename.  The browser runtime maps it to "organelles/<name>".
        let embed_path = if self.target == BuildTarget::Wasm {
            std::path::Path::new(native_path)
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_else(|| native_path.to_string())
        } else {
            native_path.to_string()
        };

        // Emit path string global and call __native_bridge_open.
        let path_global = self.builder.build_global_string_ptr(
            &embed_path, &format!("native_path_{}", self.string_count),
        ).unwrap();
        self.string_count += 1;

        let desc_ptr = self.builder.build_call(
            self.native_bridge_open_fn.unwrap(),
            &[path_global.as_pointer_value().into()],
            "native_desc",
        ).unwrap()
            .try_as_basic_value().left()
            .ok_or_else(|| "__native_bridge_open returned no value".to_string())?
            .into_pointer_value();

        // ---- Collect variable pointers ----
        let mut var_ptrs: Vec<(String, PointerValue<'ctx>)> = Vec::new();
        for (i, (name, _)) in vars.iter().enumerate() {
            let out_alloca = self.create_entry_alloca(&format!("nvar_{}", name));
            self.builder.build_call(
                self.native_bridge_get_var_fn.unwrap(),
                &[
                    desc_ptr.into(),
                    i32_type.const_int(i as u64, false).into(),
                    out_alloca.into(),
                ],
                "",
            ).unwrap();
            var_ptrs.push((name.clone(), out_alloca));
        }

        // ---- Get handler function pointers ----
        let mut handler_ptr_vals: Vec<(String, PointerValue<'ctx>)> = Vec::new();
        for (i, h) in handlers.iter().enumerate() {
            let raw_handler_ptr = self.builder.build_call(
                self.native_bridge_handler_ptr_fn.unwrap(),
                &[desc_ptr.into(), i32_type.const_int(i as u64, false).into()],
                &format!("native_handler_raw_{}", h.class_name),
            ).unwrap()
                .try_as_basic_value().left().unwrap()
                .into_pointer_value();
            handler_ptr_vals.push((h.class_name.clone(), raw_handler_ptr));
        }

        // ---- Register in scope (alias or flatten) ----
        match alias {
            Some(alias_name) => {
                // Build a module object with vars.
                let total_fields = var_ptrs.len();
                let i64_type = self.context.i64_type();

                let field_size = self.field_type.size_of().unwrap();
                let count_val = i64_type.const_int(total_fields as u64, false);
                let total_size = self.builder.build_int_mul(
                    field_size, count_val, "nmod_obj_size",
                ).unwrap();
                let raw_ptr = self.builder.build_call(
                    self.malloc_fn, &[total_size.into()], "nmod_obj_mem",
                ).unwrap()
                    .try_as_basic_value().left()
                    .ok_or_else(|| "malloc returned no value".to_string())?
                    .into_pointer_value();

                for (i, (name, val_ptr)) in var_ptrs.iter().enumerate() {
                    let val = self.builder.build_load(self.value_type, *val_ptr, "nmod_load")
                        .unwrap();
                    let idx = i32_type.const_int(i as u64, false);
                    let field_ptr = unsafe { self.builder.build_in_bounds_gep(
                        self.field_type, raw_ptr, &[idx], &format!("nmod_field_{}", i),
                    ) }.unwrap();

                    let name_global = self.builder.build_global_string_ptr(
                        name, &format!("nmod_fname_{}", self.string_count),
                    ).unwrap();
                    self.string_count += 1;
                    let name_slot = self.builder.build_struct_gep(
                        self.field_type, field_ptr, 0, "nmod_name_slot",
                    ).unwrap();
                    self.builder.build_store(name_slot, name_global.as_pointer_value()).unwrap();

                    let val_slot = self.builder.build_struct_gep(
                        self.field_type, field_ptr, 1, "nmod_val_slot",
                    ).unwrap();
                    self.builder.build_store(val_slot, val).unwrap();
                }

                let tag = self.context.i8_type().const_int(TAG_OBJECT as u64, false);
                let num = self.context.f64_type().const_float(total_fields as f64);
                let bool_val = self.context.bool_type().const_int(0, false);
                let obj_val = self.build_value(tag, num, raw_ptr, bool_val);

                let ptr = self.create_entry_alloca(alias_name);
                self.builder.build_store(ptr, obj_val).unwrap();
                self.scopes
                    .last_mut()
                    .expect("No active scope")
                    .insert(alias_name.to_string(), ptr);

                // Register types under alias.
                for t in types {
                    let key = format!("{}.{}", alias_name, t.name);
                    self.define_type(key, t.fields.clone());
                }
                // Emission types are dispatched by bare class name, not alias-prefixed.
                // Register them globally so linked-file handler declarations
                // (e.g. `MyEvent { field:String } => { }`) can resolve fields.
                for em in emissions {
                    if let Some(t) = types.iter().find(|t| t.name == em.class_name) {
                        self.define_type(em.class_name.clone(), t.fields.clone());
                    }
                }
                // Register native handler pointers under alias.
                // Also store each pointer in an LLVM global so __code_dispatch
                // can load a fresh copy without referencing main()'s instruction results.
                for (class_name, handler_ptr) in &handler_ptr_vals {
                    let key = format!("{}.{}", alias_name, class_name);
                    // Create global backing store.
                    let global_name = format!("__nhptr_{}", key.replace('.', "_"));
                    let global = self.module.add_global(i8_ptr, None, &global_name);
                    global.set_initializer(&i8_ptr.const_null());
                    self.builder.build_store(global.as_pointer_value(), *handler_ptr).unwrap();
                    self.native_handler_globals.insert(key.clone(), global);
                    self.native_handler_ptrs
                        .last_mut()
                        .expect("No active scope")
                        .insert(key, *handler_ptr);
                }
            }
            None => {
                // Flatten mode: put vars directly in current scope.
                for (name, val_ptr) in &var_ptrs {
                    if self.exists_in_any_scope(name) {
                        return Err(format!(
                            "Name conflict: native module defines '{}' which already exists in the current scope",
                            name
                        ));
                    }
                    let val = self.builder.build_load(self.value_type, *val_ptr, "nflat_load")
                        .unwrap();
                    let new_ptr = self.create_entry_alloca(name);
                    self.builder.build_store(new_ptr, val).unwrap();
                    self.scopes
                        .last_mut()
                        .expect("No active scope")
                        .insert(name.clone(), new_ptr);
                }
                for t in types {
                    self.define_type(t.name.clone(), t.fields.clone());
                }
                for (class_name, handler_ptr) in &handler_ptr_vals {
                    // Create global backing store for cross-function access.
                    let global_name = format!("__nhptr_{}", class_name.replace('.', "_"));
                    let global = self.module.add_global(i8_ptr, None, &global_name);
                    global.set_initializer(&i8_ptr.const_null());
                    self.builder.build_store(global.as_pointer_value(), *handler_ptr).unwrap();
                    self.native_handler_globals.insert(class_name.clone(), global);
                    self.native_handler_ptrs
                        .last_mut()
                        .expect("No active scope")
                        .insert(class_name.clone(), *handler_ptr);
                }
            }
        }

        Ok(())
    }

    /// Look up a native handler function pointer by class name.
    fn get_native_handler_ptr(&self, class_name: &str) -> Option<PointerValue<'ctx>> {
        for scope in self.native_handler_ptrs.iter().rev() {
            if let Some(ptr) = scope.get(class_name) {
                return Some(*ptr);
            }
        }
        None
    }

    /// Check whether the program has any NativeImport statements.
    #[allow(dead_code)]
    pub fn program_has_native_imports(&self) -> bool {
        self.has_native_imports
    }

    fn compile_assignment(&mut self, name: &str, value: &Expression) -> Result<(), String> {
        if let Some(existing_ptr) = self.get_var_ptr(name) {
            // Inside handler bodies, allow reassignment (imperative update).
            // At global scope, enforce single-assignment.
            if self.in_handler_depth == 0 {
                return Err(format!(
                    "Cannot reassign '{}': variables are single-assignment",
                    name
                ));
            }
            let val = self.compile_expr(value)?;
            self.builder.build_store(existing_ptr, val).unwrap();
            return Ok(());
        }

        let ptr = self.create_entry_alloca(name);
        let val = self.compile_expr(value)?;
        self.builder.build_store(ptr, val).unwrap();
        self.scopes
            .last_mut()
            .expect("No active scope")
            .insert(name.to_string(), ptr);
        Ok(())
    }

    fn compile_assert(&mut self, expr: &Expression) -> Result<(), String> {
        let value = self.compile_expr(expr)?;
        let value_struct = value.into_struct_value();
        let tag = self
            .builder
            .build_extract_value(value_struct, 0, "assert_tag")
            .unwrap()
            .into_int_value();

        let is_bool = self.builder.build_int_compare(
            IntPredicate::EQ,
            tag,
            self.context.i8_type().const_int(TAG_BOOLEAN as u64, false),
            "assert_is_bool",
        ).unwrap();

        let bool_block = self.context.append_basic_block(self.main_fn, "assert_bool");
        let non_bool_block = self.context.append_basic_block(self.main_fn, "assert_non_bool");
        let fail_bool_block = self.context.append_basic_block(self.main_fn, "assert_fail_bool");
        let fail_exc_block = self.context.append_basic_block(self.main_fn, "assert_fail_exc");
        let cont_block = self.context.append_basic_block(self.main_fn, "assert_cont");

        self.builder.build_conditional_branch(is_bool, bool_block, non_bool_block).unwrap();

        // Boolean path: check true/false.
        self.builder.position_at_end(bool_block);
        let bool_val = self
            .builder
            .build_extract_value(value_struct, 3, "assert_bool_val")
            .unwrap()
            .into_int_value();
        self.builder
            .build_conditional_branch(bool_val, cont_block, fail_bool_block).unwrap();

        // Non-boolean path: check if it's an Exception object → return it; otherwise pass.
        self.builder.position_at_end(non_bool_block);
        let is_exception = self.compile_class_name_check(value_struct, tag, "Exception")?;
        self.builder.build_conditional_branch(is_exception, fail_exc_block, cont_block).unwrap();

        // Fail path for `assert false`: build Exception { message="Assertion failed" } and return/trap.
        self.builder.position_at_end(fail_bool_block);
        if let (Some(ret_alloca), Some(exit_block)) = (self.handler_return_alloca, self.handler_exit_block) {
            let exc_val = self.compile_object(&[
                ("_class".to_string(), Expression::String("Exception".to_string())),
                ("message".to_string(), Expression::String("Assertion failed".to_string())),
                ("innerException".to_string(), Expression::Null),
            ])?;
            self.builder.build_store(ret_alloca, exc_val).unwrap();
            self.builder.build_unconditional_branch(exit_block).unwrap();
        } else {
            self.emit_trap();
        }

        // Fail path for `assert exceptionValue`: return the Exception as-is.
        self.builder.position_at_end(fail_exc_block);
        if let (Some(ret_alloca), Some(exit_block)) = (self.handler_return_alloca, self.handler_exit_block) {
            self.builder.build_store(ret_alloca, value).unwrap();
            self.builder.build_unconditional_branch(exit_block).unwrap();
        } else {
            self.emit_trap();
        }

        self.builder.position_at_end(cont_block);
        Ok(())
    }

    fn compile_expr(&mut self, expr: &Expression) -> Result<BasicValueEnum<'ctx>, String> {
        match expr {
            Expression::Number(n) => Ok(self.const_number(*n).into()),
            Expression::String(s) => Ok(self.const_string(s).into()),
            Expression::Boolean(b) => Ok(self.build_boolean(self.context.bool_type().const_int(if *b { 1 } else { 0 }, false)).into()),
            Expression::Null => Ok(self.const_null().into()),
            Expression::Identifier(name) => {
                let ptr = self
                    .get_var_ptr(name)
                    .ok_or_else(|| format!("Undefined variable '{}'", name))?;
                Ok(self
                    .builder
                    .build_load(self.value_type, ptr, "load_var").unwrap())
            }
            Expression::Object { spread: None, fields } => self.compile_object_fields(fields),
            Expression::Object { spread: Some(source), fields } => self.compile_spread_object_fields(source, fields),
            Expression::Particle { qualifier, class_name, spread, fields } => {
                self.compile_particle(qualifier.as_deref(), class_name, spread.as_deref(), fields)
            }
            Expression::PropertyAccess(receiver, field) => {
                self.compile_property_access(receiver, field)
            }
            Expression::ArrayLiteral(elements) => {
                self.compile_array_literal(elements)
            }
            Expression::IndexAccess { receiver, index } => {
                self.compile_index_access(receiver, index)
            }
            Expression::Call { callee, args } => {
                self.compile_call(callee, args)
            }
            Expression::InterpolatedString(parts) => {
                self.compile_interpolated_string(parts)
            }
            Expression::Binary { left, op, right } => {
                match op {
                    BinaryOp::Add => {
                        self.compile_add(left, right)
                    }
                    BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div => {
                        self.compile_arithmetic(left, op, right)
                    }
                    BinaryOp::Less | BinaryOp::Greater | BinaryOp::LessEqual | BinaryOp::GreaterEqual => {
                        self.compile_relational(left, op, right)
                    }
                    BinaryOp::And => {
                        self.compile_logical_and(left, right)
                    }
                    BinaryOp::Or => {
                        self.compile_logical_or(left, right)
                    }
                    _ => {
                        let left_val = self.compile_expr(left)?.into_struct_value();
                        let right_val = self.compile_expr(right)?.into_struct_value();
                        let result = self.build_compare(op.clone(), left_val, right_val)?;
                        Ok(result.into())
                    }
                }
            }
            Expression::Unary { op, operand } => {
                self.compile_unary(op, operand)
            }
            Expression::TypeCheck { expr, type_expr, negated } => {
                self.compile_type_check(expr, type_expr, *negated)
            }
        }
    }

    /// Compile a type check expression: `expr is TypeExpr` / `expr is not TypeExpr`.
    fn compile_type_check(
        &mut self,
        expr: &Expression,
        type_expr: &TypeExpr,
        negated: bool,
    ) -> Result<BasicValueEnum<'ctx>, String> {
        let val = self.compile_expr(expr)?.into_struct_value();
        let result = self.compile_type_expr_check(val, type_expr)?;

        let final_result = if negated {
            self.builder.build_not(result, "tc_neg").unwrap()
        } else {
            result
        };

        Ok(self.build_boolean(final_result).into())
    }

    /// Compile a runtime check of a struct value against a TypeExpr.
    /// Returns an i1 boolean indicating whether the value matches.
    fn compile_type_expr_check(
        &mut self,
        val: StructValue<'ctx>,
        type_expr: &TypeExpr,
    ) -> Result<inkwell::values::IntValue<'ctx>, String> {
        let tag = self.builder.build_extract_value(val, 0, "tc_tag")
            .unwrap().into_int_value();
        let i8_type = self.context.i8_type();

        match type_expr {
            TypeExpr::Named(name) => {
                // Check for built-in type tags.
                let expected_tag = match name.as_str() {
                    "Number" => Some(TAG_NUMBER),
                    "String" => Some(TAG_STRING),
                    "Boolean" => Some(TAG_BOOLEAN),
                    "Object" => Some(TAG_OBJECT),
                    "Null" => Some(TAG_NULL),
                    "Array" => Some(TAG_ARRAY),
                    _ => None,
                };

                if let Some(tag_val) = expected_tag {
                    Ok(self.builder.build_int_compare(
                        IntPredicate::EQ, tag,
                        i8_type.const_int(tag_val as u64, false),
                        "tc_result",
                    ).unwrap())
                } else {
                    // Check if it's an alias.
                    if let Some(alias_expr) = self.get_type_alias(name).cloned() {
                        return self.compile_type_expr_check(val, &alias_expr);
                    }
                    // Particle class check.
                    let expected_class = if let Some(dot_pos) = name.rfind('.') {
                        &name[dot_pos+1..]
                    } else {
                        name.as_str()
                    };
                    self.compile_class_name_check(val, tag, expected_class)
                }
            }
            TypeExpr::Literal(s) => {
                // Must be a string with the exact value.
                let is_string = self.builder.build_int_compare(
                    IntPredicate::EQ, tag,
                    i8_type.const_int(TAG_STRING as u64, false),
                    "tc_is_str",
                ).unwrap();

                let str_check_block = self.context.append_basic_block(self.main_fn, "tc_lit_check");
                let false_block = self.context.append_basic_block(self.main_fn, "tc_lit_false");
                let merge_block = self.context.append_basic_block(self.main_fn, "tc_lit_merge");

                let _pre_block = self.builder.get_insert_block().unwrap();
                self.builder.build_conditional_branch(is_string, str_check_block, false_block).unwrap();

                self.builder.position_at_end(false_block);
                let false_val = self.context.bool_type().const_int(0, false);
                self.builder.build_unconditional_branch(merge_block).unwrap();

                self.builder.position_at_end(str_check_block);
                let str_ptr = self.builder.build_extract_value(val, 2, "tc_lit_ptr")
                    .unwrap().into_pointer_value();
                let lit_global = self.builder.build_global_string_ptr(
                    s, &format!("tc_lit_{}", self.string_count),
                ).unwrap();
                self.string_count += 1;
                let cmp = self.builder.build_call(
                    self.strcmp_fn,
                    &[str_ptr.into(), lit_global.as_pointer_value().into()],
                    "tc_lit_cmp",
                ).unwrap();
                let cmp_val = cmp.try_as_basic_value().left().unwrap().into_int_value();
                let i32_type = self.context.i32_type();
                let str_match = self.builder.build_int_compare(
                    IntPredicate::EQ, cmp_val, i32_type.const_int(0, false), "tc_lit_eq",
                ).unwrap();
                self.builder.build_unconditional_branch(merge_block).unwrap();
                let str_check_end = self.builder.get_insert_block().unwrap();

                self.builder.position_at_end(merge_block);
                let phi = self.builder.build_phi(self.context.bool_type(), "tc_lit_result").unwrap();
                phi.add_incoming(&[
                    (&false_val, false_block),
                    (&str_match, str_check_end),
                ]);
                Ok(phi.as_basic_value().into_int_value())
            }
            TypeExpr::Union(variants) => {
                // OR all variant checks.
                if variants.is_empty() {
                    return Ok(self.context.bool_type().const_int(0, false));
                }
                let mut result = self.compile_type_expr_check(val, &variants[0])?;
                for variant in &variants[1..] {
                    let next = self.compile_type_expr_check(val, variant)?;
                    result = self.builder.build_or(result, next, "tc_union").unwrap();
                }
                Ok(result)
            }
            TypeExpr::Intersection(variants) => {
                // AND all variant checks.
                if variants.is_empty() {
                    return Ok(self.context.bool_type().const_int(1, false));
                }
                let mut result = self.compile_type_expr_check(val, &variants[0])?;
                for variant in &variants[1..] {
                    let next = self.compile_type_expr_check(val, variant)?;
                    result = self.builder.build_and(result, next, "tc_intersection").unwrap();
                }
                Ok(result)
            }
        }
    }

    /// Check if a value is an object with _class == expected_class.
    /// Returns an i1 boolean.
    fn compile_class_name_check(
        &mut self,
        val: StructValue<'ctx>,
        tag: inkwell::values::IntValue<'ctx>,
        expected_class: &str,
    ) -> Result<inkwell::values::IntValue<'ctx>, String> {
        let i8_type = self.context.i8_type();
        let i32_type = self.context.i32_type();
        let i8_ptr_type = i8_type.ptr_type(AddressSpace::default());

        let is_obj = self.builder.build_int_compare(
            IntPredicate::EQ, tag,
            i8_type.const_int(TAG_OBJECT as u64, false),
            "tc_is_obj",
        ).unwrap();

        let obj_block = self.context.append_basic_block(self.main_fn, "tc_obj");
        let not_obj_block = self.context.append_basic_block(self.main_fn, "tc_not_obj");
        let result_block = self.context.append_basic_block(self.main_fn, "tc_result");

        let _pre_block = self.builder.get_insert_block().unwrap();
        self.builder.build_conditional_branch(is_obj, obj_block, not_obj_block).unwrap();

        // Not object: result is false.
        self.builder.position_at_end(not_obj_block);
        let false_val = self.context.bool_type().const_int(0, false);
        self.builder.build_unconditional_branch(result_block).unwrap();

        // Object: search for _class field and compare.
        self.builder.position_at_end(obj_block);
        let count_f = self.builder.build_extract_value(val, 1, "tc_count_f")
            .unwrap().into_float_value();
        let count = self.builder.build_float_to_unsigned_int(
            count_f, i32_type, "tc_count",
        ).unwrap();
        let arr_ptr = self.builder.build_extract_value(val, 2, "tc_arr")
            .unwrap().into_pointer_value();

        let class_str = self.builder.build_global_string_ptr(
            "_class", &format!("tc_class_key_{}", self.string_count),
        ).unwrap();
        self.string_count += 1;

        let expected_str = self.builder.build_global_string_ptr(
            expected_class, &format!("tc_expected_{}", self.string_count),
        ).unwrap();
        self.string_count += 1;

        // Loop to find _class field.
        let loop_header = self.context.append_basic_block(self.main_fn, "tc_loop_hdr");
        let loop_body = self.context.append_basic_block(self.main_fn, "tc_loop_body");
        let found_block = self.context.append_basic_block(self.main_fn, "tc_found");
        let loop_next = self.context.append_basic_block(self.main_fn, "tc_loop_next");
        let not_found_block = self.context.append_basic_block(self.main_fn, "tc_not_found");

        self.builder.build_unconditional_branch(loop_header).unwrap();

        self.builder.position_at_end(loop_header);
        let i_phi = self.builder.build_phi(i32_type, "tc_i").unwrap();
        i_phi.add_incoming(&[(&i32_type.const_int(0, false), obj_block)]);
        let i_val = i_phi.as_basic_value().into_int_value();
        let done = self.builder.build_int_compare(
            IntPredicate::UGE, i_val, count, "tc_done",
        ).unwrap();
        self.builder.build_conditional_branch(done, not_found_block, loop_body).unwrap();

        self.builder.position_at_end(loop_body);
        let field_ptr = unsafe { self.builder.build_in_bounds_gep(
            self.field_type, arr_ptr, &[i_val], "tc_fptr",
        ) }.unwrap();
        let name_slot = self.builder.build_struct_gep(
            self.field_type, field_ptr, 0, "tc_nslot",
        ).unwrap();
        let name_val = self.builder.build_load(i8_ptr_type, name_slot, "tc_name")
            .unwrap().into_pointer_value();
        let cmp = self.builder.build_call(
            self.strcmp_fn,
            &[name_val.into(), class_str.as_pointer_value().into()],
            "tc_cmp",
        ).unwrap();
        let cmp_val = cmp.try_as_basic_value().left().unwrap().into_int_value();
        let is_match = self.builder.build_int_compare(
            IntPredicate::EQ, cmp_val, i32_type.const_int(0, false), "tc_match",
        ).unwrap();
        self.builder.build_conditional_branch(is_match, found_block, loop_next).unwrap();

        self.builder.position_at_end(loop_next);
        let i_next = self.builder.build_int_add(
            i_val, i32_type.const_int(1, false), "tc_inext",
        ).unwrap();
        i_phi.add_incoming(&[(&i_next, loop_next)]);
        self.builder.build_unconditional_branch(loop_header).unwrap();

        // Found _class: load its value and compare the string with expected.
        self.builder.position_at_end(found_block);
        let val_slot = self.builder.build_struct_gep(
            self.field_type, field_ptr, 1, "tc_vslot",
        ).unwrap();
        let class_val = self.builder.build_load(
            self.value_type, val_slot, "tc_class_val",
        ).unwrap().into_struct_value();
        let class_ptr = self.builder.build_extract_value(class_val, 2, "tc_class_ptr")
            .unwrap().into_pointer_value();
        let class_cmp = self.builder.build_call(
            self.strcmp_fn,
            &[class_ptr.into(), expected_str.as_pointer_value().into()],
            "tc_class_cmp",
        ).unwrap();
        let class_cmp_val = class_cmp.try_as_basic_value().left().unwrap().into_int_value();
        let class_match = self.builder.build_int_compare(
            IntPredicate::EQ, class_cmp_val, i32_type.const_int(0, false), "tc_class_match",
        ).unwrap();
        self.builder.build_unconditional_branch(result_block).unwrap();
        let found_end_block = self.builder.get_insert_block().unwrap();

        // Not found: result is false.
        self.builder.position_at_end(not_found_block);
        self.builder.build_unconditional_branch(result_block).unwrap();

        // Merge results.
        self.builder.position_at_end(result_block);
        let phi = self.builder.build_phi(self.context.bool_type(), "tc_phi").unwrap();
        phi.add_incoming(&[
            (&false_val, not_obj_block),
            (&class_match, found_end_block),
            (&false_val, not_found_block),
        ]);

        Ok(phi.as_basic_value().into_int_value())
    }

    /// Generate the body of `__code_values_equal(value_type*, value_type*) -> i1`.
    fn build_values_equal_fn(&mut self) {
        let saved_block = self.builder.get_insert_block().unwrap();

        let fn_val = self.values_equal_fn;
        let left_ptr = fn_val.get_nth_param(0).unwrap().into_pointer_value();
        let right_ptr = fn_val.get_nth_param(1).unwrap().into_pointer_value();

        let i8_type = self.context.i8_type();
        let i32_type = self.context.i32_type();
        let i8_ptr_type = i8_type.ptr_type(AddressSpace::default());
        let zero_i32 = i32_type.const_int(0, false);
        let one_i32 = i32_type.const_int(1, false);
        let false_val = self.context.bool_type().const_int(0, false);
        let true_val = self.context.bool_type().const_int(1, false);

        let entry = self.context.append_basic_block(fn_val, "entry");
        let return_false = self.context.append_basic_block(fn_val, "return_false");
        let return_true = self.context.append_basic_block(fn_val, "return_true");
        let same_tag = self.context.append_basic_block(fn_val, "same_tag");
        let cmp_num = self.context.append_basic_block(fn_val, "cmp_num");
        let non_num = self.context.append_basic_block(fn_val, "non_num");
        let cmp_str = self.context.append_basic_block(fn_val, "cmp_str");
        let non_str = self.context.append_basic_block(fn_val, "non_str");
        let cmp_bool = self.context.append_basic_block(fn_val, "cmp_bool");
        let non_bool = self.context.append_basic_block(fn_val, "non_bool");
        let cmp_null = self.context.append_basic_block(fn_val, "cmp_null");
        let cmp_obj = self.context.append_basic_block(fn_val, "cmp_obj");
        let obj_loop_entry = self.context.append_basic_block(fn_val, "obj_loop_entry");
        let outer_header = self.context.append_basic_block(fn_val, "outer_header");
        let inner_setup = self.context.append_basic_block(fn_val, "inner_setup");
        let inner_header = self.context.append_basic_block(fn_val, "inner_header");
        let inner_check = self.context.append_basic_block(fn_val, "inner_check");
        let inner_next = self.context.append_basic_block(fn_val, "inner_next");
        let compare_vals = self.context.append_basic_block(fn_val, "compare_vals");
        let outer_next = self.context.append_basic_block(fn_val, "outer_next");

        // entry: load values, compare tags
        self.builder.position_at_end(entry);
        let left_val = self.builder.build_load(self.value_type, left_ptr, "left_val")
            .unwrap().into_struct_value();
        let right_val = self.builder.build_load(self.value_type, right_ptr, "right_val")
            .unwrap().into_struct_value();
        let left_tag = self.builder.build_extract_value(left_val, 0, "ltag")
            .unwrap().into_int_value();
        let right_tag = self.builder.build_extract_value(right_val, 0, "rtag")
            .unwrap().into_int_value();
        let tags_eq = self.builder.build_int_compare(
            IntPredicate::EQ, left_tag, right_tag, "tags_eq",
        ).unwrap();
        self.builder.build_conditional_branch(tags_eq, same_tag, return_false).unwrap();

        self.builder.position_at_end(return_false);
        self.builder.build_return(Some(&false_val)).unwrap();

        self.builder.position_at_end(return_true);
        self.builder.build_return(Some(&true_val)).unwrap();

        // same_tag: dispatch by type
        self.builder.position_at_end(same_tag);
        let is_num = self.builder.build_int_compare(
            IntPredicate::EQ, left_tag,
            i8_type.const_int(TAG_NUMBER as u64, false), "is_num",
        ).unwrap();
        self.builder.build_conditional_branch(is_num, cmp_num, non_num).unwrap();

        // cmp_num
        self.builder.position_at_end(cmp_num);
        let ln = self.builder.build_extract_value(left_val, 1, "ln")
            .unwrap().into_float_value();
        let rn = self.builder.build_extract_value(right_val, 1, "rn")
            .unwrap().into_float_value();
        let num_eq = self.builder.build_float_compare(
            FloatPredicate::OEQ, ln, rn, "num_eq",
        ).unwrap();
        self.builder.build_return(Some(&num_eq)).unwrap();

        // non_num: check string
        self.builder.position_at_end(non_num);
        let is_str = self.builder.build_int_compare(
            IntPredicate::EQ, left_tag,
            i8_type.const_int(TAG_STRING as u64, false), "is_str",
        ).unwrap();
        self.builder.build_conditional_branch(is_str, cmp_str, non_str).unwrap();

        // cmp_str
        self.builder.position_at_end(cmp_str);
        let ls = self.builder.build_extract_value(left_val, 2, "ls")
            .unwrap().into_pointer_value();
        let rs = self.builder.build_extract_value(right_val, 2, "rs")
            .unwrap().into_pointer_value();
        let cmp_call = self.builder.build_call(
            self.strcmp_fn, &[ls.into(), rs.into()], "strcmp",
        ).unwrap();
        let cmp_val = cmp_call.try_as_basic_value().left().unwrap().into_int_value();
        let str_eq = self.builder.build_int_compare(
            IntPredicate::EQ, cmp_val, i32_type.const_int(0, false), "str_eq",
        ).unwrap();
        self.builder.build_return(Some(&str_eq)).unwrap();

        // non_str: check bool
        self.builder.position_at_end(non_str);
        let is_bool = self.builder.build_int_compare(
            IntPredicate::EQ, left_tag,
            i8_type.const_int(TAG_BOOLEAN as u64, false), "is_bool",
        ).unwrap();
        self.builder.build_conditional_branch(is_bool, cmp_bool, non_bool).unwrap();

        // cmp_bool
        self.builder.position_at_end(cmp_bool);
        let lb = self.builder.build_extract_value(left_val, 3, "lb")
            .unwrap().into_int_value();
        let rb = self.builder.build_extract_value(right_val, 3, "rb")
            .unwrap().into_int_value();
        let bool_eq = self.builder.build_int_compare(
            IntPredicate::EQ, lb, rb, "bool_eq",
        ).unwrap();
        self.builder.build_return(Some(&bool_eq)).unwrap();

        // non_bool: check null
        self.builder.position_at_end(non_bool);
        let is_null = self.builder.build_int_compare(
            IntPredicate::EQ, left_tag,
            i8_type.const_int(TAG_NULL as u64, false), "is_null",
        ).unwrap();
        let cmp_array = self.context.append_basic_block(fn_val, "cmp_array");
        self.builder.build_conditional_branch(is_null, cmp_null, cmp_array).unwrap();

        // cmp_null: two Nulls are always equal
        self.builder.position_at_end(cmp_null);
        self.builder.build_return(Some(&true_val)).unwrap();

        // cmp_array: check if arrays, then compare element-wise
        self.builder.position_at_end(cmp_array);
        let is_arr = self.builder.build_int_compare(
            IntPredicate::EQ, left_tag,
            i8_type.const_int(TAG_ARRAY as u64, false), "is_arr",
        ).unwrap();
        let arr_eq_block = self.context.append_basic_block(fn_val, "arr_eq");
        self.builder.build_conditional_branch(is_arr, arr_eq_block, cmp_obj).unwrap();

        // arr_eq: compare array element counts, then positional comparison.
        self.builder.position_at_end(arr_eq_block);
        let l_arr_count_f = self.builder.build_extract_value(left_val, 1, "l_arr_count_f")
            .unwrap().into_float_value();
        let r_arr_count_f = self.builder.build_extract_value(right_val, 1, "r_arr_count_f")
            .unwrap().into_float_value();
        let l_arr_count = self.builder.build_float_to_unsigned_int(l_arr_count_f, i32_type, "l_arr_count").unwrap();
        let r_arr_count = self.builder.build_float_to_unsigned_int(r_arr_count_f, i32_type, "r_arr_count").unwrap();
        let arr_counts_eq = self.builder.build_int_compare(
            IntPredicate::EQ, l_arr_count, r_arr_count, "arr_counts_eq",
        ).unwrap();
        let arr_loop_entry = self.context.append_basic_block(fn_val, "arr_loop_entry");
        self.builder.build_conditional_branch(arr_counts_eq, arr_loop_entry, return_false).unwrap();

        self.builder.position_at_end(arr_loop_entry);
        let arr_is_empty = self.builder.build_int_compare(
            IntPredicate::EQ, l_arr_count, zero_i32, "arr_is_empty",
        ).unwrap();
        let l_arr_ptr = self.builder.build_extract_value(left_val, 2, "l_arr_ptr")
            .unwrap().into_pointer_value();
        let r_arr_ptr = self.builder.build_extract_value(right_val, 2, "r_arr_ptr")
            .unwrap().into_pointer_value();
        let arr_loop_header = self.context.append_basic_block(fn_val, "arr_loop_header");
        let arr_cmp_elem = self.context.append_basic_block(fn_val, "arr_cmp_elem");
        let arr_loop_next = self.context.append_basic_block(fn_val, "arr_loop_next");
        self.builder.build_conditional_branch(arr_is_empty, return_true, arr_loop_header).unwrap();

        self.builder.position_at_end(arr_loop_header);
        let ai_phi = self.builder.build_phi(i32_type, "ai").unwrap();
        ai_phi.add_incoming(&[(&zero_i32, arr_loop_entry)]);
        let ai_val = ai_phi.as_basic_value().into_int_value();
        let arr_done = self.builder.build_int_compare(
            IntPredicate::UGE, ai_val, l_arr_count, "arr_done",
        ).unwrap();
        self.builder.build_conditional_branch(arr_done, return_true, arr_cmp_elem).unwrap();

        self.builder.position_at_end(arr_cmp_elem);
        let l_elem_ptr = unsafe { self.builder.build_in_bounds_gep(
            self.value_type, l_arr_ptr, &[ai_val], "l_elem_ptr",
        ) }.unwrap();
        let r_elem_ptr = unsafe { self.builder.build_in_bounds_gep(
            self.value_type, r_arr_ptr, &[ai_val], "r_elem_ptr",
        ) }.unwrap();
        let elems_eq = self.builder.build_call(
            self.values_equal_fn,
            &[l_elem_ptr.into(), r_elem_ptr.into()],
            "elems_eq",
        ).unwrap();
        let elems_eq_val = elems_eq.try_as_basic_value().left().unwrap().into_int_value();
        self.builder.build_conditional_branch(elems_eq_val, arr_loop_next, return_false).unwrap();

        self.builder.position_at_end(arr_loop_next);
        let ai_next = self.builder.build_int_add(ai_val, one_i32, "ai_next").unwrap();
        ai_phi.add_incoming(&[(&ai_next, arr_loop_next)]);
        self.builder.build_unconditional_branch(arr_loop_header).unwrap();

        // cmp_obj: compare field counts
        self.builder.position_at_end(cmp_obj);
        let l_count_f = self.builder.build_extract_value(left_val, 1, "lcount_f")
            .unwrap().into_float_value();
        let r_count_f = self.builder.build_extract_value(right_val, 1, "rcount_f")
            .unwrap().into_float_value();
        let l_count = self.builder.build_float_to_unsigned_int(
            l_count_f, i32_type, "lcount",
        ).unwrap();
        let r_count = self.builder.build_float_to_unsigned_int(
            r_count_f, i32_type, "rcount",
        ).unwrap();
        let counts_eq = self.builder.build_int_compare(
            IntPredicate::EQ, l_count, r_count, "counts_eq",
        ).unwrap();
        self.builder.build_conditional_branch(counts_eq, obj_loop_entry, return_false).unwrap();

        // obj_loop_entry
        self.builder.position_at_end(obj_loop_entry);
        let is_empty = self.builder.build_int_compare(
            IntPredicate::EQ, l_count, zero_i32, "is_empty",
        ).unwrap();
        let l_arr = self.builder.build_extract_value(left_val, 2, "l_arr")
            .unwrap().into_pointer_value();
        let r_arr = self.builder.build_extract_value(right_val, 2, "r_arr")
            .unwrap().into_pointer_value();
        self.builder.build_conditional_branch(is_empty, return_true, outer_header).unwrap();

        // outer_header
        self.builder.position_at_end(outer_header);
        let i_phi = self.builder.build_phi(i32_type, "i").unwrap();
        i_phi.add_incoming(&[(&zero_i32, obj_loop_entry)]);
        let i_val = i_phi.as_basic_value().into_int_value();
        let outer_done = self.builder.build_int_compare(
            IntPredicate::UGE, i_val, l_count, "outer_done",
        ).unwrap();
        self.builder.build_conditional_branch(outer_done, return_true, inner_setup).unwrap();

        // inner_setup
        self.builder.position_at_end(inner_setup);
        let left_field_ptr = unsafe { self.builder.build_in_bounds_gep(
            self.field_type, l_arr, &[i_val], "lf_ptr",
        ) }.unwrap();
        let left_name_slot = self.builder.build_struct_gep(
            self.field_type, left_field_ptr, 0, "ln_slot",
        ).unwrap();
        let left_name = self.builder.build_load(i8_ptr_type, left_name_slot, "ln")
            .unwrap().into_pointer_value();
        self.builder.build_unconditional_branch(inner_header).unwrap();

        // inner_header
        self.builder.position_at_end(inner_header);
        let j_phi = self.builder.build_phi(i32_type, "j").unwrap();
        j_phi.add_incoming(&[(&zero_i32, inner_setup)]);
        let j_val = j_phi.as_basic_value().into_int_value();
        let inner_done = self.builder.build_int_compare(
            IntPredicate::UGE, j_val, r_count, "inner_done",
        ).unwrap();
        self.builder.build_conditional_branch(inner_done, return_false, inner_check).unwrap();

        // inner_check
        self.builder.position_at_end(inner_check);
        let right_field_ptr = unsafe { self.builder.build_in_bounds_gep(
            self.field_type, r_arr, &[j_val], "rf_ptr",
        ) }.unwrap();
        let right_name_slot = self.builder.build_struct_gep(
            self.field_type, right_field_ptr, 0, "rn_slot",
        ).unwrap();
        let right_name = self.builder.build_load(i8_ptr_type, right_name_slot, "rn")
            .unwrap().into_pointer_value();
        let name_cmp = self.builder.build_call(
            self.strcmp_fn, &[left_name.into(), right_name.into()], "name_cmp",
        ).unwrap();
        let name_cmp_val = name_cmp.try_as_basic_value().left().unwrap().into_int_value();
        let names_match = self.builder.build_int_compare(
            IntPredicate::EQ, name_cmp_val, zero_i32, "names_match",
        ).unwrap();
        self.builder.build_conditional_branch(names_match, compare_vals, inner_next).unwrap();

        // inner_next
        self.builder.position_at_end(inner_next);
        let j_next = self.builder.build_int_add(j_val, one_i32, "j_next").unwrap();
        j_phi.add_incoming(&[(&j_next, inner_next)]);
        self.builder.build_unconditional_branch(inner_header).unwrap();

        // compare_vals
        self.builder.position_at_end(compare_vals);
        let left_val_slot = self.builder.build_struct_gep(
            self.field_type, left_field_ptr, 1, "lv_slot",
        ).unwrap();
        let right_val_slot = self.builder.build_struct_gep(
            self.field_type, right_field_ptr, 1, "rv_slot",
        ).unwrap();
        let vals_eq = self.builder.build_call(
            self.values_equal_fn,
            &[left_val_slot.into(), right_val_slot.into()],
            "vals_eq",
        ).unwrap();
        let vals_eq_val = vals_eq.try_as_basic_value().left().unwrap().into_int_value();
        self.builder.build_conditional_branch(vals_eq_val, outer_next, return_false).unwrap();

        // outer_next
        self.builder.position_at_end(outer_next);
        let i_next = self.builder.build_int_add(i_val, one_i32, "i_next").unwrap();
        i_phi.add_incoming(&[(&i_next, outer_next)]);
        self.builder.build_unconditional_branch(outer_header).unwrap();

        self.builder.position_at_end(saved_block);
    }

    /// Compile an object literal from ObjectField variants (static or computed).
    fn compile_object_fields(&mut self, fields: &[ObjectField]) -> Result<BasicValueEnum<'ctx>, String> {
        // Separate into static and computed fields.
        let mut static_fields: Vec<(&str, &Expression)> = Vec::new();
        let mut computed_fields: Vec<(&Expression, &Expression)> = Vec::new();
        for f in fields {
            match f {
                ObjectField::Static(name, expr) => static_fields.push((name.as_str(), expr)),
                ObjectField::Computed(key, val) => computed_fields.push((key, val)),
            }
        }

        if computed_fields.is_empty() {
            // All static: use the tuple-based compile_object
            let tuples: Vec<(String, Expression)> = static_fields.iter()
                .map(|(n, e)| (n.to_string(), (*e).clone()))
                .collect();
            return self.compile_object(&tuples);
        }

        // Has computed fields: need dynamic allocation.
        let total = fields.len();
        let i32_type = self.context.i32_type();
        let i64_type = self.context.i64_type();

        let field_size = self.field_type.size_of().unwrap();
        let count_val = i64_type.const_int(total as u64, false);
        let total_size = self.builder.build_int_mul(field_size, count_val, "cobj_size").unwrap();
        let raw_ptr = self.builder.build_call(
            self.malloc_fn, &[total_size.into()], "cobj_mem",
        ).unwrap()
            .try_as_basic_value().left()
            .ok_or_else(|| "malloc returned no value".to_string())?
            .into_pointer_value();

        for (i, f) in fields.iter().enumerate() {
            let idx = i32_type.const_int(i as u64, false);
            let field_ptr = unsafe { self.builder.build_in_bounds_gep(
                self.field_type, raw_ptr, &[idx], &format!("cfield_{}", i),
            ) }.unwrap();

            match f {
                ObjectField::Static(name, expr) => {
                    let val = self.compile_expr(expr)?;
                    let name_global = self.builder.build_global_string_ptr(
                        name, &format!("cfname_{}", self.string_count),
                    ).unwrap();
                    self.string_count += 1;
                    let name_slot = self.builder.build_struct_gep(
                        self.field_type, field_ptr, 0, "cname_slot",
                    ).unwrap();
                    self.builder.build_store(name_slot, name_global.as_pointer_value()).unwrap();
                    let val_slot = self.builder.build_struct_gep(
                        self.field_type, field_ptr, 1, "cval_slot",
                    ).unwrap();
                    self.builder.build_store(val_slot, val).unwrap();
                }
                ObjectField::Computed(key_expr, val_expr) => {
                    let key_val = self.compile_expr(key_expr)?.into_struct_value();
                    // Extract string pointer from the key value (must be a string).
                    let key_ptr = self.builder.build_extract_value(key_val, 2, "ckey_ptr")
                        .unwrap().into_pointer_value();
                    let val = self.compile_expr(val_expr)?;
                    let name_slot = self.builder.build_struct_gep(
                        self.field_type, field_ptr, 0, "cname_slot",
                    ).unwrap();
                    self.builder.build_store(name_slot, key_ptr).unwrap();
                    let val_slot = self.builder.build_struct_gep(
                        self.field_type, field_ptr, 1, "cval_slot",
                    ).unwrap();
                    self.builder.build_store(val_slot, val).unwrap();
                }
            }
        }

        let tag = self.context.i8_type().const_int(TAG_OBJECT as u64, false);
        let num = self.context.f64_type().const_float(total as f64);
        let bool_val = self.context.bool_type().const_int(0, false);
        Ok(self.build_value(tag, num, raw_ptr, bool_val).into())
    }

    /// Compile an object literal.
    fn compile_object(&mut self, fields: &[(String, Expression)]) -> Result<BasicValueEnum<'ctx>, String> {
        let count = fields.len();
        let i64_type = self.context.i64_type();

        let field_size = self.field_type.size_of().unwrap();
        let count_val = i64_type.const_int(count as u64, false);
        let total_size = self.builder.build_int_mul(
            field_size, count_val, "obj_size",
        ).unwrap();
        let raw_ptr = self.builder.build_call(
            self.malloc_fn, &[total_size.into()], "obj_mem",
        ).unwrap()
            .try_as_basic_value().left()
            .ok_or_else(|| "malloc returned no value".to_string())?
            .into_pointer_value();

        let i32_type = self.context.i32_type();
        for (i, (name, expr)) in fields.iter().enumerate() {
            let val = self.compile_expr(expr)?;
            let idx = i32_type.const_int(i as u64, false);
            let field_ptr = unsafe { self.builder.build_in_bounds_gep(
                self.field_type, raw_ptr, &[idx], &format!("field_{}", i),
            ) }.unwrap();

            let name_global = self.builder.build_global_string_ptr(
                name, &format!("fname_{}", self.string_count),
            ).unwrap();
            self.string_count += 1;
            let name_slot = self.builder.build_struct_gep(
                self.field_type, field_ptr, 0, "name_slot",
            ).unwrap();
            self.builder.build_store(name_slot, name_global.as_pointer_value()).unwrap();

            let val_slot = self.builder.build_struct_gep(
                self.field_type, field_ptr, 1, "val_slot",
            ).unwrap();
            self.builder.build_store(val_slot, val).unwrap();
        }

        let tag = self.context.i8_type().const_int(TAG_OBJECT as u64, false);
        let num = self.context.f64_type().const_float(count as f64);
        let bool_val = self.context.bool_type().const_int(0, false);
        Ok(self.build_value(tag, num, raw_ptr, bool_val).into())
    }

    /// Compile an Object literal with a spread source from ObjectField variants.
    fn compile_spread_object_fields(
        &mut self,
        source: &Expression,
        fields: &[ObjectField],
    ) -> Result<BasicValueEnum<'ctx>, String> {
        // Extract static fields as tuples for the existing spread object implementation.
        let tuples: Vec<(String, Expression)> = fields.iter().filter_map(|f| match f {
            ObjectField::Static(n, e) => Some((n.clone(), e.clone())),
            ObjectField::Computed(_, _) => None,
        }).collect();
        self.compile_spread_object(source, &tuples)
    }

    /// Compile an Object literal with a spread source: `{ ...source, field = val }`.
    /// At runtime: allocate (src_count + overrides) slots, copy non-overridden
    /// source fields, then write explicit override fields.
    fn compile_spread_object(
        &mut self,
        source: &Expression,
        overrides: &[(String, Expression)],
    ) -> Result<BasicValueEnum<'ctx>, String> {
        let i8_type = self.context.i8_type();
        let i32_type = self.context.i32_type();
        let i64_type = self.context.i64_type();
        let i8_ptr_type = i8_type.ptr_type(AddressSpace::default());
        let m = overrides.len() as u64;

        // 1. Evaluate spread source.
        let src_val = self.compile_expr(source)?.into_struct_value();

        // 2. Extract source field count and array pointer.
        let src_count_f = self.builder.build_extract_value(src_val, 1, "spread_count_f")
            .unwrap().into_float_value();
        let src_count = self.builder.build_float_to_unsigned_int(
            src_count_f, i32_type, "spread_count",
        ).unwrap();
        let src_arr = self.builder.build_extract_value(src_val, 2, "spread_arr")
            .unwrap().into_pointer_value();

        // 3. Allocate max output: src_count + M slots.
        let m_val = i32_type.const_int(m, false);
        let max_count = self.builder.build_int_add(src_count, m_val, "spread_max").unwrap();
        let max_count_64 = self.builder.build_int_z_extend(max_count, i64_type, "spread_max64").unwrap();
        let field_size = self.field_type.size_of().unwrap();
        let total_size = self.builder.build_int_mul(field_size, max_count_64, "spread_alloc").unwrap();
        let out_arr = self.builder.build_call(
            self.malloc_fn, &[total_size.into()], "spread_mem",
        ).unwrap()
            .try_as_basic_value().left()
            .ok_or_else(|| "malloc returned no value".to_string())?
            .into_pointer_value();

        // 4. Create a write-index alloca.
        let widx_alloca = self.builder.build_alloca(i32_type, "spread_widx").unwrap();
        self.builder.build_store(widx_alloca, i32_type.const_int(0, false)).unwrap();

        // 5. Create global string pointers for override names (for strcmp matching).
        let mut override_name_globals = Vec::new();
        for (name, _) in overrides {
            let gv = self.builder.build_global_string_ptr(
                name, &format!("spread_oname_{}", self.string_count),
            ).unwrap();
            self.string_count += 1;
            override_name_globals.push(gv.as_pointer_value());
        }

        // 6. Loop over source fields: copy those not in overrides.
        let loop_hdr = self.context.append_basic_block(self.main_fn, "spread_loop_hdr");
        let loop_body = self.context.append_basic_block(self.main_fn, "spread_loop_body");
        let loop_copy = self.context.append_basic_block(self.main_fn, "spread_loop_copy");
        let loop_skip = self.context.append_basic_block(self.main_fn, "spread_loop_skip");
        let loop_end = self.context.append_basic_block(self.main_fn, "spread_loop_end");

        let entry_bb = self.builder.get_insert_block().unwrap();
        self.builder.build_unconditional_branch(loop_hdr).unwrap();

        // Loop header: i = phi(0, i+1).
        self.builder.position_at_end(loop_hdr);
        let i_phi = self.builder.build_phi(i32_type, "spread_i").unwrap();
        i_phi.add_incoming(&[(&i32_type.const_int(0, false), entry_bb)]);
        let i_val = i_phi.as_basic_value().into_int_value();
        let done = self.builder.build_int_compare(
            IntPredicate::UGE, i_val, src_count, "spread_done",
        ).unwrap();
        self.builder.build_conditional_branch(done, loop_end, loop_body).unwrap();

        // Loop body: get source field name, check against overrides.
        self.builder.position_at_end(loop_body);
        let src_field_ptr = unsafe { self.builder.build_in_bounds_gep(
            self.field_type, src_arr, &[i_val], "spread_sfptr",
        ) }.unwrap();
        let src_name_slot = self.builder.build_struct_gep(
            self.field_type, src_field_ptr, 0, "spread_nslot",
        ).unwrap();
        let src_name = self.builder.build_load(i8_ptr_type, src_name_slot, "spread_sname")
            .unwrap().into_pointer_value();

        // Unrolled strcmp against each override name.
        let mut _cur_block = loop_body;
        for (oi, gptr) in override_name_globals.iter().enumerate() {
            let cmp = self.builder.build_call(
                self.strcmp_fn,
                &[src_name.into(), (*gptr).into()],
                &format!("spread_cmp_{}", oi),
            ).unwrap();
            let cmp_val = cmp.try_as_basic_value().left().unwrap().into_int_value();
            let is_match = self.builder.build_int_compare(
                IntPredicate::EQ, cmp_val, i32_type.const_int(0, false),
                &format!("spread_match_{}", oi),
            ).unwrap();
            let next_check = self.context.append_basic_block(self.main_fn, &format!("spread_chk_{}", oi));
            self.builder.build_conditional_branch(is_match, loop_skip, next_check).unwrap();
            self.builder.position_at_end(next_check);
            _cur_block = next_check;
        }
        // No override matched — copy this field.
        self.builder.build_unconditional_branch(loop_copy).unwrap();

        // Copy block: copy name+value from source to output at widx.
        self.builder.position_at_end(loop_copy);
        let widx = self.builder.build_load(i32_type, widx_alloca, "spread_widx_ld").unwrap().into_int_value();
        let dst_field_ptr = unsafe { self.builder.build_in_bounds_gep(
            self.field_type, out_arr, &[widx], "spread_dfptr",
        ) }.unwrap();
        // Copy name.
        let dst_name_slot = self.builder.build_struct_gep(self.field_type, dst_field_ptr, 0, "spread_dn").unwrap();
        self.builder.build_store(dst_name_slot, src_name).unwrap();
        // Copy value.
        let src_val_slot = self.builder.build_struct_gep(self.field_type, src_field_ptr, 1, "spread_sv").unwrap();
        let src_val_loaded = self.builder.build_load(self.value_type, src_val_slot, "spread_sval").unwrap();
        let dst_val_slot = self.builder.build_struct_gep(self.field_type, dst_field_ptr, 1, "spread_dv").unwrap();
        self.builder.build_store(dst_val_slot, src_val_loaded).unwrap();
        // Increment write index.
        let widx_next = self.builder.build_int_add(widx, i32_type.const_int(1, false), "spread_widx_inc").unwrap();
        self.builder.build_store(widx_alloca, widx_next).unwrap();
        self.builder.build_unconditional_branch(loop_skip).unwrap();

        // Skip block: increment i, loop back.
        self.builder.position_at_end(loop_skip);
        let i_next = self.builder.build_int_add(i_val, i32_type.const_int(1, false), "spread_inext").unwrap();
        i_phi.add_incoming(&[(&i_next, loop_skip)]);
        self.builder.build_unconditional_branch(loop_hdr).unwrap();

        // After loop: write all explicit override fields.
        self.builder.position_at_end(loop_end);
        for (oi, (name, expr)) in overrides.iter().enumerate() {
            let val = self.compile_expr(expr)?;
            let widx = self.builder.build_load(i32_type, widx_alloca, &format!("spread_ow_{}", oi)).unwrap().into_int_value();
            let dst_field_ptr = unsafe { self.builder.build_in_bounds_gep(
                self.field_type, out_arr, &[widx], &format!("spread_odfp_{}", oi),
            ) }.unwrap();
            let name_global = self.builder.build_global_string_ptr(
                name, &format!("spread_ofname_{}", self.string_count),
            ).unwrap();
            self.string_count += 1;
            let dst_name_slot = self.builder.build_struct_gep(self.field_type, dst_field_ptr, 0, &format!("spread_on_{}", oi)).unwrap();
            self.builder.build_store(dst_name_slot, name_global.as_pointer_value()).unwrap();
            let dst_val_slot = self.builder.build_struct_gep(self.field_type, dst_field_ptr, 1, &format!("spread_ov_{}", oi)).unwrap();
            self.builder.build_store(dst_val_slot, val).unwrap();
            let widx_next = self.builder.build_int_add(widx, i32_type.const_int(1, false), &format!("spread_owi_{}", oi)).unwrap();
            self.builder.build_store(widx_alloca, widx_next).unwrap();
        }

        // Build final object value.
        let final_count = self.builder.build_load(i32_type, widx_alloca, "spread_final_count").unwrap().into_int_value();
        let final_count_f = self.builder.build_unsigned_int_to_float(
            final_count, self.context.f64_type(), "spread_count_f64",
        ).unwrap();
        let tag = i8_type.const_int(TAG_OBJECT as u64, false);
        let bool_val = self.context.bool_type().const_int(0, false);
        Ok(self.build_value(tag, final_count_f, out_arr, bool_val).into())
    }

    /// Compile a Particle literal with spread: `ClassName { ...source, field = val }`.
    /// Uses the type schema (if known) to expand spread into PropertyAccess expressions,
    /// then delegates to compile_object for static field layout.
    fn compile_spread_particle(
        &mut self,
        qualifier: Option<&str>,
        class_name: &str,
        source: &Expression,
        overrides: &[(String, Expression)],
    ) -> Result<BasicValueEnum<'ctx>, String> {
        let type_key = match qualifier {
            Some(q) => format!("{}.{}", q, class_name),
            None => class_name.to_string(),
        };

        let override_names: std::collections::HashSet<&str> =
            overrides.iter().map(|(n, _)| n.as_str()).collect();

        if let Some(schema) = self.get_type_def(&type_key).cloned() {
            // Validate override field names against schema.
            let schema_names: std::collections::HashSet<&str> =
                schema.iter().map(|(n, _, _)| n.as_str()).collect();
            for name in &override_names {
                if !schema_names.contains(name) {
                    return Err(format!(
                        "Unknown field '{}' for type '{}'",
                        name, type_key
                    ));
                }
            }
            // Validate override field types.
            for (name, expr) in overrides {
                if let Some((_, expected_type, is_optional)) = schema.iter().find(|(n, _, _)| n == name) {
                    match self.infer_expr_type(expr) {
                        Ok(actual_type) => {
                            if !self.inferred_matches_type_expr(&actual_type, expected_type) {
                                if !(*is_optional && actual_type == "Null") {
                                    return Err(format!(
                                        "Type mismatch for field '{}' of '{}': expected {}, got {}",
                                        name, type_key, expected_type, actual_type
                                    ));
                                }
                            }
                        }
                        Err(_) => {}
                    }
                }
            }

            // Compile source once, store in alloca.
            let src_val = self.compile_expr(source)?;
            let src_alloca = self.builder.build_alloca(self.value_type, "spread_psrc").unwrap();
            self.builder.build_store(src_alloca, src_val).unwrap();

            // Build complete field list: _class, _created, then for each schema field
            // use override value if provided, otherwise PropertyAccess from source.
            let mut all_fields: Vec<(String, Expression)> = vec![
                ("_class".to_string(), Expression::String(class_name.to_string())),
                ("_created".to_string(), Expression::Number(0.0)),
            ];

            // Track which schema fields need runtime property access from source.
            let mut runtime_fields: Vec<String> = Vec::new();

            for (sf_name, _, _) in &schema {
                if override_names.contains(sf_name.as_str()) {
                    // Use override — will be compiled below.
                    if let Some((_, expr)) = overrides.iter().find(|(n, _)| n == sf_name) {
                        all_fields.push((sf_name.clone(), expr.clone()));
                    }
                } else {
                    // Placeholder — will be replaced with property access from stored value.
                    runtime_fields.push(sf_name.clone());
                    all_fields.push((sf_name.clone(), Expression::Null)); // placeholder
                }
            }

            // Compile as object, but for runtime_fields, replace placeholders with
            // property accesses from the stored source value.
            let count = all_fields.len();
            let i64_type = self.context.i64_type();
            let field_size = self.field_type.size_of().unwrap();
            let count_val = i64_type.const_int(count as u64, false);
            let total_size = self.builder.build_int_mul(field_size, count_val, "pspread_size").unwrap();
            let raw_ptr = self.builder.build_call(
                self.malloc_fn, &[total_size.into()], "pspread_mem",
            ).unwrap()
                .try_as_basic_value().left()
                .ok_or_else(|| "malloc returned no value".to_string())?
                .into_pointer_value();

            let i32_type = self.context.i32_type();
            for (i, (name, expr)) in all_fields.iter().enumerate() {
                let val = if runtime_fields.contains(name) {
                    // Load from stored source and do property access.
                    let loaded_src = self.builder.build_load(self.value_type, src_alloca, "pspread_src_ld").unwrap();
                    self.compile_property_access_from_value(loaded_src.into_struct_value(), name)?
                } else {
                    self.compile_expr(expr)?
                };

                let idx = i32_type.const_int(i as u64, false);
                let field_ptr = unsafe { self.builder.build_in_bounds_gep(
                    self.field_type, raw_ptr, &[idx], &format!("pspread_f_{}", i),
                ) }.unwrap();

                let name_global = self.builder.build_global_string_ptr(
                    name, &format!("pspread_fn_{}", self.string_count),
                ).unwrap();
                self.string_count += 1;
                let name_slot = self.builder.build_struct_gep(self.field_type, field_ptr, 0, "pspread_ns").unwrap();
                self.builder.build_store(name_slot, name_global.as_pointer_value()).unwrap();

                let val_slot = self.builder.build_struct_gep(self.field_type, field_ptr, 1, "pspread_vs").unwrap();
                self.builder.build_store(val_slot, val).unwrap();
            }

            let tag = self.context.i8_type().const_int(TAG_OBJECT as u64, false);
            let num = self.context.f64_type().const_float(count as f64);
            let bool_val = self.context.bool_type().const_int(0, false);
            Ok(self.build_value(tag, num, raw_ptr, bool_val).into())
        } else {
            // No schema — fall back to runtime spread merge.
            // Prepend _class and _created fields to overrides.
            let mut all_overrides = vec![
                ("_class".to_string(), Expression::String(class_name.to_string())),
                ("_created".to_string(), Expression::Number(0.0)),
            ];
            for (k, v) in overrides {
                all_overrides.push((k.clone(), v.clone()));
            }
            self.compile_spread_object(source, &all_overrides)
        }
    }

    fn compile_particle(
        &mut self,
        qualifier: Option<&str>,
        class_name: &str,
        spread: Option<&Expression>,
        fields: &[ObjectField],
    ) -> Result<BasicValueEnum<'ctx>, String> {
        let type_key = match qualifier {
            Some(q) => format!("{}.{}", q, class_name),
            None => class_name.to_string(),
        };

        // Convert ObjectField to tuples for internal use (only static fields for validation)
        let static_fields: Vec<(String, Expression)> = fields.iter().filter_map(|f| match f {
            ObjectField::Static(n, e) => Some((n.clone(), e.clone())),
            ObjectField::Computed(_, _) => None,
        }).collect();

        // If spread is used, delegate to compile_spread_particle.
        if let Some(source_expr) = spread {
            return self.compile_spread_particle(qualifier, class_name, source_expr, &static_fields);
        }

        // Determine optional fields that need Null injection.
        let mut optional_missing: Vec<String> = Vec::new();

        if let Some(schema) = self.get_type_def(&type_key).cloned() {
            let provided: std::collections::HashSet<&str> =
                static_fields.iter().map(|(n, _)| n.as_str()).collect();
            for (sf_name, _, is_optional) in &schema {
                if !provided.contains(sf_name.as_str()) {
                    if *is_optional {
                        optional_missing.push(sf_name.clone());
                    } else {
                        return Err(format!(
                            "Missing field '{}' for type '{}'",
                            sf_name, type_key
                        ));
                    }
                }
            }
            let schema_names: std::collections::HashSet<&str> =
                schema.iter().map(|(n, _, _)| n.as_str()).collect();
            for (pf_name, _) in &static_fields {
                if !schema_names.contains(pf_name.as_str()) {
                    return Err(format!(
                        "Unknown field '{}' for type '{}'",
                        pf_name, type_key
                    ));
                }
            }
            // Validate field types.
            for (name, expr) in &static_fields {
                if let Some((_, expected_type, is_optional)) = schema.iter().find(|(n, _, _)| n == name) {
                    match self.infer_expr_type(expr) {
                        Ok(actual_type) => {
                            if !self.inferred_matches_type_expr(&actual_type, expected_type) {
                                if !(*is_optional && actual_type == "Null") {
                                    return Err(format!(
                                        "Type mismatch for field '{}' of '{}': expected {}, got {}",
                                        name, type_key, expected_type, actual_type
                                    ));
                                }
                            }
                        }
                        Err(_) => {} // Can't infer field type statically, skip check
                    }
                }
            }
        }

        // Build all fields: _class, _created, user fields, optional missing fields as Null.
        let mut all_fields: Vec<(String, Expression)> = vec![
            ("_class".to_string(), Expression::String(class_name.to_string())),
            ("_created".to_string(), Expression::Number(0.0)),
        ];
        for (k, v) in &static_fields {
            all_fields.push((k.clone(), v.clone()));
        }
        for missing in &optional_missing {
            all_fields.push((missing.clone(), Expression::Null));
        }

        self.compile_object(&all_fields)
    }

    /// Compile property access.
    fn compile_property_access(
        &mut self,
        receiver: &Expression,
        field: &str,
    ) -> Result<BasicValueEnum<'ctx>, String> {
        let recv_val = self.compile_expr(receiver)?.into_struct_value();
        let tag = self.builder.build_extract_value(recv_val, 0, "recv_tag")
            .unwrap().into_int_value();
        let i8_type = self.context.i8_type();
        let i32_type = self.context.i32_type();
        let i8_ptr_type = i8_type.ptr_type(AddressSpace::default());

        let is_obj = self.builder.build_int_compare(
            IntPredicate::EQ, tag,
            i8_type.const_int(TAG_OBJECT as u64, false),
            "is_obj",
        ).unwrap();

        let obj_ok = self.context.append_basic_block(self.main_fn, "prop_obj_ok");
        let prop_not_obj = self.context.append_basic_block(self.main_fn, "prop_not_obj");
        self.builder.build_conditional_branch(is_obj, obj_ok, prop_not_obj).unwrap();

        // Non-object receiver: return Null instead of aborting (matches interpreter behaviour).
        self.builder.position_at_end(prop_not_obj);
        let null_for_non_obj = self.const_null();
        // We'll branch to prop_continue at the very end (after defining it).

        self.builder.position_at_end(obj_ok);
        let count_f = self.builder.build_extract_value(recv_val, 1, "field_count_f")
            .unwrap().into_float_value();
        let count = self.builder.build_float_to_unsigned_int(
            count_f, i32_type, "field_count",
        ).unwrap();
        let arr_ptr = self.builder.build_extract_value(recv_val, 2, "fields_ptr")
            .unwrap().into_pointer_value();

        let target_name = self.builder.build_global_string_ptr(
            field, &format!("prop_{}", self.string_count),
        ).unwrap();
        self.string_count += 1;

        let loop_header = self.context.append_basic_block(self.main_fn, "prop_loop_hdr");
        let loop_body = self.context.append_basic_block(self.main_fn, "prop_loop_body");
        let prop_found = self.context.append_basic_block(self.main_fn, "prop_found");
        let loop_next = self.context.append_basic_block(self.main_fn, "prop_loop_next");
        let prop_not_found = self.context.append_basic_block(self.main_fn, "prop_not_found");
        let prop_continue = self.context.append_basic_block(self.main_fn, "prop_continue");

        self.builder.build_unconditional_branch(loop_header).unwrap();

        self.builder.position_at_end(loop_header);
        let i_phi = self.builder.build_phi(i32_type, "prop_i").unwrap();
        i_phi.add_incoming(&[(&i32_type.const_int(0, false), obj_ok)]);
        let i_val = i_phi.as_basic_value().into_int_value();
        let done = self.builder.build_int_compare(
            IntPredicate::UGE, i_val, count, "prop_done",
        ).unwrap();
        self.builder.build_conditional_branch(done, prop_not_found, loop_body).unwrap();

        self.builder.position_at_end(loop_body);
        let field_ptr = unsafe { self.builder.build_in_bounds_gep(
            self.field_type, arr_ptr, &[i_val], "prop_fptr",
        ) }.unwrap();
        let name_slot = self.builder.build_struct_gep(
            self.field_type, field_ptr, 0, "prop_nslot",
        ).unwrap();
        let name_val = self.builder.build_load(i8_ptr_type, name_slot, "prop_name")
            .unwrap().into_pointer_value();
        let cmp = self.builder.build_call(
            self.strcmp_fn,
            &[name_val.into(), target_name.as_pointer_value().into()],
            "prop_cmp",
        ).unwrap();
        let cmp_val = cmp.try_as_basic_value().left().unwrap().into_int_value();
        let is_match = self.builder.build_int_compare(
            IntPredicate::EQ, cmp_val, i32_type.const_int(0, false), "prop_match",
        ).unwrap();
        self.builder.build_conditional_branch(is_match, prop_found, loop_next).unwrap();

        self.builder.position_at_end(loop_next);
        let i_next = self.builder.build_int_add(
            i_val, i32_type.const_int(1, false), "prop_inext",
        ).unwrap();
        i_phi.add_incoming(&[(&i_next, loop_next)]);
        self.builder.build_unconditional_branch(loop_header).unwrap();

        self.builder.position_at_end(prop_found);
        let val_slot = self.builder.build_struct_gep(
            self.field_type, field_ptr, 1, "prop_vslot",
        ).unwrap();
        let loaded_val = self.builder.build_load(
            self.value_type, val_slot, "prop_val",
        ).unwrap();
        self.builder.build_unconditional_branch(prop_continue).unwrap();

        self.builder.position_at_end(prop_not_found);
        let null_val = self.const_null();
        self.builder.build_unconditional_branch(prop_continue).unwrap();

        // Now wire the non-object branch to prop_continue too.
        self.builder.position_at_end(prop_not_obj);
        self.builder.build_unconditional_branch(prop_continue).unwrap();

        self.builder.position_at_end(prop_continue);
        let phi = self.builder.build_phi(self.value_type, "prop_result").unwrap();
        phi.add_incoming(&[(&loaded_val, prop_found), (&null_val, prop_not_found), (&null_for_non_obj, prop_not_obj)]);
        Ok(phi.as_basic_value())
    }

    /// Like compile_property_access but takes an already-compiled value instead
    /// of an expression. Used by compile_spread_particle to look up fields from
    /// the source value without re-evaluating the source expression.
    fn compile_property_access_from_value(
        &mut self,
        recv_val: StructValue<'ctx>,
        field: &str,
    ) -> Result<BasicValueEnum<'ctx>, String> {
        let i8_type = self.context.i8_type();
        let i32_type = self.context.i32_type();
        let i8_ptr_type = i8_type.ptr_type(AddressSpace::default());

        let count_f = self.builder.build_extract_value(recv_val, 1, "spv_count_f")
            .unwrap().into_float_value();
        let count = self.builder.build_float_to_unsigned_int(
            count_f, i32_type, "spv_count",
        ).unwrap();
        let arr_ptr = self.builder.build_extract_value(recv_val, 2, "spv_arr")
            .unwrap().into_pointer_value();

        let target_name = self.builder.build_global_string_ptr(
            field, &format!("spv_{}", self.string_count),
        ).unwrap();
        self.string_count += 1;

        let entry_bb = self.builder.get_insert_block().unwrap();
        let loop_header = self.context.append_basic_block(self.main_fn, "spv_loop_hdr");
        let loop_body = self.context.append_basic_block(self.main_fn, "spv_loop_body");
        let found_bb = self.context.append_basic_block(self.main_fn, "spv_found");
        let loop_next = self.context.append_basic_block(self.main_fn, "spv_loop_next");
        let not_found = self.context.append_basic_block(self.main_fn, "spv_not_found");
        let cont_bb = self.context.append_basic_block(self.main_fn, "spv_cont");

        self.builder.build_unconditional_branch(loop_header).unwrap();

        self.builder.position_at_end(loop_header);
        let i_phi = self.builder.build_phi(i32_type, "spv_i").unwrap();
        i_phi.add_incoming(&[(&i32_type.const_int(0, false), entry_bb)]);
        let i_val = i_phi.as_basic_value().into_int_value();
        let done = self.builder.build_int_compare(
            IntPredicate::UGE, i_val, count, "spv_done",
        ).unwrap();
        self.builder.build_conditional_branch(done, not_found, loop_body).unwrap();

        self.builder.position_at_end(loop_body);
        let field_ptr = unsafe { self.builder.build_in_bounds_gep(
            self.field_type, arr_ptr, &[i_val], "spv_fptr",
        ) }.unwrap();
        let name_slot = self.builder.build_struct_gep(
            self.field_type, field_ptr, 0, "spv_nslot",
        ).unwrap();
        let name_val = self.builder.build_load(i8_ptr_type, name_slot, "spv_name")
            .unwrap().into_pointer_value();
        let cmp = self.builder.build_call(
            self.strcmp_fn,
            &[name_val.into(), target_name.as_pointer_value().into()],
            "spv_cmp",
        ).unwrap();
        let cmp_val = cmp.try_as_basic_value().left().unwrap().into_int_value();
        let is_match = self.builder.build_int_compare(
            IntPredicate::EQ, cmp_val, i32_type.const_int(0, false), "spv_match",
        ).unwrap();
        self.builder.build_conditional_branch(is_match, found_bb, loop_next).unwrap();

        self.builder.position_at_end(loop_next);
        let i_next = self.builder.build_int_add(
            i_val, i32_type.const_int(1, false), "spv_inext",
        ).unwrap();
        i_phi.add_incoming(&[(&i_next, loop_next)]);
        self.builder.build_unconditional_branch(loop_header).unwrap();

        self.builder.position_at_end(found_bb);
        let val_slot = self.builder.build_struct_gep(
            self.field_type, field_ptr, 1, "spv_vslot",
        ).unwrap();
        let loaded_val = self.builder.build_load(
            self.value_type, val_slot, "spv_val",
        ).unwrap();
        self.builder.build_unconditional_branch(cont_bb).unwrap();

        self.builder.position_at_end(not_found);
        let null_val = self.const_null();
        self.builder.build_unconditional_branch(cont_bb).unwrap();

        self.builder.position_at_end(cont_bb);
        let phi = self.builder.build_phi(self.value_type, "spv_result").unwrap();
        phi.add_incoming(&[(&loaded_val, found_bb), (&null_val, not_found)]);
        Ok(phi.as_basic_value())
    }

    fn build_compare(
        &mut self,
        op: BinaryOp,
        left: StructValue<'ctx>,
        right: StructValue<'ctx>,
    ) -> Result<StructValue<'ctx>, String> {
        let tag_type = self.context.i8_type();
        let left_tag = self.builder.build_extract_value(left, 0, "left_tag")
            .unwrap().into_int_value();
        let right_tag = self.builder.build_extract_value(right, 0, "right_tag")
            .unwrap().into_int_value();

        let tags_equal = self.builder.build_int_compare(
            IntPredicate::EQ, left_tag, right_tag, "tags_equal",
        ).unwrap();

        let same_type_block = self.context.append_basic_block(self.main_fn, "cmp_same_type");
        let diff_type_block = self.context.append_basic_block(self.main_fn, "cmp_diff_type");
        let merge_block = self.context.append_basic_block(self.main_fn, "cmp_merge");

        self.builder.build_conditional_branch(tags_equal, same_type_block, diff_type_block).unwrap();

        self.builder.position_at_end(diff_type_block);
        let diff_result = match op {
            BinaryOp::Equal => self.context.bool_type().const_int(0, false),
            BinaryOp::NotEqual => self.context.bool_type().const_int(1, false),
            _ => unreachable!(),
        };
        self.builder.build_unconditional_branch(merge_block).unwrap();
        let diff_block = self.builder.get_insert_block().unwrap();

        self.builder.position_at_end(same_type_block);
        let is_number = self.builder.build_int_compare(
            IntPredicate::EQ, left_tag,
            tag_type.const_int(TAG_NUMBER as u64, false), "is_number",
        ).unwrap();
        let number_block = self.context.append_basic_block(self.main_fn, "cmp_number");
        let non_number_block = self.context.append_basic_block(self.main_fn, "cmp_non_number");
        self.builder.build_conditional_branch(is_number, number_block, non_number_block).unwrap();

        self.builder.position_at_end(non_number_block);
        let is_string = self.builder.build_int_compare(
            IntPredicate::EQ, left_tag,
            tag_type.const_int(TAG_STRING as u64, false), "is_string",
        ).unwrap();
        let string_block = self.context.append_basic_block(self.main_fn, "cmp_string");
        let non_string_block = self.context.append_basic_block(self.main_fn, "cmp_non_string");
        self.builder.build_conditional_branch(is_string, string_block, non_string_block).unwrap();

        self.builder.position_at_end(non_string_block);
        let is_bool = self.builder.build_int_compare(
            IntPredicate::EQ, left_tag,
            tag_type.const_int(TAG_BOOLEAN as u64, false), "is_bool",
        ).unwrap();
        let bool_block = self.context.append_basic_block(self.main_fn, "cmp_bool");
        let non_bool_block = self.context.append_basic_block(self.main_fn, "cmp_non_bool");
        self.builder.build_conditional_branch(is_bool, bool_block, non_bool_block).unwrap();

        // non_bool: check null
        self.builder.position_at_end(non_bool_block);
        let is_null = self.builder.build_int_compare(
            IntPredicate::EQ, left_tag,
            tag_type.const_int(TAG_NULL as u64, false), "is_null",
        ).unwrap();
        let null_block = self.context.append_basic_block(self.main_fn, "cmp_null");
        let array_check_block = self.context.append_basic_block(self.main_fn, "cmp_array_check");
        self.builder.build_conditional_branch(is_null, null_block, array_check_block).unwrap();

        // Number comparison
        self.builder.position_at_end(number_block);
        let left_num = self.builder.build_extract_value(left, 1, "left_num")
            .unwrap().into_float_value();
        let right_num = self.builder.build_extract_value(right, 1, "right_num")
            .unwrap().into_float_value();
        let num_cmp = match op {
            BinaryOp::Equal => self.builder.build_float_compare(
                FloatPredicate::OEQ, left_num, right_num, "num_eq",
            ).unwrap(),
            BinaryOp::NotEqual => self.builder.build_float_compare(
                FloatPredicate::ONE, left_num, right_num, "num_ne",
            ).unwrap(),
            _ => unreachable!(),
        };
        self.builder.build_unconditional_branch(merge_block).unwrap();
        let number_block_end = self.builder.get_insert_block().unwrap();

        // String comparison
        self.builder.position_at_end(string_block);
        let left_str = self.builder.build_extract_value(left, 2, "left_str")
            .unwrap().into_pointer_value();
        let right_str = self.builder.build_extract_value(right, 2, "right_str")
            .unwrap().into_pointer_value();
        let strcmp_call = self.builder.build_call(
            self.strcmp_fn, &[left_str.into(), right_str.into()], "strcmp",
        ).unwrap();
        let strcmp_val = strcmp_call.try_as_basic_value().left()
            .ok_or_else(|| "Internal error: strcmp returned no value".to_string())?
            .into_int_value();
        let zero = self.context.i32_type().const_int(0, false);
        let str_cmp = match op {
            BinaryOp::Equal => self.builder.build_int_compare(
                IntPredicate::EQ, strcmp_val, zero, "str_eq",
            ).unwrap(),
            BinaryOp::NotEqual => self.builder.build_int_compare(
                IntPredicate::NE, strcmp_val, zero, "str_ne",
            ).unwrap(),
            _ => unreachable!(),
        };
        self.builder.build_unconditional_branch(merge_block).unwrap();
        let string_block_end = self.builder.get_insert_block().unwrap();

        // Bool comparison
        self.builder.position_at_end(bool_block);
        let left_bool = self.builder.build_extract_value(left, 3, "left_bool")
            .unwrap().into_int_value();
        let right_bool = self.builder.build_extract_value(right, 3, "right_bool")
            .unwrap().into_int_value();
        let bool_cmp = match op {
            BinaryOp::Equal => self.builder.build_int_compare(
                IntPredicate::EQ, left_bool, right_bool, "bool_eq",
            ).unwrap(),
            BinaryOp::NotEqual => self.builder.build_int_compare(
                IntPredicate::NE, left_bool, right_bool, "bool_ne",
            ).unwrap(),
            _ => unreachable!(),
        };
        self.builder.build_unconditional_branch(merge_block).unwrap();
        let bool_block_end = self.builder.get_insert_block().unwrap();

        // Null comparison: Null == Null is true, Null != Null is false
        self.builder.position_at_end(null_block);
        let null_cmp = match op {
            BinaryOp::Equal => self.context.bool_type().const_int(1, false),
            BinaryOp::NotEqual => self.context.bool_type().const_int(0, false),
            _ => unreachable!(),
        };
        self.builder.build_unconditional_branch(merge_block).unwrap();
        let null_block_end = self.builder.get_insert_block().unwrap();

        // Array check: separate from object comparison.
        self.builder.position_at_end(array_check_block);
        let is_array = self.builder.build_int_compare(
            IntPredicate::EQ, left_tag,
            tag_type.const_int(TAG_ARRAY as u64, false), "is_array",
        ).unwrap();
        let array_block = self.context.append_basic_block(self.main_fn, "cmp_array");
        let object_block = self.context.append_basic_block(self.main_fn, "cmp_object");
        self.builder.build_conditional_branch(is_array, array_block, object_block).unwrap();

        // Array comparison: use values_equal function.
        self.builder.position_at_end(array_block);
        let arr_left_alloca = self.create_entry_alloca("cmp_arr_lhs");
        self.builder.build_store(arr_left_alloca, left).unwrap();
        let arr_right_alloca = self.create_entry_alloca("cmp_arr_rhs");
        self.builder.build_store(arr_right_alloca, right).unwrap();
        let arr_eq_call = self.builder.build_call(
            self.values_equal_fn,
            &[arr_left_alloca.into(), arr_right_alloca.into()],
            "arr_eq",
        ).unwrap();
        let arr_eq_val = arr_eq_call.try_as_basic_value().left()
            .ok_or_else(|| "values_equal returned no value".to_string())?
            .into_int_value();
        let arr_result = match op {
            BinaryOp::Equal => arr_eq_val,
            BinaryOp::NotEqual => self.builder.build_not(arr_eq_val, "arr_ne").unwrap(),
            _ => unreachable!(),
        };
        self.builder.build_unconditional_branch(merge_block).unwrap();
        let array_block_end = self.builder.get_insert_block().unwrap();

        // Object comparison
        self.builder.position_at_end(object_block);
        let left_alloca = self.create_entry_alloca("cmp_lhs");
        self.builder.build_store(left_alloca, left).unwrap();
        let right_alloca = self.create_entry_alloca("cmp_rhs");
        self.builder.build_store(right_alloca, right).unwrap();
        let obj_eq_call = self.builder.build_call(
            self.values_equal_fn,
            &[left_alloca.into(), right_alloca.into()],
            "obj_eq",
        ).unwrap();
        let obj_eq_val = obj_eq_call.try_as_basic_value().left()
            .ok_or_else(|| "values_equal returned no value".to_string())?
            .into_int_value();
        let obj_result = match op {
            BinaryOp::Equal => obj_eq_val,
            BinaryOp::NotEqual => self.builder.build_not(obj_eq_val, "obj_ne").unwrap(),
            _ => unreachable!(),
        };
        self.builder.build_unconditional_branch(merge_block).unwrap();
        let object_block_end = self.builder.get_insert_block().unwrap();

        self.builder.position_at_end(merge_block);
        let phi = self.builder.build_phi(self.context.bool_type(), "cmp_phi").unwrap();
        phi.add_incoming(&[
            (&diff_result, diff_block),
            (&num_cmp, number_block_end),
            (&str_cmp, string_block_end),
            (&bool_cmp, bool_block_end),
            (&null_cmp, null_block_end),
            (&arr_result, array_block_end),
            (&obj_result, object_block_end),
        ]);

        Ok(self.build_boolean(phi.as_basic_value().into_int_value()))
    }

    fn const_number(&self, n: f64) -> StructValue<'ctx> {
        let tag = self.context.i8_type().const_int(TAG_NUMBER as u64, false);
        let num = self.context.f64_type().const_float(n);
        let str_ptr = self.context.i8_type().ptr_type(AddressSpace::default()).const_null();
        let bool_val = self.context.bool_type().const_int(0, false);
        self.build_value(tag, num, str_ptr, bool_val)
    }

    fn const_string(&mut self, s: &str) -> StructValue<'ctx> {
        let name = format!("str_{}", self.string_count);
        self.string_count += 1;
        let global = self.builder.build_global_string_ptr(s, &name).unwrap();
        let tag = self.context.i8_type().const_int(TAG_STRING as u64, false);
        let num = self.context.f64_type().const_float(0.0);
        let bool_val = self.context.bool_type().const_int(0, false);
        self.build_value(tag, num, global.as_pointer_value(), bool_val)
    }

    fn const_null(&self) -> StructValue<'ctx> {
        let tag = self.context.i8_type().const_int(TAG_NULL as u64, false);
        let num = self.context.f64_type().const_float(0.0);
        let str_ptr = self.context.i8_type().ptr_type(AddressSpace::default()).const_null();
        let bool_val = self.context.bool_type().const_int(0, false);
        self.build_value(tag, num, str_ptr, bool_val)
    }

    fn build_boolean(&self, val: inkwell::values::IntValue<'ctx>) -> StructValue<'ctx> {
        let tag = self.context.i8_type().const_int(TAG_BOOLEAN as u64, false);
        let num = self.context.f64_type().const_float(0.0);
        let str_ptr = self.context.i8_type().ptr_type(AddressSpace::default()).const_null();
        self.build_value(tag, num, str_ptr, val)
    }

    fn build_value(
        &self,
        tag: inkwell::values::IntValue<'ctx>,
        num: inkwell::values::FloatValue<'ctx>,
        str_ptr: PointerValue<'ctx>,
        bool_val: inkwell::values::IntValue<'ctx>,
    ) -> StructValue<'ctx> {
        let mut value = self.value_type.get_undef();
        value = self.builder.build_insert_value(value, tag, 0, "val_tag")
            .unwrap().into_struct_value();
        value = self.builder.build_insert_value(value, num, 1, "val_num")
            .unwrap().into_struct_value();
        value = self.builder.build_insert_value(value, str_ptr, 2, "val_str")
            .unwrap().into_struct_value();
        value = self.builder.build_insert_value(value, bool_val, 3, "val_bool")
            .unwrap().into_struct_value();
        value
    }

    fn create_entry_alloca(&self, name: &str) -> PointerValue<'ctx> {
        let builder = self.context.create_builder();
        let entry = self.main_fn.get_first_basic_block().unwrap();
        match entry.get_first_instruction() {
            Some(inst) => builder.position_before(&inst),
            None => builder.position_at_end(entry),
        }
        builder.build_alloca(self.value_type, name).unwrap()
    }

    fn emit_trap(&self) {
        self.builder.build_call(self.abort_fn, &[], "trap").unwrap();
        self.builder.build_unreachable().unwrap();
    }

    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
        self.type_annotations.push(HashMap::new());
        self.type_registry.push(HashMap::new());
        self.handler_registry.push(HashMap::new());
        self.type_alias_registry.push(HashMap::new());
        self.native_handler_ptrs.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
        self.type_annotations.pop();
        self.type_registry.pop();
        self.handler_registry.pop();
        self.type_alias_registry.pop();
        self.native_handler_ptrs.pop();
    }

    fn current_scope_has(&self, name: &str) -> bool {
        self.scopes.last().map(|scope| scope.contains_key(name)).unwrap_or(false)
    }

    fn exists_in_any_scope(&self, name: &str) -> bool {
        self.scopes.iter().any(|scope| scope.contains_key(name))
    }

    fn get_var_ptr(&self, name: &str) -> Option<PointerValue<'ctx>> {
        for scope in self.scopes.iter().rev() {
            if let Some(ptr) = scope.get(name) {
                return Some(*ptr);
            }
        }
        None
    }

    fn set_type_annotation(&mut self, name: String, type_expr: TypeExpr) {
        self.type_annotations.last_mut().expect("No active scope").insert(name, type_expr);
    }

    fn get_type_annotation(&self, name: &str) -> Option<&TypeExpr> {
        for scope in self.type_annotations.iter().rev() {
            if let Some(t) = scope.get(name) {
                return Some(t);
            }
        }
        None
    }

    fn define_type(&mut self, name: String, fields: Vec<FieldConstraint>) {
        let tuples: Vec<(String, TypeExpr, bool)> = fields.iter().map(|fc| fc.to_type_tuple()).collect();
        self.type_registry.last_mut().expect("No active scope").insert(name, tuples);
    }

    fn get_type_def(&self, name: &str) -> Option<&Vec<(String, TypeExpr, bool)>> {
        for scope in self.type_registry.iter().rev() {
            if let Some(fields) = scope.get(name) {
                return Some(fields);
            }
        }
        None
    }

    fn _define_type_alias(&mut self, name: String, type_expr: TypeExpr) {
        self.type_alias_registry.last_mut().expect("No active scope").insert(name, type_expr);
    }

    fn get_type_alias(&self, name: &str) -> Option<&TypeExpr> {
        for scope in self.type_alias_registry.iter().rev() {
            if let Some(te) = scope.get(name) {
                return Some(te);
            }
        }
        None
    }

    /// Check if an inferred type name matches a TypeExpr at compile time.
    fn inferred_matches_type_expr(&self, inferred: &str, type_expr: &TypeExpr) -> bool {
        match type_expr {
            TypeExpr::Named(name) => {
                if name == "Any" || name == inferred {
                    return true;
                }
                // Check if it's an alias and resolve.
                if let Some(alias_expr) = self.get_type_alias(name) {
                    let alias_expr = alias_expr.clone();
                    return self.inferred_matches_type_expr(inferred, &alias_expr);
                }
                // For particle types, inferred is "Object" and the type_expr is a class name.
                if inferred == "Object" {
                    return true; // can't statically distinguish at compile time
                }
                false
            }
            TypeExpr::Literal(_) => {
                // Literal types only match string values at runtime; compile-time check:
                // if inferred is "String", it *might* match (defer to runtime).
                inferred == "String"
            }
            TypeExpr::Union(variants) => {
                variants.iter().any(|v| self.inferred_matches_type_expr(inferred, v))
            }
            TypeExpr::Intersection(variants) => {
                variants.iter().all(|v| self.inferred_matches_type_expr(inferred, v))
            }
        }
    }

    fn define_handler(&mut self, class_name: String, body: Vec<Spanned<Statement>>) {
        self.handler_registry.last_mut().expect("No active scope").insert(class_name, body);
    }

    fn get_handler(&self, class_name: &str) -> Option<&Vec<Spanned<Statement>>> {
        for scope in self.handler_registry.iter().rev() {
            if let Some(body) = scope.get(class_name) {
                return Some(body);
            }
        }
        None
    }

    /// Allocate an `i8*`-typed slot in the function entry block (same pattern
    /// as `create_entry_alloca` but for pointer types instead of value_type).
    fn create_entry_ptr_alloca(&self, name: &str) -> PointerValue<'ctx> {
        let builder = self.context.create_builder();
        let entry = self.main_fn.get_first_basic_block().unwrap();
        match entry.get_first_instruction() {
            Some(inst) => builder.position_before(&inst),
            None => builder.position_at_end(entry),
        }
        let i8_ptr = self.context.i8_type().ptr_type(AddressSpace::default());
        builder.build_alloca(i8_ptr, name).unwrap()
    }

    /// Compile the end-of-program keep-alive drain loop (compiled path).
    ///
    /// Emitted after all program statements when native emissions are present.
    /// Checks `__native_bridge_is_keep_alive()`: if false the program exits
    /// normally; if true it enters an infinite loop that drains the emission
    /// queue and dispatches each particle to the appropriate Code handler.
    /// Used when a native organelle emits `__KeepAlive` to signal that a
    /// background thread is running (e.g. the HTTP server organelle).
    fn compile_native_drain_loop(&mut self) -> Result<(), String> {
        let ka_fn = match self.native_bridge_is_keep_alive_fn {
            Some(f) => f,
            None => return Ok(()),
        };
        let poll_fn = match self.native_bridge_poll_emission_fn {
            Some(f) => f,
            None => return Ok(()),
        };

        let i32_type = self.context.i32_type();
        let i8_ptr   = self.context.i8_type().ptr_type(AddressSpace::default());

        // Check keep-alive flag.
        let ka_raw = self.builder.build_call(ka_fn, &[], "ka_flag")
            .unwrap().try_as_basic_value().left().unwrap().into_int_value();
        let is_alive = self.builder.build_int_compare(
            IntPredicate::NE, ka_raw, i32_type.const_int(0, false), "is_alive",
        ).unwrap();

        let drain_block = self.context.append_basic_block(self.main_fn, "ka_drain");
        let exit_block  = self.context.append_basic_block(self.main_fn, "ka_exit");
        self.builder.build_conditional_branch(is_alive, drain_block, exit_block).unwrap();

        // Drain loop: poll → dispatch → repeat.
        self.builder.position_at_end(drain_block);

        let out_particle   = self.create_entry_alloca("ka_particle");
        let out_class_pptr = self.create_entry_ptr_alloca("ka_cls_ptr");

        self.builder.build_store(out_class_pptr, i8_ptr.const_null()).unwrap();

        let got_raw = self.builder.build_call(
            poll_fn,
            &[out_particle.into(), out_class_pptr.into()],
            "ka_got",
        ).unwrap().try_as_basic_value().left().unwrap().into_int_value();

        let got_true = self.builder.build_int_compare(
            IntPredicate::NE, got_raw, i32_type.const_int(0, false), "ka_got_t",
        ).unwrap();

        let dispatch_block = self.context.append_basic_block(self.main_fn, "ka_dispatch");
        // If nothing, loop back to poll again (poll blocks 50 ms internally).
        self.builder.build_conditional_branch(got_true, dispatch_block, drain_block).unwrap();

        // Dispatch block: match _class and invoke handler.
        self.builder.position_at_end(dispatch_block);
        let class_str = self.builder.build_load(i8_ptr, out_class_pptr, "ka_cls")
            .unwrap().into_pointer_value();
        let particle_val = self.builder.build_load(self.value_type, out_particle, "ka_pv")
            .unwrap().into_struct_value();

        let emission_classes = self.emission_handler_classes.clone();

        // Also include gene-defined handlers (from .gene.code files) that have
        // no dot in their name (i.e., not aliased module handlers like "server.Respond").
        // This covers typed particles emitted directly by native organelles (e.g.,
        // FetchTodos, FetchTags from the server organelle) that weren't pre-declared
        // in the organelle's emissions manifest but DO have gene-level handlers.
        let mut all_dispatch_classes = emission_classes.clone();
        let gene_handler_classes: Vec<String> = self
            .handler_registry
            .iter()
            .flat_map(|scope| scope.keys().cloned())
            .filter(|name| !name.contains('.') && !all_dispatch_classes.contains(name))
            .collect();
        all_dispatch_classes.extend(gene_handler_classes);

        for (i, class_name) in all_dispatch_classes.iter().enumerate() {
            let class_global = self.builder.build_global_string_ptr(
                class_name, &format!("ka_cls_{}", i),
            ).unwrap();

            let cmp = self.builder.build_call(
                self.strcmp_fn,
                &[class_str.into(), class_global.as_pointer_value().into()],
                &format!("ka_cmp_{}", i),
            ).unwrap().try_as_basic_value().left().unwrap().into_int_value();

            let is_match = self.builder.build_int_compare(
                IntPredicate::EQ, cmp, i32_type.const_int(0, false),
                &format!("ka_match_{}", i),
            ).unwrap();

            let match_block    = self.context.append_basic_block(self.main_fn, &format!("ka_h_{}", class_name));
            let no_match_block = self.context.append_basic_block(self.main_fn, &format!("ka_nm_{}", i));

            self.builder.build_conditional_branch(is_match, match_block, no_match_block).unwrap();

            self.builder.position_at_end(match_block);
            self.compile_handler_invoke_with_val(particle_val, class_name, &HandlerTarget::This)?;
            if self.builder.get_insert_block().unwrap().get_terminator().is_none() {
                self.builder.build_unconditional_branch(drain_block).unwrap();
            }

            self.builder.position_at_end(no_match_block);
        }

        // Unknown class — loop back.
        if self.builder.get_insert_block().unwrap().get_terminator().is_none() {
            self.builder.build_unconditional_branch(drain_block).unwrap();
        }

        self.builder.position_at_end(exit_block);
        Ok(())
    }

    /// Compile an infinite loop: `loop { ... }`.
    ///
    /// If the program has native module emissions, an inline emission drain
    /// step is appended at the end of each iteration.  This replaces the
    /// previous standalone drain loop and `__KeepAlive` mechanism.
    fn compile_loop_infinite(&mut self, result: Option<&str>, body: &[Spanned<Statement>]) -> Result<(), String> {
        let i8_type = self.context.i8_type();
        let i32_type = self.context.i32_type();
        let i64_type = self.context.i64_type();

        let loop_header = self.context.append_basic_block(self.main_fn, "loop_inf_header");
        let loop_body   = self.context.append_basic_block(self.main_fn, "loop_inf_body");
        let loop_end    = self.context.append_basic_block(self.main_fn, "loop_inf_end");

        // If `get` is present, allocate yield collector with an initial capacity.
        let yield_allocas = if result.is_some() {
            let initial_cap = 16u64;
            let elem_size = self.value_type.size_of().unwrap();
            let total_size = self.builder.build_int_mul(
                elem_size,
                i64_type.const_int(initial_cap, false),
                "linf_alloc",
            ).unwrap();
            let dst_ptr = self.builder.build_call(
                self.malloc_fn, &[total_size.into()], "linf_mem",
            ).unwrap()
                .try_as_basic_value().left()
                .ok_or_else(|| "malloc returned no value".to_string())?
                .into_pointer_value();

            let yield_count_alloca = self.create_entry_alloca("__yield_count");
            self.builder.build_store(yield_count_alloca, i32_type.const_int(0, false)).unwrap();
            let yield_arr_alloca = self.create_entry_alloca("__yield_arr");
            self.builder.build_store(yield_arr_alloca, dst_ptr).unwrap();

            let prev_arr = self.yield_arr_ptr.take();
            let prev_count = self.yield_count_ptr.take();
            self.yield_arr_ptr = Some(yield_arr_alloca);
            self.yield_count_ptr = Some(yield_count_alloca);
            Some((yield_count_alloca, dst_ptr, prev_arr, prev_count))
        } else {
            None
        };

        self.builder.build_unconditional_branch(loop_header).unwrap();

        self.builder.position_at_end(loop_header);
        self.builder.build_unconditional_branch(loop_body).unwrap();

        self.builder.position_at_end(loop_body);

        // Save and set break context.
        let prev_break = self.break_exit_block.take();
        self.break_exit_block = Some(loop_end);

        self.push_scope();

        for stmt in body {
            self.compile_statement(&stmt.node)?;
        }

        self.pop_scope();

        // Restore break context.
        self.break_exit_block = prev_break;

        // If we have native emissions, drain the C queue after each iteration.
        if self.has_native_emissions {
            self.compile_emission_drain_step(loop_header)?;
        } else {
            // No emissions — just loop back.
            if self.builder.get_insert_block().unwrap().get_terminator().is_none() {
                self.builder.build_unconditional_branch(loop_header).unwrap();
            }
        }

        self.builder.position_at_end(loop_end);

        if let (Some(result_name), Some((yield_count_alloca, dst_ptr, prev_arr, prev_count))) = (result, yield_allocas) {
            self.yield_arr_ptr = prev_arr;
            self.yield_count_ptr = prev_count;

            let final_count = self.builder.build_load(i32_type, yield_count_alloca, "linf_final_count")
                .unwrap().into_int_value();
            let result_count_f = self.builder.build_unsigned_int_to_float(
                final_count, self.context.f64_type(), "linf_result_count",
            ).unwrap();
            let result_tag = i8_type.const_int(TAG_ARRAY as u64, false);
            let bool_val = self.context.bool_type().const_int(0, false);
            let result_val = self.build_value(result_tag, result_count_f, dst_ptr, bool_val);

            let result_ptr = self.create_entry_alloca(result_name);
            self.builder.build_store(result_ptr, result_val).unwrap();
            self.scopes
                .last_mut()
                .expect("No active scope")
                .insert(result_name.to_string(), result_ptr);
        }

        Ok(())
    }

    /// Emit an inline drain step: poll all pending emissions from the C queue
    /// and dispatch each to the appropriate Code-level handler.
    /// After draining, branches back to `loop_back_block`.
    fn compile_emission_drain_step(
        &mut self,
        loop_back_block: inkwell::basic_block::BasicBlock<'ctx>,
    ) -> Result<(), String> {
        // If the body already terminated (e.g. break), skip.
        if self.builder.get_insert_block().unwrap().get_terminator().is_some() {
            return Ok(());
        }

        let poll_fn = match self.native_bridge_poll_emission_fn {
            Some(f) => f,
            None => {
                self.builder.build_unconditional_branch(loop_back_block).unwrap();
                return Ok(());
            }
        };

        let i32_type = self.context.i32_type();
        let i8_ptr   = self.context.i8_type().ptr_type(AddressSpace::default());

        let out_particle   = self.create_entry_alloca("ed_particle");
        let out_class_pptr = self.create_entry_ptr_alloca("ed_cls_ptr");

        // Drain loop: poll until queue is empty, then continue to main loop header.
        let drain_poll = self.context.append_basic_block(self.main_fn, "ed_poll");
        self.builder.build_unconditional_branch(drain_poll).unwrap();

        self.builder.position_at_end(drain_poll);
        self.builder.build_store(out_class_pptr, i8_ptr.const_null()).unwrap();

        let got_raw = self.builder.build_call(
            poll_fn,
            &[out_particle.into(), out_class_pptr.into()],
            "ed_got",
        ).unwrap().try_as_basic_value().left().unwrap().into_int_value();

        let got_true = self.builder.build_int_compare(
            IntPredicate::NE, got_raw, i32_type.const_int(0, false), "ed_got_t",
        ).unwrap();

        let dispatch_block = self.context.append_basic_block(self.main_fn, "ed_dispatch");
        // If nothing in queue, go back to main loop header.
        self.builder.build_conditional_branch(got_true, dispatch_block, loop_back_block).unwrap();

        // Dispatch block: class comparison chain.
        self.builder.position_at_end(dispatch_block);
        let class_str = self.builder.build_load(i8_ptr, out_class_pptr, "ed_cls")
            .unwrap().into_pointer_value();
        let particle_val = self.builder.build_load(self.value_type, out_particle, "ed_pv")
            .unwrap().into_struct_value();

        let emission_classes = self.emission_handler_classes.clone();
        let mut all_dispatch_classes = emission_classes.clone();
        let gene_handler_classes: Vec<String> = self
            .handler_registry
            .iter()
            .flat_map(|scope| scope.keys().cloned())
            .filter(|name| !name.contains('.') && !all_dispatch_classes.contains(name))
            .collect();
        all_dispatch_classes.extend(gene_handler_classes);

        for (i, class_name) in all_dispatch_classes.iter().enumerate() {
            let class_global = self.builder.build_global_string_ptr(
                class_name, &format!("ed_cls_{}", i),
            ).unwrap();

            let cmp = self.builder.build_call(
                self.strcmp_fn,
                &[class_str.into(), class_global.as_pointer_value().into()],
                &format!("ed_cmp_{}", i),
            ).unwrap().try_as_basic_value().left().unwrap().into_int_value();

            let is_match = self.builder.build_int_compare(
                IntPredicate::EQ, cmp, i32_type.const_int(0, false),
                &format!("ed_match_{}", i),
            ).unwrap();

            let match_block    = self.context.append_basic_block(self.main_fn, &format!("ed_h_{}", class_name));
            let no_match_block = self.context.append_basic_block(self.main_fn, &format!("ed_nm_{}", i));

            self.builder.build_conditional_branch(is_match, match_block, no_match_block).unwrap();

            // Match: invoke the handler, then poll for more.
            self.builder.position_at_end(match_block);
            self.compile_handler_invoke_with_val(particle_val, class_name, &HandlerTarget::This)?;
            if self.builder.get_insert_block().unwrap().get_terminator().is_none() {
                self.builder.build_unconditional_branch(drain_poll).unwrap();
            }

            // No match: try next class.
            self.builder.position_at_end(no_match_block);
        }

        // Fell through all comparisons — unknown emission, skip. Poll for more.
        if self.builder.get_insert_block().unwrap().get_terminator().is_none() {
            self.builder.build_unconditional_branch(drain_poll).unwrap();
        }

        Ok(())
    }

    /// Variant of `compile_handler_invoke` that accepts a pre-loaded particle
    /// `StructValue` and a compile-time class name string.  Used by the
    /// emission drain step where the particle comes from the C queue.
    fn compile_handler_invoke_with_val(
        &mut self,
        particle_val: inkwell::values::StructValue<'ctx>,
        class_name: &str,
        target: &HandlerTarget,
    ) -> Result<(), String> {
        // Look up the handler body.
        let handler_bodies: Vec<Vec<Spanned<Statement>>> = match target {
            HandlerTarget::This => self
                .get_handler(class_name)
                .cloned()
                .into_iter()
                .collect(),
            HandlerTarget::ModuleAlias(alias) => {
                let key = format!("{}.{}", alias, class_name);
                self.get_handler(&key).cloned().into_iter().collect()
            }
            HandlerTarget::Base => self
                .handler_registry
                .iter()
                .rev()
                .skip(1)
                .filter_map(|scope| scope.get(class_name).cloned())
                .collect(),
        };

        if handler_bodies.is_empty() {
            return Ok(());
        }

        // Determine field names from the type registry.
        let field_names: Vec<String> = if let Some(schema) = self.get_type_def(class_name) {
            schema.iter()
                .map(|(n, _, _)| n.clone())
                .filter(|n| n != "_class" && n != "_created")
                .collect()
        } else {
            Vec::new()
        };

        let i32_type  = self.context.i32_type();
        let i8_type   = self.context.i8_type();
        let i8_ptr    = i8_type.ptr_type(AddressSpace::default());

        for handler_body in handler_bodies {
            let ret_alloca = self.create_entry_alloca(&format!("dh_ret_{}", class_name));
            self.builder.build_store(ret_alloca, self.const_null()).unwrap();

            let exit_block = self.context.append_basic_block(
                self.main_fn, &format!("dh_exit_{}", class_name),
            );

            let prev_alloca = self.handler_return_alloca.take();
            let prev_exit   = self.handler_exit_block.take();
            self.handler_return_alloca = Some(ret_alloca);
            self.handler_exit_block    = Some(exit_block);
            self.in_handler_depth     += 1;

            self.push_scope();

            // Bind particle fields from particle_val into local scope.
            let count_f = self.builder.build_extract_value(particle_val, 1, "dh_cnt_f")
                .unwrap().into_float_value();
            let count = self.builder.build_float_to_unsigned_int(count_f, i32_type, "dh_cnt")
                .unwrap();
            let arr_ptr = self.builder.build_extract_value(particle_val, 2, "dh_arr")
                .unwrap().into_pointer_value();

            for field_name in &field_names {
                let target_name_g = self.builder.build_global_string_ptr(
                    field_name, &format!("dh_fn_{}", self.string_count),
                ).unwrap();
                self.string_count += 1;

                let pre_block = self.builder.get_insert_block().unwrap();
                let lhdr  = self.context.append_basic_block(self.main_fn, "dh_lhdr");
                let lbody = self.context.append_basic_block(self.main_fn, "dh_lbody");
                let lfound= self.context.append_basic_block(self.main_fn, "dh_lfound");
                let lnext = self.context.append_basic_block(self.main_fn, "dh_lnext");
                let lnf   = self.context.append_basic_block(self.main_fn, "dh_lnf");
                let lcont = self.context.append_basic_block(self.main_fn, "dh_lcont");

                self.builder.build_unconditional_branch(lhdr).unwrap();

                self.builder.position_at_end(lhdr);
                let i_phi = self.builder.build_phi(i32_type, "dh_i").unwrap();
                let zero  = i32_type.const_int(0, false);
                i_phi.add_incoming(&[(&zero, pre_block)]);
                let i_val = i_phi.as_basic_value().into_int_value();
                let done  = self.builder.build_int_compare(IntPredicate::UGE, i_val, count, "dh_done").unwrap();
                self.builder.build_conditional_branch(done, lnf, lbody).unwrap();

                self.builder.position_at_end(lbody);
                let fptr  = unsafe { self.builder.build_in_bounds_gep(self.field_type, arr_ptr, &[i_val], "dh_fptr") }.unwrap();
                let nslot = self.builder.build_struct_gep(self.field_type, fptr, 0, "dh_nslot").unwrap();
                let nval  = self.builder.build_load(i8_ptr, nslot, "dh_nval").unwrap().into_pointer_value();
                let cmp   = self.builder.build_call(
                    self.strcmp_fn, &[nval.into(), target_name_g.as_pointer_value().into()], "dh_cmp",
                ).unwrap().try_as_basic_value().left().unwrap().into_int_value();
                let mch   = self.builder.build_int_compare(IntPredicate::EQ, cmp, zero, "dh_mch").unwrap();
                self.builder.build_conditional_branch(mch, lfound, lnext).unwrap();

                self.builder.position_at_end(lnext);
                let inext = self.builder.build_int_add(i_val, i32_type.const_int(1, false), "dh_inext").unwrap();
                i_phi.add_incoming(&[(&inext, lnext)]);
                self.builder.build_unconditional_branch(lhdr).unwrap();

                self.builder.position_at_end(lfound);
                let vslot    = self.builder.build_struct_gep(self.field_type, fptr, 1, "dh_vslot").unwrap();
                let loaded_v = self.builder.build_load(self.value_type, vslot, "dh_vval").unwrap();
                self.builder.build_unconditional_branch(lcont).unwrap();

                self.builder.position_at_end(lnf);
                let default_v = self.const_null();
                self.builder.build_unconditional_branch(lcont).unwrap();

                self.builder.position_at_end(lcont);
                let phi = self.builder.build_phi(self.value_type, &format!("dh_{}_v", field_name)).unwrap();
                phi.add_incoming(&[(&loaded_v, lfound), (&default_v, lnf)]);

                let slot = self.create_entry_alloca(field_name);
                self.builder.build_store(slot, phi.as_basic_value()).unwrap();
                self.scopes.last_mut().expect("scope").insert(field_name.clone(), slot);
            }

            // Execute handler body.
            for stmt in &handler_body {
                // Prevent handler from mutating outer-scope variables.
                if let Statement::Constraint { variable, constraint: ConstraintExpr::Equals(_), .. } = &stmt.node {
                    if self.exists_in_any_scope(variable) && !self.current_scope_has(variable) {
                        self.pop_scope();
                        return Err(format!(
                            "Cannot redefine '{}' inside handler: shadowing is not allowed",
                            variable
                        ));
                    }
                }
                self.compile_statement(&stmt.node)?;
            }

            // Fall-through to exit.
            if self.builder.get_insert_block().unwrap().get_terminator().is_none() {
                self.builder.build_unconditional_branch(exit_block).unwrap();
            }

            self.builder.position_at_end(exit_block);
            self.in_handler_depth     -= 1;
            self.handler_return_alloca = prev_alloca;
            self.handler_exit_block    = prev_exit;
            self.pop_scope();
        }

        Ok(())
    }

    /// Compile a handler invocation. Returns the handler's return value.
    fn compile_handler_invoke(
        &mut self,
        particle: &Expression,
        target: &HandlerTarget,
    ) -> Result<BasicValueEnum<'ctx>, String> {
        let particle_val = self.compile_expr(particle)?.into_struct_value();

        let class_name = self.infer_particle_class(particle)?;

        let invocations: Vec<(String, Vec<Spanned<Statement>>)> = match target {
            HandlerTarget::This => self
                .get_handler(&class_name)
                .cloned()
                .map(|body| vec![(class_name.clone(), body)])
                .unwrap_or_default(),
            HandlerTarget::ModuleAlias(alias) => {
                let key = format!("{}.{}", alias, class_name);
                self.get_handler(&key)
                    .cloned()
                    .map(|body| vec![(key, body)])
                    .unwrap_or_default()
            }
            HandlerTarget::Base => self
                .handler_registry
                .iter()
                .rev()
                .skip(1)
                .filter_map(|scope| {
                    scope
                        .get(&class_name)
                        .cloned()
                        .map(|body| (class_name.clone(), body))
                })
                .collect(),
        };

        // Check for native handler pointer (from native module imports).
        let native_handler_ptr: Option<PointerValue<'ctx>> = match target {
            HandlerTarget::This => self.get_native_handler_ptr(&class_name),
            HandlerTarget::ModuleAlias(alias) => {
                let key = format!("{}.{}", alias, class_name);
                self.get_native_handler_ptr(&key)
            }
            HandlerTarget::Base => self
                .native_handler_ptrs
                .iter()
                .rev()
                .skip(1)
                .find_map(|scope| scope.get(&class_name).copied()),
        };

        if invocations.is_empty() && native_handler_ptr.is_none() {
            return Ok(self.const_null().into());
        }

        let mut last_ret: Option<BasicValueEnum<'ctx>> = None;

        for (handler_key, handler_body) in invocations {
            // Create return alloca initialized to Null.
            let ret_alloca = self.create_entry_alloca("handler_ret");
            self.builder.build_store(ret_alloca, self.const_null()).unwrap();

            // Create exit block.
            let exit_block = self.context.append_basic_block(self.main_fn, "handler_exit");

            // Save previous handler context.
            let prev_alloca = self.handler_return_alloca.take();
            let prev_exit = self.handler_exit_block.take();
            self.handler_return_alloca = Some(ret_alloca);
            self.handler_exit_block = Some(exit_block);
            self.in_handler_depth += 1;

            // Execute handler in a new scope with particle fields as locals.
            self.push_scope();

            let field_names = self.get_particle_field_names(particle, &handler_key);

            let i32_type = self.context.i32_type();
            let i8_type = self.context.i8_type();
            let i8_ptr_type = i8_type.ptr_type(AddressSpace::default());

            let count_f = self.builder.build_extract_value(particle_val, 1, "hndl_count_f")
                .unwrap().into_float_value();
            let count = self.builder.build_float_to_unsigned_int(
                count_f, i32_type, "hndl_count",
            ).unwrap();
            let arr_ptr = self.builder.build_extract_value(particle_val, 2, "hndl_arr")
                .unwrap().into_pointer_value();

            for field_name in &field_names {
                if field_name == "_class" || field_name == "_created" {
                    continue;
                }

                let target_name_global = self.builder.build_global_string_ptr(
                    field_name, &format!("hndl_fn_{}", self.string_count),
                ).unwrap();
                self.string_count += 1;

                let pre_loop_block = self.builder.get_insert_block().unwrap();

                let loop_header = self.context.append_basic_block(self.main_fn, "hndl_fld_hdr");
                let loop_body = self.context.append_basic_block(self.main_fn, "hndl_fld_body");
                let found = self.context.append_basic_block(self.main_fn, "hndl_fld_found");
                let loop_next = self.context.append_basic_block(self.main_fn, "hndl_fld_next");
                let not_found = self.context.append_basic_block(self.main_fn, "hndl_fld_nf");
                let cont = self.context.append_basic_block(self.main_fn, "hndl_fld_cont");

                self.builder.build_unconditional_branch(loop_header).unwrap();

                self.builder.position_at_end(loop_header);
                let i_phi = self.builder.build_phi(i32_type, "hndl_i").unwrap();
                let zero_i32 = i32_type.const_int(0, false);
                i_phi.add_incoming(&[(&zero_i32, pre_loop_block)]);
                let i_val = i_phi.as_basic_value().into_int_value();
                let done = self.builder.build_int_compare(
                    IntPredicate::UGE, i_val, count, "hndl_done",
                ).unwrap();
                self.builder.build_conditional_branch(done, not_found, loop_body).unwrap();

                self.builder.position_at_end(loop_body);
                let field_ptr = unsafe { self.builder.build_in_bounds_gep(
                    self.field_type, arr_ptr, &[i_val], "hndl_fptr",
                ) }.unwrap();
                let name_slot = self.builder.build_struct_gep(
                    self.field_type, field_ptr, 0, "hndl_nslot",
                ).unwrap();
                let name_val = self.builder.build_load(i8_ptr_type, name_slot, "hndl_name")
                    .unwrap().into_pointer_value();
                let cmp = self.builder.build_call(
                    self.strcmp_fn,
                    &[name_val.into(), target_name_global.as_pointer_value().into()],
                    "hndl_cmp",
                ).unwrap();
                let cmp_val = cmp.try_as_basic_value().left().unwrap().into_int_value();
                let is_match = self.builder.build_int_compare(
                    IntPredicate::EQ, cmp_val, i32_type.const_int(0, false), "hndl_match",
                ).unwrap();
                self.builder.build_conditional_branch(is_match, found, loop_next).unwrap();

                self.builder.position_at_end(loop_next);
                let i_next = self.builder.build_int_add(
                    i_val, i32_type.const_int(1, false), "hndl_inext",
                ).unwrap();
                i_phi.add_incoming(&[(&i_next, loop_next)]);
                self.builder.build_unconditional_branch(loop_header).unwrap();

                self.builder.position_at_end(found);
                let val_slot = self.builder.build_struct_gep(
                    self.field_type, field_ptr, 1, "hndl_vslot",
                ).unwrap();
                let loaded_val = self.builder.build_load(
                    self.value_type, val_slot, "hndl_val",
                ).unwrap();
                self.builder.build_unconditional_branch(cont).unwrap();

                // Not found: default to Null.
                self.builder.position_at_end(not_found);
                let default_val = self.const_null();
                self.builder.build_unconditional_branch(cont).unwrap();

                self.builder.position_at_end(cont);
                let val_phi = self.builder.build_phi(self.value_type, &format!("hndl_{}_val", field_name)).unwrap();
                val_phi.add_incoming(&[
                    (&loaded_val, found),
                    (&default_val, not_found),
                ]);

                let ptr = self.create_entry_alloca(field_name);
                self.builder.build_store(ptr, val_phi.as_basic_value()).unwrap();
                self.scopes
                    .last_mut()
                    .expect("No active scope")
                    .insert(field_name.clone(), ptr);
            }

            // Compile the handler body.
            for stmt in &handler_body {
                // Prevent handler from mutating outer-scope variables.
                if let Statement::Constraint { variable, constraint: ConstraintExpr::Equals(_), .. } = &stmt.node {
                    if self.exists_in_any_scope(variable) && !self.current_scope_has(variable) {
                        self.pop_scope();
                        return Err(format!(
                            "Cannot redefine '{}' inside handler: shadowing is not allowed",
                            variable
                        ));
                    }
                }
                self.compile_statement(&stmt.node)?;
            }

            // Fall-through: branch to exit (if not already terminated by a HandlerReturn).
            if self.builder.get_insert_block().unwrap().get_terminator().is_none() {
                self.builder.build_unconditional_branch(exit_block).unwrap();
            }

            // Position at exit block.
            self.builder.position_at_end(exit_block);

            // Restore previous handler context.
            self.in_handler_depth -= 1;
            self.handler_return_alloca = prev_alloca;
            self.handler_exit_block = prev_exit;

            // Load the return value.
            let ret_val = self.builder.build_load(self.value_type, ret_alloca, "handler_ret_val").unwrap();

            self.pop_scope();
            last_ret = Some(ret_val);
        }

        // Call native handler if present (via C bridge).
        if let Some(handler_ptr) = native_handler_ptr {
            let particle_alloca = self.create_entry_alloca("nh_particle");
            self.builder.build_store(particle_alloca, particle_val).unwrap();

            let result_alloca = self.create_entry_alloca("nh_result");
            self.builder.build_call(
                self.native_bridge_call_handler_fn.unwrap(),
                &[handler_ptr.into(), particle_alloca.into(), result_alloca.into()],
                "",
            ).unwrap();

            last_ret = Some(
                self.builder
                    .build_load(self.value_type, result_alloca, "nh_ret")
                    .unwrap(),
            );
        }

        Ok(last_ret.unwrap_or_else(|| self.const_null().into()))
    }

    /// Infer the particle class name from an expression (compile-time).
    fn infer_particle_class(&self, expr: &Expression) -> Result<String, String> {
        match expr {
            Expression::Particle { class_name, .. } => Ok(class_name.clone()),
            Expression::Identifier(name) => {
                Err(format!(
                    "Cannot statically determine particle class for identifier '{}' in handler invoke. \
                     Use a particle literal (e.g., ClassName{{...}} => target) for LLVM codegen.",
                    name
                ))
            }
            _ => Err("Cannot determine particle class for handler invoke".to_string()),
        }
    }

    /// Get field names for a particle (from expression or type definition).
    fn get_particle_field_names(&self, expr: &Expression, type_key: &str) -> Vec<String> {
        if let Expression::Particle { fields, .. } = expr {
            let mut names: Vec<String> = fields.iter().filter_map(|f| match f {
                ObjectField::Static(n, _) => Some(n.clone()),
                ObjectField::Computed(_, _) => None,
            }).collect();
            // Also include optional fields from the schema that aren't in the expression.
            if let Some(schema) = self.get_type_def(type_key) {
                for (sf_name, _, _) in schema {
                    if !names.contains(sf_name) {
                        names.push(sf_name.clone());
                    }
                }
            }
            return names;
        }
        if let Some(schema) = self.get_type_def(type_key) {
            return schema.iter().map(|(n, _, _)| n.clone()).collect();
        }
        Vec::new()
    }

    /// Evaluate a value as truthy/falsy (matches interpreter behaviour).
    /// Null → false, false → false, 0 → false, "" → false, everything else → true.
    fn compile_truthy(&mut self, val: inkwell::values::StructValue<'ctx>) -> inkwell::values::IntValue<'ctx> {
        let tag = self.builder.build_extract_value(val, 0, "truthy_tag")
            .unwrap().into_int_value();
        let i8_type = self.context.i8_type();

        // Null → false
        let is_null = self.builder.build_int_compare(
            IntPredicate::EQ, tag,
            i8_type.const_int(TAG_NULL as u64, false),
            "truthy_is_null",
        ).unwrap();

        let check_bool_bb = self.context.append_basic_block(self.main_fn, "truthy_check_bool");
        let null_bb = self.context.append_basic_block(self.main_fn, "truthy_null");
        let merge_bb = self.context.append_basic_block(self.main_fn, "truthy_merge");

        self.builder.build_conditional_branch(is_null, null_bb, check_bool_bb).unwrap();

        // Null → false
        self.builder.position_at_end(null_bb);
        let false_val = self.context.bool_type().const_int(0, false);
        self.builder.build_unconditional_branch(merge_bb).unwrap();

        // Check boolean
        self.builder.position_at_end(check_bool_bb);
        let is_bool = self.builder.build_int_compare(
            IntPredicate::EQ, tag,
            i8_type.const_int(TAG_BOOLEAN as u64, false),
            "truthy_is_bool",
        ).unwrap();
        let bool_bb = self.context.append_basic_block(self.main_fn, "truthy_bool");
        let other_bb = self.context.append_basic_block(self.main_fn, "truthy_other");
        self.builder.build_conditional_branch(is_bool, bool_bb, other_bb).unwrap();

        // Boolean → use bool field directly
        self.builder.position_at_end(bool_bb);
        let bool_val = self.builder.build_extract_value(val, 3, "truthy_bval")
            .unwrap().into_int_value();
        self.builder.build_unconditional_branch(merge_bb).unwrap();

        // Everything else (number, string, object, array) → true
        self.builder.position_at_end(other_bb);
        let true_val = self.context.bool_type().const_int(1, false);
        self.builder.build_unconditional_branch(merge_bb).unwrap();

        // Merge
        self.builder.position_at_end(merge_bb);
        let phi = self.builder.build_phi(self.context.bool_type(), "truthy_result").unwrap();
        phi.add_incoming(&[
            (&false_val, null_bb),
            (&bool_val, bool_bb),
            (&true_val, other_bb),
        ]);
        phi.as_basic_value().into_int_value()
    }

    /// Compile an if statement: `if <expr> { ... }`.
    fn compile_if(&mut self, condition: &Expression, body: &[Spanned<Statement>]) -> Result<(), String> {
        let cond_val = self.compile_expr(condition)?.into_struct_value();

        let bool_val = self.compile_truthy(cond_val);
        let then_block = self.context.append_basic_block(self.main_fn, "if_then");
        let merge_block = self.context.append_basic_block(self.main_fn, "if_merge");
        self.builder.build_conditional_branch(bool_val, then_block, merge_block).unwrap();

        // Then block.
        self.builder.position_at_end(then_block);
        self.push_scope();
        for stmt in body {
            self.compile_statement(&stmt.node)?;
        }
        self.pop_scope();
        if self.builder.get_insert_block().unwrap().get_terminator().is_none() {
            self.builder.build_unconditional_branch(merge_block).unwrap();
        }

        self.builder.position_at_end(merge_block);
        Ok(())
    }

    /// Compile a loop-over statement: `loop <var>[, <index>] over <expr> [get <result>] { ... }`.
    fn compile_loop_over(
        &mut self,
        variable: &str,
        index: Option<&str>,
        iterable: &Expression,
        result: Option<&str>,
        body: &[Spanned<Statement>],
    ) -> Result<(), String> {
        let i8_type = self.context.i8_type();
        let i32_type = self.context.i32_type();
        let i64_type = self.context.i64_type();
        let i8_ptr_type = i8_type.ptr_type(AddressSpace::default());
        let iter_val = self.compile_expr(iterable)?.into_struct_value();

        // Verify it's an array (tag check).
        let tag = self.builder.build_extract_value(iter_val, 0, "loop_tag")
            .unwrap().into_int_value();
        let is_array = self.builder.build_int_compare(
            IntPredicate::EQ, tag,
            i8_type.const_int(TAG_ARRAY as u64, false),
            "loop_is_array",
        ).unwrap();
        let array_ok = self.context.append_basic_block(self.main_fn, "loop_array_ok");
        let array_not = self.context.append_basic_block(self.main_fn, "loop_array_not");
        let array_merge = self.context.append_basic_block(self.main_fn, "loop_array_merge");
        self.builder.build_conditional_branch(is_array, array_ok, array_not).unwrap();

        // Non-array iterable: treat as zero-element loop (matches interpreter behaviour).
        self.builder.position_at_end(array_not);
        let zero_count = i32_type.const_int(0, false);
        let null_arr_ptr = i8_type.ptr_type(AddressSpace::default()).const_null();
        self.builder.build_unconditional_branch(array_merge).unwrap();

        self.builder.position_at_end(array_ok);

        // Extract count and array pointer.
        let count_f = self.builder.build_extract_value(iter_val, 1, "loop_count_f")
            .unwrap().into_float_value();
        let real_count = self.builder.build_float_to_unsigned_int(
            count_f, i32_type, "loop_count",
        ).unwrap();
        let real_arr_ptr = self.builder.build_extract_value(iter_val, 2, "loop_arr")
            .unwrap().into_pointer_value();
        self.builder.build_unconditional_branch(array_merge).unwrap();

        // Merge: phi count and arr_ptr from array_ok and array_not paths.
        self.builder.position_at_end(array_merge);
        let count_phi = self.builder.build_phi(i32_type, "loop_count_phi").unwrap();
        count_phi.add_incoming(&[(&real_count, array_ok), (&zero_count, array_not)]);
        let count = count_phi.as_basic_value().into_int_value();

        let ptr_type = i8_type.ptr_type(AddressSpace::default());
        let arr_phi = self.builder.build_phi(ptr_type, "loop_arr_phi").unwrap();
        arr_phi.add_incoming(&[(&real_arr_ptr, array_ok), (&null_arr_ptr, array_not)]);
        let arr_ptr = arr_phi.as_basic_value().into_pointer_value();

        // If `get` is present, allocate yield collector.
        let yield_allocas = if result.is_some() {
            let elem_size = self.value_type.size_of().unwrap();
            let count_64 = self.builder.build_int_z_extend(count, i64_type, "lc_count64").unwrap();
            let total_size = self.builder.build_int_mul(elem_size, count_64, "lc_alloc").unwrap();
            let dst_ptr = self.builder.build_call(
                self.malloc_fn, &[total_size.into()], "lc_mem",
            ).unwrap()
                .try_as_basic_value().left()
                .ok_or_else(|| "malloc returned no value".to_string())?
                .into_pointer_value();

            let yield_count_alloca = self.create_entry_alloca("__yield_count");
            self.builder.build_store(yield_count_alloca, i32_type.const_int(0, false)).unwrap();
            let yield_arr_alloca = self.create_entry_alloca("__yield_arr");
            self.builder.build_store(yield_arr_alloca, dst_ptr).unwrap();

            let prev_arr = self.yield_arr_ptr.take();
            let prev_count = self.yield_count_ptr.take();
            self.yield_arr_ptr = Some(yield_arr_alloca);
            self.yield_count_ptr = Some(yield_count_alloca);
            Some((yield_count_alloca, dst_ptr, prev_arr, prev_count))
        } else {
            None
        };

        let loop_header = self.context.append_basic_block(self.main_fn, "loop_header");
        let loop_body = self.context.append_basic_block(self.main_fn, "loop_body");
        let loop_next = self.context.append_basic_block(self.main_fn, "loop_next");
        let loop_end = self.context.append_basic_block(self.main_fn, "loop_end");

        self.builder.build_unconditional_branch(loop_header).unwrap();

        // Loop header: check index < count.
        self.builder.position_at_end(loop_header);
        let i_phi = self.builder.build_phi(i32_type, "loop_i").unwrap();
        i_phi.add_incoming(&[(&i32_type.const_int(0, false), array_merge)]);
        let i_val = i_phi.as_basic_value().into_int_value();
        let done = self.builder.build_int_compare(
            IntPredicate::UGE, i_val, count, "loop_done",
        ).unwrap();
        self.builder.build_conditional_branch(done, loop_end, loop_body).unwrap();

        // Loop body: load element, execute body.
        self.builder.position_at_end(loop_body);
        let elem_ptr = unsafe { self.builder.build_in_bounds_gep(
            self.value_type, arr_ptr, &[i_val], "loop_elem_ptr",
        ) }.unwrap();
        let elem_val = self.builder.build_load(self.value_type, elem_ptr, "loop_elem")
            .unwrap();

        self.push_scope();

        let var_ptr = self.create_entry_alloca(variable);
        self.builder.build_store(var_ptr, elem_val).unwrap();
        self.scopes
            .last_mut()
            .expect("No active scope")
            .insert(variable.to_string(), var_ptr);

        // Bind index variable if present.
        if let Some(idx_name) = index {
            let i_f64 = self.builder.build_unsigned_int_to_float(
                i_val, self.context.f64_type(), "loop_idx_f",
            ).unwrap();
            let idx_tagged = self.build_value(
                i8_type.const_int(TAG_NUMBER as u64, false),
                i_f64,
                i8_ptr_type.const_null(),
                self.context.bool_type().const_int(0, false),
            );
            let idx_ptr = self.create_entry_alloca(idx_name);
            self.builder.build_store(idx_ptr, idx_tagged).unwrap();
            self.scopes
                .last_mut()
                .expect("No active scope")
                .insert(idx_name.to_string(), idx_ptr);
        }

        // Save and set break context.
        let prev_break = self.break_exit_block.take();
        self.break_exit_block = Some(loop_end);

        for stmt in body {
            self.compile_statement(&stmt.node)?;
        }

        // Restore break context.
        self.break_exit_block = prev_break;

        self.pop_scope();

        // Branch to loop_next if not already terminated (by break).
        if self.builder.get_insert_block().unwrap().get_terminator().is_none() {
            self.builder.build_unconditional_branch(loop_next).unwrap();
        }

        // Loop next: increment and branch back to header.
        self.builder.position_at_end(loop_next);
        let i_next = self.builder.build_int_add(
            i_val, i32_type.const_int(1, false), "loop_inext",
        ).unwrap();
        i_phi.add_incoming(&[(&i_next, loop_next)]);
        self.builder.build_unconditional_branch(loop_header).unwrap();

        // End: build result if `get` is present.
        self.builder.position_at_end(loop_end);

        if let (Some(result_name), Some((yield_count_alloca, dst_ptr, prev_arr, prev_count))) = (result, yield_allocas) {
            self.yield_arr_ptr = prev_arr;
            self.yield_count_ptr = prev_count;

            let final_count = self.builder.build_load(i32_type, yield_count_alloca, "lc_final_count")
                .unwrap().into_int_value();
            let result_count_f = self.builder.build_unsigned_int_to_float(
                final_count, self.context.f64_type(), "lc_result_count",
            ).unwrap();
            let result_tag = i8_type.const_int(TAG_ARRAY as u64, false);
            let bool_val = self.context.bool_type().const_int(0, false);
            let result_val = self.build_value(result_tag, result_count_f, dst_ptr, bool_val);

            let result_ptr = self.create_entry_alloca(result_name);
            self.builder.build_store(result_ptr, result_val).unwrap();
            self.scopes
                .last_mut()
                .expect("No active scope")
                .insert(result_name.to_string(), result_ptr);
        }

        Ok(())
    }

    /// Compile an array literal: `[expr, ...]`.
    fn compile_array_literal(
        &mut self,
        elements: &[Expression],
    ) -> Result<BasicValueEnum<'ctx>, String> {
        let count = elements.len();
        let i64_type = self.context.i64_type();
        let i32_type = self.context.i32_type();
        let i8_type = self.context.i8_type();
        let _i8_ptr_type = i8_type.ptr_type(AddressSpace::default());

        // Allocate memory for the array elements (each is a value_type struct).
        let elem_size = self.value_type.size_of().unwrap();
        let count_val = i64_type.const_int(count as u64, false);
        let total_size = self.builder.build_int_mul(
            elem_size, count_val, "arr_size",
        ).unwrap();
        let raw_ptr = self.builder.build_call(
            self.malloc_fn, &[total_size.into()], "arr_mem",
        ).unwrap()
            .try_as_basic_value().left()
            .ok_or_else(|| "malloc returned no value".to_string())?
            .into_pointer_value();

        // Store each element.
        for (i, elem) in elements.iter().enumerate() {
            let val = self.compile_expr(elem)?;
            let idx = i32_type.const_int(i as u64, false);
            let elem_ptr = unsafe { self.builder.build_in_bounds_gep(
                self.value_type, raw_ptr, &[idx], &format!("arr_elem_{}", i),
            ) }.unwrap();
            self.builder.build_store(elem_ptr, val).unwrap();
        }

        let tag = i8_type.const_int(TAG_ARRAY as u64, false);
        let num = self.context.f64_type().const_float(count as f64);
        let bool_val = self.context.bool_type().const_int(0, false);
        Ok(self.build_value(tag, num, raw_ptr, bool_val).into())
    }

    /// Compile index access: `expr[expr]`.
    fn compile_index_access(
        &mut self,
        receiver: &Expression,
        index: &Expression,
    ) -> Result<BasicValueEnum<'ctx>, String> {
        let recv_val = self.compile_expr(receiver)?.into_struct_value();
        let idx_val = self.compile_expr(index)?.into_struct_value();

        // Check receiver is an array.
        let recv_tag = self.builder.build_extract_value(recv_val, 0, "idx_recv_tag")
            .unwrap().into_int_value();
        let i8_type = self.context.i8_type();
        let i32_type = self.context.i32_type();
        let is_array = self.builder.build_int_compare(
            IntPredicate::EQ, recv_tag,
            i8_type.const_int(TAG_ARRAY as u64, false),
            "idx_is_array",
        ).unwrap();
        let array_ok = self.context.append_basic_block(self.main_fn, "idx_array_ok");
        let array_fail = self.context.append_basic_block(self.main_fn, "idx_array_fail");
        self.builder.build_conditional_branch(is_array, array_ok, array_fail).unwrap();

        self.builder.position_at_end(array_fail);
        self.emit_trap();

        // Array ok: extract count+arr, get index.
        self.builder.position_at_end(array_ok);
        let count_f = self.builder.build_extract_value(recv_val, 1, "idx_count_f")
            .unwrap().into_float_value();
        let count = self.builder.build_float_to_unsigned_int(
            count_f, i32_type, "idx_count",
        ).unwrap();
        let arr_ptr = self.builder.build_extract_value(recv_val, 2, "idx_arr")
            .unwrap().into_pointer_value();

        // Get index value as integer.
        let idx_f = self.builder.build_extract_value(idx_val, 1, "idx_f")
            .unwrap().into_float_value();
        let idx_i = self.builder.build_float_to_unsigned_int(
            idx_f, i32_type, "idx_i",
        ).unwrap();

        // Bounds check: if idx >= count, return Null.
        let in_bounds = self.builder.build_int_compare(
            IntPredicate::ULT, idx_i, count, "idx_in_bounds",
        ).unwrap();
        let ok_block = self.context.append_basic_block(self.main_fn, "idx_ok");
        let oob_block = self.context.append_basic_block(self.main_fn, "idx_oob");
        let merge_block = self.context.append_basic_block(self.main_fn, "idx_merge");
        self.builder.build_conditional_branch(in_bounds, ok_block, oob_block).unwrap();

        // In bounds: load element.
        self.builder.position_at_end(ok_block);
        let elem_ptr = unsafe { self.builder.build_in_bounds_gep(
            self.value_type, arr_ptr, &[idx_i], "idx_elem_ptr",
        ) }.unwrap();
        let elem_val = self.builder.build_load(self.value_type, elem_ptr, "idx_elem")
            .unwrap();
        self.builder.build_unconditional_branch(merge_block).unwrap();
        let ok_end = self.builder.get_insert_block().unwrap();

        // Out of bounds: return Null.
        self.builder.position_at_end(oob_block);
        let null_val: BasicValueEnum = self.const_null().into();
        self.builder.build_unconditional_branch(merge_block).unwrap();
        let oob_end = self.builder.get_insert_block().unwrap();

        // Merge.
        self.builder.position_at_end(merge_block);
        let phi = self.builder.build_phi(self.value_type, "idx_result").unwrap();
        phi.add_incoming(&[
            (&elem_val, ok_end),
            (&null_val, oob_end),
        ]);

        Ok(phi.as_basic_value())
    }

    /// Compile a function call: `callee(args)`.
    /// Handles built-in functions: `timestamp()`, `length(x)`.
    fn compile_call(
        &mut self,
        callee: &Expression,
        args: &[Expression],
    ) -> Result<BasicValueEnum<'ctx>, String> {
        // Only identifiers are supported as callees for now (built-in functions).
        let name = match callee {
            Expression::Identifier(n) => n.as_str(),
            _ => return Err("Only named function calls are supported".to_string()),
        };

        match name {
            "timestamp" => {
                if !args.is_empty() {
                    return Err("timestamp() takes no arguments".to_string());
                }
                let i8_ptr_type = self.context.i8_type().ptr_type(AddressSpace::default());
                let null_ptr = i8_ptr_type.const_null();
                let ts = self.builder.build_call(
                    self.time_fn, &[null_ptr.into()], "ts_raw",
                ).unwrap().try_as_basic_value().left().unwrap().into_int_value();
                let ts_f64 = self.builder.build_signed_int_to_float(
                    ts, self.context.f64_type(), "ts_f64",
                ).unwrap();
                let tag = self.context.i8_type().const_int(TAG_NUMBER as u64, false);
                let str_ptr = i8_ptr_type.const_null();
                let bool_val = self.context.bool_type().const_int(0, false);
                Ok(self.build_value(tag, ts_f64, str_ptr, bool_val).into())
            }
            "length" => {
                if args.len() != 1 {
                    return Err("length() takes exactly 1 argument".to_string());
                }
                let val = self.compile_expr(&args[0])?.into_struct_value();
                let tag = self.builder.build_extract_value(val, 0, "len_tag")
                    .unwrap().into_int_value();
                let i8_type = self.context.i8_type();
                let i8_ptr_type = i8_type.ptr_type(AddressSpace::default());
                let f64_type = self.context.f64_type();

                let is_array = self.builder.build_int_compare(
                    IntPredicate::EQ, tag,
                    i8_type.const_int(TAG_ARRAY as u64, false),
                    "len_is_arr",
                ).unwrap();

                let arr_bb = self.context.append_basic_block(self.main_fn, "len_arr");
                let not_arr_bb = self.context.append_basic_block(self.main_fn, "len_not_arr");
                let str_bb = self.context.append_basic_block(self.main_fn, "len_str");
                let else_bb = self.context.append_basic_block(self.main_fn, "len_else");
                let merge_bb = self.context.append_basic_block(self.main_fn, "len_merge");

                self.builder.build_conditional_branch(is_array, arr_bb, not_arr_bb).unwrap();

                // Array: length is stored in the num field (element_count).
                self.builder.position_at_end(arr_bb);
                let arr_count = self.builder.build_extract_value(val, 1, "arr_count")
                    .unwrap().into_float_value();
                self.builder.build_unconditional_branch(merge_bb).unwrap();

                // Not array: check string.
                self.builder.position_at_end(not_arr_bb);
                let is_string = self.builder.build_int_compare(
                    IntPredicate::EQ, tag,
                    i8_type.const_int(TAG_STRING as u64, false),
                    "len_is_str",
                ).unwrap();
                self.builder.build_conditional_branch(is_string, str_bb, else_bb).unwrap();

                // String: call strlen.
                self.builder.position_at_end(str_bb);
                let str_ptr = self.builder.build_extract_value(val, 2, "len_str_ptr")
                    .unwrap().into_pointer_value();
                let str_len = self.builder.build_call(
                    self.strlen_fn, &[str_ptr.into()], "str_len",
                ).unwrap().try_as_basic_value().left().unwrap().into_int_value();
                let str_len_f64 = self.builder.build_unsigned_int_to_float(
                    str_len, f64_type, "str_len_f64",
                ).unwrap();
                self.builder.build_unconditional_branch(merge_bb).unwrap();

                // Else: return 0.
                self.builder.position_at_end(else_bb);
                let zero = f64_type.const_float(0.0);
                self.builder.build_unconditional_branch(merge_bb).unwrap();

                // Merge: phi the result.
                self.builder.position_at_end(merge_bb);
                let len_phi = self.builder.build_phi(f64_type, "len_val").unwrap();
                len_phi.add_incoming(&[
                    (&arr_count, arr_bb),
                    (&str_len_f64, str_bb),
                    (&zero, else_bb),
                ]);
                let result_f64 = len_phi.as_basic_value().into_float_value();
                let result_tag = i8_type.const_int(TAG_NUMBER as u64, false);
                let null_ptr = i8_ptr_type.const_null();
                let bool_val = self.context.bool_type().const_int(0, false);
                Ok(self.build_value(result_tag, result_f64, null_ptr, bool_val).into())
            }
            _ => Err(format!("Unknown function: {}", name)),
        }
    }

    /// Compile an interpolated string: `"...$var..."`.
    fn compile_interpolated_string(
        &mut self,
        parts: &[crate::ast::StringPart],
    ) -> Result<BasicValueEnum<'ctx>, String> {
        // Strategy: build each part as a C string, concatenate at runtime.
        let mut string_ptrs: Vec<PointerValue<'ctx>> = Vec::new();
        let _i8_ptr_type = self.context.i8_type().ptr_type(AddressSpace::default());

        for part in parts {
            match part {
                crate::ast::StringPart::Literal(s) => {
                    let global = self.builder.build_global_string_ptr(
                        s, &format!("interp_lit_{}", self.string_count),
                    ).unwrap();
                    self.string_count += 1;
                    string_ptrs.push(global.as_pointer_value());
                }
                crate::ast::StringPart::Variable(name) => {
                    let ptr = self.get_var_ptr(name).ok_or_else(|| {
                        format!("Undefined variable '{}' in string interpolation", name)
                    })?;
                    let val = self.builder.build_load(self.value_type, ptr, "interp_load")
                        .unwrap().into_struct_value();
                    // Convert value to C-string using __value_to_cstr (handles all types).
                    let i32_type = self.context.i32_type();
                    let tag = self.builder.build_extract_value(val, 0, "interp_tag")
                        .unwrap().into_int_value();
                    let tag_i32 = self.builder.build_int_z_extend(tag, i32_type, "interp_tag32")
                        .unwrap();
                    let num = self.builder.build_extract_value(val, 1, "interp_num")
                        .unwrap().into_float_value();
                    let ptr_val = self.builder.build_extract_value(val, 2, "interp_ptr")
                        .unwrap().into_pointer_value();
                    let str_ptr = self.builder.build_call(
                        self.value_to_cstr_fn,
                        &[tag_i32.into(), num.into(), ptr_val.into()],
                        "interp_cstr",
                    ).unwrap().try_as_basic_value().left().unwrap().into_pointer_value();
                    string_ptrs.push(str_ptr);
                }
            }
        }

        // Concatenate all parts using strlen + malloc + memcpy.
        let result_ptr = self.build_strcat_multiple(&string_ptrs)?;

        let tag = self.context.i8_type().const_int(TAG_STRING as u64, false);
        let num = self.context.f64_type().const_float(0.0);
        let bool_val = self.context.bool_type().const_int(0, false);
        Ok(self.build_value(tag, num, result_ptr, bool_val).into())
    }

    /// Concatenate multiple C strings into a new malloc'd buffer.
    fn build_strcat_multiple(
        &mut self,
        ptrs: &[PointerValue<'ctx>],
    ) -> Result<PointerValue<'ctx>, String> {
        let i64_type = self.context.i64_type();
        let i8_type = self.context.i8_type();
        let _i8_ptr_type = i8_type.ptr_type(AddressSpace::default());

        // Calculate total length.
        let mut total_len = i64_type.const_int(1, false); // +1 for null terminator
        let mut lengths: Vec<inkwell::values::IntValue<'ctx>> = Vec::new();
        for (i, ptr) in ptrs.iter().enumerate() {
            let len = self.builder.build_call(
                self.strlen_fn, &[(*ptr).into()], &format!("len_{}", i),
            ).unwrap()
                .try_as_basic_value().left().unwrap().into_int_value();
            lengths.push(len);
            total_len = self.builder.build_int_add(total_len, len, &format!("total_{}", i)).unwrap();
        }

        // Malloc the result buffer.
        let result_ptr = self.builder.build_call(
            self.malloc_fn, &[total_len.into()], "strcat_buf",
        ).unwrap()
            .try_as_basic_value().left()
            .ok_or_else(|| "malloc returned no value".to_string())?
            .into_pointer_value();

        // Copy each string into the buffer.
        let mut offset = i64_type.const_int(0, false);
        for (i, (ptr, len)) in ptrs.iter().zip(lengths.iter()).enumerate() {
            let dest = unsafe { self.builder.build_in_bounds_gep(
                i8_type, result_ptr, &[offset], &format!("dest_{}", i),
            ) }.unwrap();
            self.builder.build_call(
                self.memcpy_fn, &[dest.into(), (*ptr).into(), (*len).into()],
                &format!("copy_{}", i),
            ).unwrap();
            offset = self.builder.build_int_add(offset, *len, &format!("off_{}", i)).unwrap();
        }

        // Null terminate.
        let last = unsafe { self.builder.build_in_bounds_gep(
            i8_type, result_ptr, &[offset], "null_term",
        ) }.unwrap();
        self.builder.build_store(last, i8_type.const_int(0, false)).unwrap();

        Ok(result_ptr)
    }

    /// Compile the `+` operator for strings, arrays, and numbers.
    fn compile_add(
        &mut self,
        left: &Expression,
        right: &Expression,
    ) -> Result<BasicValueEnum<'ctx>, String> {
        let left_val = self.compile_expr(left)?.into_struct_value();
        let right_val = self.compile_expr(right)?.into_struct_value();

        let left_tag = self.builder.build_extract_value(left_val, 0, "add_ltag")
            .unwrap().into_int_value();
        let right_tag = self.builder.build_extract_value(right_val, 0, "add_rtag")
            .unwrap().into_int_value();

        let i8_type = self.context.i8_type();
        let i32_type = self.context.i32_type();
        let _i64_type = self.context.i64_type();
        let i8_ptr_type = i8_type.ptr_type(AddressSpace::default());

        // Check if left is string.
        let l_is_str = self.builder.build_int_compare(
            IntPredicate::EQ, left_tag,
            i8_type.const_int(TAG_STRING as u64, false),
            "add_l_is_str",
        ).unwrap();

        let str_block = self.context.append_basic_block(self.main_fn, "add_str");
        let non_str_block = self.context.append_basic_block(self.main_fn, "add_non_str");
        let merge_block = self.context.append_basic_block(self.main_fn, "add_merge");

        self.builder.build_conditional_branch(l_is_str, str_block, non_str_block).unwrap();

        // String concat: left is string → concat with right's string repr.
        self.builder.position_at_end(str_block);
        let l_str = self.builder.build_extract_value(left_val, 2, "add_l_str")
            .unwrap().into_pointer_value();
        // Convert right to C-string regardless of its type (Number, Boolean, etc.).
        let r_tag  = self.builder.build_extract_value(right_val, 0, "add_r_tag")
            .unwrap().into_int_value();
        let r_tag_i32 = self.builder.build_int_z_extend(r_tag, i32_type, "add_r_tag32").unwrap();
        let r_num  = self.builder.build_extract_value(right_val, 1, "add_r_num")
            .unwrap().into_float_value();
        let r_ptr  = self.builder.build_extract_value(right_val, 2, "add_r_ptr")
            .unwrap().into_pointer_value();
        let r_str  = self.builder.build_call(
            self.value_to_cstr_fn,
            &[r_tag_i32.into(), r_num.into(), r_ptr.into()],
            "add_r_cstr",
        ).unwrap().try_as_basic_value().left().unwrap().into_pointer_value();
        let concat_ptr = self.build_strcat_multiple(&[l_str, r_str])?;
        let str_result = self.build_value(
            i8_type.const_int(TAG_STRING as u64, false),
            self.context.f64_type().const_float(0.0),
            concat_ptr,
            self.context.bool_type().const_int(0, false),
        );
        self.builder.build_unconditional_branch(merge_block).unwrap();
        let str_end = self.builder.get_insert_block().unwrap();

        // Non-string left: check if right is string.
        self.builder.position_at_end(non_str_block);
        let r_is_str = self.builder.build_int_compare(
            IntPredicate::EQ, right_tag,
            i8_type.const_int(TAG_STRING as u64, false),
            "add_r_is_str",
        ).unwrap();
        let r_str_block = self.context.append_basic_block(self.main_fn, "add_r_str_block");
        let non_str2_block = self.context.append_basic_block(self.main_fn, "add_non_str2");
        self.builder.build_conditional_branch(r_is_str, r_str_block, non_str2_block).unwrap();

        // Right is string: convert left to C-string, then concat with right.
        self.builder.position_at_end(r_str_block);
        let l_tag  = self.builder.build_extract_value(left_val, 0, "add_l_tag2")
            .unwrap().into_int_value();
        let l_tag_i32 = self.builder.build_int_z_extend(l_tag, i32_type, "add_l_tag32").unwrap();
        let l_num  = self.builder.build_extract_value(left_val, 1, "add_l_num2")
            .unwrap().into_float_value();
        let l_ptr2 = self.builder.build_extract_value(left_val, 2, "add_l_ptrv")
            .unwrap().into_pointer_value();
        let l_str2 = self.builder.build_call(
            self.value_to_cstr_fn,
            &[l_tag_i32.into(), l_num.into(), l_ptr2.into()],
            "add_l_cstr2",
        ).unwrap().try_as_basic_value().left().unwrap().into_pointer_value();
        let r_str2 = self.builder.build_extract_value(right_val, 2, "add_r_str2")
            .unwrap().into_pointer_value();
        let concat_ptr2 = self.build_strcat_multiple(&[l_str2, r_str2])?;
        let r_str_result = self.build_value(
            i8_type.const_int(TAG_STRING as u64, false),
            self.context.f64_type().const_float(0.0),
            concat_ptr2,
            self.context.bool_type().const_int(0, false),
        );
        self.builder.build_unconditional_branch(merge_block).unwrap();
        let r_str_end = self.builder.get_insert_block().unwrap();

        // Neither is string: check for number or array.
        self.builder.position_at_end(non_str2_block);
        let l_is_num = self.builder.build_int_compare(
            IntPredicate::EQ, left_tag,
            i8_type.const_int(TAG_NUMBER as u64, false),
            "add_l_is_num",
        ).unwrap();
        let num_block = self.context.append_basic_block(self.main_fn, "add_num");
        let arr_check_block = self.context.append_basic_block(self.main_fn, "add_arr_check");
        self.builder.build_conditional_branch(l_is_num, num_block, arr_check_block).unwrap();

        // Number addition.
        self.builder.position_at_end(num_block);
        let l_num = self.builder.build_extract_value(left_val, 1, "add_l_num")
            .unwrap().into_float_value();
        let r_num = self.builder.build_extract_value(right_val, 1, "add_r_num")
            .unwrap().into_float_value();
        let sum = self.builder.build_float_add(l_num, r_num, "add_sum").unwrap();
        let num_result = self.build_value(
            i8_type.const_int(TAG_NUMBER as u64, false),
            sum,
            i8_ptr_type.const_null(),
            self.context.bool_type().const_int(0, false),
        );
        self.builder.build_unconditional_branch(merge_block).unwrap();
        let num_end = self.builder.get_insert_block().unwrap();

        // Array operations.
        self.builder.position_at_end(arr_check_block);
        let l_is_arr = self.builder.build_int_compare(
            IntPredicate::EQ, left_tag,
            i8_type.const_int(TAG_ARRAY as u64, false),
            "add_l_is_arr",
        ).unwrap();
        let arr_block = self.context.append_basic_block(self.main_fn, "add_arr");
        let r_arr_check = self.context.append_basic_block(self.main_fn, "add_r_arr_check");
        self.builder.build_conditional_branch(l_is_arr, arr_block, r_arr_check).unwrap();

        // Left is array: check if right is also array.
        self.builder.position_at_end(arr_block);
        let r_is_arr = self.builder.build_int_compare(
            IntPredicate::EQ, right_tag,
            i8_type.const_int(TAG_ARRAY as u64, false),
            "add_r_is_arr_too",
        ).unwrap();
        let arr_arr_block = self.context.append_basic_block(self.main_fn, "add_arr_arr");
        let arr_val_block = self.context.append_basic_block(self.main_fn, "add_arr_val");
        self.builder.build_conditional_branch(r_is_arr, arr_arr_block, arr_val_block).unwrap();

        // Array + Array = concat.
        self.builder.position_at_end(arr_arr_block);
        let arr_arr_result = self.build_array_concat(left_val, right_val)?;
        self.builder.build_unconditional_branch(merge_block).unwrap();
        let arr_arr_end = self.builder.get_insert_block().unwrap();

        // Array + value = append.
        self.builder.position_at_end(arr_val_block);
        let arr_append_result = self.build_array_append(left_val, right_val)?;
        self.builder.build_unconditional_branch(merge_block).unwrap();
        let arr_val_end = self.builder.get_insert_block().unwrap();

        // Right is array (left is not string/number/array) = value + array = prepend.
        self.builder.position_at_end(r_arr_check);
        let r_is_arr2 = self.builder.build_int_compare(
            IntPredicate::EQ, right_tag,
            i8_type.const_int(TAG_ARRAY as u64, false),
            "add_r_is_arr2",
        ).unwrap();
        let prepend_block = self.context.append_basic_block(self.main_fn, "add_prepend");
        let obj_check_block = self.context.append_basic_block(self.main_fn, "add_obj_check");
        self.builder.build_conditional_branch(r_is_arr2, prepend_block, obj_check_block).unwrap();

        self.builder.position_at_end(prepend_block);
        let prepend_result = self.build_array_prepend(left_val, right_val)?;
        self.builder.build_unconditional_branch(merge_block).unwrap();
        let prepend_end = self.builder.get_insert_block().unwrap();

        // Object + Object merge.
        self.builder.position_at_end(obj_check_block);
        let l_is_obj = self.builder.build_int_compare(
            IntPredicate::EQ, left_tag,
            i8_type.const_int(TAG_OBJECT as u64, false),
            "add_l_is_obj",
        ).unwrap();
        let obj_block = self.context.append_basic_block(self.main_fn, "add_obj_merge");
        let err_block = self.context.append_basic_block(self.main_fn, "add_err");
        self.builder.build_conditional_branch(l_is_obj, obj_block, err_block).unwrap();

        self.builder.position_at_end(obj_block);
        let obj_merge_result = self.build_object_merge(left_val, right_val)?;
        self.builder.build_unconditional_branch(merge_block).unwrap();
        let obj_end = self.builder.get_insert_block().unwrap();

        // Error: unsupported + types.
        self.builder.position_at_end(err_block);
        self.emit_trap();

        // Merge all results.
        self.builder.position_at_end(merge_block);
        let phi = self.builder.build_phi(self.value_type, "add_result").unwrap();
        phi.add_incoming(&[
            (&str_result, str_end),
            (&r_str_result, r_str_end),
            (&num_result, num_end),
            (&arr_arr_result, arr_arr_end),
            (&arr_append_result, arr_val_end),
            (&prepend_result, prepend_end),
            (&obj_merge_result, obj_end),
        ]);

        Ok(phi.as_basic_value())
    }

    /// Build array + array concatenation. Returns an Array-tagged value.
    fn build_array_concat(
        &mut self,
        left: StructValue<'ctx>,
        right: StructValue<'ctx>,
    ) -> Result<StructValue<'ctx>, String> {
        let i32_type = self.context.i32_type();
        let i64_type = self.context.i64_type();
        let i8_type = self.context.i8_type();
        let _i8_ptr_type = i8_type.ptr_type(AddressSpace::default());

        let l_count_f = self.builder.build_extract_value(left, 1, "cc_l_count_f")
            .unwrap().into_float_value();
        let r_count_f = self.builder.build_extract_value(right, 1, "cc_r_count_f")
            .unwrap().into_float_value();
        let l_count = self.builder.build_float_to_unsigned_int(l_count_f, i32_type, "cc_lcount").unwrap();
        let r_count = self.builder.build_float_to_unsigned_int(r_count_f, i32_type, "cc_rcount").unwrap();
        let total = self.builder.build_int_add(l_count, r_count, "cc_total").unwrap();

        let l_arr = self.builder.build_extract_value(left, 2, "cc_l_arr")
            .unwrap().into_pointer_value();
        let r_arr = self.builder.build_extract_value(right, 2, "cc_r_arr")
            .unwrap().into_pointer_value();

        // Allocate new array.
        let elem_size = self.value_type.size_of().unwrap();
        let total_i64 = self.builder.build_int_z_extend(total, i64_type, "cc_total_i64").unwrap();
        let alloc_size = self.builder.build_int_mul(elem_size, total_i64, "cc_alloc").unwrap();
        let new_ptr = self.builder.build_call(
            self.malloc_fn, &[alloc_size.into()], "cc_mem",
        ).unwrap().try_as_basic_value().left().unwrap().into_pointer_value();

        // Copy left elements.
        let l_bytes = self.builder.build_int_mul(
            elem_size,
            self.builder.build_int_z_extend(l_count, i64_type, "cc_lc64").unwrap(),
            "cc_lbytes",
        ).unwrap();
        self.builder.build_call(
            self.memcpy_fn,
            &[new_ptr.into(), l_arr.into(), l_bytes.into()],
            "cc_copy_l",
        ).unwrap();

        // Copy right elements after left.
        let dest_offset = unsafe { self.builder.build_in_bounds_gep(
            self.value_type, new_ptr, &[l_count], "cc_dest_off",
        ) }.unwrap();
        let r_bytes = self.builder.build_int_mul(
            elem_size,
            self.builder.build_int_z_extend(r_count, i64_type, "cc_rc64").unwrap(),
            "cc_rbytes",
        ).unwrap();
        self.builder.build_call(
            self.memcpy_fn,
            &[dest_offset.into(), r_arr.into(), r_bytes.into()],
            "cc_copy_r",
        ).unwrap();

        let total_f = self.builder.build_unsigned_int_to_float(
            total, self.context.f64_type(), "cc_total_f",
        ).unwrap();
        Ok(self.build_value(
            i8_type.const_int(TAG_ARRAY as u64, false),
            total_f,
            new_ptr,
            self.context.bool_type().const_int(0, false),
        ))
    }

    /// Build array + value (append). Returns an Array-tagged value.
    fn build_array_append(
        &mut self,
        arr_val: StructValue<'ctx>,
        elem_val: StructValue<'ctx>,
    ) -> Result<StructValue<'ctx>, String> {
        let i32_type = self.context.i32_type();
        let i64_type = self.context.i64_type();
        let i8_type = self.context.i8_type();

        let count_f = self.builder.build_extract_value(arr_val, 1, "app_count_f")
            .unwrap().into_float_value();
        let count = self.builder.build_float_to_unsigned_int(count_f, i32_type, "app_count").unwrap();
        let new_count = self.builder.build_int_add(count, i32_type.const_int(1, false), "app_ncount").unwrap();

        let arr_ptr = self.builder.build_extract_value(arr_val, 2, "app_arr")
            .unwrap().into_pointer_value();

        let elem_size = self.value_type.size_of().unwrap();
        let nc_i64 = self.builder.build_int_z_extend(new_count, i64_type, "app_nc64").unwrap();
        let alloc_size = self.builder.build_int_mul(elem_size, nc_i64, "app_alloc").unwrap();
        let new_ptr = self.builder.build_call(
            self.malloc_fn, &[alloc_size.into()], "app_mem",
        ).unwrap().try_as_basic_value().left().unwrap().into_pointer_value();

        // Copy existing elements.
        let old_bytes = self.builder.build_int_mul(
            elem_size,
            self.builder.build_int_z_extend(count, i64_type, "app_c64").unwrap(),
            "app_old_bytes",
        ).unwrap();
        self.builder.build_call(
            self.memcpy_fn,
            &[new_ptr.into(), arr_ptr.into(), old_bytes.into()],
            "app_copy",
        ).unwrap();

        // Store new element at end.
        let last_ptr = unsafe { self.builder.build_in_bounds_gep(
            self.value_type, new_ptr, &[count], "app_last",
        ) }.unwrap();
        self.builder.build_store(last_ptr, elem_val).unwrap();

        let new_count_f = self.builder.build_unsigned_int_to_float(
            new_count, self.context.f64_type(), "app_nf",
        ).unwrap();
        Ok(self.build_value(
            i8_type.const_int(TAG_ARRAY as u64, false),
            new_count_f,
            new_ptr,
            self.context.bool_type().const_int(0, false),
        ))
    }

    /// Build value + array (prepend). Returns an Array-tagged value.
    fn build_array_prepend(
        &mut self,
        elem_val: StructValue<'ctx>,
        arr_val: StructValue<'ctx>,
    ) -> Result<StructValue<'ctx>, String> {
        let i32_type = self.context.i32_type();
        let i64_type = self.context.i64_type();
        let i8_type = self.context.i8_type();

        let count_f = self.builder.build_extract_value(arr_val, 1, "pre_count_f")
            .unwrap().into_float_value();
        let count = self.builder.build_float_to_unsigned_int(count_f, i32_type, "pre_count").unwrap();
        let new_count = self.builder.build_int_add(count, i32_type.const_int(1, false), "pre_ncount").unwrap();

        let arr_ptr = self.builder.build_extract_value(arr_val, 2, "pre_arr")
            .unwrap().into_pointer_value();

        let elem_size = self.value_type.size_of().unwrap();
        let nc_i64 = self.builder.build_int_z_extend(new_count, i64_type, "pre_nc64").unwrap();
        let alloc_size = self.builder.build_int_mul(elem_size, nc_i64, "pre_alloc").unwrap();
        let new_ptr = self.builder.build_call(
            self.malloc_fn, &[alloc_size.into()], "pre_mem",
        ).unwrap().try_as_basic_value().left().unwrap().into_pointer_value();

        // Store the new element at position 0.
        self.builder.build_store(new_ptr, elem_val).unwrap();

        // Copy existing elements after.
        let dest = unsafe { self.builder.build_in_bounds_gep(
            self.value_type, new_ptr, &[i32_type.const_int(1, false)], "pre_dest",
        ) }.unwrap();
        let old_bytes = self.builder.build_int_mul(
            elem_size,
            self.builder.build_int_z_extend(count, i64_type, "pre_c64").unwrap(),
            "pre_old_bytes",
        ).unwrap();
        self.builder.build_call(
            self.memcpy_fn,
            &[dest.into(), arr_ptr.into(), old_bytes.into()],
            "pre_copy",
        ).unwrap();

        let new_count_f = self.builder.build_unsigned_int_to_float(
            new_count, self.context.f64_type(), "pre_nf",
        ).unwrap();
        Ok(self.build_value(
            i8_type.const_int(TAG_ARRAY as u64, false),
            new_count_f,
            new_ptr,
            self.context.bool_type().const_int(0, false),
        ))
    }

    /// Build Object + Object merge. Concatenates fields from both objects.
    fn build_object_merge(
        &mut self,
        left: StructValue<'ctx>,
        right: StructValue<'ctx>,
    ) -> Result<StructValue<'ctx>, String> {
        let i32_type = self.context.i32_type();
        let i64_type = self.context.i64_type();
        let i8_type = self.context.i8_type();

        let l_count_f = self.builder.build_extract_value(left, 1, "om_l_count_f")
            .unwrap().into_float_value();
        let r_count_f = self.builder.build_extract_value(right, 1, "om_r_count_f")
            .unwrap().into_float_value();
        let l_count = self.builder.build_float_to_unsigned_int(l_count_f, i32_type, "om_lcount").unwrap();
        let r_count = self.builder.build_float_to_unsigned_int(r_count_f, i32_type, "om_rcount").unwrap();
        let total = self.builder.build_int_add(l_count, r_count, "om_total").unwrap();

        let l_arr = self.builder.build_extract_value(left, 2, "om_l_arr")
            .unwrap().into_pointer_value();
        let r_arr = self.builder.build_extract_value(right, 2, "om_r_arr")
            .unwrap().into_pointer_value();

        let field_size = self.field_type.size_of().unwrap();
        let total_i64 = self.builder.build_int_z_extend(total, i64_type, "om_total64").unwrap();
        let alloc_size = self.builder.build_int_mul(field_size, total_i64, "om_alloc").unwrap();
        let new_ptr = self.builder.build_call(
            self.malloc_fn, &[alloc_size.into()], "om_mem",
        ).unwrap().try_as_basic_value().left().unwrap().into_pointer_value();

        // Copy left fields.
        let l_bytes = self.builder.build_int_mul(
            field_size,
            self.builder.build_int_z_extend(l_count, i64_type, "om_lc64").unwrap(),
            "om_lbytes",
        ).unwrap();
        self.builder.build_call(
            self.memcpy_fn,
            &[new_ptr.into(), l_arr.into(), l_bytes.into()],
            "om_copy_l",
        ).unwrap();

        // Copy right fields after left.
        let dest_offset = unsafe { self.builder.build_in_bounds_gep(
            self.field_type, new_ptr, &[l_count], "om_dest_off",
        ) }.unwrap();
        let r_bytes = self.builder.build_int_mul(
            field_size,
            self.builder.build_int_z_extend(r_count, i64_type, "om_rc64").unwrap(),
            "om_rbytes",
        ).unwrap();
        self.builder.build_call(
            self.memcpy_fn,
            &[dest_offset.into(), r_arr.into(), r_bytes.into()],
            "om_copy_r",
        ).unwrap();

        let total_f = self.builder.build_unsigned_int_to_float(
            total, self.context.f64_type(), "om_total_f",
        ).unwrap();
        Ok(self.build_value(
            i8_type.const_int(TAG_OBJECT as u64, false),
            total_f,
            new_ptr,
            self.context.bool_type().const_int(0, false),
        ))
    }

    /// Compile arithmetic operators: `-`, `*`, `/`.
    /// Both operands must be Numbers. Division by zero traps.
    fn compile_arithmetic(
        &mut self,
        left: &Expression,
        op: &BinaryOp,
        right: &Expression,
    ) -> Result<BasicValueEnum<'ctx>, String> {
        let left_val = self.compile_expr(left)?.into_struct_value();
        let right_val = self.compile_expr(right)?.into_struct_value();

        let i8_type = self.context.i8_type();
        let f64_type = self.context.f64_type();
        let tag_num = i8_type.const_int(TAG_NUMBER as u64, false);

        // Check left is Number.
        let left_tag = self.builder.build_extract_value(left_val, 0, "arith_ltag")
            .unwrap().into_int_value();
        let l_ok = self.builder.build_int_compare(
            IntPredicate::EQ, left_tag, tag_num, "arith_lok",
        ).unwrap();
        let left_ok_bb = self.context.append_basic_block(self.main_fn, "arith_lok_bb");
        let trap_bb = self.context.append_basic_block(self.main_fn, "arith_trap");
        self.builder.build_conditional_branch(l_ok, left_ok_bb, trap_bb).unwrap();

        self.builder.position_at_end(trap_bb);
        self.emit_trap();

        self.builder.position_at_end(left_ok_bb);

        // Check right is Number.
        let right_tag = self.builder.build_extract_value(right_val, 0, "arith_rtag")
            .unwrap().into_int_value();
        let r_ok = self.builder.build_int_compare(
            IntPredicate::EQ, right_tag, tag_num, "arith_rok",
        ).unwrap();
        let right_ok_bb = self.context.append_basic_block(self.main_fn, "arith_rok_bb");
        let trap_bb2 = self.context.append_basic_block(self.main_fn, "arith_trap2");
        self.builder.build_conditional_branch(r_ok, right_ok_bb, trap_bb2).unwrap();

        self.builder.position_at_end(trap_bb2);
        self.emit_trap();

        self.builder.position_at_end(right_ok_bb);

        let l_num = self.builder.build_extract_value(left_val, 1, "arith_lnum")
            .unwrap().into_float_value();
        let r_num = self.builder.build_extract_value(right_val, 1, "arith_rnum")
            .unwrap().into_float_value();

        let result_num = match op {
            BinaryOp::Sub => self.builder.build_float_sub(l_num, r_num, "sub").unwrap(),
            BinaryOp::Mul => self.builder.build_float_mul(l_num, r_num, "mul").unwrap(),
            BinaryOp::Div => {
                // Division by zero check.
                let zero = f64_type.const_float(0.0);
                let is_zero = self.builder.build_float_compare(
                    inkwell::FloatPredicate::OEQ, r_num, zero, "div_zero_check",
                ).unwrap();
                let div_ok_bb = self.context.append_basic_block(self.main_fn, "div_ok");
                let div_trap_bb = self.context.append_basic_block(self.main_fn, "div_trap");
                self.builder.build_conditional_branch(is_zero, div_trap_bb, div_ok_bb).unwrap();

                self.builder.position_at_end(div_trap_bb);
                self.emit_trap();

                self.builder.position_at_end(div_ok_bb);
                self.builder.build_float_div(l_num, r_num, "div").unwrap()
            }
            _ => unreachable!(),
        };

        let null_ptr = self.context.i8_type().ptr_type(AddressSpace::default()).const_null();
        let result = self.build_value(
            tag_num,
            result_num,
            null_ptr,
            self.context.bool_type().const_int(0, false),
        );
        Ok(result.into())
    }

    /// Compile relational operators: `<`, `>`, `<=`, `>=`.
    /// Both operands must be Numbers. Result is Boolean.
    fn compile_relational(
        &mut self,
        left: &Expression,
        op: &BinaryOp,
        right: &Expression,
    ) -> Result<BasicValueEnum<'ctx>, String> {
        let left_val = self.compile_expr(left)?.into_struct_value();
        let right_val = self.compile_expr(right)?.into_struct_value();

        let i8_type = self.context.i8_type();
        let tag_num = i8_type.const_int(TAG_NUMBER as u64, false);

        // Check left is Number.
        let left_tag = self.builder.build_extract_value(left_val, 0, "rel_ltag")
            .unwrap().into_int_value();
        let l_ok = self.builder.build_int_compare(
            IntPredicate::EQ, left_tag, tag_num, "rel_lok",
        ).unwrap();
        let left_ok_bb = self.context.append_basic_block(self.main_fn, "rel_lok_bb");
        let trap_bb = self.context.append_basic_block(self.main_fn, "rel_trap");
        self.builder.build_conditional_branch(l_ok, left_ok_bb, trap_bb).unwrap();

        self.builder.position_at_end(trap_bb);
        self.emit_trap();

        self.builder.position_at_end(left_ok_bb);

        // Check right is Number.
        let right_tag = self.builder.build_extract_value(right_val, 0, "rel_rtag")
            .unwrap().into_int_value();
        let r_ok = self.builder.build_int_compare(
            IntPredicate::EQ, right_tag, tag_num, "rel_rok",
        ).unwrap();
        let right_ok_bb = self.context.append_basic_block(self.main_fn, "rel_rok_bb");
        let trap_bb2 = self.context.append_basic_block(self.main_fn, "rel_trap2");
        self.builder.build_conditional_branch(r_ok, right_ok_bb, trap_bb2).unwrap();

        self.builder.position_at_end(trap_bb2);
        self.emit_trap();

        self.builder.position_at_end(right_ok_bb);

        let l_num = self.builder.build_extract_value(left_val, 1, "rel_lnum")
            .unwrap().into_float_value();
        let r_num = self.builder.build_extract_value(right_val, 1, "rel_rnum")
            .unwrap().into_float_value();

        let predicate = match op {
            BinaryOp::Less => inkwell::FloatPredicate::OLT,
            BinaryOp::Greater => inkwell::FloatPredicate::OGT,
            BinaryOp::LessEqual => inkwell::FloatPredicate::OLE,
            BinaryOp::GreaterEqual => inkwell::FloatPredicate::OGE,
            _ => unreachable!(),
        };

        let cmp = self.builder.build_float_compare(predicate, l_num, r_num, "rel_cmp").unwrap();

        let null_ptr = self.context.i8_type().ptr_type(AddressSpace::default()).const_null();
        let result = self.build_value(
            i8_type.const_int(TAG_BOOLEAN as u64, false),
            self.context.f64_type().const_float(0.0),
            null_ptr,
            cmp,
        );
        Ok(result.into())
    }

    /// Compile logical AND (`&&`) with short-circuit evaluation.
    /// Both operands must be Boolean.
    fn compile_logical_and(
        &mut self,
        left: &Expression,
        right: &Expression,
    ) -> Result<BasicValueEnum<'ctx>, String> {
        let i8_type = self.context.i8_type();
        let tag_bool = i8_type.const_int(TAG_BOOLEAN as u64, false);
        let null_ptr = i8_type.ptr_type(AddressSpace::default()).const_null();
        let f64_zero = self.context.f64_type().const_float(0.0);

        // Compile left.
        let left_val = self.compile_expr(left)?.into_struct_value();
        let left_tag = self.builder.build_extract_value(left_val, 0, "and_ltag")
            .unwrap().into_int_value();
        let l_ok = self.builder.build_int_compare(
            IntPredicate::EQ, left_tag, tag_bool, "and_lok",
        ).unwrap();
        let l_ok_bb = self.context.append_basic_block(self.main_fn, "and_lok_bb");
        let l_trap = self.context.append_basic_block(self.main_fn, "and_ltrap");
        self.builder.build_conditional_branch(l_ok, l_ok_bb, l_trap).unwrap();

        self.builder.position_at_end(l_trap);
        self.emit_trap();

        self.builder.position_at_end(l_ok_bb);
        let l_bool = self.builder.build_extract_value(left_val, 3, "and_lbool")
            .unwrap().into_int_value();

        // Short-circuit: if left is false, result is false. Otherwise evaluate right.
        let eval_right_bb = self.context.append_basic_block(self.main_fn, "and_eval_right");
        let merge_bb = self.context.append_basic_block(self.main_fn, "and_merge");
        self.builder.build_conditional_branch(l_bool, eval_right_bb, merge_bb).unwrap();
        let l_false_bb = self.builder.get_insert_block().unwrap();

        // Evaluate right side.
        self.builder.position_at_end(eval_right_bb);
        let right_val = self.compile_expr(right)?.into_struct_value();
        let right_tag = self.builder.build_extract_value(right_val, 0, "and_rtag")
            .unwrap().into_int_value();
        let r_ok = self.builder.build_int_compare(
            IntPredicate::EQ, right_tag, tag_bool, "and_rok",
        ).unwrap();
        let r_ok_bb = self.context.append_basic_block(self.main_fn, "and_rok_bb");
        let r_trap = self.context.append_basic_block(self.main_fn, "and_rtrap");
        self.builder.build_conditional_branch(r_ok, r_ok_bb, r_trap).unwrap();

        self.builder.position_at_end(r_trap);
        self.emit_trap();

        self.builder.position_at_end(r_ok_bb);
        let r_bool = self.builder.build_extract_value(right_val, 3, "and_rbool")
            .unwrap().into_int_value();
        self.builder.build_unconditional_branch(merge_bb).unwrap();
        let r_end_bb = self.builder.get_insert_block().unwrap();

        // Merge: phi node.
        self.builder.position_at_end(merge_bb);
        let bool_type = self.context.bool_type();
        let phi = self.builder.build_phi(bool_type, "and_result").unwrap();
        phi.add_incoming(&[
            (&bool_type.const_int(0, false), l_false_bb),
            (&r_bool, r_end_bb),
        ]);
        let result_bool = phi.as_basic_value().into_int_value();

        let result = self.build_value(tag_bool, f64_zero, null_ptr, result_bool);
        Ok(result.into())
    }

    /// Compile logical OR (`||`) with short-circuit evaluation.
    /// Both operands must be Boolean.
    fn compile_logical_or(
        &mut self,
        left: &Expression,
        right: &Expression,
    ) -> Result<BasicValueEnum<'ctx>, String> {
        let i8_type = self.context.i8_type();
        let tag_bool = i8_type.const_int(TAG_BOOLEAN as u64, false);
        let null_ptr = i8_type.ptr_type(AddressSpace::default()).const_null();
        let f64_zero = self.context.f64_type().const_float(0.0);

        // Compile left.
        let left_val = self.compile_expr(left)?.into_struct_value();
        let left_tag = self.builder.build_extract_value(left_val, 0, "or_ltag")
            .unwrap().into_int_value();
        let l_ok = self.builder.build_int_compare(
            IntPredicate::EQ, left_tag, tag_bool, "or_lok",
        ).unwrap();
        let l_ok_bb = self.context.append_basic_block(self.main_fn, "or_lok_bb");
        let l_trap = self.context.append_basic_block(self.main_fn, "or_ltrap");
        self.builder.build_conditional_branch(l_ok, l_ok_bb, l_trap).unwrap();

        self.builder.position_at_end(l_trap);
        self.emit_trap();

        self.builder.position_at_end(l_ok_bb);
        let l_bool = self.builder.build_extract_value(left_val, 3, "or_lbool")
            .unwrap().into_int_value();

        // Short-circuit: if left is true, result is true. Otherwise evaluate right.
        let eval_right_bb = self.context.append_basic_block(self.main_fn, "or_eval_right");
        let merge_bb = self.context.append_basic_block(self.main_fn, "or_merge");
        self.builder.build_conditional_branch(l_bool, merge_bb, eval_right_bb).unwrap();
        let l_true_bb = self.builder.get_insert_block().unwrap();

        // Evaluate right side.
        self.builder.position_at_end(eval_right_bb);
        let right_val = self.compile_expr(right)?.into_struct_value();
        let right_tag = self.builder.build_extract_value(right_val, 0, "or_rtag")
            .unwrap().into_int_value();
        let r_ok = self.builder.build_int_compare(
            IntPredicate::EQ, right_tag, tag_bool, "or_rok",
        ).unwrap();
        let r_ok_bb = self.context.append_basic_block(self.main_fn, "or_rok_bb");
        let r_trap = self.context.append_basic_block(self.main_fn, "or_rtrap");
        self.builder.build_conditional_branch(r_ok, r_ok_bb, r_trap).unwrap();

        self.builder.position_at_end(r_trap);
        self.emit_trap();

        self.builder.position_at_end(r_ok_bb);
        let r_bool = self.builder.build_extract_value(right_val, 3, "or_rbool")
            .unwrap().into_int_value();
        self.builder.build_unconditional_branch(merge_bb).unwrap();
        let r_end_bb = self.builder.get_insert_block().unwrap();

        // Merge: phi node.
        self.builder.position_at_end(merge_bb);
        let bool_type = self.context.bool_type();
        let phi = self.builder.build_phi(bool_type, "or_result").unwrap();
        phi.add_incoming(&[
            (&bool_type.const_int(1, false), l_true_bb),
            (&r_bool, r_end_bb),
        ]);
        let result_bool = phi.as_basic_value().into_int_value();

        let result = self.build_value(tag_bool, f64_zero, null_ptr, result_bool);
        Ok(result.into())
    }

    /// Compile unary operators (currently only `!`).
    fn compile_unary(
        &mut self,
        op: &UnaryOp,
        operand: &Expression,
    ) -> Result<BasicValueEnum<'ctx>, String> {
        match op {
            UnaryOp::Not => {
                let val = self.compile_expr(operand)?.into_struct_value();
                let i8_type = self.context.i8_type();
                let tag = self.builder.build_extract_value(val, 0, "not_tag")
                    .unwrap().into_int_value();
                let tag_bool = i8_type.const_int(TAG_BOOLEAN as u64, false);
                let is_bool = self.builder.build_int_compare(
                    IntPredicate::EQ, tag, tag_bool, "not_is_bool",
                ).unwrap();

                let ok_bb = self.context.append_basic_block(self.main_fn, "not_ok");
                let trap_bb = self.context.append_basic_block(self.main_fn, "not_trap");
                self.builder.build_conditional_branch(is_bool, ok_bb, trap_bb).unwrap();

                self.builder.position_at_end(trap_bb);
                self.emit_trap();

                self.builder.position_at_end(ok_bb);
                let bool_val = self.builder.build_extract_value(val, 3, "not_val")
                    .unwrap().into_int_value();
                let negated = self.builder.build_not(bool_val, "not_result").unwrap();

                let null_ptr = i8_type.ptr_type(AddressSpace::default()).const_null();
                let result = self.build_value(
                    tag_bool,
                    self.context.f64_type().const_float(0.0),
                    null_ptr,
                    negated,
                );
                Ok(result.into())
            }
        }
    }

    fn _compile_typed_assignment(
        &mut self,
        name: &str,
        type_expr: &TypeExpr,
        value: &Expression,
    ) -> Result<(), String> {
        let inferred = self.infer_expr_type(value);
        match &inferred {
            Ok(actual_type) if !self.inferred_matches_type_expr(actual_type, type_expr) => {
                return Err(format!(
                    "Type mismatch for '{}': expected {}, got {}",
                    name, type_expr, actual_type
                ));
            }
            _ => {} // Either matches or couldn't infer (will runtime check)
        }

        self.compile_assignment(name, value)?;

        // If we couldn't infer statically, emit a runtime tag check.
        if inferred.is_err() {
            self.emit_runtime_type_check_expr(name, type_expr)?;
        }

        self.set_type_annotation(name.to_string(), type_expr.clone());
        Ok(())
    }

    /// Emit a runtime type check for a variable against a TypeExpr.
    /// If the value doesn't match, the program aborts.
    fn emit_runtime_type_check_expr(&mut self, name: &str, type_expr: &TypeExpr) -> Result<(), String> {
        let ptr = self.get_var_ptr(name).ok_or_else(|| {
            format!("Undefined variable '{}' for runtime type check", name)
        })?;
        let val = self.builder.build_load(self.value_type, ptr, "rtcheck_val")
            .unwrap().into_struct_value();

        let matches = self.compile_type_expr_check(val, type_expr)?;

        let ok_bb = self.context.append_basic_block(self.main_fn, "rtcheck_ok");
        let fail_bb = self.context.append_basic_block(self.main_fn, "rtcheck_fail");
        self.builder.build_conditional_branch(matches, ok_bb, fail_bb).unwrap();

        self.builder.position_at_end(fail_bb);
        self.emit_trap();

        self.builder.position_at_end(ok_bb);
        Ok(())
    }

    /// Infer the type of an expression at compile time, using scope information.
    fn infer_expr_type(&self, expr: &Expression) -> Result<String, String> {
        match expr {
            Expression::Number(_) => Ok("Number".to_string()),
            Expression::String(_) | Expression::InterpolatedString(_) => Ok("String".to_string()),
            Expression::Boolean(_) => Ok("Boolean".to_string()),
            Expression::Null => Ok("Null".to_string()),
            Expression::Object { .. } => Ok("Object".to_string()),
            Expression::Particle { .. } => Ok("Object".to_string()),
            Expression::ArrayLiteral(_) => Ok("Array".to_string()),
            Expression::Binary { op: BinaryOp::Add, left, right } => {
                // Try to infer both operand types; if they agree, that's the result.
                let lt = self.infer_expr_type(left);
                let rt = self.infer_expr_type(right);
                match (lt, rt) {
                    (Ok(l), Ok(r)) if l == r && (l == "Number" || l == "String" || l == "Array") => Ok(l),
                    (Ok(l), Ok(r)) if l != r => Err(format!(
                        "Cannot infer type of '+': operands have different types ({} vs {})", l, r
                    )),
                    _ => Err("Cannot infer type of '+' expression at compile time".to_string()),
                }
            }
            Expression::Binary { op: BinaryOp::Sub, .. }
            | Expression::Binary { op: BinaryOp::Mul, .. }
            | Expression::Binary { op: BinaryOp::Div, .. } => Ok("Number".to_string()),
            Expression::Binary { op: BinaryOp::Less, .. }
            | Expression::Binary { op: BinaryOp::Greater, .. }
            | Expression::Binary { op: BinaryOp::LessEqual, .. }
            | Expression::Binary { op: BinaryOp::GreaterEqual, .. }
            | Expression::Binary { op: BinaryOp::And, .. }
            | Expression::Binary { op: BinaryOp::Or, .. } => Ok("Boolean".to_string()),
            Expression::Binary { .. } | Expression::TypeCheck { .. } => Ok("Boolean".to_string()),
            Expression::Unary { op: UnaryOp::Not, .. } => Ok("Boolean".to_string()),
            Expression::Identifier(name) => {
                // Look up the variable's type annotation.
                self.get_type_annotation(name)
                    .map(|te| te.to_string())
                    .ok_or_else(|| {
                        format!("Cannot infer type of '{}' (no type annotation)", name)
                    })
            }
            Expression::PropertyAccess(_, _) | Expression::IndexAccess { .. } | Expression::Call { .. } => {
                Err("Cannot infer type of non-literal expression at compile time".to_string())
            }
        }
    }
}
