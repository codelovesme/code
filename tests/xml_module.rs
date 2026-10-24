#![cfg(feature = "llvm")]
#[path = "support/utility_modules.rs"]
mod utilities;

/// Parse, and malformed XML answering an Exception (ticket 103), in every backend.
#[test]
fn xml_agrees_across_backends() {
    utilities::fixture("xml", "xml_utilities.code");
}
