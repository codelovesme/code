# Code website

The site is static HTML, CSS and JavaScript, assembled by `build.py` and
published through `.github/workflows/pages.yml`. It uses the published browser
engine pinned in that workflow; building the site does not publish a language
release or replace the engine with a source build.

## Pages and shared shell

| Public route | Purpose | Source |
|---|---|---|
| `index.html` | Short introduction and next steps | `index.html` content fragment |
| `learn.html` | Immediate greeting and the ordered four-lesson path | `learn.html`, `lessons.json` |
| `learn/quick-start.html` | Ready browser warm-up using the shared lesson layout | `learn/quick-start.html`, `lesson.html`, `lessons.py` |
| `playground.html` | Four featured programs, editor/results and an expandable paged collection | `playground.html`, `playground.mjs` |
| `docs.html` | Reference, CLI, modules, embedding and authoring | `docs.html` content fragment |
| `modules.html` | Install, link, reuse and target support | `modules.html`, `module-examples/` |
| `execution.html` | Run/cache/interpret/native and the two WASM paths | `execution.html`, `execution-examples/` |
| `memory.html` | Independent value updates, sharing and reclamation | `memory.html`, `examples/value_snapshot.code` |
| `compare.html` | Concise native speed, run modes, snapshot RSS and reproducible evidence | `compare.html`, `performance.py` |

`layout.html` wraps the content fragments. Every page, including Compare, uses
`navigation.html`, `footer.html`, `navigation.css` and `navigation.js`.
`site.css` supplies common content and chart styles. Modules, Execution and Memory keep Docs selected in the navigation.
Comparison is a top-level destination with its own current-page state. The
shared menu is Learn, Playground, Docs, Comparison; GitHub remains in the
footer. Follow-up 229 restores the chart-led comparison after the initial
navigation-only change (228).

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
`?example=hello_home` link opens that example intentionally. Choosing another
example clears that incoming source selector, so reload restores subsequent
edits. The legacy `#examples` fragment opens the advanced collection.

Compare retains its public route and existing performance/memory/fit anchors.
Docs keeps the old `packages`/`features` anchors and gives individual integration
cards stable `cli`, `applications`, `embedding`, `code-modules`, `native-modules`
and `publishing` anchors. Native build instructions and package versions remain
separate from the playground engine pin.

## Examples and compatibility

The gallery still comes from the real `tests/*.code` fixtures plus
`site/examples/*.code`. Existing native-link, stress and intentionally invalid
fixture exclusions remain. `demos.json` names the four featured programs, with titles, explanations and
expected output. Home/Learn/Memory render previews from those same sources; the build writes the full
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
| 215–216 | Verified evidence in `benchmarks/website-evidence/`; focused Performance view on the existing `compare.html` route |
| 217 | Playground first-use examples and gallery presentation |
| 218 | Learn hub, shared nested lesson layout and the ready Quick start; authoring guide in `learn/README.md` |
| 219–222 | Deliver the four individual lessons; publish through `lessons.json` only after each is checked |
| 223 | Real-application proof on Home and project details |
| 224 | Further responsive/accessibility polish across the shared shell and lessons |
| 225 | Share image and metadata on the public pages |
| 227 | Integrated version, content and deployed-site verification |

Add routes only when they contain usable content. Keep navigation/footer shared,
relative links valid, executable previews checked and old public destinations
working when pages move again.


## Performance evidence (216)

`performance.py` renders the medians directly from the retained, reviewed public
v2.23.0 JSON in `benchmarks/website-evidence/results/2026-11-04-v2.23.0/`. It does
not benchmark during a site build or follow a floating/latest release. Report,
raw-data and reproduction links pin the reviewed evidence commit `65403bd`.
`test_build.py` independently checks displayed medians and ranges against the
raw CSV, with milliseconds and MiB kept separate. Updating a dataset requires
reviewing the copy, representations, claims and scope too.

The current view (229) restores five visible charts: native runtime, source-run
runtime, compiler time, application RSS and snapshot RSS. Broader native
workloads are visible, with exact ranges and all 35 measurements expandable
on the same page. All eager-clone and equivalent-COW representations remain. The
existing Compare route and every previous fragment ID remain useful. The long
seven-language essay and mixed-host October ranking are replaced; original
October artifacts remain unchanged and linked through the historical inventory.

Check native HTML details, all numbers, source links, old anchors, phone widths
and keyboard interaction in a real browser (no WASM load on this page):

```sh
PLAYWRIGHT_MODULE=/tmp/code-site-tools/node_modules/playwright/index.mjs \
  node site/check_performance.mjs http://localhost:8955/code/
```


