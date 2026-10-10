//! Machine code on every core (ticket 116).
//!
//! In a large program almost all of a build is LLVM's backend — instruction
//! selection, register allocation — working through one module on one core
//! (Aquarium: ~3.3 s of 3.7 s). LLVM's backend is not thread-safe within one
//! `Context`, so the module is cut into parts, each part is read into a
//! `Context` of its own on a thread of its own, and each becomes its own
//! object file; the linker joins them as it joins the runtime. This is what
//! LLVM's `splitCodeGen` does for parallel LTO, and what rustc's codegen
//! units are.
//!
//! **The output does not depend on the machine.** Which function goes in
//! which part is decided by the program alone — its functions in module
//! order, packed into parts of about `PART_SIZE` IR instructions — and the
//! objects are linked in part order. The number of cores only decides how
//! many parts are compiled at once. A program smaller than one part is not
//! split at all and builds exactly as before.
//!
//! Every part is a full copy of the module, read back from bitcode, with
//! the functions other parts own turned into declarations and the mutable
//! globals left to part 0. Constant data a part's code uses (strings) stays
//! in the part as a private copy; constant data it does not use is dropped.
//! Functions and globals another part may reach are given external linkage
//! with hidden visibility first, so they link across parts without leaving
//! the program (nor a shared library's symbol table).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use inkwell::context::{AsContextRef, Context};
use inkwell::llvm_sys;
use inkwell::module::{Linkage, Module};
use inkwell::passes::PassManager;
use inkwell::targets::{FileType, TargetMachine};
use inkwell::values::{AsValueRef, FunctionValue, GlobalValue};
use inkwell::GlobalVisibility;

/// About how many IR instructions a part holds. Small enough that a large
/// app has a part per core and more (Aquarium: ~128,000 instructions),
/// large enough that a small program stays in one part and pays nothing.
pub(crate) const PART_SIZE: usize = 6_000;

