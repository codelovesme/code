//! Real native binaries must produce the interpreter's existing trace contract.
use std::{fs, process::Command};

fn cli(dir: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_code"))
        .current_dir(dir)
        .env("CODE_CHECK_LEAKS", "1")
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn compiled_boundaries_match_and_replay() {
    let dir = std::env::temp_dir().join(format!("compiled-trace-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("child.code"),
        "Child {} =>\n    emit Parent to base get p\n    return p\n",
    )
    .unwrap();
    fs::write(dir.join("main.code"), "link \"child.code\"\nParent {} =>\n    emit Length { value = [true, null, \"é\\n\"] } to core get n\n    return Answer { value = n.value }\nOuter {} =>\n    emit Child to this get c\n    emit Missing to this\n    return c\nBroken {} =>\n    x = 1 / 0\n    return x\nemit Outer to this\nemit Broken to this\nemit Unknown to core\n").unwrap();
    let interpreted = cli(&dir, &["trace"]);
    assert!(
        interpreted.status.success(),
        "{}",
        String::from_utf8_lossy(&interpreted.stderr)
    );
    let compiled = cli(&dir, &["trace", "--compiled"]);
    assert!(
        compiled.status.success(),
        "{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&compiled.stdout),
        String::from_utf8_lossy(&interpreted.stdout)
    );
    let trace = code::trace::parse_trace(std::str::from_utf8(&compiled.stdout).unwrap()).unwrap();
    assert_eq!(
        trace
            .events
            .iter()
            .map(|e| (e.sequence, e.depth, e.target.as_str()))
            .collect::<Vec<_>>(),
        vec![
            (0, 0, "this"),
            (1, 1, "this"),
            (2, 2, "base"),
            (3, 3, "core"),
            (4, 1, "this"),
            (5, 0, "this"),
            (6, 0, "core")
        ]
    );
    assert_eq!(compiled.stdout, cli(&dir, &["trace", "--compiled"]).stdout);
    assert!(cli(&dir, &["trace", "--compiled", "-o", "trace.json"])
        .status
        .success());
    assert!(cli(&dir, &["replay", "trace.json"]).status.success());
    assert!(cli(&dir, &["build", "main.code", "-o", "plain"])
        .status
        .success());
    let plain = Command::new(dir.join("plain"))
        .env("CODE_TRACE_FILE", dir.join("off.json"))
        .output()
        .unwrap();
    assert!(plain.status.success());
    assert!(plain.stdout.is_empty());
    assert!(!dir.join("off.json").exists());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn compiled_trace_errors_do_not_publish_a_successful_trace() {
    let dir = std::env::temp_dir().join(format!("compiled-trace-error-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    for source in [
        "emit 5 to this\n",
        "x = 1 / 0\n",
        "emit Length { value = 4 } to core\nx = 1 / 0\n",
    ] {
        fs::write(dir.join("main.code"), source).unwrap();
        for args in [vec!["trace"], vec!["trace", "--compiled"]] {
            let out = cli(&dir, &args);
            assert!(!out.status.success());
            assert!(out.stdout.is_empty());
            assert!(String::from_utf8_lossy(&out.stderr).contains("error:"));
        }
    }
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn native_aliases_failed_dispatch_and_snapshots_match() {
    let dir = std::env::temp_dir().join(format!("compiled-trace-native-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    // A real native module, returning the borrowed input without changing it.
    fs::write(
        dir.join("echo.c"),
        r#"
#include "code_abi.h"
#include <string.h>
void code_release(CodeValue *value) { (void)value; }
unsigned int code_module_abi_version(void) { return CODE_ABI_VERSION; }
void code_module_dispatch(CodeValue *out, const CodeValue *particle) {
    *out = *particle;
    for (long long i = 0; i < particle->len; i++) {
        if (!strcmp(particle->keys[i], "reply"))
            *out = *(CodeValue *)((char *)particle->items + i * CODE_VALUE_SLOT_SIZE);
    }
    out->heap = 0;
}
"#,
    )
    .unwrap();
    let built = Command::new("cc")
        .args(["-shared", "-fPIC", "-I"])
        .arg(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"))
        .arg(dir.join("echo.c"))
        .arg("-o")
        .arg(dir.join("echo.so"))
        .output()
        .unwrap();
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    fs::write(
        dir.join("main.code"),
        r#"link "echo.so" as echo
bad = { _module = 999 }
Fail {} =>
    emit Missing to bad
    return Unreachable
Again {} =>
    p = Again
    emit p to this get r
    return r
Nested {} =>
    emit Length { value = 4 } to core
    emit Packet { values = [1.25, -0.0, true, false, null, "é\n\t\"\\"] } to echo get a
    return a
emit Fail to this
emit Again to this
i = 0
loop
    if i = 50, break
    emit Nested to this
    i = i + 1
emit Packet { reply = "quoted\" line\n" } to echo
emit Packet { reply = [1, true, null] } to echo
emit Packet { reply = 42.5 } to echo
emit Packet { reply = false } to echo
emit Packet { reply = null } to echo
emit { _class = 4 } to core
"#,
    )
    .unwrap();
    let interpreted = cli(&dir, &["trace"]);
    let compiled = cli(&dir, &["trace", "--compiled"]);
    assert!(
        interpreted.status.success(),
        "{}",
        String::from_utf8_lossy(&interpreted.stderr)
    );
    assert!(
        compiled.status.success(),
        "{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&compiled.stdout),
        String::from_utf8_lossy(&interpreted.stdout)
    );
    let trace = code::trace::parse_trace(std::str::from_utf8(&compiled.stdout).unwrap()).unwrap();
    assert_eq!(trace.events[1].target, "module:bad");
    assert_eq!(trace.events[1].answer, code::value::Value::Null);
    assert_eq!(trace.events[4].depth, 0);
    assert_eq!(trace.events.last().unwrap().particle_class, "");
    fs::remove_dir_all(dir).unwrap();
}
