#![cfg(feature = "llvm")]
#[path = "support/utility_modules.rs"]
mod utilities;
#[test]
fn existing_strings_and_length_agree_across_backends() {
    for name in [
        "strings_length.code",
        "strings_shout.code",
        "strings_echo_nested.code",
        "strings_split.code",
        "strings_join.code",
        "strings_trim.code",
        "strings_case.code",
        "strings_refuses_with_exception.code",
    ] {
        utilities::fixture("strings", name);
    }
}
