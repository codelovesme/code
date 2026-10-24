#![cfg(feature = "llvm")]
#[path = "support/utility_modules.rs"]
mod utilities;

/// Match, Find, Replace and Split (ticket 103), in every backend.
#[test]
fn regex_agrees_across_backends() {
    utilities::fixture("regex", "regex_utilities.code");
}
