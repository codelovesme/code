# Code website

The site is static HTML, CSS and JavaScript, assembled by `build.py` and
published through `.github/workflows/pages.yml`. It uses the published browser
engine pinned in that workflow; building the site does not publish a language
release or replace the engine with a source build.

## Pages and shared shell

| Public route | Purpose | Source |
|---|---|---|
| `index.html` | Short introduction and next steps | `index.html` content fragment |
| `learn.html` | Working first example and orientation | `learn.html` content fragment |
| `playground.html` | Editor, results and complete browser-compatible gallery | `playground.html`, `playground.mjs` |
| `docs.html` | Reference, CLI, modules, embedding and authoring | `docs.html` content fragment |
| `modules.html` | Install, link, reuse and target support | `modules.html`, `module-examples/` |
| `execution.html` | Run/cache/interpret/native and the two WASM paths | `execution.html`, `execution-examples/` |
| `memory.html` | Independent value updates, sharing and reclamation | `memory.html`, `examples/value_snapshot.code` |
| `compare.html` | Existing comparisons, measurements and methodology | Standalone comparison template |

`layout.html` wraps the content fragments. Every page, including Compare, uses
`navigation.html`, `footer.html`, `navigation.css` and `navigation.js`.
`site.css` supplies the common content styles; Compare retains its chart/content
styles so the navigation split does not redesign its measurements.
Modules, Execution and Memory are Docs destinations and keep Docs selected in the shared navigation.

Preview generated output rather than serving the source fragments directly.
There is no router/server framework, new package manager or client build step.
Relative URLs keep the artifact usable under GitHub Pages' `/code/` prefix.

## Existing links and drafts

The home page forwards these published fragment URLs with `location.replace`,
so Back does not return to a redirect loop:

| Old home fragment | Destination |
|---|---|
| `#playground` | `playground.html#playground` |
| `#examples` | `playground.html#examples` |
| `#packages` | `docs.html#packages` |
| `#features` | `docs.html#features` |
| `#src=<encoded program>` | `playground.html#src=<encoded program>` |

Both the directory URL and `index.html` accept these fragments, including later
hash changes. The playground keeps the existing `code-playground-src` storage
key, so drafts survive the split. A source share wins over a draft; a named
`?example=hello_home` link opens that example intentionally.

Compare retains its public route and existing performance/memory/fit anchors.
Docs keeps the old `packages`/`features` anchors and gives individual integration
cards stable `cli`, `applications`, `embedding`, `code-modules`, `native-modules`
and `publishing` anchors. Native build instructions and package versions remain
separate from the playground engine pin.

## Examples and compatibility

The gallery still comes from the real `tests/*.code` fixtures plus
`site/examples/*.code`. Existing native-link, stress and intentionally invalid
fixture exclusions remain. `demos.json` names the source examples used for the
static Home/Learn/Memory previews and their expected output; the build writes the full
preview manifest for the same engine check as the gallery. Rendering a preview
never requires loading WASM on the home page.

The 2.16.0 engine rejects the newer `handler_recursion` fixture. This failure
also reproduces with the unmodified previous site. `BROWSER_INCOMPATIBILITIES`
in `build.py` records that single version-specific exception, which is printed
during assembly and written to `browser-compatibility.json`. The fixture stays
in the language suite. A different engine pin automatically includes it again
and must pass the runtime check; do not turn the exception into a permanent
exclusion or silently filter any other failing example.

The Modules page's CLI-only snippets come from `module-examples/`, not the
playground gallery. The build renders those exact files and copies them as
downloadable assets. It does not send a multi-file native example to the
console-only browser interpreter. Target availability is scoped to published
v2.23.0 release manifests, with DOM's native stub distinguished from browser
operations; future source APIs do not imply new release availability.

Verify the native examples with the actual published **Linux v2.23.0 CLI**:

```sh
python3 site/check_module_examples.py /absolute/path/to/code-v2.23.0-x86_64-linux/code
```

The checker needs network access, `curl` and `cc`. In a temporary project it
installs the public release's console module, verifies its lock version/source/
hash, runs the displayed source and compiled-library callers interpreted and
compiled, proves private-name access is refused, and builds shared/static
libraries. It refuses a different CLI version and never uses source-built
module assets. This explicit native check is separate from the Pages workflow's
published-browser-engine check. Run it when changing these versioned examples.

The execution guide reuses the exact Home/Playground greeting across native
and compiled-WASM checks. To verify with the **published v2.23.0 CLI**, use a
new browser-output directory:

