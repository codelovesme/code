# @codelovesme/code-wasm

Run [Code](https://codelovesme.github.io/code/) — a constraint-based
programming language with no user-defined functions and no core I/O — in
any browser or JS host. A small WASM bridge around the language's real
parser and interpreter, zero JS dependencies.

Try it live first: [playground](https://codelovesme.github.io/code/playground/).
Learn the language: [guide](https://codelovesme.github.io/code/guide.html) ·
[tutorial](https://codelovesme.github.io/code/tutorial.html) ·
[reference](https://codelovesme.github.io/code/reference.html).

## Install

```
npm install @codelovesme/code-wasm
```

## Usage

One export, `run_source(src)`, plus a default init function you call once
before the first run. `run_source` is synchronous — no `await` on the call
itself, only on `init()`.

### In a bundler (Vite, webpack 5, Rollup, …)

```js
import init, { run_source } from "@codelovesme/code-wasm";

await init();

const result = run_source(`
  a = 5
  b > 3
  b < 10
  assert a = 5
`);

console.log(result);
// {
//   ok: true,
//   bindings: [
//     { name: "a", value: "5", kind: "Number" },
//     { name: "b", domain: "3 < _ < 10" },   // narrowed, never pinned to one value
//   ],
//   diagnostics: [],
// }
```

### Directly in a browser, no bundler

```html
<script type="module">
  import init, { run_source } from "https://esm.sh/@codelovesme/code-wasm";
  await init();
  console.log(run_source("x = 1 + 1\nassert x = 2\n"));
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
import init, { run_source } from "@codelovesme/code-wasm";

const wasmUrl = import.meta.resolve("@codelovesme/code-wasm/dist/code_wasm_bg.wasm");
await init({ module_or_path: readFileSync(new URL(wasmUrl)) });

console.log(run_source("a = 1\n"));
```

## The `run_source` result shape

This is a public contract — see this package's
[source ticket](https://github.com/codelovesme/code/blob/main/docs/tickets/low/19-browser-playground.md)
before depending on internals beyond what's documented here.

```ts
type RunResult = {
  ok: boolean;
  bindings: Binding[];
  diagnostics: Diagnostic[];
};

type Binding =
  | { name: string; value: string; kind: string }   // resolved to one value
  | { name: string; domain: string };                // narrowed, never pinned

type Diagnostic = {
  message: string;
  start: number;  // char offset into the source you passed in
  end: number;
};
```

Code has no core I/O — a program never prints anything. Its only
observable result is its final top-level variable bindings (plus any
`assert` that fails, surfaced as a diagnostic). That's what `bindings`
is: the entire visible output of the program, the same thing a
constraint-solver's final variable assignment would be.

## Scope (v1)

- A single, self-contained snippet — no `link` (module import) support yet.
  No filesystem access in a browser; module linking via an in-memory
  source map is a planned follow-up, not yet wired into this bridge.
- No native (`.so`) module linking — native code has no meaning in a wasm
  sandbox.

## Releasing (maintainers)

Published via **npm Trusted Publishing (OIDC)** from GitHub Actions
(`.github/workflows/publish-npm-wasm.yml`) — no `NPM_TOKEN` stored
anywhere. The workflow exchanges a short-lived GitHub OIDC token for a
short-lived npm publish credential at publish time; there's no
long-lived secret to leak or rotate.

**One-time setup, done once by an owner of the `codelovesme` npm org
(can't be done from CI — this is npmjs.com account configuration):**

1. On [npmjs.com](https://www.npmjs.com), open (or create, unpublished —
   npm supports configuring a trusted publisher before the first
   publish) the `@codelovesme/code-wasm` package.
2. **Settings → Trusted Publisher → GitHub Actions**, and set:
   - Organization or user: `codelovesme`
   - Repository: `code`
   - Workflow filename: `publish-npm-wasm.yml`
   - Environment name: leave blank (the workflow doesn't use one)
3. Once a real publish has gone through this way, go back to
   **Settings → Publishing access** and turn on **"Require two-factor
   authentication and disallow tokens"** — this is what actually closes
   the door on a stolen classic token being used to publish, which is
   the whole point of doing this over a token in the first place.

**Every release after that** is just:

```bash
git tag code-wasm-v0.2.0   # whatever the new version is — no `v` prefix elsewhere
git push origin code-wasm-v0.2.0
```

The workflow sets `package.json`'s version from the tag itself (one
source of truth for what's being published, rather than trusting a
hand-edited `package.json` matches), builds, smoke-tests the actual
packaged artifact, and publishes with `--provenance`. Bump the version
deliberately — this JS API is a public contract the moment it's
published (see the T19 ticket in the main repo).

`workflow_dispatch` (the "Run workflow" button in the Actions tab) does
everything except the actual publish — a real dry run against the exact
package that would ship, not a separate code path. To test locally
before tagging:

```bash
bash build.sh                # builds dist/ from the current Rust source
npm pack --dry-run           # sanity-check the tarball contents
node smoke-test.mjs          # run the actual packaged artifact, not just build it
```

## License

GPL-3.0-or-later — see [LICENSE](./LICENSE). Source:
[github.com/codelovesme/code](https://github.com/codelovesme/code),
under `crates/code-wasm`.
