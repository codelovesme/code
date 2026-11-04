//! Browser tracing uses the same opt-in and particle shape as native tracing.
//! Node covers actual wasm/codegen; the apps suite covers Chromium hosting.
#![cfg(feature = "llvm")]
use std::{fs, process::Command};

#[test]
fn wasm_trace_matches_native_contract_and_quiet_builds_have_no_hooks() {
    let dir = std::env::temp_dir().join(format!("code-trace-wasm-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let source = dir.join("traced.code");
    let fixture = fs::read_to_string("tests/trace_handler.code").unwrap();
    fs::write(
        &source,
        format!("{fixture}\nReadTraces => return Recorded {{ traces = traces }}\n"),
    )
    .unwrap();
    let quiet = dir.join("quiet.code");
    fs::write(
        &quiet,
        format!(
            "Ping => return Pong\nHuge => return Payload {{ value = \"{}\" }}\nemit Ping to this\n",
            "a".repeat(70000)
        ),
    )
    .unwrap();
    for release in [false, true] {
        code::compile_file(
            &source,
            code::BuildTarget::Wasm,
            &dir.join("traced.wasm"),
            release,
        )
        .unwrap();
        code::compile_file(
            &quiet,
            code::BuildTarget::Wasm,
            &dir.join("quiet.wasm"),
            release,
        )
        .unwrap();
        fs::write(dir.join("probe.mjs"), r#"
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {createHost} from './host.mjs';
const traced = await WebAssembly.compile(readFileSync(new URL('./traced.wasm', import.meta.url)));
const quiet = await WebAssembly.compile(readFileSync(new URL('./quiet.wasm', import.meta.url)));
assert(WebAssembly.Module.imports(traced).some(x => x.name === 'code_host_flow_begin'));
assert(!WebAssembly.Module.imports(quiet).some(x => x.name.startsWith('code_host_flow_')));
const host = createHost();
const instance = await WebAssembly.instantiate(traced, {env: host.env});
assert.equal(host.start(instance), 0); // existing native fixture's assertions, including cap/unwinding
assert.equal(host.ask({_class:'Outer', word:'page'}).size, 4);
let trace = host.ask({_class:'ReadTraces'}).traces.at(-1);
assert.equal(trace.root.from, 'page');
assert.equal(trace.root.class, 'Outer');
assert.deepEqual(trace.steps.map(s=>s.parent), [null,0,0]);
assert(trace.steps.every(s=>s.at >= 0 && s.ms >= 0 && s.program === null));
const before = host.ask({_class:'ReadTraces'}).traces.length;
host.ask({_class:'NotHandled'});
assert.equal(host.ask({_class:'ReadTraces'}).traces.length, before + 1); // only previous ReadTraces
assert.equal(host.ask({_class:'Fails'})._class, 'Exception');
trace = host.ask({_class:'ReadTraces'}).traces.at(-1);
assert.equal(trace.ok, false);
assert.equal(trace.steps[0].ok, false);
// A large root can grow the shared event buffer during Trace delivery.
assert.equal(host.ask({_class:'Outer', word:'a'.repeat(70000)}).size, 70000);
host.fire({_class:'Outer', word:'fired'});
trace = host.ask({_class:'ReadTraces'}).traces.at(-1);
assert.equal(trace.root.particle.word, 'fired');
host.stop();
assert.equal(host.ask({_class:'Outer'}), null);
const plain = createHost();
const p = await WebAssembly.instantiate(quiet, {env:plain.env});
assert.equal(plain.start(p), 0);
assert.equal(plain.ask({_class:'Ping'})._class, 'Pong');
assert.equal(plain.ask({_class:'Huge'}).value.length, 70000);
assert.equal(plain.ask({_class:'Ping'})._class, 'Pong');
plain.stop();
"#).unwrap();
        let output = Command::new("node")
            .arg(dir.join("probe.mjs"))
            .output()
            .expect("Node is required for wasm tracing regression");
        assert!(
            output.status.success(),
            "release={release}: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    fs::remove_dir_all(dir).unwrap();
}
