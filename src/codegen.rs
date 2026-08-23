use std::collections::{HashMap, HashSet};
use std::path::Path;

use inkwell::basic_block::BasicBlock;
use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::targets::{
    CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine,
};
use inkwell::types::{IntType, PointerType};
use inkwell::values::{FunctionValue, IntValue, PointerValue};
use inkwell::AddressSpace;
use inkwell::IntPredicate;
use inkwell::OptimizationLevel;

use crate::ast::{BinOp, EmitTarget, Expr, NativeFormat, Program, Stmt, UnOp};

/// Byte size of one runtime `CodeValue` slot (`src/runtime.c`; 64 bytes on
/// x86_64, this leaves headroom). Codegen never inspects the struct's fields
/// — it only allocates opaque, 8-byte-aligned buffers of this size on
/// `main`'s stack and lets `runtime.c`'s constructors fill them in,
/// addressed purely as `i8*`. See `runtime.c`'s top-of-file comment for why
/// constructors write through an out-pointer instead of returning by value
/// (C-struct-by-value ABI matching is the thing this sidesteps), and its
/// `_Static_assert` for the check that catches the two numbers drifting.
const VALUE_SIZE: u64 = 80;
const VALUE_ALIGN: u32 = 8;

/// A `.a` static module's three (`vars` optional) entry points, declared up
/// front in `compile_to_object` — see the comment there for why this has to
/// happen before `Gen` exists rather than inside one of its methods.
struct StaticModuleFns<'a> {
    abi_version: FunctionValue<'a>,
    dispatch: FunctionValue<'a>,
    vars: Option<FunctionValue<'a>>,
}

/// Checks every `Expr::Ident` is reachable from an earlier assignment,
/// mirroring the interpreter's runtime "undefined variable" error as a
/// compile-time error instead (the language has no forward references or
/// hoisting, so this is a simple sequential scan) — scope-aware since `if`
/// bodies get their own scope (see memory `new-code-if-scoping`): a name
/// first assigned inside an `if` is only "defined" for the rest of that
/// `if`'s body, not after it, unless it was already defined outside.
fn verify_defined(program: &Program) -> Result<(), String> {
    let mut scopes = vec![HashSet::new()];
    let mut natives = HashSet::new();
    verify_stmts(&program.statements, &mut scopes, &mut natives)
}

fn verify_stmts(
    stmts: &[Stmt],
    scopes: &mut Vec<HashSet<String>>,
    natives: &mut HashSet<String>,
) -> Result<(), String> {
    for stmt in stmts {
        match stmt {
            Stmt::Let { name, value, .. } => {
                verify_expr(value, scopes)?;
                // Always binds in the current scope, even if `name` is
                // already defined here or further out — shadowing.
                scopes.last_mut().unwrap().insert(name.clone());
            }
            Stmt::Link { path, .. } => {
                return Err(format!(
                    "internal error: link \"{path}\" reached codegen unresolved"
                ))
            }
            Stmt::Import {
                alias,
                body,
                exports,
            } => {
                scopes.push(HashSet::new());
                let result = verify_stmts(body, scopes, natives);
                scopes.pop();
                result?;
                // The module's own scope is gone; only what it exported is
                // reachable from here.
                match alias {
                    Some(alias) => {
                        scopes.last_mut().unwrap().insert(alias.clone());
                    }
                    None => {
                        for name in exports {
                            scopes.last_mut().unwrap().insert(name.clone());
                        }
                    }
                }
            }
            Stmt::ImportNative { alias, .. } => {
                // The alias serves two roles, so it is recorded in both
                // namespaces: in `natives` (a separate namespace from
                // `scopes`, matching `interpreter::Environment::native_modules`)
                // so `emit ... to <alias>` can dispatch to the module, and in
                // `scopes` so `alias.name` — the module's exported variables,
                // bound as an object — resolves as an ordinary field access.
                natives.insert(alias.clone());
                scopes.last_mut().unwrap().insert(alias.clone());
            }
            Stmt::Assign { name, value } => {
                verify_expr(value, scopes)?;
                if !is_defined(scopes, name) {
                    return Err(format!(
                        "undefined variable '{name}' (use 'let {name} = ...' to declare it)"
                    ));
                }
            }
            Stmt::Assert(expr) => verify_expr(expr, scopes)?,
            Stmt::If { condition, body } => {
                verify_expr(condition, scopes)?;
                scopes.push(HashSet::new());
                let result = verify_stmts(body, scopes, natives);
                scopes.pop();
                result?;
            }
            Stmt::Block(body) => {
                scopes.push(HashSet::new());
                let result = verify_stmts(body, scopes, natives);
                scopes.pop();
                result?;
            }
            Stmt::Loop {
                var,
                index,
                iterable,
                body,
            } => {
                // The iterable is evaluated in the *enclosing* scope, before
                // the loop variables exist — so `loop x over x` correctly
                // resolves the right-hand `x` to an outer binding, or errors
                // if there isn't one.
                verify_expr(iterable, scopes)?;
                let mut scope = HashSet::new();
                scope.insert(var.clone());
                if let Some(index) = index {
                    scope.insert(index.clone());
                }
                scopes.push(scope);
                let result = verify_stmts(body, scopes, natives);
                scopes.pop();
                result?;
            }
            Stmt::Emit {
                particle,
                target,
                result,
            } => {
                verify_expr(particle, scopes)?;
                if let EmitTarget::Module(alias) = target {
                    if !natives.contains(alias) {
                        return Err(format!(
                            "'emit ... to {alias}' but no native module is linked as '{alias}'"
                        ));
                    }
                }
                if let Some(name) = result {
                    scopes.last_mut().unwrap().insert(name.clone());
                }
            }
            // Nothing to check — the parser already rejected any `break`
            // that isn't inside a loop.
            Stmt::Break => {}
        }
    }
    Ok(())
}

fn is_defined(scopes: &[HashSet<String>], name: &str) -> bool {
    scopes.iter().rev().any(|s| s.contains(name))
}

fn verify_expr(expr: &Expr, scopes: &[HashSet<String>]) -> Result<(), String> {
    match expr {
        Expr::Number(_) | Expr::Str(_) | Expr::Bool(_) | Expr::Null => Ok(()),
        Expr::Ident(name) => {
            if is_defined(scopes, name) {
                Ok(())
            } else {
                Err(format!("undefined variable '{name}'"))
            }
        }
        Expr::Array(items) => items.iter().try_for_each(|item| verify_expr(item, scopes)),
        Expr::Object(fields) => fields
            .iter()
            .try_for_each(|(_, value)| verify_expr(value, scopes)),
        Expr::Field(obj, _) => verify_expr(obj, scopes),
        Expr::Index(arr, index) => {
            verify_expr(arr, scopes)?;
            verify_expr(index, scopes)
        }
        Expr::Unary(_, e) => verify_expr(e, scopes),
        Expr::Binary(lhs, _, rhs) => {
            verify_expr(lhs, scopes)?;
            verify_expr(rhs, scopes)
        }
    }
}

