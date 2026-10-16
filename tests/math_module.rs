#![cfg(feature = "llvm")]
#[path = "support/utility_modules.rs"]
mod utilities;
#[test]
fn numeric_utilities_agree_across_backends() {
    utilities::fixture("math", "math_utilities.code");
    utilities::fixture("math", "math_refuses_with_exception.code");
}

#[test]
fn browser_host_value_edge_contracts() {
    let output = match std::process::Command::new("node")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/utility_page_contract.mjs"
        ))
        .output()
    {
        Ok(output) => output,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => panic!("node: {error}"),
    };
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
