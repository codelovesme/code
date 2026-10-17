#![cfg(feature = "llvm")]
#[path = "support/modules.rs"]
mod modules;
use std::{fs, process::Command};

#[test]
fn wasm_reads_pointer_width_without_using_the_union_padding() {
    let archiver = ["llvm-ar", "llvm-ar-17"]
        .into_iter()
        .find(|tool| Command::new(tool).arg("--version").output().is_ok());
    let sysroot = Command::new("rustc")
        .args(["--print", "sysroot"])
        .output()
        .unwrap();
    if archiver.is_none()
        || Command::new("node").arg("--version").output().is_err()
        || !std::path::Path::new(String::from_utf8_lossy(&sysroot.stdout).trim())
            .join("lib/rustlib/wasm32-unknown-unknown")
            .is_dir()
    {
        eprintln!("skipped wasm padding check: needs Node, LLVM ar and wasm32 target");
        return;
    }
    let dir = std::env::temp_dir().join(format!("code-length-padding-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    fs::copy(modules::build_wasm32_a("arrays"), dir.join("arrays.a")).unwrap();
    for (tool, args) in [
        (
            "clang",
            vec![
                "--target=wasm32-unknown-unknown".to_owned(),
                "-O2".into(),
                "-Isrc".into(),
                "-c".into(),
                "tests/native_modules/test_length_padding.c".into(),
                "-o".into(),
                dir.join("padding.o").to_str().unwrap().to_owned(),
            ],
        ),
        (
            archiver.unwrap(),
            vec![
                "rcs".into(),
                dir.join("padding.a").to_str().unwrap().to_owned(),
                dir.join("padding.o").to_str().unwrap().to_owned(),
            ],
        ),
    ] {
        let output = Command::new(tool)
            .args(args)
            .current_dir(modules::repo())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let source = dir.join("main.code");
    fs::write(&source,"link \"arrays.a\" as arrays\nlink \"padding.a\" as padding\nemit Length { value = [1, 2] } to arrays get ordinary\nassert ordinary.value = 2\nemit Check to padding get empty\nassert empty = LengthResult { value = 0 }\n").unwrap();
    code::compile_file(
        &source,
        code::BuildTarget::Wasm,
        &dir.join("main.wasm"),
        true,
    )
    .unwrap();
    fs::write(
        dir.join("probe.mjs"),
        r#"
import {readFileSync} from 'node:fs';
import {createHost} from './host.mjs';
const messages=[];const host=createHost({log:s=>messages.push(s)});
const {instance}=await WebAssembly.instantiate(readFileSync('./main.wasm'),{env:host.env});
// Make an accidental null dereference visible; address zero is not an object.
new DataView(instance.exports.memory.buffer).setBigInt64(0,123n,true);
if(host.start(instance)!==0)throw Error(messages.join('\n') || 'padding changed the array count');
"#,
    )
    .unwrap();
    let output = Command::new("node")
        .arg("probe.mjs")
        .current_dir(&dir)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    fs::remove_dir_all(dir).unwrap();
}
