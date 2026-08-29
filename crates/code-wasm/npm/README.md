# code-wasm

Run [Code](https://codelovesme.github.io/code/) — a language with no
user-defined functions, where behavior comes from emitting particles at
compiled-in or linked handlers — in any browser or JS host. A small WASM
bridge around the language's real parser and interpreter.

Try it live first: [playground](https://codelovesme.github.io/code/).

## Install

```
npm install code-wasm
```

## Usage

Two exports — `run(src)` and `run_with_modules(src, modules)` — plus a
default init function you call once before the first run. Both `run`
functions are synchronous — no `await` on the call itself, only on
`init()`.

### In a bundler (Vite, webpack 5, Rollup, …)

```js
import init, { run } from "code-wasm";

await init();

console.log(run("let a = 5\nassert a = 5\n"));
// "a = 5\n"
```

### Directly in a browser, no bundler

```html
<script type="module">
  import init, { run } from "https://esm.sh/code-wasm";
  await init();
  console.log(run("let x = 1 + 1\nassert x = 2\n"));
</script>
```

### In Node.js

`init()`'s default (no-argument) form resolves the `.wasm` file via
`import.meta.url` and `fetch()`, which works in a browser but not under
plain Node. Pass the bytes directly instead, resolved with
[`import.meta.resolve`](https://nodejs.org/api/esm.html#importmetaresolvespecifier)
(Node 20.6+):

```js
import { readFileSync } from "node:fs";
import init, { run } from "code-wasm";

const wasmUrl = import.meta.resolve("code-wasm/dist/code_wasm_bg.wasm");
await init({ module_or_path: readFileSync(new URL(wasmUrl)) });

console.log(run("let a = 1\n"));
```

## `run(src)`

Runs the program and returns `""` on success or `"error: ..."` on any
failure (parse error, undefined variable, a failed `assert`, …). A
program's observable output is whatever it emits itself through a linked
module — there is no bindings dump to return. There is no structured
result type; parse the string yourself if you need one.

```js
run('let a = 5\nassert a = 5\n');
// ''
```

## `run_with_modules(src, modules)`

Lets `src` `link` third-party modules — the thing plain `run` refuses (see
Scope below). `modules` is a plain JS object; each own-enumerable key is a
name your script can `link "<name>" as <alias>`, mapped to
`{ dispatch(particleJson) -> resultJson, vars?() -> varsJson }`:

- **`dispatch`** is called once per `emit <particle> to <alias>` — `code`
  serializes the particle to a JSON string (its own value model already
  *is* JSON), calls your function, and parses whatever JSON string it
  returns back into a value. Both calls are synchronous.
- **`vars`** (optional) is called once, when `link` runs, and becomes
  `<alias>.<name>` field access — the same role a `.so` module's exported
  variables play natively. A module with no `vars` gets an empty object.

```js
const modules = {
  math: {
    dispatch(particleJson) {
      const p = JSON.parse(particleJson);
      if (p._class === "Double") {
        return JSON.stringify({ _class: "DoubleResult", value: p.value * 2 });
      }
      throw new Error("unknown handler");
    },
    vars() {
      return JSON.stringify({ pi: 3.14159 });
    },
  },
};

run_with_modules(
  'link "math" as m\n' +
    'emit Double { _class = "Double", value = 21 } to m get n\n' +
    "assert n.value = 42\n",
  modules,
);
```

`code` never touches WebAssembly bytes directly — turning an actual
`.wasm` file into this `{dispatch, vars}` shape (instantiating it, reading
its exports, marshaling to/from its linear memory) is entirely your job.
A minimal sketch, assuming the module exports `alloc`/`dispatch`/`dealloc`
functions that read/write UTF-8 JSON through its own memory:

```js
const { instance } = await WebAssembly.instantiateStreaming(fetch(url));
const enc = new TextEncoder(), dec = new TextDecoder();

function callWasm(fn, json) {
  const bytes = enc.encode(json);
  const ptr = instance.exports.alloc(bytes.length);
  new Uint8Array(instance.exports.memory.buffer, ptr, bytes.length).set(bytes);
  const [outPtr, outLen] = fn(ptr, bytes.length); // your module's own convention
  const result = dec.decode(new Uint8Array(instance.exports.memory.buffer, outPtr, outLen));
  instance.exports.dealloc(ptr, bytes.length);
  return result;
}

const modules = {
  mymodule: {
    dispatch: (json) => callWasm(instance.exports.dispatch, json),
  },
};
```

Because everything crosses this boundary as plain JSON text, a module
backed by a plain JS function (no WebAssembly at all, as in the first
example above) is just as valid — `code` can't tell the difference, and
doesn't need to.

`link` is resolved entirely before `run_with_modules` starts running
`src` — there is no way for a script to trigger a *new*
`WebAssembly.instantiate` mid-run (that's inherently asynchronous;
`link`ing isn't). Provide every module you might need up front.

## Scope

- `run` (no modules) has no `link` support at all, deliberately — the
  playground's plain snippets never need it.
- No native `.so`/`.a` module linking — native machine code has no
  meaning inside a WebAssembly sandbox; see
  [`docs/todo/native-module-linking.md`](https://github.com/codelovesme/code/blob/main/docs/todo/native-module-linking.md)
  for why `.wasm`/JS modules are the wasm32 answer to what `.so`/`.a` are
  natively.

## Releasing (maintainers)

Published via **npm Trusted Publishing (OIDC)** from GitHub Actions
(`.github/workflows/publish-npm-wasm.yml`) — no `NPM_TOKEN` stored
anywhere. The workflow exchanges a short-lived GitHub OIDC token for a
short-lived npm publish credential at publish time; there's no
long-lived secret to leak or rotate.

`code-wasm` on npm already has `0.1.0`/`0.1.1` published under this same
repo, from the *old* language (a different API entirely —
`run_source`/structured `{ok, bindings, diagnostics}`, superseded here by
plain `run`/`run_with_modules` string results). That's why this package
starts at `1.0.0` rather than continuing the `0.1.x` line: a real API
break deserves a major bump, not a version someone's `^0.1.0` pin would
silently accept. If the old repo's Trusted Publisher was already
configured for this exact package name + repo + workflow filename, the
one-time npm-side setup below is likely already done — check the
package's **Settings → Trusted Publisher** page on npmjs.com before
redoing it.

**One-time setup (can't be done from CI — this is npmjs.com account
configuration, done once by whoever's publishing this the first time):**

Option A — pre-configure before the package exists at all: npm's
**Staged Packages** feature (account menu → Staged Packages) lets you
register a trusted publisher for a package name that's never been
published yet, so the very first publish is already OIDC-only.

Option B — if that's not available: publish once manually to claim the
name (`npm login && npm publish` from this directory, after `bash
build.sh`), then configure trusted publishing on the now-existing
package's own **Settings → Trusted Publisher** page for every release
after that.

Either way, the trusted-publisher configuration itself is:

- Organization or user: your npm username
- Repository: `codelovesme/code`
- Workflow filename: `publish-npm-wasm.yml`
- Environment name: leave blank (the workflow doesn't use one)

Once a real publish has gone through via trusted publishing, go back to
**Settings → Publishing access** and turn on **"Require two-factor
authentication and disallow tokens"** — this is what actually closes the
door on a stolen classic token being used to publish, which is the whole
point of doing this over a token in the first place.

**Every release after the one-time setup** is just:

```bash
git tag code-wasm-v0.2.0   # whatever the new version is — no `v` prefix elsewhere
git push origin code-wasm-v0.2.0
```

The workflow sets `package.json`'s version from the tag itself (one
source of truth for what's being published, rather than trusting a
hand-edited `package.json` matches), builds, smoke-tests the actual
packaged artifact, and publishes with `--provenance`. Bump the version
deliberately — this JS API is a public contract the moment it's
published.

`workflow_dispatch` (the "Run workflow" button in the Actions tab) does
everything except the actual publish — a real dry run against the exact
package that would ship, not a separate code path. To test locally
before tagging (already verified once while building this):

```bash
bash build.sh                # builds dist/ from the current Rust source
npm pack --dry-run           # sanity-check the tarball contents
node smoke-test.mjs          # run the actual packaged artifact, not just build it
```

## License

GPL-3.0-or-later — see [LICENSE](./LICENSE). Source:
[github.com/codelovesme/code](https://github.com/codelovesme/code),
under `crates/code-wasm`.