## Featured playground experience (217)

A fresh visit opens `hello_home`, matching Home and Learn, rather than whichever
fixture sorts first. Four buttons load short complete programs: console
greeting, a reusable local handler, an independent value update and a returned
failure. The current example's explanation and original output sit beside the
editor; actual Result is separate. Drafts/shared source have their own context
and do not inherit a misleading expected-output panel. Reset restores the
selected example, or the greeting for a custom draft.

The full browser-compatible collection stays embedded/validated, but its cards
are created only when the advanced details opens. Category and name/description
search combine; pagination renders at most 12 cards. Named example URLs still
load every program directly. Home renders no gallery.

Source selection, reset and editing work while the engine is loading. Run,
keyboard execution and auto-run are guarded until ready; the latest source
runs when initialization finishes. An engine JS/WASM download failure keeps
execution disabled and displays connection/reload guidance. Tab indentation
saves through the same input path as typing; Escape leaves the editor. Scrolling
respects reduced motion. The page supplies only console, and links native
installation/multi-file reuse to the Modules guide.

The pinned-engine gate checks all gallery programs and every featured output.
For real-browser verification, including all pages of the collection, pending
WASM and blocked JS/WASM downloads:

```sh
PLAYWRIGHT_MODULE=/tmp/code-site-tools/node_modules/playwright/index.mjs \
  node site/check_playground.mjs http://localhost:8955/code/
```


## Learn path and lesson shell (218)

`lessons.json` is the single ordered publication plan: Quick start, first
program, values/control flow, handlers/messages/reuse, small program. Only
ready records receive links or generated pages. The hub stays useful through
its greeting and Playground/Docs links while later lessons are pending.
Quick start is the browser warm-up; lesson 1 adds the checked local setup.

`lesson.html` wraps short content fragments with a goal, current position,
expandable path, previous/next and back-to-Learn navigation. `__ROOT__` in the
shared shell and lesson fragments resolves site links/assets under nested
`learn/` routes without a base tag; local `#main` stays local. Root pages retain
their existing relative URLs. A reused output drops withdrawn lesson pages.
Static tests simulate publishing/withdrawing the next lesson and verify ready
neighbors, rather than shipping an empty lesson as a template demonstration.

See [lesson authoring](learn/README.md) for the shared contract with 219–222.
`lesson.mjs` adds optional copy feedback and selects the source for manual copy
when clipboard permission fails. Without JS, source and navigation remain
usable and the inactive copy button stays hidden. Lessons load no runtime;
Open and run goes to the existing Playground. The generated
`lesson-examples.json` adds original/edited warm-up programs to the same
published-engine check as gallery/featured programs.

```sh
PLAYWRIGHT_MODULE=/tmp/code-site-tools/node_modules/playwright/index.mjs \
  node site/check_learn.mjs http://localhost:8955/code/
```

This checks Home → Learn → nested Quick start → Playground, edited output,
copy/clipboard refusal, reference/back links, readiness, keyboard/skip targets,
four widths and the readable no-JS lesson. Use `check_navigation.mjs` for the
complete site flows, including all twelve published routes.


## First program lesson (219)

`learn/first-program.html` is lesson 1, linked from Home and Learn. It uses the
existing console greeting and an Ada edit, checked both in the pinned 2.16.0
browser interpreter and the public v2.23.0 Linux x86_64 CLI. Local setup is
optional/expandable; source-building is an advanced link to that release's
README, with native platform limitations stated. Lesson 1 now links directly to the ready values lesson; later lessons stay
gated until their content is ready.

The three displayed command blocks come from `lesson-examples/first-program/`.
The build injects the canonical greeting into `project.sh`, emits the exact
snippets and combines them into downloadable `start.sh`. It also emits the
canonical `hello.code`. Assets follow lesson readiness and are removed when
withdrawn; raw placeholder templates are not copied to the public artifact.
Console and the CLI are pinned to v2.23.0; an absolute `code_cli` variable
selects the language while leaving the VS Code editor command alone.

Check the real download/install/run/build flow in a new temporary directory:

```sh
python3 site/check_first_program.py /absolute/path/to/generated/site
```

This needs Linux x86_64, `sh`, `curl`, `tar`, `cc` and network access. It runs
the actual assembled commands, downloads the published CLI, checks the console
lock/version/hash and exact source, verifies the structured check report, and
runs both original and edited programs interpreted, cached and prebuilt. It
uses isolated cache/project directories, including a path containing spaces.
No source-build success is inferred from this prebuilt-CLI check.

