//! Guards against drift between the Rust ABI contract (the `code-abi` crate,
//! re-exported through `code_lang::native_module`) and the C header that native
//! module authors include, `tests/native_modules/code_abi.h`.
//!
//! Two checks:
//!  1. The `#define` constants (version, tags, emit target) match the Rust ones.
//!  2. Every struct's size and field offsets match, verified by compiling a C
//!     probe full of `_Static_assert`s seeded with the Rust layout. This needs a
//!     C compiler; when none is available the layout check is skipped (the
//!     constant check still runs, and CI always has `cc`).

use std::mem::{offset_of, size_of};
use std::path::Path;
use std::process::Command;

use code_lang::native_module as abi;

const HEADER_DIR: &str = "tests/native_modules";
const HEADER_SRC: &str = include_str!("native_modules/code_abi.h");

/// Read an integer `#define NAME VALUE` from the header.
fn header_define(name: &str) -> Option<i64> {
    for line in HEADER_SRC.lines() {
        let l = line.trim();
        if let Some(rest) = l.strip_prefix("#define ") {
            let mut it = rest.split_whitespace();
            if it.next() == Some(name) {
                return it.next().and_then(|v| v.parse::<i64>().ok());
            }
        }
    }
    None
}

#[test]
fn header_constants_match_rust() {
    let cases: &[(&str, i64)] = &[
        ("CODE_ABI_VERSION", abi::CODE_ABI_VERSION as i64),
        ("CODE_EMIT_TARGET_BASE", abi::CODE_EMIT_TARGET_BASE as i64),
        ("CODE_TAG_NUMBER", abi::CODE_TAG_NUMBER as i64),
        ("CODE_TAG_STRING", abi::CODE_TAG_STRING as i64),
        ("CODE_TAG_BOOLEAN", abi::CODE_TAG_BOOLEAN as i64),
        ("CODE_TAG_OBJECT", abi::CODE_TAG_OBJECT as i64),
        ("CODE_TAG_NULL", abi::CODE_TAG_NULL as i64),
        ("CODE_TAG_ARRAY", abi::CODE_TAG_ARRAY as i64),
    ];
    for (name, rust_val) in cases {
        assert_eq!(
            header_define(name),
            Some(*rust_val),
            "code_abi.h `#define {name}` drifted from the code-abi crate (expected {rust_val})",
        );
    }
}

/// Emit `_Static_assert`s for a struct's size and each field's offset.
macro_rules! layout_asserts {
    ($out:expr, $ty:ty, $cname:literal, [$($field:ident),* $(,)?]) => {{
        $out.push_str(&format!(
            "_Static_assert(sizeof({c}) == {sz}, \"{c} size drifted\");\n",
            c = $cname, sz = size_of::<$ty>(),
        ));
        $(
            $out.push_str(&format!(
                "_Static_assert(offsetof({c}, {f}) == {off}, \"{c}.{f} offset drifted\");\n",
                c = $cname, f = stringify!($field), off = offset_of!($ty, $field),
            ));
        )*
    }};
}

fn find_cc() -> Option<String> {
    let cc = std::env::var("CC").unwrap_or_else(|_| "cc".to_string());
    Command::new(&cc)
        .arg("--version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|_| cc)
}

#[test]
fn header_struct_layout_matches_rust() {
    let Some(cc) = find_cc() else {
        eprintln!("skip: no C compiler found (set CC or install cc) — layout check skipped");
        return;
    };

    let mut body = String::from("#include <stddef.h>\n#include \"code_abi.h\"\n\n");
    layout_asserts!(body, abi::CodeValue, "CodeValue",
        [tag, number, string, boolean, fields, field_count, elements, element_count]);
    layout_asserts!(body, abi::CodeField, "CodeField", [name, value]);
    layout_asserts!(body, abi::CodeExportVar, "CodeExportVar", [name, value]);
    layout_asserts!(body, abi::CodeExportHandler, "CodeExportHandler", [class_name, handler]);
    layout_asserts!(body, abi::CodeTypeField, "CodeTypeField", [name, type_name, is_optional]);
    layout_asserts!(body, abi::CodeExportType, "CodeExportType", [name, fields, field_count]);
    layout_asserts!(body, abi::CodeEmission, "CodeEmission", [class_name, target]);
    layout_asserts!(body, abi::CodeModuleDesc, "CodeModuleDesc",
        [abi_version, vars, var_count, handlers, handler_count, types, type_count,
         emissions, emission_count]);
    body.push_str("int main(void) { return 0; }\n");

    let probe = std::env::temp_dir().join(format!("code_abi_probe_{}.c", std::process::id()));
    std::fs::write(&probe, &body).expect("write C probe");

    let out = Command::new(&cc)
        .args(["-std=c11", "-fsyntax-only", "-I", HEADER_DIR])
        .arg(&probe)
        .output()
        .expect("run cc on ABI probe");

    let _ = std::fs::remove_file(&probe);

    assert!(
        out.status.success(),
        "C header {}/code_abi.h layout drifted from the code-abi crate:\n{}",
        HEADER_DIR,
        String::from_utf8_lossy(&out.stderr),
    );

    // Sanity: the header path used above actually exists.
    assert!(Path::new(HEADER_DIR).join("code_abi.h").exists());
}
