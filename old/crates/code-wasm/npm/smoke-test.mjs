// Functional smoke test for the *published* npm package artifact — run
// against the actual built dist/code_wasm.js + dist/code_wasm_bg.wasm, not
// just a build-only check. Same reasoning as ../smoke-test/run.js (the
// --target nodejs build used for CI's interpreter-level check): a build
// succeeding says nothing about whether the module actually runs — this
// project has hit two real runtime-only failures before (rust-lld SIGSEGV
// at link time, a wasm stack overflow at run time) that only running the
// module ever caught.
//
// This one specifically exercises the --target web build (the one that
// actually gets published and that a third-party embedder would use),
// loaded directly from raw wasm bytes — no HTTP server, no bundler —
// so it doubles as a check that the package works with nothing more than
// plain `node smoke-test.mjs` after `npm install`.
//
// Usage: node smoke-test.mjs [path-to-dist-dir]  (defaults to ./dist)
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

const distDir = resolve(process.argv[2] ?? "./dist");
const mod = await import(resolve(distDir, "code_wasm.js"));
const wasmBytes = readFileSync(resolve(distDir, "code_wasm_bg.wasm"));
await mod.default({ module_or_path: wasmBytes });

let failures = 0;

function normalize(result) {
  if (result && Array.isArray(result.bindings)) {
    return { ...result, bindings: [...result.bindings].sort((a, b) => a.name.localeCompare(b.name)) };
  }
  return result;
}

function check(label, actual, expected) {
  const a = JSON.stringify(normalize(actual));
  const e = JSON.stringify(normalize(expected));
  if (a !== e) {
    failures++;
    console.error(`FAIL ${label}\n  expected: ${e}\n  actual:   ${a}`);
  } else {
    console.log(`ok   ${label}`);
  }
}

check(
  "resolved bindings",
  mod.run_source('a = 5\nb = "hi"\nassert a = 5\n'),
  {
    ok: true,
    bindings: [
      { name: "a", value: "5", kind: "Number" },
      { name: "b", value: "hi", kind: "String" },
    ],
    diagnostics: [],
  },
);

check(
  "particle construction and dispatch",
  mod.run_source(
    'Log = Particle ∩ { _class ∈ "Log", message ∈ String }\n' +
      "Log => {\n    return Log { message = message }\n}\n" +
      "emit Log { message = \"hi\" } to this get r\n" +
      'assert r.message = "hi"\n',
  ),
  {
    ok: true,
    bindings: [
      {
        name: "Log",
        value: '{_class ∈ (must be of type String ∩ "Log"), _created ∈ (must be of type Number), message ∈ (must be of type String)}',
        kind: "Schema",
      },
      { name: "r", value: "{ _created = 0, _class = Log, message = hi }", kind: "Object" },
    ],
    diagnostics: [],
  },
);

check(
  "parse error",
  mod.run_source("a = = 5\n"),
  {
    ok: false,
    bindings: [],
    diagnostics: [{ message: "Unexpected: a = = 5", start: 0, end: 7 }],
  },
);

if (failures > 0) {
  console.error(`\n${failures} check(s) failed`);
  process.exit(1);
}
console.log("\nall checks passed");
