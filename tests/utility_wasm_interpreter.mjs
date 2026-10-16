// Run against freshly bound code-wasm: node ... <absolute pkg directory>.
import { readFileSync } from 'node:fs';
import { pathToFileURL } from 'node:url';
import { resolve } from 'node:path';
import { runInThisContext } from 'node:vm';
const pkg = resolve(process.argv[2]);
const { default: init, run_with_modules } = await import(pathToFileURL(pkg + '/code_wasm.js'));
await init({ module_or_path: readFileSync(pkg + '/code_wasm_bg.wasm') });
const root = new URL('../', import.meta.url);
const modules = {};
for (const name of ['math', 'strings', 'time']) {
  const factory = runInThisContext('(' + readFileSync(new URL('crates/modules/' + name + '/page.mjs', root), 'utf8') + ')');
  const handler = factory({})[1];
  modules['native_modules/' + name + '.so'] = { dispatch: json => JSON.stringify(handler(JSON.parse(json))) };
}
for (const file of ['math_utilities', 'math_double', 'math_sum', 'math_refuses_with_exception', 'strings_utilities', 'strings_length', 'strings_case', 'strings_echo_nested', 'strings_split', 'strings_join', 'strings_trim', 'strings_shout', 'strings_refuses_with_exception', 'time_queries']) {
  const result = run_with_modules(readFileSync(new URL('tests/' + file + '.code', root), 'utf8'), modules);
  if (result !== '') throw new Error(file + ': ' + result);
}
console.log('14 utility fixtures passed in actual code-wasm interpreter');