pub fn compile_to_object(program: &Program, obj_path: &Path) -> Result<(), String> {
    verify_defined(program)?;

    let context = Context::create();
    let module = context.create_module("code");
    let builder = context.create_builder();

    let i8_ty = context.i8_type();
    let i32_ty = context.i32_type();
    let i64_ty = context.i64_type();
    let f64_ty = context.f64_type();
    let void_ty = context.void_type();
    let i8_ptr_ty = i8_ty.ptr_type(AddressSpace::default());
    let i8_ptr_ptr_ty = i8_ptr_ty.ptr_type(AddressSpace::default());

    let fn_number = module.add_function(
        "code_number",
        void_ty.fn_type(&[i8_ptr_ty.into(), f64_ty.into()], false),
        None,
    );
    let fn_str = module.add_function(
        "code_str",
        void_ty.fn_type(&[i8_ptr_ty.into(), i8_ptr_ty.into()], false),
        None,
    );
    let fn_bool = module.add_function(
        "code_bool",
        void_ty.fn_type(&[i8_ptr_ty.into(), i32_ty.into()], false),
        None,
    );
    let fn_null = module.add_function(
        "code_null",
        void_ty.fn_type(&[i8_ptr_ty.into()], false),
        None,
    );
    let fn_array = module.add_function(
        "code_array",
        void_ty.fn_type(&[i8_ptr_ty.into(), i8_ptr_ty.into(), i64_ty.into()], false),
        None,
    );
    let fn_object = module.add_function(
        "code_object",
        void_ty.fn_type(
            &[
                i8_ptr_ty.into(),
                i8_ptr_ptr_ty.into(),
                i8_ptr_ty.into(),
                i64_ty.into(),
            ],
            false,
        ),
        None,
    );
    let fn_copy = module.add_function(
        "code_copy",
        void_ty.fn_type(&[i8_ptr_ty.into(), i8_ptr_ty.into()], false),
        None,
    );
    let fn_field = module.add_function(
        "code_field",
        void_ty.fn_type(
            &[i8_ptr_ty.into(), i8_ptr_ty.into(), i8_ptr_ty.into()],
            false,
        ),
        None,
    );
    let fn_index = module.add_function(
        "code_index",
        void_ty.fn_type(
            &[i8_ptr_ty.into(), i8_ptr_ty.into(), i8_ptr_ty.into()],
            false,
        ),
        None,
    );
    let fn_iter_len = module.add_function(
        "code_iter_len",
        i64_ty.fn_type(&[i8_ptr_ty.into()], false),
        None,
    );
    let fn_iter_at = module.add_function(
        "code_iter_at",
        void_ty.fn_type(&[i8_ptr_ty.into(), i8_ptr_ty.into(), i64_ty.into()], false),
        None,
    );
    let fn_release = module.add_function(
        "code_release",
        void_ty.fn_type(&[i8_ptr_ty.into()], false),
        None,
    );
    let fn_core_dispatch = module.add_function(
        "code_core_dispatch",
        void_ty.fn_type(&[i8_ptr_ty.into(), i8_ptr_ty.into()], false),
        None,
    );
    let fn_native_open = module.add_function(
        "code_native_open",
        i8_ptr_ty.fn_type(&[i8_ptr_ty.into()], false),
        None,
    );
    let fn_native_dispatch = module.add_function(
        "code_native_dispatch",
        void_ty.fn_type(
            &[i8_ptr_ty.into(), i8_ptr_ty.into(), i8_ptr_ty.into()],
            false,
        ),
        None,
    );
    let fn_native_vars_object = module.add_function(
        "code_native_vars_object",
        void_ty.fn_type(&[i8_ptr_ty.into(), i8_ptr_ty.into()], false),
        None,
    );
    let fn_native_close = module.add_function(
        "code_native_close",
        void_ty.fn_type(&[i8_ptr_ty.into()], false),
        None,
    );
    let fn_static_module_check = module.add_function(
        "code_static_module_check",
        void_ty.fn_type(&[i32_ty.into(), i8_ptr_ty.into()], false),
        None,
    );
    let fn_static_vars_object = module.add_function(
        "code_static_vars_object",
        void_ty.fn_type(&[i8_ptr_ty.into(), i8_ptr_ty.into()], false),
        None,
    );
    let fn_check_leaks = module.add_function("code_check_leaks", void_ty.fn_type(&[], false), None);
    let fn_dump = module.add_function(
        "code_dump_bindings",
        void_ty.fn_type(
            &[i8_ptr_ptr_ty.into(), i8_ptr_ty.into(), i64_ty.into()],
            false,
        ),
        None,
    );
    let arith_ty = void_ty.fn_type(
        &[i8_ptr_ty.into(), i8_ptr_ty.into(), i8_ptr_ty.into()],
        false,
    );
    let fn_add = module.add_function("code_add", arith_ty, None);
    let fn_sub = module.add_function("code_sub", arith_ty, None);
    let fn_mul = module.add_function("code_mul", arith_ty, None);
    let fn_div = module.add_function("code_div", arith_ty, None);
    let fn_compare = module.add_function(
        "code_compare",
        i64_ty.fn_type(&[i8_ptr_ty.into(), i8_ptr_ty.into()], false),
        None,
    );
    let fn_neg = module.add_function(
        "code_neg",
        void_ty.fn_type(&[i8_ptr_ty.into(), i8_ptr_ty.into()], false),
        None,
    );
    let fn_not = module.add_function(
        "code_not",
        void_ty.fn_type(&[i8_ptr_ty.into(), i8_ptr_ty.into()], false),
        None,
    );
    let fn_bool_value = module.add_function(
        "code_bool_value",
        i32_ty.fn_type(&[i8_ptr_ty.into(), i8_ptr_ty.into()], false),
        None,
    );
    let fn_values_equal = module.add_function(
        "code_values_equal",
        i32_ty.fn_type(&[i8_ptr_ty.into(), i8_ptr_ty.into()], false),
        None,
    );
    let fn_assert = module.add_function(
        "code_assert",
        void_ty.fn_type(&[i8_ptr_ty.into()], false),
        None,
    );

    // Every `.a` static module's `<prefix>_code_module_*` functions,
    // declared up front by alias, in this same textual region as every
    // other `module.add_function` call above — deliberately *not* done
    // inside a `Gen` method later: `Gen`'s methods never name `Module`
    // itself (only the `FunctionValue`s it returns, which — unlike
    // `Module` — carry no `Drop` impl), so `Gen`'s single lifetime
    // parameter never has to be unified with a Drop-implementing local's
    // own generic parameter, which is what a `&Module` parameter or field
    // on `Gen` ran into (rustc's dropck rejects it: `module`, `builder` and
    // `alloca_builder` are all local to this function and must be provably
    // droppable in *some* valid order, which dropck can't confirm once a
    // reference to `Module` — Drop, generic over the same lifetime as
    // everything else here — flows through a `Gen` field or method
    // signature). See `docs/todo/native-module-linking.md`.
    let mut static_native_fns: HashMap<String, StaticModuleFns> = HashMap::new();
    for stmt in &program.statements {
        if let Stmt::ImportNative {
            alias,
            format: NativeFormat::Static { prefix, has_vars },
            ..
        } = stmt
        {
            let abi_version = module.add_function(
                &format!("{prefix}_code_module_abi_version"),
                i32_ty.fn_type(&[], false),
                None,
            );
            let dispatch = module.add_function(
                &format!("{prefix}_code_module_dispatch"),
                void_ty.fn_type(&[i8_ptr_ty.into(), i8_ptr_ty.into()], false),
                None,
            );
            let vars = has_vars.then(|| {
                module.add_function(
                    &format!("{prefix}_code_module_vars"),
                    i8_ptr_ty.fn_type(&[], false),
                    None,
                )
            });
            static_native_fns.insert(
                alias.clone(),
                StaticModuleFns {
                    abi_version,
                    dispatch,
                    vars,
                },
            );
        }
    }

    let main_fn = module.add_function("main", i32_ty.fn_type(&[], false), None);
    // Two blocks, not one: `entry` collects *every* alloca in the program
    // (see `Gen::alloca_builder`) and nothing else, then falls through to
    // `start` where the actual statements go. Allocas have to be gathered
    // somewhere that runs exactly once — an alloca left inside a loop body
    // would be executed per iteration and LLVM reclaims none of them until
    // `main` returns, which is precisely the unbounded stack growth this
    // arrangement exists to prevent.
    let entry = context.append_basic_block(main_fn, "entry");
    let start = context.append_basic_block(main_fn, "start");
    let alloca_builder = context.create_builder();
    alloca_builder.position_at_end(entry);
    builder.position_at_end(start);

    let mut gen = Gen {
        context: &context,
        builder: &builder,
        alloca_builder: &alloca_builder,
        main_fn,
        i8_ty,
        i32_ty,
        i64_ty,
        f64_ty,
        i8_ptr_ty,
        fn_number,
        fn_str,
        fn_bool,
        fn_null,
        fn_array,
        fn_object,
        fn_copy,
        fn_field,
        fn_index,
        fn_add,
        fn_sub,
        fn_mul,
        fn_div,
        fn_compare,
        fn_neg,
        fn_not,
        fn_bool_value,
        fn_values_equal,
        fn_assert,
        fn_iter_len,
        fn_iter_at,
        fn_release,
        fn_core_dispatch,
        fn_native_open,
        fn_native_dispatch,
        fn_native_vars_object,
        fn_native_close,
        fn_static_module_check,
        fn_static_vars_object,
        env: vec![HashMap::new()],
        order: Vec::new(),
        loop_exits: Vec::new(),
        slots: Vec::new(),
        native_links: HashMap::new(),
        static_native_fns,
    };

    for stmt in &program.statements {
        gen.gen_stmt(stmt)?;
    }
    gen.emit_dump(fn_dump)?;
    gen.emit_cleanup(fn_check_leaks)?;

    builder
        .build_return(Some(&i32_ty.const_int(0, false)))
        .map_err(|e| e.to_string())?;

    // Closed only now: every alloca and its zero-init had to be appended to
    // `entry` first, and a block stops accepting instructions once it has a
    // terminator.
    alloca_builder
        .build_unconditional_branch(start)
        .map_err(|e| e.to_string())?;

    module.verify().map_err(|e| e.to_string())?;

    Target::initialize_native(&InitializationConfig::default())?;
    let triple = TargetMachine::get_default_triple();
    let target = Target::from_triple(&triple).map_err(|e| e.to_string())?;
    // PIC, not Default: the system `cc` we link with produces PIE
    // executables by default on this target, which requires
    // position-independent object code — Default relocation produced
    // relocations `ld` rejected ("can not be used when making a PIE
    // object").
    let target_machine = target
        .create_target_machine(
            &triple,
            "generic",
            "",
            OptimizationLevel::Default,
            RelocMode::PIC,
            CodeModel::Default,
        )
        .ok_or_else(|| "failed to create target machine".to_string())?;

    target_machine
        .write_to_file(&module, FileType::Object, obj_path)
        .map_err(|e| e.to_string())
}

