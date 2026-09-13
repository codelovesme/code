//! The `azure_blob` module, against a real Azure Blob endpoint.
//!
//! The same round trip `tests/blob_storage_module.rs` drives, deliberately:
//! the two modules answer the same particles, so the case that proves one
//! should read as the case that proves the other. What differs is the setup
//! particle and the store behind it.
//!
//! Runs only when one is reachable: set `AZURE_BLOB_CONNECTION_STRING` and
//! `AZURE_BLOB_CONTAINER`. Azurite is what CI wires up and what a laptop can
//! run in one container; a machine without one skips this cleanly.

#![cfg(all(feature = "llvm", feature = "native-modules"))]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, fs};

fn build_module() -> PathBuf {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/modules/azure_blob");
    let status = Command::new("cargo")
        .args(["build", "--release"])
        .current_dir(&crate_dir)
        .status()
        .expect("run cargo for crates/modules/azure_blob");
    assert!(status.success(), "cargo failed to build azure_blob");
    crate_dir.join("target/release/libazure_blob.so")
}

#[test]
fn round_trip_against_a_real_azure_endpoint() {
    let (Ok(connection), Ok(container)) = (
        env::var("AZURE_BLOB_CONNECTION_STRING"),
        env::var("AZURE_BLOB_CONTAINER"),
    ) else {
        eprintln!(
            "skipping azure_blob_module: set AZURE_BLOB_CONNECTION_STRING/AZURE_BLOB_CONTAINER to run it"
        );
        return;
    };

    let dir = std::env::temp_dir().join(format!("code-azblob-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create test directory");
    fs::copy(build_module(), dir.join("azure_blob.so")).expect("copy azure_blob.so");

    // A prefix unique to this run, cleaned at both ends.
    let p = format!("codetest/{}/", std::process::id());
    let program = format!(
        r#"link "azure_blob.so" as blobs

emit Config {{
    bucket = "{container}", connection_string = "{connection}", create = true
}} to blobs get c
assert c.ok

emit Delete {{ key = "{p}a.txt" }} to blobs
emit Delete {{ key = "{p}b.bin"  }} to blobs

emit Get {{ key = "{p}a.txt" }} to blobs get g0
assert g0.found = false

emit Put {{ key = "{p}a.txt", data = "hello object", content_type = "text/plain" }} to blobs get put
assert put.key = "{p}a.txt"

emit Get {{ key = "{p}a.txt" }} to blobs get g
assert g.found
assert g.data = "hello object"
assert g.content_type = "text/plain"

emit Put {{ key = "{p}b.bin", data = "aGVsbG8=", base64 = true }} to blobs
emit Get {{ key = "{p}b.bin" }} to blobs get gb
assert gb.data = "hello"
emit Get {{ key = "{p}b.bin", base64 = true }} to blobs get gb64
assert gb64.data = "aGVsbG8="

emit List {{ prefix = "{p}" }} to blobs get l
assert l.count = 2
assert l.keys = ["{p}a.txt", "{p}b.bin"]

emit Delete {{ key = "{p}a.txt" }} to blobs get d
assert d.existed
emit Delete {{ key = "{p}a.txt" }} to blobs get d2
assert d2.existed = false
emit Delete {{ key = "{p}b.bin" }} to blobs
"#
    );

    for mode in ["run", "build"] {
        let source = dir.join(format!("{mode}.code"));
        fs::write(&source, &program).expect("write program");
        let ok = if mode == "run" {
            Command::new(env!("CARGO_BIN_EXE_code"))
                .args(["run", source.to_str().unwrap()])
                .current_dir(&dir)
                .status()
                .expect("spawn code run")
                .success()
        } else {
            let exe = dir.join(mode);
            code::compile_file(&source, code::BuildTarget::Exe, &exe, false).expect("compile");
            Command::new(&exe)
                .current_dir(&dir)
                .status()
                .expect("spawn compiled program")
                .success()
        };
        assert!(ok, "{mode} mode: the Azure round trip exited non-zero");
    }

    let _ = fs::remove_dir_all(&dir);
}
