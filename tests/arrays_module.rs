#![cfg(feature = "llvm")]
#[path = "support/utility_modules.rs"]
mod utilities;
#[test]
fn arrays_agree_across_backends() {
    utilities::fixture("arrays", "arrays_utilities.code");
}