```sh
PLAYWRIGHT_MODULE=/tmp/code-site-tools/node_modules/playwright/index.mjs \
  node site/check_first_program.mjs http://localhost:8955/code/
```

This checks the browser edit, Home/hub/direct lesson links, copied/downloaded
command equality, optional setup, neighbors, upcoming-topic overview, four
widths and no-JS reading. Run it separately from other clipboard-writing
browser checks. `check_learn.mjs` and `check_navigation.mjs` retain shared
Learn/site coverage. The additional lesson variants join the actual-engine
gate (`lesson-examples.json`); no native setup is offered as a browser module.


## Values and control flow lesson (220)

`learn/values.html` teaches one shopping-list example progressively: a named
record, an independent updated copy, a list loop, a condition and a short
exercise combining a copied list/nested record update with filtering. The
solution is expandable and its target output is generated from the same
expected-output record used in engine checks. Full type rules stay in an
optional reference link. Lesson 1 links forward; lesson 2 now links forward to the ready messages lesson.

The five programs live in `examples/learn_values_*.code` and join the validated
advanced gallery. A lesson's optional `steps` records name those canonical
examples, titles, outputs and optional single-match edits. `lessons.py` renders
the complete copy/run/output panels from that data and includes all originals
and edits in `lesson-examples.json`. Source is never duplicated in lesson HTML.
The same `steps` contract can be reused for later lessons. The four featured
Playground choices and its saved-draft behavior are unchanged.

```sh
PLAYWRIGHT_MODULE=/tmp/code-site-tools/node_modules/playwright/index.mjs \
  node site/check_values.mjs http://localhost:8955/code/
```

This runs all five programs and both suggested edits through their real
Playground links, verifies the exercise solution and unchanged original, checks
copy, ready neighbors, four widths, keyboard interaction and no-JS reading.
Run clipboard-writing checks separately. The actual pinned-engine gate now
checks 177 cases: 146 gallery programs, four featured previews and 27 lesson
program/output and error-boundary variants. No extra browser module or engine version is added.


## Handlers, messages and reuse lesson (221)

`learn/messages.html` uses three small console-only programs for a named
handler/returned particle, an explicitly returned Exception, and a loop exercise
that handles an empty name and continues. Two suggested edits also have checked
outputs. The actual published engine is unchanged; no browser source-file host
or VFS is added. Local source reuse is an expandable two-file CLI extension
with downloadable exact `greeter.code`/`main.code` and run commands. Copy actions
for local files say to save them in the named file, rather than send them to the
single-file playground. Bindings stay private to their source file.

A ready lesson's optional `checks` names canonical probes under `lesson-checks/`
with exact output and, for intentional failures, an explicit `error_prefix`.
These join `lesson-examples.json` and the actual-engine gate. Ordinary gallery
and lesson programs still must succeed; only declared checks can expect an
error. Four probes cover ignored returned failure, a handler's runtime failure,
a stopped top-level assertion and pre-run undefined-name refusal. The lesson
distinguishes these instead of claiming every error can be handled as data.

```sh
python3 site/check_messages.py /absolute/path/to/public-v2.23.0/code /absolute/path/to/generated/site
PLAYWRIGHT_MODULE=/tmp/code-site-tools/node_modules/playwright/index.mjs \
  node site/check_messages.mjs http://localhost:8955/code/
```

The native checker requires curl/cc/network, the actual Linux public CLI and a
new temporary project/cache. It installs/verifies console, runs the exact
shown local files/commands interpreted and compiled, changes the module's
greeting without changing its caller's same-named binding, checks a handled
empty request, refuses private-name access, and verifies all four error probes
in the appropriate interpreted/compiled paths. No source-build CLI is assumed.

The browser checker exercises actual run links, edits, handled failure and
continued exercise, local downloads/copy feedback, neighbors/overview, four
widths and no-JS content. Run clipboard-writing checks separately. The pinned
engine gate checks 177 cases: 146 gallery programs, four featured previews and
27 lesson programs/edits/error-boundary checks. Lesson 4 remains gated.


## Cart receipt lesson (222)

`learn/small-program.html` completes the four-lesson course. Four console-only
programs progress from one line to a cart receipt with checked negative counts
and prices. Three edits cover an updated count, invalid price and zero count.
Whole-number prices are sample arithmetic. No new input service is required.

The complete download is generated from `learn_cart_final.code`; optional
`downloads` entries in a ready lesson map asset paths to canonical example names.
Withdrawing the lesson removes its route and downloads. The two-file local
version separates `cart.code` from its caller and reuses lesson 1's CLI setup.