/// Codegen state for one module. Only ever used within `compile_to_object`,
/// so a single implicit lifetime tying everything to `context`/`module`/
/// `builder`'s scope is enough — no separate `'ctx` parameter needed.
struct Gen<'a> {
    /// Needed to create new basic blocks for `and`/`or` short-circuiting —
    /// the one place this codegen needs actual control flow, not just
    /// straight-line calls (see `gen_and_or`).
    context: &'a Context,
    builder: &'a Builder<'a>,
    /// Parked permanently at the end of `main`'s `entry` block, which holds
    /// nothing but allocas and their zero-init. Every stack allocation goes
    /// through this builder rather than the main one, so none of them ever
    /// lands inside a loop body — see `gen_loop`'s comment for why that
    /// matters, and `alloc_slot` for why reusing a slot is safe.
    alloca_builder: &'a Builder<'a>,
    main_fn: FunctionValue<'a>,
    i8_ty: inkwell::types::IntType<'a>,
    i32_ty: IntType<'a>,
    i64_ty: IntType<'a>,
    f64_ty: inkwell::types::FloatType<'a>,
    i8_ptr_ty: PointerType<'a>,
    fn_number: FunctionValue<'a>,
    fn_str: FunctionValue<'a>,
    fn_bool: FunctionValue<'a>,
    fn_null: FunctionValue<'a>,
    fn_array: FunctionValue<'a>,
    fn_object: FunctionValue<'a>,
    fn_copy: FunctionValue<'a>,
    fn_field: FunctionValue<'a>,
    fn_index: FunctionValue<'a>,
    fn_add: FunctionValue<'a>,
    fn_sub: FunctionValue<'a>,
    fn_mul: FunctionValue<'a>,
    fn_div: FunctionValue<'a>,
    fn_compare: FunctionValue<'a>,
    fn_neg: FunctionValue<'a>,
    fn_not: FunctionValue<'a>,
    fn_bool_value: FunctionValue<'a>,
    fn_values_equal: FunctionValue<'a>,
    fn_assert: FunctionValue<'a>,
    fn_iter_len: FunctionValue<'a>,
    fn_iter_at: FunctionValue<'a>,
    fn_release: FunctionValue<'a>,
    fn_core_dispatch: FunctionValue<'a>,
    fn_native_open: FunctionValue<'a>,
    fn_native_dispatch: FunctionValue<'a>,
    fn_native_vars_object: FunctionValue<'a>,
    fn_native_close: FunctionValue<'a>,
    fn_static_module_check: FunctionValue<'a>,
    fn_static_vars_object: FunctionValue<'a>,
    /// Scope stack, innermost last — mirrors `interpreter::Environment`
    /// (see memory `new-code-if-scoping`). Each name maps to a *permanent*
    /// slot, allocated once on its first assignment and never reallocated:
    /// every assignment (first or not) `code_copy`s the computed value into
    /// that slot rather than rebinding the pointer to a fresh one. Two
    /// reasons this is always a copy, no zero-copy fast path even for a
    /// first assignment: (1) an `if` that conditionally reassigns an outer
    /// name needs the slot to keep whatever it held before when the branch
    /// doesn't run, which falls out for free *only* if reassignment always
    /// writes into the same stable memory rather than repointing; (2) if
    /// `gen_expr`'s result were ever adopted directly as a brand new name's
    /// permanent slot, `x = y` would alias `x` and `y` onto the exact same
    /// slot (`Expr::Ident` returns the existing pointer, not a copy — see
    /// `gen_expr`'s doc comment) — copying always avoids that regardless of
    /// what the right-hand side is. The slot itself lives on `main`'s stack
    /// for the whole program; what it *names* is a refcounted heap block
    /// that `code_copy` releases on the way out, so rebinding a variable
    /// drops the old value at that point rather than at exit (see
    /// memory `new-code-memory-management`).
    env: Vec<HashMap<String, PointerValue<'a>>>,
    /// First-assignment order, for the final bindings dump — mirrors
    /// `interpreter::Environment::order` exactly.
    order: Vec<String>,
    /// Exit block of each enclosing `loop`, innermost last — where a
    /// `break` branches to. Mirrors `interpreter::Flow::Break` propagating
    /// only as far as the innermost loop.
    loop_exits: Vec<BasicBlock<'a>>,
    /// Every `CodeValue` slot allocated in `entry`, with how many slots it
    /// spans (1 for `alloc_slot`, `len` for `alloc_buffer`). Used only by
    /// `emit_cleanup`, which releases all of them as the program's last act
    /// so that a finished program owns nothing — see `code_check_leaks` in
    /// `runtime.c` for why that is worth the extra calls.
    slots: Vec<(PointerValue<'a>, u64)>,
    /// What `link "x" as x` bound `x` to for `emit ... to x` dispatch, by
    /// alias. A raw SSA value either way, not a `CodeValue` slot (see
    /// `alloc_slot`'s doc comment; this never needs to survive a
    /// reassignment or a loop-body reuse the way a value slot does — it's
    /// written once at `link` time and only ever read afterward). Storing it
    /// directly relies on `link` being top-level-only: the block that opens
    /// a module always dominates every block that could `emit ... to` it.
    native_links: HashMap<String, NativeLink<'a>>,
    /// Every `.a` static module's declared entry points, by alias — built
    /// once before `Gen` exists (see `compile_to_object`), consumed (via
    /// `remove`) the one time each is `link`ed.
    static_native_fns: HashMap<String, StaticModuleFns<'a>>,
}

