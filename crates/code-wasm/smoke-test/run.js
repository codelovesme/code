// Functional smoke test for the code-wasm bridge (T19), run against the real
// compiled .wasm via node — not just a "does it compile" check. This exists
// because a build-only check would NOT have caught either toolchain issue
// found while building this crate:
//   1. A wasm stack overflow ("memory access out of bounds") on essentially
//      any real program — the parser's chumsky combinator recursion needs a
//      much larger stack than the linker reserves by default. The CLI works
//      around this with a 16MB-stack thread (src/main.rs); wasm32 has no OS
//      threads, so the stack has to be sized at link time instead (see
//      ../../../.cargo/config.toml).
//   2. A `rust-lld` SIGSEGV linking a debug build of this crate (release
//      mode doesn't trigger it) — see the crate's build instructions.
// Both are silent at compile time; only running the module surfaces them.
//
// Usage: node run.js <path-to-code_wasm.js>  (the wasm-bindgen `--target
// nodejs` glue output — see the crate README for how to generate it).

const modulePath = process.argv[2];
if (!modulePath) {
  console.error('usage: node run.js <path-to-code_wasm.js>');
  process.exit(2);
}
// A bare relative path passed to require() is resolved as a module-name
// lookup (node_modules), not a file path — resolve it explicitly first.
const { run_source } = require(require('path').resolve(process.cwd(), modulePath));

let failures = 0;

function check(label, actual, expected) {
  const a = JSON.stringify(actual);
  const e = JSON.stringify(expected);
  if (a !== e) {
    failures++;
    console.error(`FAIL ${label}\n  expected: ${e}\n  actual:   ${a}`);
  } else {
    console.log(`ok   ${label}`);
  }
}

// A normal program's bindings are reported correctly (also exercises the
// full parser/interpreter path deeply enough to catch a stack overflow).
check(
  'resolved bindings',
  run_source('a = 5\nb = "hi"\nassert a = 5\n'),
  {
    ok: true,
    bindings: [
      { name: 'a', value: '5', kind: 'Number' },
      { name: 'b', value: 'hi', kind: 'String' },
    ],
    diagnostics: [],
  },
);

// A range-constrained-but-never-pinned variable has no resolved value.
check(
  'unresolved domain',
  run_source('a > 5\na < 12\n'),
  { ok: true, bindings: [{ name: 'a' }], diagnostics: [] },
);

// A parse error is reported, located.
check(
  'parse error',
  run_source('a = = 5\n'),
  {
    ok: false,
    bindings: [],
    diagnostics: [{ message: 'Unexpected: a = = 5', start: 0, end: 7 }],
  },
);

// A runtime error (failed top-level assert) is reported, located.
check(
  'runtime error',
  run_source('assert false\n'),
  {
    ok: false,
    bindings: [],
    diagnostics: [{ message: 'Assertion failed', start: 0, end: 12 }],
  },
);

if (failures > 0) {
  console.error(`\n${failures} check(s) failed`);
  process.exit(1);
}
console.log('\nall checks passed');