/// How many parts to compile at once: `CODE_BUILD_JOBS`, else the
/// machine's cores. Only the speed depends on it, never the output.
fn jobs() -> usize {
    std::env::var("CODE_BUILD_JOBS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|&n| n > 0)
        .or_else(|| std::thread::available_parallelism().ok().map(|n| n.get()))
        .unwrap_or(1)
}

fn instructions(function: FunctionValue) -> usize {
    let mut count = 0;
    for block in function.get_basic_blocks() {
        let mut at = block.get_first_instruction();
        while let Some(instruction) = at {
            count += 1;
            at = instruction.get_next_instruction();
        }
    }
    count
}

/// Each defined function's part, by name: functions in module order, a new
/// part begun once the current one holds `PART_SIZE` instructions. One part
/// means "do not split".
pub(crate) fn plan(module: &Module) -> (usize, HashMap<String, usize>) {
    let mut owner = HashMap::new();
    let mut part = 0;
    let mut filled = 0;
    let mut next = module.get_first_function();
    while let Some(function) = next {
        next = function.get_next_function();
        if function.count_basic_blocks() == 0 {
            continue;
        }
        if filled >= PART_SIZE {
            part += 1;
            filled = 0;
        }
        filled += instructions(function);
        owner.insert(name_of(function.as_global_value()), part);
    }
    (part + 1, owner)
}

fn name_of(global: GlobalValue) -> String {
    global.get_name().to_string_lossy().into_owned()
}

/// Constant data with no identity of its own (a string): each part may keep
/// its own copy.
fn copyable(global: GlobalValue) -> bool {
    global.is_constant()
        && global.has_unnamed_addr()
        && matches!(global.get_linkage(), Linkage::Private | Linkage::Internal)
}

/// Readies the whole module for splitting, once, before it is copied: every
/// definition gets a name, and every definition another part may refer to
/// gets external linkage, hidden.
pub(crate) fn prepare(module: &Module) {
    let mut unnamed = 0;
    let mut name_it = |global: GlobalValue| {
        if global.get_name().to_bytes().is_empty() {
            global.set_name(&format!("__code_part_anon_{unnamed}"));
            unnamed += 1;
        }
    };
    let promote = |global: GlobalValue| {
        if matches!(global.get_linkage(), Linkage::Private | Linkage::Internal) {
            global.set_linkage(Linkage::External);
            global.set_visibility(GlobalVisibility::Hidden);
        }
    };
    let mut next = module.get_first_function();
    while let Some(function) = next {
        next = function.get_next_function();
        if function.count_basic_blocks() > 0 {
            name_it(function.as_global_value());
            promote(function.as_global_value());
        }
    }
    let mut next = module.get_first_global();
    while let Some(global) = next {
        next = global.get_next_global();
        if global.get_initializer().is_some() {
            name_it(global);
            if !copyable(global) && global.get_linkage() != Linkage::Appending {
                promote(global);
            }
        }
    }
}

/// Cuts `module` — read lazily, so no function body is loaded yet — down to
/// part `part` of a plan: this part's functions are loaded, every other
/// definition becomes a declaration without its body ever being read.
fn keep_part(module: &Module, part: usize, owner: &HashMap<String, usize>) {
    // Loading a function's body is what the legacy function pass manager
    // does before it runs; with no passes in it, that is all it does.
    let loader = PassManager::create(module);
    loader.initialize();
    let mut next = module.get_first_function();
    while let Some(function) = next {
        next = function.get_next_function();
        if owner.get(&name_of(function.as_global_value())) == Some(&part) {
            loader.run_on(&function);
        }
    }
    // Listed first: each declaration added below joins the end of the list
    // under the same name, and must not be met again.
    let mut functions = Vec::new();
    let mut next = module.get_first_function();
    while let Some(function) = next {
        next = function.get_next_function();
        functions.push(function);
    }
    for function in functions {
        let name = name_of(function.as_global_value());
        if !matches!(owner.get(&name), Some(&p) if p != part) {
            continue;
        }
        // A declaration in its place: the same name and type, every use
        // moved to it, then the body gone with the function.
        let visibility = function.as_global_value().get_visibility();
        function
            .as_global_value()
            .set_name(&format!("{name}.elsewhere"));
        let declared = module.add_function(&name, function.get_type(), Some(Linkage::External));
        declared.as_global_value().set_visibility(visibility);
        function.replace_all_uses_with(declared);
        // SAFETY: nothing uses `function` any longer, and its body was never
        // read, so nothing points into it.
        unsafe { function.delete() };
    }
    let mut next = module.get_first_global();
    while let Some(global) = next {
        next = global.get_next_global();
        if global.get_initializer().is_none() {
            continue;
        }
        if global.get_linkage() == Linkage::Appending {
            // `llvm.global_ctors` and its kind: once, in part 0.
            if part != 0 {
                // SAFETY: an appending global is only ever read by LLVM.
                unsafe { global.delete() };
            }
        } else if copyable(global) {
            // SAFETY: only read to see whether anything uses it.
            let used = unsafe { !llvm_sys::core::LLVMGetFirstUse(global.as_value_ref()).is_null() };
            if !used {
                // SAFETY: nothing uses it.
                unsafe { global.delete() };
            }
        } else if part != 0 {
            // Part 0 owns the program's state; here it becomes `extern`.
            // SAFETY: a null initializer is how LLVM says "declaration".
            unsafe {
                llvm_sys::core::LLVMSetInitializer(global.as_value_ref(), std::ptr::null_mut())
            };
            global.set_linkage(Linkage::External);
        }
    }
}

/// The module in `bitcode`, with its function bodies left unread until
/// asked for (`keep_part`): a part reads only its own share, not the whole
/// program — reading all of it for every part cost more than the parts saved.
fn read_lazily<'ctx>(context: &'ctx Context, bitcode: &[u8]) -> Result<Module<'ctx>, String> {
    use llvm_sys::bit_reader::LLVMGetBitcodeModuleInContext2;
    use llvm_sys::core::{LLVMCreateMemoryBufferWithMemoryRangeCopy, LLVMDisposeMemoryBuffer};
    // SAFETY: the buffer is a copy LLVM owns; on success the lazily read
    // module keeps it (it reads bodies out of it later), on failure it is
    // freed here.
    unsafe {
        let buffer = LLVMCreateMemoryBufferWithMemoryRangeCopy(
            bitcode.as_ptr().cast(),
            bitcode.len(),
            c"part".as_ptr(),
        );
        let mut module = std::ptr::null_mut();
        if LLVMGetBitcodeModuleInContext2(context.as_ctx_ref(), buffer, &mut module) != 0 {
            LLVMDisposeMemoryBuffer(buffer);
            return Err("could not read the program's bitcode back".into());
        }
        Ok(Module::new(module))
    }
}

/// Compiles `module`, split by `plan`, into one object per part beside
/// `obj_path` (`<obj_path>.<part>.o`), returned in part order.
/// `machine` makes each thread's own target machine.
pub(crate) fn compile_parts(
    module: &Module,
    parts: usize,
    owner: &HashMap<String, usize>,
    obj_path: &Path,
    machine: &(dyn Fn() -> Result<TargetMachine, String> + Sync),
) -> Result<Vec<PathBuf>, String> {
    let bitcode = module.write_bitcode_to_memory().as_slice().to_vec();
    let paths: Vec<PathBuf> = (0..parts)
        .map(|part| obj_path.with_extension(format!("{part}.o")))
        .collect();
    let taken = AtomicUsize::new(0);
    let results: Vec<Result<(), String>> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..jobs().min(parts))
            .map(|_| {
                scope.spawn(|| -> Result<(), String> {
                    let machine = machine()?;
                    loop {
                        let part = taken.fetch_add(1, Ordering::Relaxed);
                        if part >= parts {
                            return Ok(());
                        }
                        let context = Context::create();
                        let copy = read_lazily(&context, &bitcode)
                            .map_err(|e| format!("part {part}: {e}"))?;
                        keep_part(&copy, part, owner);
                        copy.verify().map_err(|e| format!("part {part}: {e}"))?;
                        machine
                            .write_to_file(&copy, FileType::Object, &paths[part])
                            .map_err(|e| format!("part {part}: {e}"))?;
                    }
                })
            })
            .collect();
        workers
            .into_iter()
            .map(|w| {
                w.join()
                    .unwrap_or_else(|_| Err("a codegen thread panicked".into()))
            })
            .collect()
    });
    for result in results {
        result?;
    }
    Ok(paths)
}
