use std::collections::{HashMap, HashSet};
use std::path::Path;

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

use crate::ast::{BinOp, Expr, Program, Stmt, UnOp};

/// Byte size of one runtime `CodeValue` slot (`src/runtime.c`; currently 56
/// bytes on x86_64, this leaves headroom). Codegen never inspects the
/// struct's fields — it only allocates opaque, 8-byte-aligned buffers of
/// this size on `main`'s stack and lets `runtime.c`'s constructors fill them
/// in, addressed purely as `i8*`. See `runtime.c`'s top-of-file comment for
/// why constructors write through an out-pointer instead of returning by
/// value (C-struct-by-value ABI matching is the thing this sidesteps).
const VALUE_SIZE: u64 = 64;
const VALUE_ALIGN: u32 = 8;

/// Checks every `Expr::Ident` is reachable from an earlier `Stmt::Assign`,
/// mirroring the interpreter's runtime "undefined variable" error as a
/// compile-time error instead (the language has no forward references or
/// hoisting, so this is a simple sequential scan).
fn verify_defined(program: &Program) -> Result<(), String> {
    let mut defined = HashSet::new();
    for stmt in &program.statements {
        match stmt {
            Stmt::Assign { name, value } => {
                verify_expr(value, &defined)?;
                defined.insert(name.clone());
            }
            Stmt::Assert(expr) => verify_expr(expr, &defined)?,
        }
    }
    Ok(())
}

fn verify_expr(expr: &Expr, defined: &HashSet<String>) -> Result<(), String> {
    match expr {
        Expr::Number(_) | Expr::Str(_) | Expr::Bool(_) | Expr::Null => Ok(()),
        Expr::Ident(name) => {
            if defined.contains(name) {
                Ok(())
            } else {
                Err(format!("undefined variable '{name}'"))
            }
        }
        Expr::Array(items) => items.iter().try_for_each(|item| verify_expr(item, defined)),
        Expr::Object(fields) => fields
            .iter()
            .try_for_each(|(_, value)| verify_expr(value, defined)),
        Expr::Field(obj, _) => verify_expr(obj, defined),
        Expr::Index(arr, index) => {
            verify_expr(arr, defined)?;
            verify_expr(index, defined)
        }
        Expr::Unary(_, e) => verify_expr(e, defined),
        Expr::Binary(lhs, _, rhs) => {
            verify_expr(lhs, defined)?;
            verify_expr(rhs, defined)
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

    let main_fn = module.add_function("main", i32_ty.fn_type(&[], false), None);
    let entry = context.append_basic_block(main_fn, "entry");
    builder.position_at_end(entry);

    let mut gen = Gen {
        context: &context,
        builder: &builder,
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
        env: HashMap::new(),
        order: Vec::new(),
    };

    for stmt in &program.statements {
        gen.gen_stmt(stmt)?;
    }
    gen.emit_dump(fn_dump)?;

    builder
        .build_return(Some(&i32_ty.const_int(0, false)))
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
    /// name -> pointer to that name's current (most-recently-assigned)
    /// `CodeValue` slot. Reassigning a name just rebinds this pointer to a
    /// freshly generated slot — nothing is freed, matching the interpreter's
    /// "everything mutable, nothing explicitly deallocated here" stance (see
    /// memory `new-code-memory-management`: no heap allocation exists yet at
    /// all, every slot lives on `main`'s stack for the program's duration).
    env: HashMap<String, PointerValue<'a>>,
    /// First-assignment order, for the final bindings dump — mirrors
    /// `interpreter::Environment::order` exactly.
    order: Vec<String>,
}

impl<'a> Gen<'a> {
    fn alloc_slot(&self, hint: &str) -> Result<PointerValue<'a>, String> {
        let count = self.i64_ty.const_int(VALUE_SIZE, false);
        let ptr = self
            .builder
            .build_array_alloca(self.i8_ty, count, hint)
            .map_err(|e| e.to_string())?;
        self.set_alignment(ptr)?;
        Ok(ptr)
    }

    fn alloc_buffer(&self, len: u64, hint: &str) -> Result<PointerValue<'a>, String> {
        let count = self.i64_ty.const_int(VALUE_SIZE * len, false);
        let ptr = self
            .builder
            .build_array_alloca(self.i8_ty, count, hint)
            .map_err(|e| e.to_string())?;
        self.set_alignment(ptr)?;
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
            Stmt::Assign { name, value } => {
                let ptr = self.gen_expr(value)?;
                if !self.env.contains_key(name) {
                    self.order.push(name.clone());
                }
                self.env.insert(name.clone(), ptr);
                Ok(())
            }
            Stmt::Assert(expr) => {
                let ptr = self.gen_expr(expr)?;
                self.builder
                    .build_call(self.fn_assert, &[ptr.into()], "")
                    .map_err(|e| e.to_string())?;
                Ok(())
            }
        }
    }

    /// Evaluates `expr` into a `CodeValue` and returns a pointer to it.
    /// `Expr::Ident` returns the *existing* slot pointer directly rather
    /// than copying — cheap structural sharing, identical in spirit to the
    /// interpreter's `Rc`-based sharing (see memory
    /// `new-code-memory-management`). Everywhere else that a value needs to
    /// land in caller-owned contiguous storage (array elements, object
    /// field values, the final bindings dump), the caller copies via
    /// `code_copy` after the fact instead of this function copying eagerly.
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
                .env
                .get(name)
                .copied()
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
                let keys_buf = self
                    .builder
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
            .builder
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
            .builder
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

            let src = *self.env.get(name).expect("binding must exist by dump time");
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
}
