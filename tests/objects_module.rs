#![cfg(feature = "llvm")]
#[path = "support/utility_modules.rs"]
mod utilities;

/// Keys, Values, Has and Delete (ticket 102), in every backend.
#[test]
fn objects_agree_across_backends() {
    utilities::fixture("objects", "objects_utilities.code");
}