`check_cart.py PUBLIC_CODE BUILT_SITE` verifies every browser source/edit and
the exact local files/commands using public v2.23.0, interpreted and compiled,
with a fresh project/cache and verified console provenance/hash.
`check_cart.mjs BASE_URL` uses PLAYWRIGHT_MODULE like the other lesson checks;
it covers actual browser runs, clipboard/downloads, finish links, keyboard,
no-JS reading and four widths. All four lessons are ready; application proof
and the remaining website polish tasks are separate. The pinned engine gate
checks 177 cases: 146 gallery programs, four previews and 27 lesson variants.

## Real applications on Home (223)

`index.html#built-with-code` shows two bounded static previews, each with one
sentence about Code's role, a live destination and native HTML build details.
The shared introduction credits Euglena assembly/hosting. Aquarium uses canvas
and timer; Tic-Tac-Toe uses dom and storage. Those are linked modules, not core
language UI or persistence. Home loads no application, engine, audio or video.
The images have fixed dimensions, descriptive alt text and lazy loading; their
combined size is under 200 KB. Details and links work without JavaScript.

Live destinations checked on 2026-11-05:

- `https://aqua.codeloves.me` returns the Aquarium sign-in page; the card says
  sign-in required. Its scene preview is explicitly a standalone-build capture.
- `https://apps.codeloves.me/tictac` opens the game anonymously. The capture
  checker plays an X win, verifies persisted score on reload, then resets.
- `https://github.com/codelovesme/euglena` is the public framework source.

The application source repository is private. Source and manifests were reviewed
internally; no app-source link or open-source claim is shown to visitors, and
no private source is copied into the site. Source revisions inspected:
Aquarium `bf43e77bec7339930dbaa1915300f974ea95a933`, Tic-Tac-Toe
`fd2e87a3900925a4dd4e7886429224d6d6d24b12`. These identify source review,
not a reproducible binary attestation. The existing standalone Aquarium artifact
used for the capture had SHA-256
`94314a4fe56c79e6d47b2bdb951574faa71063afaaecf9e8a5bc24b2533ddd8d`;
its existing host.mjs had
`de678eb589802adb039eb8279920cde108c4379dd6ecdde17ce9f565812bce1a`.
No application was rebuilt, modified or deployed for this work.

To refresh previews, serve an existing Aquarium web build on loopback, then:

```sh
PLAYWRIGHT_MODULE=/absolute/path/to/playwright/index.mjs \
  node site/capture_apps.mjs http://127.0.0.1:8961/
PLAYWRIGHT_MODULE=/absolute/path/to/playwright/index.mjs \
  node site/check_apps.mjs http://127.0.0.1:8955/code/
```

The capture script writes both PNGs using fresh anonymous browser contexts.
Aquarium checks six fish, add controls, advancing frames and pointer startle;
its audio is muted and controls fade normally before capture. Tic-Tac-Toe's
shell header is hidden only during the element screenshot to keep the board's
score unobscured; the game itself is unchanged. All state is throwaway browser
state; no accounts, messages or server records are created. The screenshots
contain no personal information. The website checker verifies exact image bytes,
size/dimensions, links, responsive layout at four widths, keyboard/no-JS build
details and absence of application/engine requests. It saves local review
screenshots under `/var/tmp/code-223-showcase-{390,1440}.png`.

## Mobile and accessibility review (224)

The shared theme now uses readable secondary text for status, pane labels,
comments and installation notes. Inline prose/caption links are underlined.
Lesson run-link names contain their visible wording; expanded course outlines
have a level-two heading before their lesson headings. Memory's ABI diagram has
an image role and a name describing both halves. Long headings and course cards
wrap when text is enlarged; code previews stay inside their cards.

On phones the editor/search controls use 16 px type, the editor is shorter and
static code is larger. Touch controls/nav links provide 44 px height, and touch
anchor offsets account for wrapped sticky navigation. The first example's
expected result is visible before editing; View result jumps directly to actual
output. Shift+Tab and Escape leave the editor; ordinary Tab still indents.
Output is a named focusable region. Result copying has live feedback and a
selection/manual-copy fallback, without putting feedback inside program output.

Progressive markup contains the canonical greeting and expected result before
scripts load. It is read-only with disabled execution/search controls and a
visible explanation; boot enables editing and hides the explanation. The same
fallback works when script execution is blocked (not just native noscript).
Pending/failed WASM keeps source readable and Run/auto-run disabled. Forced
colors show native textarea text over a system background and hide the syntax
layer. Reduced motion retains static content and stops decorative animation.

