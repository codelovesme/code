//! Execute the actual shared fixture through native and generated browser hosts.
#![allow(dead_code)]
#[path = "modules.rs"]
mod modules;
use std::{fs, path::Path, process::Command};

pub fn fixture(module: &str, file: &str) {
    let dir = std::env::temp_dir().join(format!(
        "code-utility-{module}-{file}-{}",
        std::process::id()
    ));
    fs::create_dir_all(dir.join("native_modules")).unwrap();
    let so = modules::build_so(module);
    fs::copy(so, dir.join(format!("native_modules/{module}.so"))).unwrap();
    let text = fs::read_to_string(modules::repo().join("tests").join(file)).unwrap();
    let source = dir.join("fixture.code");
    fs::write(&source, &text).unwrap();
    code::run_file(&source).unwrap_or_else(|e| panic!("{module} interpreter: {e}"));
    for release in [false, true] {
        let bin = dir.join(if release { "release" } else { "dev" });
        code::compile_file(&source, code::BuildTarget::Exe, &bin, release).unwrap();
        let out = Command::new(&bin).output().unwrap();
        assert!(
            out.status.success(),
            "{module} native: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let sysroot = Command::new("rustc")
        .args(["--print", "sysroot"])
        .output()
        .unwrap();
    if Command::new("node").arg("--version").output().is_err()
        || !Path::new(String::from_utf8_lossy(&sysroot.stdout).trim())
            .join("lib/rustlib/wasm32-unknown-unknown")
            .is_dir()
    {
        eprintln!("skipped browser utility check: node/wasm target unavailable");
        return;
    }
    let archive = modules::build_wasm32_a(module);
    fs::copy(archive, dir.join(format!("native_modules/{module}.a"))).unwrap();
    fs::write(
        &source,
        text.replace(&format!("{module}.so"), &format!("{module}.a")),
    )
    .unwrap();
    code::compile_file(
        &source,
        code::BuildTarget::Wasm,
        &dir.join("fixture.wasm"),
        false,
    )
    .unwrap();
    fs::write(dir.join("probe.mjs"), r#"
import { readFileSync } from 'node:fs';
import { createHost } from './host.mjs';
const moduleName = '__MODULE__';
Date.now = () => 1790000000123;
const messages = [];
const host = createHost({ log: (...parts) => messages.push(parts.join(' ')) });
const { instance } = await WebAssembly.instantiate(readFileSync('./fixture.wasm'), { env: host.env });
if (host.start(instance) !== 0) throw new Error(messages.join('\n') || 'utility wasm fixture failed');
if (moduleName === 'time') {
  const clock = host.ask({ _class: 'ClockSnapshot' });
  const offset = process.env.TZ === 'UTC' ? 0 : 120;
  if (clock.value !== 1790000000 || clock.core_value !== 1790000000 || clock.offset !== offset) {
    throw new Error('clock units/offset: ' + JSON.stringify(clock));
  }
}
"#.replace("__MODULE__", module)).unwrap();
    for timezone in ["UTC", "Etc/GMT-2"] {
        let out = Command::new("node")
            .arg("probe.mjs")
            .current_dir(&dir)
            .env("TZ", timezone)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{module} browser {timezone}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    fs::remove_dir_all(dir).unwrap();
}
