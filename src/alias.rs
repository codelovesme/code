//! Which memory a generated load or store touches, told to LLVM (ticket 122
//! stage C).
//!
//! Generated code reads and writes three kinds of memory: a variable's or a
//! temporary's own slot (a `_code_slot_…` global in `main`, an `alloca` in a
//! handler), a slot inside a list's or object's heap block, and a block's
//! header (its count and shape, just before it). They never overlap — a
//! variable's slot is never inside a heap block — and within each, a field
//! (`tag`, `number`, `items`, …) is always read as the same type at the same
//! offset. LLVM cannot see either, so after `x = pts[i].x + 1` writes `x`'s
//! number it assumes `pts`'s length or `pts[i]`'s shape may have changed and
//! reads them again. This tags every load and store whose memory it can
//! place with a TBAA type naming that place — *local.8*, *heap.0*,
//! *header.-16* — so two accesses of different places are known apart and
//! LLVM keeps what it read: a loop's fast version (`codegen::gen_loop`)
//! finds `pts[i]` once per iteration, and `pts` itself once per loop.
//!
//! An access it cannot place (through a pointer a runtime call answered, a
//! phi, an argument) is left untagged, which LLVM treats as touching
//! anything — the safe default. Runtime calls are untagged too.

use std::collections::HashMap;

use inkwell::context::Context;
use inkwell::llvm_sys::core::*;
use inkwell::llvm_sys::prelude::LLVMValueRef;
use inkwell::llvm_sys::{LLVMOpcode, LLVMTypeKind};
use inkwell::module::Module;
use inkwell::values::{AsValueRef, InstructionOpcode, MetadataValue};

/// The bytes of one `CodeValue` slot, as `codegen::VALUE_SIZE`.
const SLOT: i64 = 80;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Region {
    /// A variable's or temporary's own slot (or a run of them).
    Local,
    /// A heap block: list elements, an object's values; a header before it.
    Block,
}

/// The fields of a slot that generated code reads and writes, by offset:
/// (offset, width in bytes, is a float). `wasm` is the 32-bit layout.
fn fields(wasm: bool) -> &'static [(i64, u32, bool)] {
    if wasm {
        // tag, heap, number, str, bool, items, keys, len
        &[
            (0, 4, false),
            (4, 4, false),
            (8, 8, true),
            (16, 4, false),
            (20, 4, false),
            (24, 4, false),
            (28, 4, false),
            (32, 8, false),
        ]
    } else {
        &[
            (0, 4, false),
            (4, 4, false),
            (8, 8, true),
            (16, 8, false),
            (24, 4, false),
            (32, 8, false),
            (40, 8, false),
            (48, 8, false),
        ]
    }
}

/// Tags every load and store in `module` it can place.
pub fn tag(context: &Context, module: &Module, wasm: bool) {
    let kind = context.get_kind_id("tbaa");
    let root = context.metadata_node(&[context.metadata_string("code values").into()]);
    let mut tags: HashMap<String, MetadataValue> = HashMap::new();
    let mut tag_for = |name: String| -> MetadataValue {
        *tags.entry(name.clone()).or_insert_with(|| {
            let zero = context.i64_type().const_zero();
            let ty = context.metadata_node(&[
                context.metadata_string(&name).into(),
                root.into(),
                zero.into(),
            ]);
            context.metadata_node(&[ty.into(), ty.into(), zero.into()])
        })
    };
    let pointer_items = if wasm { [24, 28] } else { [32, 40] };
    for function in module.get_functions() {
        for block in function.get_basic_blocks() {
            let mut at = block.get_first_instruction();
            while let Some(instruction) = at {
                at = instruction.get_next_instruction();
                let (pointer, accessed) = match instruction.get_opcode() {
                    InstructionOpcode::Load => (0, instruction.as_value_ref()),
                    InstructionOpcode::Store => {
                        (1, unsafe { LLVMGetOperand(instruction.as_value_ref(), 0) })
                    }
                    _ => continue,
                };
                let address = unsafe { LLVMGetOperand(instruction.as_value_ref(), pointer) };
                let Some(place) = place(address, wasm, &pointer_items) else {
                    continue;
                };
                let Some(name) = name_of(place, unsafe { LLVMTypeOf(accessed) }, wasm) else {
                    continue;
                };
                let _ = instruction.set_metadata(tag_for(name), kind);
            }
        }
    }
}