The audit uses external development tools only; no dependency ships on the
site. Install axe-core 4.14.0 in a temporary directory and use existing Playwright:

```sh
npm install --prefix /var/tmp/code-site-a11y --no-audit --no-fund axe-core@4.14.0
AXE_PATH=/var/tmp/code-site-a11y/node_modules/axe-core/axe.min.js \
  PLAYWRIGHT_MODULE=/absolute/path/to/playwright/index.mjs \
  node site/check_accessibility.mjs http://127.0.0.1:8955/code/
```

The checker audits all 13 pages at four widths, opens details, runs 26 axe scans
on phone/desktop, visits all 146 phone gallery cards, tests 200% text, keyboard,
copy fallback, no-JS, forced colors and touch. It saves the full automated
review list to `/var/tmp/code-224-accessibility.json` and Home/Learn/lesson/
Playground review screenshots under `/var/tmp/code-224-{page}-{width}.png`.
Axe's uncertain contrast items on gradients/composited backgrounds are retained
for visual review; a zero-violation scan is not a conformance certification or
proof of every assistive-technology/browser combination. Checks here use real
Chromium keyboard/DOM access and visual review, not a spoken screen-reader test.
`check_playground.mjs` additionally gates engine download to check slow loading,
then aborts WASM and JS engine requests separately to check failure states.

## Share previews (225)

Every generated page has a unique absolute canonical/og:url (Home uses `/code/`),
its own existing title/description, and OG plus large-image social-card tags.
The shared `share-code.png` uses the current logo, palette and approved slogan:
Built for reuse. Compiled for performance. It adds interpreter/native compiler/
WebAssembly without a language-wide benchmark claim. The 1200×630 PNG has
explicit type/dimensions and descriptive OG/social alt text; all URLs are public
HTTPS URLs under the actual Pages prefix. This does not change engine pins.

`share-card/index.html` is a native HTML/CSS authoring source, not a public site
route; `render_share.mjs` renders the committed PNG and a 600×315 review capture
at `/var/tmp/code-225-share-feed.png`. No external fonts or image service are
used. Run with the same PLAYWRIGHT_MODULE setup as the other browser tools.
`check_sharing.mjs BASE_URL` verifies all 13 crawler-visible pages without
JavaScript, canonical/title/description/social agreement and the exact PNG's
public response, bytes, dimensions and type. The static suite also guards this
contract in Pages CI. Metadata is present in the original HTML, not injected by
browser scripts. Home's modularity/native positioning and Learn/Performance's
specific titles remain intact. A locally rendered card/valid public metadata
cannot guarantee when a third-party crawler refreshes its own cache.

Metadata follows https://ogp.me/. LinkedIn's optional inspection entry point is
https://www.linkedin.com/post-inspector/; inability to access its UI does not
constitute a verified LinkedIn-rendered preview. Website work never posts a
message or publishes to a social account.


## Chart-led Comparison (229)

Historical inspection found four main chart categories in the pre-216 page:
prebuilt runtime, source-run runtime, compile time and process memory. Those
are again visible charts on `compare.html`, using the verified November
v2.23.0 data. A fifth chart retains the newer snapshot-sharing comparison.
No historical Java/Go measurements are mixed into this host's rankings.

`performance.py` builds 25 ascending rows across five zero-based linear
charts, with exact units and values. Cold compilation/native run, cached
native, forced bytecode, Node/V8 JIT and CPython are explicitly distinguished.
Application RSS excludes cold/compile measurements; those stay in the full
results. Each chart has same-page exact medians/ranges. All 35 report entries
with time/RSS medians, full ranges, sample counts and process contexts are
also on the page. Broader native workloads stay visible, including Code's
slower cases, and equivalent-COW baselines stay beside eager clones.

Comparison supplies its own measurement/machine/version context; external
links are raw evidence or optional language guides, not a replacement for
comparison content. Execution-choice explanations live on Home and the
execution guide; the four-path number-card presentation was removed here.

The static CSV check now validates minima/maxima as well as medians for all
210 unique evidence keys. `check_performance.mjs` verifies all displayed
numbers, all 25 bar widths/rankings, five visible charts, all 35 result rows,
legacy anchors, no runtime requests, keyboard/no-JS and four viewport widths.
It allows half a displayed rounding unit when comparing Python's half-even
formatting with numeric JS values; raw CSV checks retain exact Python format
comparison. Review captures: `/var/tmp/code-229-comparison-{390,1440}.png`.
