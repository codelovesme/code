#![cfg(feature = "llvm")]
#[path = "support/utility_modules.rs"]
mod utilities;
#[test]
fn arrays_agree_across_backends() {
    utilities::fixture("arrays", "arrays_utilities.code");
}

#[test]
fn core_length_is_removed_while_index_length_and_modules_work() {
    utilities::fixture("arrays", "core_length_removed.code");
}

#[test]
fn length_metadata_dispatch_agrees_across_backends() {
    utilities::fixture("length", "length_borrowed.code");
}
