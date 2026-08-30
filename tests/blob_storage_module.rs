//! The `blob_storage` module, against a real S3-compatible store.
//!
//! `tests/blob_storage_error_paths.code` covers everything that needs no
//! server. This drives the full put/get/list/delete round trip — but only
//! when one is reachable: set `S3_ENDPOINT`, `S3_ACCESS_KEY`, `S3_SECRET_KEY`
//! and `S3_BUCKET` (a `minio/minio` container will do). CI wires up MinIO; a
//! laptop without one skips this cleanly.

#![cfg(all(feature = "llvm", feature = "native-modules"))]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, fs};

fn build_module() -> PathBuf {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/modules/blob_storage");
    let status = Command::new("cargo")
        .args(["build", "--release"])
        .current_dir(&crate_dir)
        .status()
        .expect("run cargo for crates/modules/blob_storage");
    assert!(status.success(), "cargo failed to build blob_storage");
    crate_dir.join("target/release/libblob_storage.so")
}

#[test]
fn round_trip_against_a_real_object_store() {
    let (Ok(endpoint), Ok(key), Ok(secret), Ok(bucket)) = (
        env::var("S3_ENDPOINT"),
        env::var("S3_ACCESS_KEY"),
        env::var("S3_SECRET_KEY"),
        env::var("S3_BUCKET"),
    ) else {
        eprintln!("skipping blob_storage_module: set S3_ENDPOINT/S3_ACCESS_KEY/S3_SECRET_KEY/S3_BUCKET to run it");
        return;
    };

    let dir = std::env::temp_dir().join(format!("code-blob-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create test directory");
    fs::copy(build_module(), dir.join("blob_storage.so")).expect("copy blob_storage.so");

    // A prefix unique to this run, cleaned at both ends.
    let p = format!("codetest/{}/", std::process::id());
    let program = format!(
        r#"link "blob_storage.so" as blobs

emit Config {{
    bucket = "{bucket}", access_key = "{key}", secret_key = "{secret}",
    endpoint = "{endpoint}", create = true
}} to blobs get c
assert c.ok

emit Delete {{ key = "{p}a.txt" }} to blobs get _
emit Delete {{ key = "{p}b.bin"  }} to blobs get _

emit Get {{ key = "{p}a.txt" }} to blobs get g0
assert g0.found = false

emit Put {{ key = "{p}a.txt", data = "hello object", content_type = "text/plain" }} to blobs get put
assert put.key = "{p}a.txt"

emit Get {{ key = "{p}a.txt" }} to blobs get g
assert g.found
assert g.data = "hello object"
assert g.content_type = "text/plain"

emit Put {{ key = "{p}b.bin", data = "aGVsbG8=", base64 = true }} to blobs get _
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
emit Delete {{ key = "{p}b.bin" }} to blobs get _
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
        assert!(
            ok,
            "{mode} mode: the object-store round trip exited non-zero"
        );
    }

    let _ = fs::remove_dir_all(&dir);
}
