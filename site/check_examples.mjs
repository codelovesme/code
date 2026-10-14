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
// Checks returned error strings as well as thrown errors. The one deliberate
// undefined-variable example must fail; the landing demos must print exactly
// what their output panels show.
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { runInNewContext } from "node:vm";

const dist = resolve(process.argv[2] ?? "dist");
const { default: init, run_with_modules } = await import(`${dist}/pkg/code_wasm.js`);
await init({ module_or_path: readFileSync(`${dist}/pkg/code_wasm_bg.wasm`) });

// The same `console` the page supplies, in its smallest honest form: the
// site's own examples `link "console"`, and an engine handed no such module
// would refuse them for a reason that has nothing to do with the language.
let printed = [];
const consoleModule = {
  dispatch(particleJson) {
    const particle = JSON.parse(particleJson);
    if (particle._class !== "Print") return "null";
    const text =
      typeof particle.value === "string"
        ? particle.value
        : JSON.stringify(particle.value);
    printed.push(text);
    return JSON.stringify({ _class: "PrintResult", value: [...text].length });
  },
};

const examples = JSON.parse(readFileSync(`${dist}/examples.json`, "utf8"));
// The animated landing-page programs must produce the output they advertise.
const page = readFileSync(`${dist}/index.html`, "utf8");
const demos = runInNewContext(page.match(/const DEMOS = (\[[\s\S]*?\n  \]);/)[1]);
for (const demo of demos) {
  examples.push({ name: demo.file, code: demo.code, output: demo.out });
}
const failures = [];
for (const example of examples) {
  try {
    printed = [];
    const result = run_with_modules(example.code, { console: consoleModule });
    const expectedError = example.name === "fail_undefined_variable";
    if (expectedError ? !result.startsWith("error: undefined variable") : result !== "") {
      throw new Error(result || "expected an undefined-variable error");
    }
    if (example.output !== undefined && printed.join("\n") !== example.output) {
      throw new Error(`expected output ${JSON.stringify(example.output)}, got ${JSON.stringify(printed.join("\n"))}`);
    }
  } catch (error) {
    failures.push(`${example.name}: ${String(error).split("\n")[0]}`);
  }
}

if (failures.length > 0) {
  console.error(
    `${failures.length} of ${examples.length} playground examples do not run in the pinned engine:`,
  );
  for (const failure of failures) console.error(`  ${failure}`);
  console.error("\nCheck the pinned code-wasm version, browser module support, and expected output.");
  process.exit(1);
}
console.log(`${examples.length} playground and landing examples checked in the pinned engine`);
