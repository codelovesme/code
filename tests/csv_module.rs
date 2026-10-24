#![cfg(feature = "llvm")]
#[path = "support/utility_modules.rs"]
mod utilities;

/// Parse and Stringify (ticket 103), in every backend.
#[test]
fn csv_agrees_across_backends() {
    utilities::fixture("csv", "csv_utilities.code");
}
