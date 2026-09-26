//! The `mongodb` module, against a real database.
//!
//! `tests/mongodb_error_paths.code` covers everything that needs no server.
//! This drives the full CRUD round trip — but only when a MongoDB is
//! reachable: set `MONGO_URI` (a `docker run -p 27017:27017 mongo` will do)
//! and it runs, otherwise it prints why and returns. CI wires it up with a
//! service container; a laptop without one skips it cleanly.

#![cfg(all(feature = "llvm", feature = "native-modules"))]

#[path = "support/modules.rs"]
mod modules;

use std::path::PathBuf;
use std::process::Command;
use std::{env, fs};

fn build_module() -> PathBuf {
    modules::build_so("mongodb")
}

#[test]
fn crud_round_trip_against_a_real_mongodb() {
    let Ok(uri) = env::var("MONGO_URI") else {
        eprintln!("skipping mongodb_module: set MONGO_URI to run it");
        return;
    };

    let dir = std::env::temp_dir().join(format!("code-mongodb-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create test directory");
    fs::copy(build_module(), dir.join("mongodb.so")).expect("copy mongodb.so");

    // One database per test run, dropped at both ends, so two runs (or the
    // two output modes) never see each other's rows.
    let db = format!("code_test_{}", std::process::id());
    let program = format!(
        r#"link "mongodb.so" as db

emit Config {{ url = "{uri}", database = "{db}" }} to db get c
assert c ∈ ConfigResult
assert c.ok

emit Drop {{ collection = "users" }} to db
emit Drop {{ collection = "state" }} to db

| key/value, types preserved
emit Store {{ key = "prefs", value = {{ theme = "dark", n = 3, tags = ["a", "b"] }} }} to db get s
assert s.key = "prefs"
emit Fetch {{ key = "prefs" }} to db get f
assert f.found
assert f.value = {{ theme = "dark", n = 3, tags = ["a", "b"] }}
emit Fetch {{ key = "missing" }} to db get miss
assert miss.found = false
assert miss.value = null
emit Delete {{ key = "prefs" }} to db get d
assert d.existed
emit Delete {{ key = "prefs" }} to db get d2
assert d2.existed = false

| documents
emit InsertMany {{ collection = "users", docs = [
    {{ name = "ada", age = 36, role = "admin" }}
    {{ name = "bob", age = 29, role = "user" }}
    {{ name = "cy",  age = 41, role = "user" }}
] }} to db get im
assert im.count = 3

emit Insert {{ collection = "users", doc = {{ _id = "deb", name = "deb", age = 22, role = "user" }} }} to db get ins
assert ins.id ≠ ""

emit EnsureIndex {{ collection = "users", name = "role_age", keys = {{ role = 1, age = -1 }} }} to db get indexed
assert indexed.ok
emit ReplaceOne {{ collection = "users", filter = {{ _id = ins.id, age = 22 }}, doc = {{ _id = ins.id, name = "deb", age = 23, role = "user" }} }} to db get replaced
assert replaced.matched = 1
emit ReplaceOne {{ collection = "users", filter = {{ _id = ins.id, age = 22 }}, doc = {{ _id = ins.id, name = "deb", age = 24, role = "user" }} }} to db get stale
assert stale.matched = 0
emit DeleteOne {{ collection = "users", filter = {{ _id = ins.id }} }} to db get removed
assert removed.deleted = 1
emit Insert {{ collection = "users", doc = {{ name = "deb", age = 22, role = "user" }} }} to db

emit Count {{ collection = "users" }} to db get n
assert n.count = 4
emit Count {{ collection = "users", filter = {{ role = "user" }} }} to db get n2
assert n2.count = 3

emit Find {{ collection = "users", filter = {{ role = "user" }}, sort = {{ age = 1 }}, limit = 2 }} to db get fr
assert fr.count = 2
assert fr.items[0].name = "deb"
assert fr.items[1].name = "bob"
assert fr.items[0].age = 22

emit Drop {{ collection = "users" }} to db get last
assert last.dropped

emit EnsureIndex {{ collection = "ephemeral", name = "expiry", keys = {{ expires_at = 1 }}, expire_after_seconds = 0 }} to db get ttl
assert ttl.ok
emit Insert {{ collection = "ephemeral", doc = {{ expires_at = {{ "\$date_ms" = 1760000000000 }} }} }} to db get dated
assert dated.id ≠ ""
emit Drop {{ collection = "ephemeral" }} to db
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
        assert!(ok, "{mode} mode: the CRUD program exited non-zero");
    }

    let _ = fs::remove_dir_all(&dir);
}
