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
| `compare.html` | Existing comparisons, measurements and methodology | Standalone comparison template |

`layout.html` wraps the content fragments. Every page, including Compare, uses
`navigation.html`, `footer.html`, `navigation.css` and `navigation.js`.
`site.css` supplies the common content styles; Compare retains its chart/content
styles so the navigation split does not redesign its measurements.

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
fixture exclusions remain. `demos.json` names the source example used for the
static Home/Learn preview and its expected output; the build writes the full
preview manifest for the same engine check as the gallery. Rendering a preview
never requires loading WASM on the home page.

The 2.16.0 engine rejects the newer `handler_recursion` fixture. This failure
also reproduces with the unmodified previous site. `BROWSER_INCOMPATIBILITIES`
in `build.py` records that single version-specific exception, which is printed
during assembly and written to `browser-compatibility.json`. The fixture stays
in the language suite. A different engine pin automatically includes it again
and must pass the runtime check; do not turn the exception into a permanent
exclusion or silently filter any other failing example.

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
| 212 | Modules preview/reference from `docs.html#modules`; a focused page when ready |
| 213 | Execution explanation from `docs.html#cli`; a focused page when ready |
| 214 | Memory/value explanation, linked from Docs |
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
