//! Guards against `crates/code-native/src/abi.rs` (a vendored copy, kept so
//! `code-native` has zero runtime dependencies and can be published to
//! crates.io without an unpublishable path dependency on `code-abi`) drifting
//! from the canonical `code-abi` crate it was copied from. `code-abi` is a
//! dev-dependency only (see Cargo.toml) — it never ships in the published
//! package, so it's safe to use here.

use std::mem::{align_of, offset_of, size_of};

#[test]
fn constants_match() {
    assert_eq!(code_native::CODE_ABI_VERSION, code_abi::CODE_ABI_VERSION);
    assert_eq!(code_native::CODE_EMIT_TARGET_BASE, code_abi::CODE_EMIT_TARGET_BASE);
    assert_eq!(code_native::CODE_TAG_NUMBER, code_abi::CODE_TAG_NUMBER);
    assert_eq!(code_native::CODE_TAG_STRING, code_abi::CODE_TAG_STRING);
    assert_eq!(code_native::CODE_TAG_BOOLEAN, code_abi::CODE_TAG_BOOLEAN);
    assert_eq!(code_native::CODE_TAG_OBJECT, code_abi::CODE_TAG_OBJECT);
    assert_eq!(code_native::CODE_TAG_NULL, code_abi::CODE_TAG_NULL);
    assert_eq!(code_native::CODE_TAG_ARRAY, code_abi::CODE_TAG_ARRAY);
}

/// Compare a struct's size, alignment, and field offsets between the two
/// copies. Takes the field list once per struct to avoid repeating it twice.
macro_rules! assert_layout_matches {
    ($native_ty:ty, $abi_ty:ty, [$($field:ident),+ $(,)?]) => {{
        assert_eq!(size_of::<$native_ty>(), size_of::<$abi_ty>(), concat!(stringify!($native_ty), " size drifted"));
        assert_eq!(align_of::<$native_ty>(), align_of::<$abi_ty>(), concat!(stringify!($native_ty), " alignment drifted"));
        $(
            assert_eq!(
                offset_of!($native_ty, $field), offset_of!($abi_ty, $field),
                concat!(stringify!($native_ty), ".", stringify!($field), " offset drifted"),
            );
        )+
    }};
}

#[test]
fn struct_layouts_match() {
    assert_layout_matches!(code_native::CodeValue, code_abi::CodeValue,
        [tag, number, string, boolean, fields, field_count, elements, element_count]);
    assert_layout_matches!(code_native::CodeField, code_abi::CodeField, [name, value]);
    assert_layout_matches!(code_native::CodeExportVar, code_abi::CodeExportVar, [name, value]);
    assert_layout_matches!(code_native::CodeExportHandler, code_abi::CodeExportHandler, [class_name, handler]);
    assert_layout_matches!(code_native::CodeTypeField, code_abi::CodeTypeField, [name, type_name, is_optional]);
    assert_layout_matches!(code_native::CodeExportType, code_abi::CodeExportType, [name, fields, field_count]);
    assert_layout_matches!(code_native::CodeEmission, code_abi::CodeEmission, [class_name, target]);
    assert_layout_matches!(code_native::CodeModuleDesc, code_abi::CodeModuleDesc,
        [abi_version, vars, var_count, handlers, handler_count, types, type_count, emissions, emission_count]);
}