/// See `Gen::native_links`. A `.so` (`NativeFormat::Dynamic`) dispatches
/// through a `code_native_open` handle, looked up per-call by
/// `code_native_dispatch`; a `.a` (`NativeFormat::Static`) is linked
/// straight into this binary, so its `<prefix>_code_module_dispatch` is
/// called directly, the same shape as `EmitTarget::Core`.
enum NativeLink<'a> {
    Dynamic(PointerValue<'a>),
    Static(FunctionValue<'a>),
}

impl<'a> Gen<'a> {
    /// One `CodeValue` slot, allocated in `entry` and zeroed there once.
    ///
    /// Every call site gets its own slot, so the total is fixed by the size
    /// of the program — a slot inside a loop body is *reused* by each
    /// iteration rather than reallocated. That reuse is safe only because a
    /// value's payload lives in a refcounted heap block, never in the slot:
    /// writing the next iteration's value releases the previous one, and
    /// anything that escaped the iteration (`let tmp = [x]` copied into an
    /// outer binding) still holds its own reference to a block this slot no
    /// longer names. Before values were refcounted, reusing slots this way
    /// would have overwritten an escaped array's storage.
    ///
    /// The zero-init is what makes the very first write to a slot safe:
    /// `runtime.c`'s constructors all release whatever `out` held first, and
    /// an all-zero `CodeValue` reads as a payload-less `CODE_NUMBER` whose
    /// `heap` flag is 0, so that release is a no-op.
    fn alloc_slot(&mut self, hint: &str) -> Result<PointerValue<'a>, String> {
        let ptr = self.alloc_zeroed(VALUE_SIZE, hint)?;
        self.slots.push((ptr, 1));
        Ok(ptr)
    }

    /// A run of `len` contiguous slots — codegen's scratch space for an
    /// array's elements or an object's field values. `runtime.c` copies out
    /// of these into a heap block rather than keeping them, so they are
    /// reusable across iterations for exactly the same reason `alloc_slot`'s
    /// slots are.
    fn alloc_buffer(&mut self, len: u64, hint: &str) -> Result<PointerValue<'a>, String> {
        let ptr = self.alloc_zeroed(VALUE_SIZE * len, hint)?;
        self.slots.push((ptr, len));
        Ok(ptr)
    }

    fn alloc_zeroed(&self, bytes: u64, hint: &str) -> Result<PointerValue<'a>, String> {
        let count = self.i64_ty.const_int(bytes, false);
        let ptr = self
            .alloca_builder
            .build_array_alloca(self.i8_ty, count, hint)
            .map_err(|e| e.to_string())?;
        self.set_alignment(ptr)?;
        if bytes > 0 {
            self.alloca_builder
                .build_memset(ptr, VALUE_ALIGN, self.i8_ty.const_zero(), count)
                .map_err(|e| e.to_string())?;
        }
        Ok(ptr)
    }

    fn set_alignment(&self, ptr: PointerValue<'a>) -> Result<(), String> {
        ptr.as_instruction()
            .expect("alloca is always an instruction")
            .set_alignment(VALUE_ALIGN)
            .map_err(|e| e.to_string())
    }

    /// Address of the `index`-th `VALUE_SIZE`-byte slot inside a buffer
    /// allocated by `alloc_buffer` — a single byte-offset GEP, since the
    /// buffer is a flat `i8` allocation, not an LLVM array/struct type.
    fn slot_at(
        &self,
        buf: PointerValue<'a>,
        index: u64,
        hint: &str,
    ) -> Result<PointerValue<'a>, String> {
        let offset = self.i64_ty.const_int(index * VALUE_SIZE, false);
        unsafe {
            self.builder
                .build_gep(self.i8_ty, buf, &[offset], hint)
                .map_err(|e| e.to_string())
        }
    }

    fn global_str(&self, s: &str, hint: &str) -> Result<PointerValue<'a>, String> {
        self.builder
            .build_global_string_ptr(s, hint)
            .map(|g| g.as_pointer_value())
            .map_err(|e| e.to_string())
    }

    fn gen_stmt(&mut self, stmt: &Stmt) -> Result<(), String> {
        match stmt {
            Stmt::Let { name, value, .. } => self.gen_let(name, value),
            Stmt::Link { path, .. } => Err(format!(
                "internal error: link \"{path}\" reached codegen unresolved"
            )),
            Stmt::Import {
                alias,
                body,
                exports,
            } => self.gen_import(alias.as_deref(), body, exports),
            Stmt::ImportNative {
                alias,
                path,
                format,
            } => self.gen_import_native(alias, path, format),
            Stmt::Assign { name, value } => self.gen_reassign(name, value),
            Stmt::Assert(expr) => {
                let ptr = self.gen_expr(expr)?;
                self.builder
                    .build_call(self.fn_assert, &[ptr.into()], "")
                    .map_err(|e| e.to_string())?;
                Ok(())
            }
            Stmt::If { condition, body } => self.gen_if(condition, body),
            Stmt::Block(body) => self.gen_block(body),
            Stmt::Loop {
                var,
                index,
                iterable,
                body,
            } => self.gen_loop(var, index.as_deref(), iterable, body),
            Stmt::Emit {
                particle,
                target,
                result,
            } => self.gen_emit(particle, target, result.as_deref()),
            Stmt::Break => self.gen_break(),
        }
    }

    /// Branches straight to the innermost enclosing loop's exit block, then
    /// leaves the builder in a *fresh* block so any statements written after
    /// the `break` still have somewhere to be emitted. That block has no
    /// predecessors, so LLVM drops it — which is exactly the semantics
    /// (`loop_break.code` covers this: statements after a `break` never run).
    fn gen_break(&mut self) -> Result<(), String> {
        let exit = *self
            .loop_exits
            .last()
            .expect("the parser rejects 'break' outside a loop");
        self.builder
            .build_unconditional_branch(exit)
            .map_err(|e| e.to_string())?;
        let dead = self.context.append_basic_block(self.main_fn, "after_break");
        self.builder.position_at_end(dead);
        Ok(())
    }

    /// `loop var[, index] over iterable { body }`. The counter and both
    /// variable slots are allocated *once*, in the block the loop starts
    /// from, and rewritten each iteration — the same permanent-slot model as
    /// every other binding (see `env`'s doc comment), which is why no PHI
    /// nodes are needed here either.
    ///
    /// A loop's memory stays bounded by the size of the program, not by the
    /// iteration count, because nothing it emits allocates: all the allocas
    /// live in `entry` (see `alloc_slot`) and every heap block a body
    /// produces is released when its slot is rewritten next time round.
    fn gen_loop(
        &mut self,
        var: &str,
        index: Option<&str>,
        iterable: &Expr,
        body: &[Stmt],
    ) -> Result<(), String> {
        // The loop owns its iterable rather than reading it through whatever
        // slot the expression happened to land in. `gen_expr` returns a
        // *borrowed* pointer for `Expr::Ident`, so iterating `xs` while the
        // body reassigns `xs` would otherwise walk a buffer that had already
        // been released underneath it (`loop_iterable_reassigned.code`).
        // This mirrors the interpreter holding the `Rc` for the loop's
        // duration.
        let evaluated = self.gen_expr(iterable)?;
        let arr_ptr = self.alloc_slot("loopiter")?;
        self.builder
            .build_call(self.fn_copy, &[arr_ptr.into(), evaluated.into()], "")
            .map_err(|e| e.to_string())?;

        let len = self
            .builder
            .build_call(self.fn_iter_len, &[arr_ptr.into()], "iterlen")
            .map_err(|e| e.to_string())?
            .try_as_basic_value()
            .left()
            .expect("code_iter_len returns i64, not void")
            .into_int_value();

        let counter = self
            .alloca_builder
            .build_alloca(self.i64_ty, "loop_i")
            .map_err(|e| e.to_string())?;
        self.builder
            .build_store(counter, self.i64_ty.const_zero())
            .map_err(|e| e.to_string())?;
        let var_slot = self.alloc_slot("loopvar")?;
        let index_slot = match index {
            Some(_) => Some(self.alloc_slot("loopidx")?),
            None => None,
        };

        let head_bb = self.context.append_basic_block(self.main_fn, "loop_head");
        let body_bb = self.context.append_basic_block(self.main_fn, "loop_body");
        let after_bb = self.context.append_basic_block(self.main_fn, "loop_after");
        self.builder
            .build_unconditional_branch(head_bb)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(head_bb);
        let i = self
            .builder
            .build_load(self.i64_ty, counter, "i")
            .map_err(|e| e.to_string())?
            .into_int_value();
        let more = self
            .builder
            .build_int_compare(IntPredicate::SLT, i, len, "loop_more")
            .map_err(|e| e.to_string())?;
        self.builder
            .build_conditional_branch(more, body_bb, after_bb)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(body_bb);
        self.builder
            .build_call(
                self.fn_iter_at,
                &[var_slot.into(), arr_ptr.into(), i.into()],
                "",
            )
            .map_err(|e| e.to_string())?;
        if let Some(index_slot) = index_slot {
            let as_f64 = self
                .builder
                .build_signed_int_to_float(i, self.f64_ty, "idx_f64")
                .map_err(|e| e.to_string())?;
            self.builder
                .build_call(self.fn_number, &[index_slot.into(), as_f64.into()], "")
                .map_err(|e| e.to_string())?;
        }

        let mut scope = HashMap::new();
        scope.insert(var.to_string(), var_slot);
        if let (Some(index), Some(index_slot)) = (index, index_slot) {
            scope.insert(index.to_string(), index_slot);
        }
        self.env.push(scope);
        self.loop_exits.push(after_bb);
        for stmt in body {
            self.gen_stmt(stmt)?;
        }
        self.loop_exits.pop();
        self.env.pop();

        // Emitted at whatever block the body *ended* in — after an `if` that
        // is `if_after`, after a `break` it's the dead block `gen_break` left
        // us in. Either way that's the fall-through path, so the increment
        // and back-edge belong there.
        let current = self
            .builder
            .build_load(self.i64_ty, counter, "i_cur")
            .map_err(|e| e.to_string())?
            .into_int_value();
        let next = self
            .builder
            .build_int_add(current, self.i64_ty.const_int(1, false), "i_next")
            .map_err(|e| e.to_string())?;
        self.builder
            .build_store(counter, next)
            .map_err(|e| e.to_string())?;
        self.builder
            .build_unconditional_branch(head_bb)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(after_bb);
        Ok(())
    }

    /// Unconditional version of `gen_if`'s scope handling, minus the
    /// condition/branch — always runs, so needs no basic blocks at all.
    fn gen_block(&mut self, body: &[Stmt]) -> Result<(), String> {
        self.env.push(HashMap::new());
        for stmt in body {
            self.gen_stmt(stmt)?;
        }
        self.env.pop();
        Ok(())
    }

    /// A resolved `link`. The module's body runs in its own scope, and only
    /// what it exported survives that scope closing.
    ///
    /// Nothing here is module-specific machinery: the alias case builds an
    /// ordinary object out of the exported names — literally by handing
    /// `gen_expr` an `Expr::Object`, so `alias.name` is the same field access
    /// as any other — and the flatten case re-registers the module's *own*
    /// slots under the enclosing scope. Slots live in `main`'s entry block
    /// (see `alloc_slot`), so they stay valid long after the scope that
    /// introduced them is gone.
    fn gen_import(
        &mut self,
        alias: Option<&str>,
        body: &[Stmt],
        exports: &[String],
    ) -> Result<(), String> {
        self.env.push(HashMap::new());
        for stmt in body {
            self.gen_stmt(stmt)?;
        }

        // Both of these have to happen while the module's scope is still on
        // the stack — that is the only place its names resolve.
        let object = match alias {
            Some(_) => {
                let fields = exports
                    .iter()
                    .map(|name| (name.clone(), Expr::Ident(name.clone())))
                    .collect();
                Some(self.gen_expr(&Expr::Object(fields))?)
            }
            None => None,
        };
        let mut pairs = Vec::with_capacity(exports.len());
        for name in exports {
            let slot = self
                .lookup(name)
                .ok_or_else(|| format!("module exports '{name}' but never defines it"))?;
            pairs.push((name.clone(), slot));
        }
        self.env.pop();

        match (alias, object) {
            (Some(alias), Some(object)) => {
                let permanent = self.alloc_slot("module")?;
                self.builder
                    .build_call(self.fn_copy, &[permanent.into(), object.into()], "")
                    .map_err(|e| e.to_string())?;
                self.bind(alias, permanent);
            }
            _ => {
                for (name, slot) in pairs {
                    if self.lookup(&name).is_some() {
                        return Err(format!(
                            "linking would redefine '{name}' — rename it, or use \
                             'link ... as <name>' to keep the module's names apart"
                        ));
                    }
                    self.bind(&name, slot);
                }
            }
        }
        Ok(())
    }

    /// A resolved native `link` (`link "x.so" as x` or `link "x.a" as x`).
    /// `Dynamic` opens the module exactly once, at the point the `link`
    /// appears in program order — `code_native_open` aborts (via
    /// `code_runtime_error`) on a bad path, wrong ABI version, or a module
    /// missing a required symbol, so there's nothing here to check; the
    /// returned handle is just remembered under `alias` for `gen_emit` to
    /// call through later.
    ///
    /// `Static` has no handle to open at all — its `<prefix>_code_module_*`
    /// functions are already declared (see `static_native_fns`), so this
    /// just calls the version check and vars lookup, then remembers the
    /// dispatch function directly for `gen_emit`.
    fn gen_import_native(
        &mut self,
        alias: &str,
        path: &str,
        format: &NativeFormat,
    ) -> Result<(), String> {
        match format {
            NativeFormat::Dynamic => {
                let path_ptr = self.global_str(path, "native_path")?;
                let handle = self
                    .builder
                    .build_call(self.fn_native_open, &[path_ptr.into()], "native_handle")
                    .map_err(|e| e.to_string())?
                    .try_as_basic_value()
                    .left()
                    .expect("code_native_open returns i8*, not void")
                    .into_pointer_value();
                self.native_links
                    .insert(alias.to_string(), NativeLink::Dynamic(handle));
                // The module's exported variables (constants) become an
                // object bound under `alias`, so `alias.name` is ordinary
                // field access — the same binding `gen_import`'s alias uses.
                // The object is built at *runtime* (the module is dlopen'd
                // at runtime, so its variables are only known then), by
                // `code_native_vars_object` reading the module's optional
                // `code_module_vars` export and deep-copying each value out.
                // A module with no such export yields an empty object.
                let permanent = self.alloc_slot("module")?;
                self.builder
                    .build_call(
                        self.fn_native_vars_object,
                        &[handle.into(), permanent.into()],
                        "",
                    )
                    .map_err(|e| e.to_string())?;
                self.bind(alias, permanent);
            }
            NativeFormat::Static { prefix, .. } => {
                // Declared up front in `compile_to_object`, by alias — see
                // `static_native_fns`'s doc comment for why that has to
                // happen before `Gen` exists rather than here.
                let fns = self
                    .static_native_fns
                    .remove(alias)
                    .expect("compile_to_object declared this alias's functions up front");

                let version = self
                    .builder
                    .build_call(fns.abi_version, &[], "static_module_version")
                    .map_err(|e| e.to_string())?
                    .try_as_basic_value()
                    .left()
                    .expect("<prefix>_code_module_abi_version returns i32, not void")
                    .into_int_value();
                let prefix_ptr = self.global_str(prefix, "native_prefix")?;
                self.builder
                    .build_call(
                        self.fn_static_module_check,
                        &[version.into(), prefix_ptr.into()],
                        "",
                    )
                    .map_err(|e| e.to_string())?;

                let vars_ptr = if let Some(vars_fn) = fns.vars {
                    self.builder
                        .build_call(vars_fn, &[], "static_module_vars")
                        .map_err(|e| e.to_string())?
                        .try_as_basic_value()
                        .left()
                        .expect("<prefix>_code_module_vars returns i8*, not void")
                        .into_pointer_value()
                } else {
                    self.i8_ptr_ty.const_null()
                };
                let permanent = self.alloc_slot("module")?;
                self.builder
                    .build_call(
                        self.fn_static_vars_object,
                        &[vars_ptr.into(), permanent.into()],
                        "",
                    )
                    .map_err(|e| e.to_string())?;
                self.bind(alias, permanent);

                self.native_links
                    .insert(alias.to_string(), NativeLink::Static(fns.dispatch));
            }
        }
        Ok(())
    }

    /// `emit particle to core|<alias> [get name]`. The handler call always
    /// writes into a temp slot first, exactly like every other `gen_expr`
    /// call — only *if* `get name` is present does that result get copied
    /// into a fresh permanent slot and bound, the same two-step `gen_let`
    /// already uses. Without `get`, the temp slot is simply never bound to
    /// anything; `emit_cleanup`'s end-of-program sweep still releases it
    /// like any other slot.
    fn gen_emit(
        &mut self,
        particle: &Expr,
        target: &EmitTarget,
        result: Option<&str>,
    ) -> Result<(), String> {
        let particle_ptr = self.gen_expr(particle)?;
        let temp = self.alloc_slot("emit_result")?;
        match target {
            EmitTarget::Core => {
                self.builder
                    .build_call(
                        self.fn_core_dispatch,
                        &[temp.into(), particle_ptr.into()],
                        "",
                    )
                    .map_err(|e| e.to_string())?;
            }
            EmitTarget::Module(alias) => {
                // `verify_defined` already rejected an `alias` that was
                // never `link`ed, so this is always present.
                match self
                    .native_links
                    .get(alias)
                    .expect("verify_defined checked this alias was linked")
                {
                    NativeLink::Dynamic(handle) => {
                        let handle = *handle;
                        self.builder
                            .build_call(
                                self.fn_native_dispatch,
                                &[handle.into(), temp.into(), particle_ptr.into()],
                                "",
                            )
                            .map_err(|e| e.to_string())?;
                    }
                    NativeLink::Static(dispatch) => {
                        // Linked straight into this binary — a direct call,
                        // no handle, exactly like `EmitTarget::Core` above.
                        let dispatch = *dispatch;
                        self.builder
                            .build_call(dispatch, &[temp.into(), particle_ptr.into()], "")
                            .map_err(|e| e.to_string())?;
                    }
                }
            }
        }
        if let Some(name) = result {
            let permanent = self.alloc_slot("var")?;
            self.builder
                .build_call(self.fn_copy, &[permanent.into(), temp.into()], "")
                .map_err(|e| e.to_string())?;
            self.bind(name, permanent);
        }
        Ok(())
    }

    /// Registers `slot` under `name` in the current scope, recording it for
    /// the bindings dump if that scope is the outermost one — the same rule
    /// `interpreter::Environment::declare` follows.
    fn bind(&mut self, name: &str, slot: PointerValue<'a>) {
        if self.env.len() == 1 && !self.env[0].contains_key(name) {
            self.order.push(name.to_string());
        }
        self.env.last_mut().unwrap().insert(name.to_string(), slot);
    }

    /// `let name = value` — always allocates a brand new permanent slot in
    /// the *current* scope, even if `name` already exists here or further
    /// out (shadowing; re-`let`-ing the same name in the same scope just
    /// overwrites that scope's map entry, still correct). See `env`'s doc
    /// comment for why every assignment copies rather than ever adopting
    /// `gen_expr`'s pointer directly.
    fn gen_let(&mut self, name: &str, value: &Expr) -> Result<(), String> {
        let value_ptr = self.gen_expr(value)?;
        let permanent = self.alloc_slot("var")?;
        self.builder
            .build_call(self.fn_copy, &[permanent.into(), value_ptr.into()], "")
            .map_err(|e| e.to_string())?;
        if self.env.len() == 1 && !self.env[0].contains_key(name) {
            self.order.push(name.to_string());
        }
        self.env
            .last_mut()
            .unwrap()
            .insert(name.to_string(), permanent);
        Ok(())
    }

    /// Bare `name = value` (no `let`) — reassigns an existing binding.
    /// `verify_defined` already guarantees `name` is bound somewhere before
    /// codegen ever runs, so `lookup` here can't miss.
    fn gen_reassign(&mut self, name: &str, value: &Expr) -> Result<(), String> {
        let value_ptr = self.gen_expr(value)?;
        let existing = self
            .lookup(name)
            .expect("verify_defined guarantees this name is bound");
        self.builder
            .build_call(self.fn_copy, &[existing.into(), value_ptr.into()], "")
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Searches the scope stack innermost-to-outermost, returning the
    /// first match — the permanent slot for `name`, wherever it lives.
    fn lookup(&self, name: &str) -> Option<PointerValue<'a>> {
        self.env
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).copied())
    }

    /// No `else`, ever — a deliberate language decision, not a missing
    /// feature (see `ast::Stmt::If`'s doc comment). `body` gets its own
    /// scope, pushed before and popped after generating it; a name
    /// assigned inside that already exists in an outer scope resolves
    /// through `gen_assign`'s `lookup` to that outer permanent slot, so
    /// mutating it here is correctly visible after the `if` — and, just as
    /// correctly, simply doesn't happen if the branch doesn't run, with no
    /// merge/phi logic needed (see `env`'s doc comment).
    fn gen_if(&mut self, condition: &Expr, body: &[Stmt]) -> Result<(), String> {
        let cond_ptr = self.gen_expr(condition)?;
        let cond_bool = self.call_bool_value(cond_ptr, "if")?;
        let cond = self
            .builder
            .build_int_compare(
                IntPredicate::NE,
                cond_bool,
                self.i32_ty.const_int(0, false),
                "if_cond",
            )
            .map_err(|e| e.to_string())?;

        let then_bb = self.context.append_basic_block(self.main_fn, "if_then");
        let after_bb = self.context.append_basic_block(self.main_fn, "if_after");
        self.builder
            .build_conditional_branch(cond, then_bb, after_bb)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(then_bb);
        self.env.push(HashMap::new());
        for stmt in body {
            self.gen_stmt(stmt)?;
        }
        self.env.pop();
        self.builder
            .build_unconditional_branch(after_bb)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(after_bb);
        Ok(())
    }

    /// Evaluates `expr` into a `CodeValue` and returns a pointer to it.
    ///
    /// The pointer is *borrowed*, not owned: for `Expr::Ident` it is the
    /// variable's own slot, so it stays valid only as long as that binding
    /// is untouched. Anything that needs the value to outlive the statement
    /// — a variable being assigned, an array element, an object field, the
    /// bindings dump, a loop's iterable — copies it into storage of its own
    /// via `code_copy`, which is what takes the reference. `gen_loop` is
    /// the one place where forgetting that was an actual bug rather than a
    /// style point; see its comment.
    fn gen_expr(&mut self, expr: &Expr) -> Result<PointerValue<'a>, String> {
        match expr {
            Expr::Number(n) => {
                let slot = self.alloc_slot("num")?;
                let arg = self.f64_ty.const_float(*n);
                self.builder
                    .build_call(self.fn_number, &[slot.into(), arg.into()], "")
                    .map_err(|e| e.to_string())?;
                Ok(slot)
            }
            Expr::Str(s) => {
                let slot = self.alloc_slot("str")?;
                let g = self.global_str(s, "strlit")?;
                self.builder
                    .build_call(self.fn_str, &[slot.into(), g.into()], "")
                    .map_err(|e| e.to_string())?;
                Ok(slot)
            }
            Expr::Bool(b) => {
                let slot = self.alloc_slot("bool")?;
                let arg = self.i32_ty.const_int(*b as u64, false);
                self.builder
                    .build_call(self.fn_bool, &[slot.into(), arg.into()], "")
                    .map_err(|e| e.to_string())?;
                Ok(slot)
            }
            Expr::Null => {
                let slot = self.alloc_slot("null")?;
                self.builder
                    .build_call(self.fn_null, &[slot.into()], "")
                    .map_err(|e| e.to_string())?;
                Ok(slot)
            }
            Expr::Ident(name) => self
                .lookup(name)
                .ok_or_else(|| format!("undefined variable '{name}'")),
            Expr::Array(items) => {
                let len = items.len() as u64;
                let buf = self.alloc_buffer(len, "arrbuf")?;
                for (i, item) in items.iter().enumerate() {
                    let item_ptr = self.gen_expr(item)?;
                    let dest = self.slot_at(buf, i as u64, "arrelem")?;
                    self.builder
                        .build_call(self.fn_copy, &[dest.into(), item_ptr.into()], "")
                        .map_err(|e| e.to_string())?;
                }
                let out = self.alloc_slot("arr")?;
                let len_val = self.i64_ty.const_int(len, false);
                self.builder
                    .build_call(self.fn_array, &[out.into(), buf.into(), len_val.into()], "")
                    .map_err(|e| e.to_string())?;
                Ok(out)
            }
            Expr::Object(fields) => {
                let len = fields.len() as u64;
                let key_count = self.i64_ty.const_int(len, false);
                // Plain pointers to string literals, fully rewritten before
                // every use, so this one needs no zero-init — but it still
                // belongs in `entry` like every other alloca.
                let keys_buf = self
                    .alloca_builder
                    .build_array_alloca(self.i8_ptr_ty, key_count, "objkeys")
                    .map_err(|e| e.to_string())?;
                let values_buf = self.alloc_buffer(len, "objvals")?;

                for (i, (key, value)) in fields.iter().enumerate() {
                    let key_ptr = self.global_str(key, "keylit")?;
                    let key_slot = unsafe {
                        self.builder
                            .build_gep(
                                self.i8_ptr_ty,
                                keys_buf,
                                &[self.i64_ty.const_int(i as u64, false)],
                                "keyslot",
                            )
                            .map_err(|e| e.to_string())?
                    };
                    self.builder
                        .build_store(key_slot, key_ptr)
                        .map_err(|e| e.to_string())?;

                    let value_ptr = self.gen_expr(value)?;
                    let dest = self.slot_at(values_buf, i as u64, "objelem")?;
                    self.builder
                        .build_call(self.fn_copy, &[dest.into(), value_ptr.into()], "")
                        .map_err(|e| e.to_string())?;
                }

                let out = self.alloc_slot("obj")?;
                let len_val = self.i64_ty.const_int(len, false);
                self.builder
                    .build_call(
                        self.fn_object,
                        &[
                            out.into(),
                            keys_buf.into(),
                            values_buf.into(),
                            len_val.into(),
                        ],
                        "",
                    )
                    .map_err(|e| e.to_string())?;
                Ok(out)
            }
            Expr::Field(obj, field) => {
                let obj_ptr = self.gen_expr(obj)?;
                let field_ptr = self.global_str(field, "fieldname")?;
                let out = self.alloc_slot("field")?;
                self.builder
                    .build_call(
                        self.fn_field,
                        &[out.into(), obj_ptr.into(), field_ptr.into()],
                        "",
                    )
                    .map_err(|e| e.to_string())?;
                Ok(out)
            }
            Expr::Index(arr, index) => {
                let arr_ptr = self.gen_expr(arr)?;
                let index_ptr = self.gen_expr(index)?;
                let out = self.alloc_slot("index")?;
                self.builder
                    .build_call(
                        self.fn_index,
                        &[out.into(), arr_ptr.into(), index_ptr.into()],
                        "",
                    )
                    .map_err(|e| e.to_string())?;
                Ok(out)
            }
            Expr::Unary(op, e) => {
                let ptr = self.gen_expr(e)?;
                let fn_val = match op {
                    UnOp::Neg => self.fn_neg,
                    UnOp::Not => self.fn_not,
                };
                let out = self.alloc_slot("unary")?;
                self.builder
                    .build_call(fn_val, &[out.into(), ptr.into()], "")
                    .map_err(|e| e.to_string())?;
                Ok(out)
            }
            // `and`/`or` need actual branches to short-circuit (the right
            // side must not even be evaluated when the left side already
            // decided the result) — every other operator here is a
            // straight-line runtime call.
            Expr::Binary(lhs, BinOp::And, rhs) => self.gen_and_or(lhs, rhs, true),
            Expr::Binary(lhs, BinOp::Or, rhs) => self.gen_and_or(lhs, rhs, false),
            Expr::Binary(lhs, BinOp::Eq, rhs) => self.gen_equality(lhs, rhs, false),
            Expr::Binary(lhs, BinOp::Ne, rhs) => self.gen_equality(lhs, rhs, true),
            Expr::Binary(lhs, op @ (BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge), rhs) => {
                self.gen_compare(lhs, *op, rhs)
            }
            Expr::Binary(lhs, op, rhs) => {
                let lhs_ptr = self.gen_expr(lhs)?;
                let rhs_ptr = self.gen_expr(rhs)?;
                let fn_val = match op {
                    BinOp::Add => self.fn_add,
                    BinOp::Sub => self.fn_sub,
                    BinOp::Mul => self.fn_mul,
                    BinOp::Div => self.fn_div,
                    BinOp::Eq | BinOp::Ne | BinOp::And | BinOp::Or => unreachable!("handled above"),
                    BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge => unreachable!("handled above"),
                };
                let out = self.alloc_slot("binop")?;
                self.builder
                    .build_call(fn_val, &[out.into(), lhs_ptr.into(), rhs_ptr.into()], "")
                    .map_err(|e| e.to_string())?;
                Ok(out)
            }
        }
    }

    /// `is_and`: `true` for `and` (short-circuits on `false`), `false` for
    /// `or` (short-circuits on `true`). Both need a real branch — the right
    /// side is only evaluated when the left side didn't already decide the
    /// result — so this builds two new basic blocks in `main` and merges
    /// through a stack slot rather than a PHI node (simpler to get right
    /// than threading `BasicBlock` predecessors through inkwell's API).
    fn gen_and_or(
        &mut self,
        lhs: &Expr,
        rhs: &Expr,
        is_and: bool,
    ) -> Result<PointerValue<'a>, String> {
        let op_name = if is_and { "and" } else { "or" };
        let result_slot = self
            .alloca_builder
            .build_alloca(self.i32_ty, "logic_result")
            .map_err(|e| e.to_string())?;

        let lhs_ptr = self.gen_expr(lhs)?;
        let lhs_bool = self.call_bool_value(lhs_ptr, op_name)?;
        let short_circuits = self
            .builder
            .build_int_compare(
                IntPredicate::EQ,
                lhs_bool,
                self.i32_ty.const_int(if is_and { 0 } else { 1 }, false),
                "short_circuits",
            )
            .map_err(|e| e.to_string())?;

        let short_bb = self.context.append_basic_block(self.main_fn, "logic_short");
        let rhs_bb = self.context.append_basic_block(self.main_fn, "logic_rhs");
        let merge_bb = self.context.append_basic_block(self.main_fn, "logic_merge");
        self.builder
            .build_conditional_branch(short_circuits, short_bb, rhs_bb)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(short_bb);
        self.builder
            .build_store(result_slot, lhs_bool)
            .map_err(|e| e.to_string())?;
        self.builder
            .build_unconditional_branch(merge_bb)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(rhs_bb);
        let rhs_ptr = self.gen_expr(rhs)?;
        let rhs_bool = self.call_bool_value(rhs_ptr, op_name)?;
        self.builder
            .build_store(result_slot, rhs_bool)
            .map_err(|e| e.to_string())?;
        self.builder
            .build_unconditional_branch(merge_bb)
            .map_err(|e| e.to_string())?;

        self.builder.position_at_end(merge_bb);
        let final_bool = self
            .builder
            .build_load(self.i32_ty, result_slot, "logic_final")
            .map_err(|e| e.to_string())?
            .into_int_value();
        let out = self.alloc_slot("logic")?;
        self.builder
            .build_call(self.fn_bool, &[out.into(), final_bool.into()], "")
            .map_err(|e| e.to_string())?;
        Ok(out)
    }

    fn call_bool_value(
        &self,
        ptr: PointerValue<'a>,
        op_name: &str,
    ) -> Result<IntValue<'a>, String> {
        let op_name_ptr = self.global_str(op_name, "opname")?;
        Ok(self
            .builder
            .build_call(self.fn_bool_value, &[ptr.into(), op_name_ptr.into()], "")
            .map_err(|e| e.to_string())?
            .try_as_basic_value()
            .left()
            .expect("code_bool_value returns i32, not void")
            .into_int_value())
    }

    /// `negate`: `false` for `==`, `true` for `!=` — both go through
    /// `code_values_equal` (well-defined for any two values, including
    /// mismatched kinds) and just flip the bit for `!=`.
    fn gen_equality(
        &mut self,
        lhs: &Expr,
        rhs: &Expr,
        negate: bool,
    ) -> Result<PointerValue<'a>, String> {
        let lhs_ptr = self.gen_expr(lhs)?;
        let rhs_ptr = self.gen_expr(rhs)?;
        let equal = self
            .builder
            .build_call(self.fn_values_equal, &[lhs_ptr.into(), rhs_ptr.into()], "")
            .map_err(|e| e.to_string())?
            .try_as_basic_value()
            .left()
            .expect("code_values_equal returns i32, not void")
            .into_int_value();
        let result = if negate {
            self.builder
                .build_int_compare(
                    IntPredicate::EQ,
                    equal,
                    self.i32_ty.const_int(0, false),
                    "ne_result",
                )
                .map_err(|e| e.to_string())?
        } else {
            self.builder
                .build_int_compare(
                    IntPredicate::NE,
                    equal,
                    self.i32_ty.const_int(0, false),
                    "eq_result",
                )
                .map_err(|e| e.to_string())?
        };
        let as_i32 = self
            .builder
            .build_int_z_extend(result, self.i32_ty, "eq_as_i32")
            .map_err(|e| e.to_string())?;
        let out = self.alloc_slot("eq")?;
        self.builder
            .build_call(self.fn_bool, &[out.into(), as_i32.into()], "")
            .map_err(|e| e.to_string())?;
        Ok(out)
    }

    /// `<`/`>`/`<=`/`>=` all go through one `code_compare` runtime call
    /// (returns -1/0/1, or aborts for unorderable operands) and then just
    /// `icmp` the result against 0 — see `runtime.c`'s `code_compare`.
    fn gen_compare(
        &mut self,
        lhs: &Expr,
        op: BinOp,
        rhs: &Expr,
    ) -> Result<PointerValue<'a>, String> {
        let lhs_ptr = self.gen_expr(lhs)?;
        let rhs_ptr = self.gen_expr(rhs)?;
        let cmp = self
            .builder
            .build_call(self.fn_compare, &[lhs_ptr.into(), rhs_ptr.into()], "")
            .map_err(|e| e.to_string())?
            .try_as_basic_value()
            .left()
            .expect("code_compare returns i64, not void")
            .into_int_value();
        let zero = self.i64_ty.const_int(0, true);
        let predicate = match op {
            BinOp::Lt => IntPredicate::SLT,
            BinOp::Gt => IntPredicate::SGT,
            BinOp::Le => IntPredicate::SLE,
            BinOp::Ge => IntPredicate::SGE,
            _ => unreachable!("gen_compare only called for ordering operators"),
        };
        let result = self
            .builder
            .build_int_compare(predicate, cmp, zero, "cmp_result")
            .map_err(|e| e.to_string())?;
        let as_i32 = self
            .builder
            .build_int_z_extend(result, self.i32_ty, "cmp_as_i32")
            .map_err(|e| e.to_string())?;
        let out = self.alloc_slot("cmp")?;
        self.builder
            .build_call(self.fn_bool, &[out.into(), as_i32.into()], "")
            .map_err(|e| e.to_string())?;
        Ok(out)
    }

    fn emit_dump(&mut self, fn_dump: FunctionValue<'a>) -> Result<(), String> {
        let count = self.order.len() as u64;
        let count_val = self.i64_ty.const_int(count, false);

        let names_buf = self
            .alloca_builder
            .build_array_alloca(self.i8_ptr_ty, count_val, "names")
            .map_err(|e| e.to_string())?;
        let values_buf = self.alloc_buffer(count, "dumpvals")?;

        let order = self.order.clone();
        for (i, name) in order.iter().enumerate() {
            let name_ptr = self.global_str(name, "namelit")?;
            let name_slot = unsafe {
                self.builder
                    .build_gep(
                        self.i8_ptr_ty,
                        names_buf,
                        &[self.i64_ty.const_int(i as u64, false)],
                        "nameslot",
                    )
                    .map_err(|e| e.to_string())?
            };
            self.builder
                .build_store(name_slot, name_ptr)
                .map_err(|e| e.to_string())?;

            let src = self.lookup(name).expect("binding must exist by dump time");
            let dest = self.slot_at(values_buf, i as u64, "dumpslot")?;
            self.builder
                .build_call(self.fn_copy, &[dest.into(), src.into()], "")
                .map_err(|e| e.to_string())?;
        }

        self.builder
            .build_call(
                fn_dump,
                &[names_buf.into(), values_buf.into(), count_val.into()],
                "",
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Releases every slot in the program, then checks nothing is left. Runs
    /// after `emit_dump`, so the values are still intact while they're being
    /// printed.
    ///
    /// Strictly speaking this is unnecessary — the process is about to exit
    /// and the OS reclaims everything either way. It exists to make the
    /// refcounting *testable*: with it, a finished program provably owns
    /// nothing, so a single missing release anywhere shows up as a non-zero
    /// `live_blocks` under `CODE_CHECK_LEAKS` instead of being invisible.
    /// The cost is one call per slot, i.e. bounded by program size, all of it
    /// after the last observable output.
    fn emit_cleanup(&mut self, fn_check_leaks: FunctionValue<'a>) -> Result<(), String> {
        let slots = std::mem::take(&mut self.slots);
        for (buf, count) in slots {
            for i in 0..count {
                let slot = self.slot_at(buf, i, "cleanup")?;
                self.builder
                    .build_call(self.fn_release, &[slot.into()], "")
                    .map_err(|e| e.to_string())?;
            }
        }
        // Every linked `.so`'s handle (see `code_native_open`) — the same
        // "owns nothing at exit" rule as the `CodeValue` slots above, just a
        // plain `free` instead of a refcount release. A `.a`'s `NativeLink`
        // owns no allocation of its own (it's just an extern function
        // reference into this very binary), so there's nothing to close.
        let links = std::mem::take(&mut self.native_links);
        for (_, link) in links {
            if let NativeLink::Dynamic(handle) = link {
                self.builder
                    .build_call(self.fn_native_close, &[handle.into()], "")
                    .map_err(|e| e.to_string())?;
            }
        }
        self.builder
            .build_call(fn_check_leaks, &[], "")
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}