/// Where `address` points: a region and a byte offset into it (a header is
/// a negative offset into a block). `None` when it cannot tell.
fn place(address: LLVMValueRef, wasm: bool, pointer_items: &[i64; 2]) -> Option<(Region, i64)> {
    unsafe {
        if !LLVMIsAGlobalVariable(address).is_null() {
            let mut len = 0;
            let name = LLVMGetValueName2(address, &mut len);
            let name = std::slice::from_raw_parts(name as *const u8, len);
            return name
                .starts_with(b"_code_slot_")
                .then_some((Region::Local, 0));
        }
        if !LLVMIsAAllocaInst(address).is_null() {
            return Some((Region::Local, 0));
        }
        // A pointer read out of a slot's `items` or `keys`: a heap block.
        if !LLVMIsALoadInst(address).is_null() {
            let from = place(LLVMGetOperand(address, 0), wasm, pointer_items)?;
            let offset = from.1.rem_euclid(SLOT);
            let width = pointer_width(wasm);
            let is_pointer =
                LLVMGetTypeKind(LLVMTypeOf(address)) == LLVMTypeKind::LLVMPointerTypeKind;
            return (from.1 >= 0 && is_pointer && pointer_items.contains(&offset) && width > 0)
                .then_some((Region::Block, 0));
        }
        let gep = !LLVMIsAGetElementPtrInst(address).is_null()
            || (!LLVMIsAConstantExpr(address).is_null()
                && LLVMGetConstOpcode(address) == LLVMOpcode::LLVMGetElementPtr);
        if !gep {
            return None;
        }
        let (region, base) = place(LLVMGetOperand(address, 0), wasm, pointer_items)?;
        let source = LLVMGetGEPSourceElementType(address);
        let operands = LLVMGetNumOperands(address) as u32;
        let mut offset = base;
        match (LLVMGetTypeKind(source), operands) {
            // `gep i8, p, k`: k bytes; a computed k only as whole slots.
            (LLVMTypeKind::LLVMIntegerTypeKind, 2) if LLVMGetIntTypeWidth(source) == 8 => {
                offset += bytes(LLVMGetOperand(address, 1))?;
            }
            // `gep [n x i8], p, j, k`: j whole arrays (only 0 or slots), k bytes.
            (LLVMTypeKind::LLVMArrayTypeKind, 3) => {
                let element = LLVMGetElementType(source);
                if LLVMGetTypeKind(element) != LLVMTypeKind::LLVMIntegerTypeKind
                    || LLVMGetIntTypeWidth(element) != 8
                {
                    return None;
                }
                let first = bytes(LLVMGetOperand(address, 1))?;
                if first != 0 {
                    return None;
                }
                offset += bytes(LLVMGetOperand(address, 2))?;
            }
            _ => return None,
        }
        Some((region, offset))
    }
}

/// A byte offset: a constant, or a number of whole slots (`i * 80`, which
/// is 0 for what a field's offset within its slot is).
unsafe fn bytes(index: LLVMValueRef) -> Option<i64> {
    if !LLVMIsAConstantInt(index).is_null() {
        return Some(LLVMConstIntGetSExtValue(index));
    }
    let is_mul = !LLVMIsAInstruction(index).is_null()
        && LLVMGetInstructionOpcode(index) == LLVMOpcode::LLVMMul;
    if is_mul {
        for side in 0..2 {
            let factor = LLVMGetOperand(index, side);
            if !LLVMIsAConstantInt(factor).is_null() && LLVMConstIntGetSExtValue(factor) == SLOT {
                return Some(0);
            }
        }
    }
    None
}

fn pointer_width(wasm: bool) -> u32 {
    if wasm {
        4
    } else {
        8
    }
}

/// The TBAA type for an access of `ty` at `place`, or `None` when it is not
/// one of a slot's fields read as that field's type, or a header word.
fn name_of(
    place: (Region, i64),
    ty: inkwell::llvm_sys::prelude::LLVMTypeRef,
    wasm: bool,
) -> Option<String> {
    let (region, offset) = place;
    let (bytes, float) = unsafe {
        match LLVMGetTypeKind(ty) {
            LLVMTypeKind::LLVMIntegerTypeKind => (LLVMGetIntTypeWidth(ty) / 8, false),
            LLVMTypeKind::LLVMDoubleTypeKind => (8, true),
            LLVMTypeKind::LLVMPointerTypeKind => (pointer_width(wasm), false),
            _ => return None,
        }
    };
    if region == Region::Block && offset < 0 {
        // The header: its count at -16, its spare word (a shape) at -8.
        return ([-16, -8].contains(&offset) && bytes == 8 && !float)
            .then(|| format!("header.{offset}"));
    }
    let within = offset.rem_euclid(SLOT);
    fields(wasm)
        .iter()
        .any(|&(at, width, is_float)| at == within && width == bytes && is_float == float)
        .then(|| {
            let region = match region {
                Region::Local => "local",
                Region::Block => "heap",
            };
            format!("{region}.{within}")
        })
}
