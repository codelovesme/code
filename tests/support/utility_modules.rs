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
    let text = fs::read_to_string(modules::repo().join("tests").join(file)).unwrap();
    let linked: Vec<_> = ["math", "strings", "arrays", "time", "objects"]
        .into_iter()
        .filter(|name| text.contains(&format!("native_modules/{name}.so")))
        .collect();
    for name in &linked {
        let so = modules::build_so(name);
        fs::copy(so, dir.join(format!("native_modules/{name}.so"))).unwrap();
    }
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
    let mut browser_text = text.clone();
    for name in &linked {
        let archive = modules::build_wasm32_a(name);
        fs::copy(archive, dir.join(format!("native_modules/{name}.a"))).unwrap();
        browser_text = browser_text.replace(&format!("{name}.so"), &format!("{name}.a"));
    }
    fs::write(&source, browser_text).unwrap();
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
let instance;
let pageCalls = 0;
if (moduleName === 'length') {
  const ask = host.env.code_web_ask;
  // Imports capture this function at instantiation.
  host.env.code_web_ask = (...args) => {
    if (args[5] !== -1n) {
      const p = JSON.parse(new TextDecoder().decode(new Uint8Array(
        instance.exports.memory.buffer, Number(args[2]), Number(args[3]))));
      if (p._class === 'Length') throw new Error('Length crossed the JSON bridge');
      pageCalls++;
    }
    return ask(...args);
  };
}
({ instance } = await WebAssembly.instantiate(readFileSync('./fixture.wasm'), { env: host.env }));
if (host.start(instance) !== 0) throw new Error(messages.join('\n') || 'utility wasm fixture failed');
if (moduleName === 'length' && pageCalls < 2) throw new Error('ordinary fallback handlers were not called');
if (moduleName === 'length') {
  // An older host knows no preflight. Any negative no-answer code must
  // fall back; even -2 is deliberately tested, not mistaken for permission.
  for (const olderReply of [-1n, -2n]) {
    const old = createHost({log: () => {}});
    const ask = old.env.code_web_ask;
    old.env.code_web_ask = (...args) => args[5] === -1n ? olderReply : ask(...args);
    const {instance} = await WebAssembly.instantiate(readFileSync('./fixture.wasm'), {env: old.env});
    if (old.start(instance) !== 0) throw new Error('older host fallback failed');
  }
  let override = false;
  const guarded = createHost({log: () => {}, guard: (name, p, local) =>
    override && name === 'arrays' ? {_class: 'LengthResult', value: 777} : local(name, p)});
  const {instance} = await WebAssembly.instantiate(readFileSync('./fixture.wasm'), {env: guarded.env});
  if (guarded.start(instance) !== 0) throw new Error('custom guarded host failed');
  override = true;
  if (guarded.ask({_class: 'ProbeLength'})?.value !== 777)
    throw new Error('local Length bypassed a custom guard');
}
if (moduleName === 'time' && '__FILE__' === 'time_queries.code') {
  const clock = host.ask({ _class: 'ClockSnapshot' });
  const offset = process.env.TZ === 'UTC' ? 0 : 120;
  if (clock.value !== 1790000000 || clock.core_value !== 1790000000 || clock.offset !== offset) {
    throw new Error('clock units/offset: ' + JSON.stringify(clock));
  }
}
"#.replace("__MODULE__", module).replace("__FILE__", file)).unwrap();
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
