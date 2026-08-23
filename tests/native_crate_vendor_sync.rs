//! Guards against `crates/code-native/vendor/{code_abi.h,runtime.c}` (a
//! verbatim copy, kept so the crate can build standalone in `cargo
//! publish`'s isolated environment — see `crates/code-native/vendor/README.md`)
//! drifting from the `src/` files it was copied from. Lives here, not inside
//! `crates/code-native`, so it never ships as part of the published package
//! and can't break `cargo publish`'s own build check.

#[test]
fn vendored_files_match_src() {
    let pairs = [
        ("src/code_abi.h", "crates/code-native/vendor/code_abi.h"),
        ("src/runtime.c", "crates/code-native/vendor/runtime.c"),
    ];
    for (canonical, vendored) in pairs {
        let want = std::fs::read_to_string(canonical).unwrap_or_else(|e| panic!("reading {canonical}: {e}"));
        let got = std::fs::read_to_string(vendored).unwrap_or_else(|e| panic!("reading {vendored}: {e}"));
        assert_eq!(
            want, got,
            "{vendored} has drifted from {canonical} — copy {canonical} over it again"
        );
    }
}
