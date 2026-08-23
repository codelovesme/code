// Functional smoke test for the *published* npm package artifact — run
// against the actual built dist/code_wasm.js + dist/code_wasm_bg.wasm, not
// just a build-only check. A build succeeding says nothing about whether
// the module actually runs.
//
// This exercises the --target web build (the one that actually gets
// published and that a third-party embedder would use), loaded directly
// from raw wasm bytes — no HTTP server, no bundler — so it doubles as a
// check that the package works with nothing more than plain
// `node smoke-test.mjs` after `npm install`.
//
// Usage: node smoke-test.mjs [path-to-dist-dir]  (defaults to ./dist)
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

const distDir = resolve(process.argv[2] ?? "./dist");
const mod = await import(resolve(distDir, "code_wasm.js"));
const wasmBytes = readFileSync(resolve(distDir, "code_wasm_bg.wasm"));
await mod.default({ module_or_path: wasmBytes });

let failures = 0;

function check(label, actual, expected) {
  if (actual !== expected) {
    failures++;
    console.error(`FAIL ${label}\n  expected: ${JSON.stringify(expected)}\n  actual:   ${JSON.stringify(actual)}`);
  } else {
    console.log(`ok   ${label}`);
  }
}

// Baseline: plain run(), no modules.
check(
  "run(), no modules",
  mod.run("let a = 5\nassert a = 5\n"),
  "a = 5\n",
);

check(
  "run(), particle construction and dispatch to core",
  mod.run('emit Length { "value": "hello" } to core get n\nassert n.value = 5\n'),
  'n = {"_class":"LengthResult","value":5}\n',
);

// A "module" backed by nothing more than a plain JS function — proving the
// run_with_modules bridge itself works without needing an actual .wasm file
// in this test. A real embedder would back `dispatch`/`vars` with a
// WebAssembly.Instance's own exports instead — see the README.
const modules = {
  m: {
    dispatch(particleJson) {
      const particle = JSON.parse(particleJson);
      if (particle._class === "Double") {
        return JSON.stringify({ _class: "DoubleResult", value: particle.value * 2 });
      }
      throw new Error("unknown handler");
    },
    vars() {
      return JSON.stringify({ answer: 42 });
    },
  },
};

check(
  "run_with_modules: dispatch",
  mod.run_with_modules(
    'link "m" as m\nemit Double { "_class": "Double", "value": 21 } to m get n\nassert n = { "_class": "DoubleResult", "value": 42 }\n',
    modules,
  ),
  'm = {"answer":42}\nn = {"_class":"DoubleResult","value":42}\n',
);

check(
  "run_with_modules: vars",
  mod.run_with_modules('link "m" as m\nassert m.answer = 42\n', modules),
  'm = {"answer":42}\n',
);

// The host registers the module as "m", but the script is free to rename it
// on `as` — the alias, not the registered name, is what becomes a binding
// and an `emit` target.
check(
  "run_with_modules: link ... as renames the host's module name",
  mod.run_with_modules('link "m" as renamed\nassert renamed.answer = 42\n', modules),
  'renamed = {"answer":42}\n',
);

check(
  "run_with_modules: unregistered alias",
  mod.run_with_modules('link "nope" as m\n', modules),
  "error: cannot link 'nope': no such module was provided to run_with_modules",
);

// A module with no `vars` at all — should bind an empty object, matching
// .so's "no code_module_vars export" default.
const noVarsModule = {
  echo: {
    dispatch(particleJson) {
      return particleJson;
    },
  },
};

check(
  "run_with_modules: module with no vars() gets an empty object",
  mod.run_with_modules('link "echo" as e\nassert e = {}\n', noVarsModule),
  "e = {}\n",
);

if (failures > 0) {
  console.error(`\n${failures} check(s) failed`);
  process.exit(1);
}
console.log("\nall checks passed");
