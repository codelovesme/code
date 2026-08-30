// Every example the page shows has to run in the engine the page loads.
//
// Those are two different things: the examples come from `tests/*.code` in
// this checkout, and the engine is a *pinned* `code-wasm` release from npm
// (see `.github/workflows/pages.yml` for why). Nothing connected them, so a
// language change shipped a playground whose own examples it could not
// parse — `∈` arrived, the pinned engine was three releases old, and the
// front page offered programs that answered `unexpected character '∈'`.
//
// Run after `site/build.py`, against the assembled `dist/`:
//
//     node site/check_examples.mjs dist
//
// Exits non-zero on the first example the engine refuses, naming it. The
// examples are exactly the fixtures that `run_language_tests.rs` proves run
// cleanly, so anything failing here is the *engine* being out of step, and
// the fix is to bump PLAYGROUND_CODE_WASM_VERSION rather than to change the
// example.
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

const dist = resolve(process.argv[2] ?? "dist");
const { default: init, run_with_modules } = await import(`${dist}/pkg/code_wasm.js`);
await init({ module_or_path: readFileSync(`${dist}/pkg/code_wasm_bg.wasm`) });

// The same `console` the page supplies, in its smallest honest form: the
// site's own examples `link "console"`, and an engine handed no such module
// would refuse them for a reason that has nothing to do with the language.
const consoleModule = {
  dispatch(particleJson) {
    const particle = JSON.parse(particleJson);
    if (particle._class !== "Print") return "null";
    const text =
      typeof particle.value === "string"
        ? particle.value
        : JSON.stringify(particle.value);
    return JSON.stringify({ _class: "TerminalResult", value: text.length });
  },
};

const examples = JSON.parse(readFileSync(`${dist}/examples.json`, "utf8"));
const failures = [];
for (const example of examples) {
  try {
    run_with_modules(example.code, { console: consoleModule });
  } catch (error) {
    failures.push(`${example.name}: ${String(error).split("\n")[0]}`);
  }
}

if (failures.length > 0) {
  console.error(
    `${failures.length} of ${examples.length} playground examples do not run in the pinned engine:`,
  );
  for (const failure of failures) console.error(`  ${failure}`);
  console.error(
    "\nThe examples come from tests/*.code and are proven to run; this is the pinned",
  );
  console.error(
    "code-wasm release being older than the language. Bump PLAYGROUND_CODE_WASM_VERSION",
  );
  console.error("in .github/workflows/pages.yml to a release that has them.");
  process.exit(1);
}
console.log(`${examples.length} playground examples run in the pinned engine`);