```sh
python3 site/check_execution.py /absolute/path/to/code-v2.23.0-x86_64-linux/code /tmp/code-execution-web
python3 -m http.server 8956 --directory /tmp/code-execution-web
```

This checks cold/warm executable reuse (inode/bytes; access updates mtime),
source/lock invalidation, interpreted/native output and O0/background-O2 tiers
in an isolated cache, then builds with the public console WASM archive.
`execution-examples/index.html.txt` is a downloadable host-page template, not
a published binary. The checker saves it as `index.html` beside the generated
`hello.wasm` and `host.mjs`. With the Playwright setup below, verify the actual
compiled program and its missing-artifact state:

```sh
PLAYWRIGHT_MODULE=/tmp/code-site-tools/node_modules/playwright/index.mjs \
  node site/check_compiled_browser.mjs http://localhost:8956/
```

These native/compiler checks are distinct from executing source through the
site's separately pinned published 2.16.0 interpreter. No release/runtime pin
is upgraded by building or verifying either website guide.

Memory's `value_snapshot` is both a runnable gallery example and a checked
preview. Verify the same source using the actual published Linux CLI/modules:

```sh
python3 site/check_memory.py /absolute/path/to/code-v2.23.0-x86_64-linux/code
```

The checker uses temporary projects/cache, native leak checks and the public
v2.23.0 console module. It checks visible snapshots plus nested path updates,
array/text growth and self-containing snapshots interpreted and compiled. It
also compiles a small C probe against the published tag's ABI header to check
the 16-byte slot and ABI version 2. This is layout/semantic evidence, not an
RSS or performance benchmark. The site keeps total memory, module/JSON
transport costs and Rust ownership trade-offs separate from that slot size.

## Build and check

From the repository root, use the **exact workflow pin** for `code-wasm`:

```sh
npm install --prefix /tmp/code-site-engine --no-save code-wasm@2.16.0
mkdir -p /tmp/code-site-preview/code/pkg
cp /tmp/code-site-engine/node_modules/code-wasm/dist/* /tmp/code-site-preview/code/pkg/
python3 site/build.py . /tmp/code-site-preview/code
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s site -p 'test_*.py'
node site/check_examples.mjs /tmp/code-site-preview/code
python3 -m http.server 8955 --directory /tmp/code-site-preview
```

Open `http://localhost:8955/code/`. The normal Pages workflow runs the static
route/anchor/determinism checks and checks all offered programs with its actual
published engine. Repository work also requires `cargo test --workspace`, per
`AGENTS.md`.

For real browser journeys, install Playwright and its Chromium browser in a
temporary tools directory, or use an existing matching installation:

```sh
npm install --prefix /tmp/code-site-tools --no-save playwright
/tmp/code-site-tools/node_modules/.bin/playwright install chromium
PLAYWRIGHT_MODULE=/tmp/code-site-tools/node_modules/playwright/index.mjs \
  node site/check_navigation.mjs http://localhost:8955/code/
```

This checks direct routes, keyboard navigation, four viewport widths, old
anchors/source shares, draft restoration, Learn/editor/Docs/back navigation,
example filters, auto-run/reset and a blocked-WASM failure state. The optional
browser tool is not part of the shipped site.

## Later content work

Shared work tickets live in `my-euglena-apps/docs/tickets/`, roadmap 210.
This split establishes usable routes; it does not complete the later roadmap.

| Tickets | Content destination |
|---|---|
| 211 | Home positioning and first-screen composition in `index.html` |
| 212 | `modules.html`, linked from Home and `docs.html#modules`; checked native snippets in `module-examples/` |
| 213 | `execution.html`, reached from Home/Docs; interpreter embedding stays at `docs.html#embedding` |
| 214 | `memory.html`, reached from Home/Docs; checked preview in `examples/value_snapshot.code` |
| 215–216 | Benchmark evidence, then a focused Performance view; preserve Compare links |
| 217 | Playground first-use examples and gallery presentation |
| 218–222 | Expand the Learn starter into a hub, lesson layout and four actual tutorials |
| 223 | Real-application proof on Home and project details |
| 224 | Further responsive/accessibility polish across the shared shell and lessons |
| 225 | Share image and metadata on the public pages |
| 227 | Integrated version, content and deployed-site verification |

Add routes only when they contain usable content. Keep navigation/footer shared,
relative links valid, executable previews checked and old public destinations
working when pages move again.
