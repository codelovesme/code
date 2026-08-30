# Community modules

Distribution story for modules: what ships inside the `code` binary, what
we publish ourselves (per host — native artifacts and browser npm
packages), and how third parties publish theirs. Direction decided with
the owner 2026-08-23; the phases below are the implementation order.

## Tiers — decided

Three tiers, distinguished by *where the bytes live*:

| tier | what | how users get it |
|---|---|---|
| **core** | `Length` (shipped), `Timestamp` | compiled into the `code` binary — nothing to install, works everywhere including the wasm playground |
| **first-party modules** | `terminal`, `console`, `math`, `strings`, `env`, `http_client`, `http_server`, … | native: GitHub Releases + `code install <name>`; browser: npm |
| **community modules** | anyone's | the author publishes to *their own* GitHub Releases (a template repo provides the CI); consumers install by URL first, by name once an index exists |

The rule separating tier 1 from tier 2: **fundamentals are core**. Only
what the language itself needs to stay meaningful (`Length`, `Timestamp`)
is compiled in. Everything else is a module, and modules are things you
install. Printing leads batch 1 as two host-specific modules — `terminal`
(native) and `console` (browser) — deliberately *not* baked into the
binary, because keeping the binary free of modules keeps "what ships
inside `code`" a closed list.

### Hosts: one module per host — decided

Each host's linker speaks exactly one dialect: native `link` loads a
`.so`/`.a` from disk, while code-wasm's resolver maps a `link` alias onto
a JS object handed to `run_with_modules` (see `PreloadedModules` in
`crates/code-wasm/src/lib.rs`). There is no artifact that serves both,
so modules are per-host: same idea, different name, different artifact.
This keeps every capability story exact — a module either has the
particle or it doesn't, no "supported except in the browser" footnotes.

Naming: the browser-side twin takes the familiar web name (`console`),
the native side the plain one (`terminal`) — decided 2026-08-23. Where
two hosts implement the same capability they share the particle's name
and result shape (`Print`, `Warn`, `Error`, `Debug`); host-only particles
make no apology (`Read` in `terminal`, `Group`/`GroupEnd` in `console`).

Packaging follows the host: native modules ship as GitHub Release assets
plus `code install`; browser modules ship as npm packages exporting a
`run_with_modules`-shaped object, version-pinned by the site/playground
exactly like `code-wasm` already is.

### Core addition (phase A)

- `Timestamp` — `emit { _class = "Timestamp" } to core` →
  `{ _class = "TimestampResult", value = <unix seconds> }`. Revived from
  the old language, which had exactly this.

Lands in `src/interpreter.rs::dispatch_core` **and**
`src/runtime.c::code_core_dispatch` with identical operand rules and error
wording (the standing two-backends parity invariant), plus fixtures run
under both `code run` and `code build`.

## Distribution (phases B–D)

### One version for everything — decided 2026-08-29

Reverses the per-module cadence this document originally described (one
`modules/<name>/v<semver>` tag per module, each shipping on its own
schedule). That was convenient for whoever cut the release and useless for
whoever consumed it: a user holding `code v0.7.0` and `terminal 1.0.0` had no
way to tell whether the two were built against the same ABI without reading
the repository.

So a single `v<semver>` tag now publishes the CLI, every first-party module,
`code-native` on crates.io and the `code-wasm` npm package, all at that
number, onto one release page. `tests/one_version.rs` holds every manifest in
the repo to it, and the publish workflow refuses a tag that disagrees with
`Cargo.toml`.

The cost, taken knowingly: an unchanged module still gets a new version on
every release, so a version bump no longer means "this module changed". The
thing it buys is that a version *match* means something, which is the
question users actually ask. `CODE_ABI_VERSION` is untouched and still says
whether a module can be loaded at all — it moves only when the ABI breaks.

The first unified version is **1.1.0**, not 1.0.0: `code-native`, the npm
package and four modules were already published at 1.0.0, and a shared line
has to start above everything already on it.

### Hosting: GitHub Releases

No registry server. Provenance is free (source and CI are visible per
artifact), and it is where the community lives anyway. Per release:

- one asset per platform per module: `math-linux-x86_64.so`, … (the extension
  follows the platform; the loader already keys off suffix)
- one `module.json` per module: name, version, ABI version, handler list,
  exported vars, supported platforms, sha256 per asset — all attached to the
  same release as the CLI tarball for that version
- everything built from source in CI (cross-compilation matrix), never
  uploaded by hand

Matrix today: linux-x86_64 only. Deferred, each one job block away once
demand shows up: linux-arm64 (cross gcc), macos-{x86_64,arm64}
(`-dynamiclib` + ad-hoc codesign — unsigned dylibs get killed/quarantined),
and **Windows**, which additionally needs a `LoadLibrary`/`GetProcAddress`
path in `src/native.rs` and `runtime.c` — real work, tracked as phase F.
Build on an older-glibc runner image for maximum compatibility.

### Install: `code install` — shipped 2026-08-23

Subcommands: `code install <name-or-url> [--global]`, `code remove <name>`
(alias `rm`), `code ls` (installed + available).

- Resolution: a first-party name resolves to **this binary's own release**;
  community modules install by full URL until a name-based community index
  exists (phase F). The manifest URL is derived from the release tag page
  (`…/releases/tag/TAG` → `…/releases/download/TAG/<name>.json`); a by-URL
  reference is the manifest URL itself. The release is overridable via
  `CODE_MODULE_RELEASE`, which is how the flow is exercised before a tag
  exists.

  **The index is gone (2026-08-29)**, one day after "one version for
  everything" removed its reason to exist. It mapped a name to that module's
  *latest* version and release page — and once every module ships at the
  CLI's version, on the CLI's release, a name plus `env!("CARGO_PKG_VERSION")`
  is already the whole address. What it cost while it lived: it was
  hand-maintained, so `env` and `http_server` shipped without entries and
  `code install env` answered "unknown module", and it still pointed at
  `modules/<name>/v1.0.0` tags that the unified release no longer produces.
  Its replacement is `module_install::FIRST_PARTY`, a compiled-in list that
  `tests/first_party_modules.rs` holds to `crates/modules/` *and* to the
  publish workflow's matrix — the drift that broke the index is now a failing
  test. A wrong name is answered locally rather than by a 404, and `code ls`
  needs no network.
- Storage: project-local `.code/modules/<name>/<version>/` plus
  `.code/lock.json` recording name, version, source URL, and sha256;
  `--global` installs to `~/.code/modules/` but records the same project
  lockfile (with a `global` flag), so there is exactly one lockfile per
  project.
- Verification: sha256 checked at download time **and** re-checked at load
  time while a lock entry exists (modules are small, so this is cheap — a
  tampered or replaced `.so` fails loudly instead of loading). Enforcement
  is scoped to bytes sitting under an install root, so vendored copies
  elsewhere stay unpinned.
- Trust model for v1: you can inspect exactly what loads — the lockfile
  pins a hash, and the hash points at a public artifact whose source and CI
  are visible. Artifact signing (minisign) is a later phase, not day one.

### Loader: fallback chain — shipped 2026-08-23

`FilesystemResolver` used to resolve `link "x.so"` against the linking
script's directory only — deliberately no search path, so where a module
comes from is always answerable by looking at the two files involved. The
agreed shape is now implemented:

1. the script's directory (unchanged — explicit wins)
2. the nearest ancestor's `.code/modules/` (walk up, like `node_modules`)
3. `$CODE_MODULE_PATH` (colon-separated, for unusual setups)
4. `~/.code/modules/` (globally installed)

plus a fifth lookup that makes installed layouts reachable: the `link`
reference maps through the project lockfile onto
`<root>/<name>/<version>/<asset>` (the layout `code install` writes). Both
spellings resolve there — the pinned asset outright
(`terminal-linux-x86_64.so`) or, more cleanly, the module with a native
extension (`terminal.so` / `terminal.a`), since the lockfile already says
which asset that is on this platform. Without the lockfile entry the layout
lookup has nothing to go on — the lockfile is the single source of truth
for "what is installed here", and flat vendored layouts keep resolving
through steps 1–4 unchanged. Documented wherever `link` is documented. The
provenance guarantee became: answerable by looking at the script, its
lockfile, and four fixed places.

## First-party modules (phases B, D)

Native modules live in this repo under `crates/modules/<name>/`, each a
tiny `cdylib` on `code-native`, sharing one CI workflow that builds the
platform matrix and publishes to GitHub Releases. Browser modules are npm
packages exporting a `run_with_modules`-shaped object (see "Hosts"),
published alongside. Every module ships `tests/*.code` fixtures run under
both output modes in CI — dogfooding the parity invariant and giving
consumers confidence.

Batch 1 (proves the pipeline):

- **terminal** — the native host's console: `Print` (stdout), `Warn` /
  `Error` (stderr, ANSI color when attached to a TTY), `Debug` (silent
  unless `CODE_DEBUG=1`), `Read` (interactive stdin prompt). Being a plain
  `.so`, `Print` just writes stdout in C — no purity seam needed in the
  interpreter.
- **console** — the browser host's console, an npm package: `Print` →
  `console.log` (DevTools pretty-prints our JSON-shaped values as
  expandable trees), `Warn` / `Error` / `Debug` → their `console.*`
  counterparts, `Group` / `GroupEnd` → `console.group`. No `Read`: there
  is no stdin in a browser, and the module doesn't pretend otherwise.
  Overlapping particles share names and result shapes with `terminal`'s.
- **math** — shipped 2026-08-24: `Double`, `Sum`. Written in Rust on
  `code-native`, inheriting the numeric half of `test_math` under the split
  proposal (`Shout`/`Echo` went to **strings**; `test_math` stays a pure
  test double with all four, so the split cost it nothing). Design
  decisions live in the module's header comment (plain `f64` end to end —
  no rounding/formatting policy invented; `Sum` over an empty array is 0;
  non-number operands refused rather than coerced).
- **strings** — shipped 2026-08-23: `Shout`, `Echo`, `Split`, `Join`,
  `Trim`, `Upper`, `Lower`. Written in Rust on `code-native` — the first
  first-party module to take the Rust path. `terminal` followed it on
  2026-08-28 and **every first-party module is Rust now**.

  The original rationale kept `terminal` in C as the canonical reference
  implementation — zero framework between a reader and the ABI — so that an
  ABI drift would fail differently in a C module and a Rust one. What
  overturned it is the guarantee a module now owes: it may never end the
  application (`errors-as-particles.md`), and that is real in Rust
  (`guarded` catches panics on the module's own side of the FFI boundary)
  and unattainable in C, where a forgotten NULL check segfaults and an
  integer `100 / 0` raises SIGFPE. A module users actually run belongs on
  the path where the promise holds. The C reference survives in
  `tests/native_modules/test_math_static*.c`, which still exercise
  `code_abi.h`'s extern declarations. Design decisions
  live in the module's header comment (ASCII-only case conversion, empty
  segments survive `Split`, `Join` refuses non-string elements, multi-char
  separators refused outright).

Batch 2 (flagship — proves the pipeline with a non-trivial module):

- **http_client** — shipped 2026-08-28 as `net`, renamed and completed
  2026-08-29: all seven HTTP methods (`Get`, `Post`, `Put`, `Patch`,
  `Delete`, `Head`, `Options`) taking a url plus optional
  `headers`/`timeout_seconds`/`max_body_bytes`, answering
  `HttpResponse { ok, status, body }`. Blocking, as sketched. Full contract
  and reasoning in [`crates/modules/http_client/README.md`](../../crates/modules/http_client/README.md);
  the shape was settled by reading `euglena-language`'s `native-http-client`
  and `http-client` organelles, which is where the per-verb particles and
  the `ok`/`status` response come from.

  Two deliberate departures from the sketch above:

  - **`ureq` + rustls, not "plain blocking sockets via libc".** The sketch
    predates the question of TLS. Writing HTTP over raw sockets is an
    afternoon; writing *HTTPS* is a certificate-verification stack, and
    getting that subtly wrong is a security bug rather than a bug. A module
    that could only speak `http://` would not be the flagship. Cost: a
    ~17 s cold build and a 3 MB `.so` — both paid once per CI run, and the
    fixtures still need no network.
  - **The name.** It shipped as `net` and was renamed on 2026-08-29, at the
    owner's question: if a client takes the name `http`, an `http_server`
    later reads as the lesser half of a pair whose other half never said it
    was only a client. `http_client`/`http_server` is symmetric, and each
    artifact does one thing — a program that only makes requests does not
    download a listener. Nothing external broke: the module was never
    tagged, so no release, index entry or lockfile referred to it.

  - **No `headers` in the *response*.** Not an omission by choice:
    `code_object` copies key *pointers*, not key strings
    (`runtime.c`'s `key_buf[i] = keys[i]`), so every field name in a value
    must outlive it — which is why `code-native`'s `object()` takes
    `&'static CStr`. Response header names arrive at runtime and would have
    to be leaked to satisfy that. Expressing them wants an ABI addition
    (an owned-keys constructor), which is its own decision and does not
    belong inside the first module that happens to want it.

- **env** — shipped 2026-08-29, and the answer to a question `http_server`
  asked by existing: where does the port come from? `Get { name, default? }`
  and `Require { name }`, with the **default's kind deciding how the variable
  is read** — a Number default parses a number, so `Listen { port =
  p.value }` is one emit rather than a string the language cannot convert.
  A variable set but unreadable as that kind is an `Exception` rather than a
  silent fallback. Full contract in
  [`crates/modules/env/README.md`](../../crates/modules/env/README.md).

  This is deliberately *not* euglena's answer. There, configuration lives in
  the app's `manifest.json` under `organelles.<alias>.sap` and the host
  delivers it — which makes it a security boundary, since the host can
  override an untrusted app's. A module reading the environment is not that:
  it is the program asking, so a program could ask for anything. The stronger
  version needs an app manifest, which is a concept this language does not
  have and should not grow for one field. Revisit if untrusted programs ever
  run here.

- **http_server** — shipped 2026-08-29, and the reason the inbound *answer*
  path exists at all. `Config { port?, … }` (optional) sets what to bind;
  `Listen {}` binds and spawns an accept thread. Every request is pushed to
  the program as `Request { method, path, query, body }`, and **whatever the
  `Request` handler returns is the response**. Serial by design (the program
  is single-threaded, so concurrency would only queue work it cannot start
  sooner), which is also why the pending request is one slot rather than a
  map — no correlation, no id in the program's hands. Full contract in
  [`crates/modules/http_server/README.md`](../../crates/modules/http_server/README.md).

  Modelled on `euglena-language`'s `server` organelle, minus what a cell
  runtime needs and this language does not have: no `Impulse` routing, no JWT,
  no per-app public-class policy. `Sap` became `Config` (the universal
  setup-particle rule), and `Listen {}` is a *separate* action — config vs.
  running are different states, and a euglena manifest can deliver `Config`
  while the app's own gene decides when to `Listen`. `Respond { request_id,
  … }`, which euglena needs because its answer has nowhere else to go, was
  built here and then thrown away when the handler's return value could carry
  it.

`json` — shipped: `Parse` (JSON text → value) and `Stringify` (value → text,
with `pretty`). The compact form matches interpolation exactly; `Parse` and
`pretty` are what interpolation cannot do. Ported from `euglena-language`'s
`json` organelle — the old `Error` particle became an `Exception`, and
`Stringify` now drops only `_class` rather than every `_`-prefixed key
(`_id` survives). Full contract in
[`crates/modules/json/README.md`](../../crates/modules/json/README.md).

### Module config: no `Sap`

`Sap` is euglena's mechanism for delivering an organelle's configuration
from a `manifest.json` at cell startup. Outside euglena it means nothing, so
no `code` module handles it. Instead:

- **Stateless where the config is trivial** — a `cost`, a `length` — is a
  per-call parameter with a default. `crypto`, `json`, `strings`, `math`,
  `env`, `terminal`, `markdown`.
- **A `Config { … }` setup particle where state is genuinely needed** — a
  directory, a secret, a bind address. It returns `ConfigResult { ok }`, and
  every other handler is an `Exception` until it has run (`jwt`, `fs`,
  `json_store`, `git`, `mailer`, `oauth`, `mongodb`, `blob_storage`,
  `cloud_drive`, `localai`). `Config` is **not** an
  action: `http_server` keeps `Config` for the port/host and adds `Listen {}`
  as the separate "start serving" action, since binding a socket has its own
  failure and its own timing.

A module's `module.json` names its setup handler in a `setup` field (absent
for a stateless module), and `code install` copies that into
`.code/lock.json`. That is the seam euglena's codegen will read to turn a
manifest config block into `emit <Setup> { … } to <alias>` — the only place
`Sap` still conceptually lives is euglena's own manifest format, and even
there the emitted particle is the module's own.

`crypto` — shipped: `Hash`/`Verify` (bcrypt) and `RandomCode`. Ported from
`euglena-language`'s `crypto` organelle. Stateless — the organelle's
`Sap { salt_rounds }` is gone; `Hash { cost }` is a per-call parameter,
default 12. Failures are `Exception`s, not the old `Error` particle; a
malformed hash in `Verify` is an `Exception` while a wrong password is just
`valid = false`.

`jwt` — shipped: `Config` (the setup particle: `secret`, `expires_in`), then
`Sign`/`Decode` for HS256 tokens. Ported from `euglena-language`'s `jwt`
organelle (`Sap` → `Config`). HS256 is small enough to do over `hmac`/`sha2`
directly rather than pull `jsonwebtoken`; a bad signature or an expired token
is `valid = false` (Decode's whole purpose), while a missing secret or field
is an `Exception`.

`markdown` — shipped: `RenderMarkdown` (CommonMark + GFM → HTML, with a flat
table of contents, a link list, and heading `id`s) and `SplitByHeading`.
Ported from `euglena-language`'s `markdown` organelle. The heading-id
injection is now real event rewriting rather than a string search on the
output, repeated headings get GitHub-style `-1`/`-2` slugs, and an empty
`src` renders to empty rather than erroring.

`fs` — shipped: `Config` (the setup particle: `base_path`), then
`ReadFile`/`WriteFile`/`DeleteFile`/`CreateDir`/`RemoveDir`/`ListDir`/
`Exists`, all resolved inside the base. Ported from `euglena-language`'s `fs`
organelle (`Sap` → `Config`), with the containment bug fixed: the organelle
let an absolute path or a `../` escape `base_path` entirely; this resolves
*every* path inside the base — a `..` that would climb out is an `Exception`.
`WriteFile` is atomic (temp + rename) as before; failures are `Exception`s,
and delete/remove of an absent target is `existed = false` rather than an
error.

`json_store` — shipped: `Config` (the setup particle: `base_dir`), then
`Store`/`Fetch`/`Delete` over one JSON file per key. Ported from
`euglena-language`'s `json_store` organelle (`Sap` → `Config`). The organelle
double-wrapped the value (a JSON string inside a JSON object) and rewrote
unsafe key characters to `_` — which silently collided `a/b` and `a_b`. This
stores the value directly (readable file), keeps its shape and field order on
`Fetch`, and refuses a key that isn't `[A-Za-z0-9._:@-]`.

`process` — shipped: `Run` (one-shot, blocks, captures `stdout`/`stderr`/
`code`) and `Spawn`/`Status`/`Wait`/`Kill`/`List` (a long-running child
tracked under a caller `id`). Ported from `euglena-language`'s `process`
organelle. The organelle's `Sap` (which only allocated an empty map) is
gone — there is no configuration, just a table that fills as you `Spawn`.
`Run` is new: the organelle only had spawn-and-track, and most callers of a
subprocess want its output, not a handle. A non-zero exit is a `RunResult`,
not an `Exception`; a command that won't start is an `Exception`. `git` is
built on this.

`git` — shipped: `Config`/`Init`/`Clone`/`Add`/`Commit`/`Push`/`SetRemote`/
`Status`, plus `Stash`/`StashPop`, all over the system `git` binary. Ported
from `euglena-language`'s `git` organelle (`Sap` → `Config`), and `Config`
does more than store a path: it refuses a folder that is inside — or is a
checkout of — a *different* repository, and it will not silently proceed on
a dirty working tree (`on_dirty` is `"error"`, `"stash"` or `"ignore"`, and
`ConfigResult` reports `dirty` / `stashed` / `branch` / `head` so an app can
decide). A `git` command that fails is an `Exception` with its stderr, and a
`user:pass@` in any URL is masked out of every message and result.

`mailer` — shipped: `Config` (the SMTP transport: host, port, auth, `from`,
`tls`) then `Send { recipient, subject?, text?, html?, cc?, bcc? }`. Ported
from `euglena-language`'s `mailer` organelle, which spoke Azure
Communication Services' signed REST API directly — SMTP over `lettre`
reaches the same providers (Azure included) with no vendor code. A message
the server rejects is an `Exception` carrying its reply; a bad address or a
missing transport is an `Exception` too. Tested against a real SMTP server
in `tests/mailer_module.rs` (a `.code` fixture only reaches the error
paths).

`oauth` — shipped: `Config` (one provider's endpoints + client credentials),
`AuthUrl { state, extra? }` (the redirect URL, pure), `ExchangeCode { code }`
→ `Identity { sub, email, name, picture, access_token, refresh_token }`
(token exchange, then userinfo if `userinfo_url` is set). Ported from
`euglena-language`'s `oauth` organelle. The organelle hard-coded Google's
`access_type=offline&prompt=select_account`; here they're `extra` params the
caller passes. `access_token`/`refresh_token` are surfaced now (the organelle
dropped them). Percent-encoding is a small RFC-3986 loop, no dep. A provider
error is an `Exception` carrying `error_description`; `AuthUrl` and the error
paths are a `.code` fixture, the exchange round trip is `tests/oauth_module.rs`.

`mongodb` — shipped: `Config { url, database }`, then two layers over one
connection — documents (`Insert`, `InsertMany`, `Find`, `Count`, `Drop`) and
a key/value trio (`Store`/`Fetch`/`Delete` by string key into a `state`
collection). Ported from `euglena-language`'s `mongodb` organelle. `Find`
returns a real array of objects rather than a JSON string (the old ABI
couldn't), `ObjectId`/`DateTime` come back as strings, and a failed
operation is an `Exception` rather than `{ ok: false }`. `Drop` is new — it
makes a test's collection reproducible. Error paths are a `.code` fixture;
the CRUD round trip is `tests/mongodb_module.rs`, run against the CI job's
`mongo:7` service and skipped without a `MONGO_URI`.

`blob_storage` — shipped: `Config { bucket, access_key, secret_key,
endpoint?, region?, path_style?, create? }`, then `Put`/`Get`/`Delete`/`List`
(`Upload`/`Download` aliases). Ported from `euglena-language`'s
`blob-storage` organelle, which spoke Azure Blob's SharedKey REST API
directly; this speaks S3 (over `rust-s3`), the interface every object store
— AWS, MinIO, R2, B2, Spaces, and Azure via its S3 gateway — now exposes.
`base64` flags on `Put`/`Get` move bytes the language can't hold; `Get` on a
missing key is `GetResult { found = false }`, not an `Exception`; `List`
returns a real array. Error paths are a `.code` fixture; the CRUD round trip
is `tests/blob_storage_module.rs`, run against the CI job's `bitnami/minio`
service and skipped without `S3_ENDPOINT`.

`cloud_drive` — shipped: `Config { client_id, client_secret, redirect_uri?,
scope?, auth_url?, token_url?, api_base? }` (the three URL fields default to
Google's), then the OAuth pair (`AuthUrl`/`BuildAuthUrl`, `ExchangeCode`,
`RefreshToken`) and five file operations (`GetQuota`, `ListFiles`,
`UploadFile`, `DownloadFile`, `DeleteFile`). Ported from `euglena-language`'s
`cloud-drive` organelle, which carried OneDrive and Yandex stubs that only
ever returned `ProviderUnavailable`; this module is Google Drive and a
`provider` other than `"google"` is an `Exception`. `AuthUrl` is pure;
`base64` flags on `UploadFile`/`DownloadFile` move bytes; `DeleteFile` on a
404 is `{ existed = false }`, not an `Exception`. Error/pure paths are a
`.code` fixture; the OAuth-and-files round trip is
`tests/cloud_drive_module.rs`, against a fake Drive stood up on loopback (no
service, no env var).

`localai` — shipped: `Config { endpoint, model?, max_tokens?, temperature?,
timeout_seconds? }`, then `Chat` / `ChatJson` (OpenAI-compatible
`/v1/chat/completions`) and `Transcribe` / `TranscribeWithOptions` (Whisper
`/v1/audio/transcriptions`). Ported from `euglena-language`'s `localai`
organelle. `<think>…</think>` blocks are stripped from every reply; `Chat`
keeps a code fence, `ChatJson` strips it and validates the payload as JSON
(and Exceptions if it isn't). New: a `messages` array on `Chat` for
multi-turn, where the organelle only took `system` + `user`. A failed call
is an `Exception`, not `{ ok: false }`. Error/guard paths are a `.code`
fixture; the chat/transcribe round trips are `tests/localai_module.rs`,
against a fake OpenAI endpoint on loopback (no service, no env var).

### Mock twins

`mailer_mock`, `oauth_mock`, `mongodb_mock`, `blob_storage_mock`,
`cloud_drive_mock`, `git_mock`, `localai_mock` — one per module that talks to
a service. Each presents the real module's exact particle and result surface
but keeps state in memory (or synthesises it): no SMTP server, no OAuth
provider, no MongoDB, no object store, no Google, no `git` subprocess, no
model server. Ported from `euglena-language`'s `*-mock` organelles, updated
to the `Config`/`Exception` conventions and the new result classes.

They are first-party and published like any other module, so `code install
mongodb_mock` works and a euglena manifest can overlay `<name>_mock.so` by
filename. Round-trip fidelity where it's cheap — `blob_storage_mock` and
`mongodb_mock` really store and query; `oauth_mock`/`cloud_drive_mock`
recover an identity from a base64-JSON code; `mailer_mock` exposes an
`Outbox` handler so a test can assert what would have been sent. Covered by
`.code` fixtures only — a mock never touches a network, so there is nothing
an integration test would add.

Later candidates, only when asked for: date/time formatting (raw seconds are
core `Timestamp`; human-readable formats belong in a module).

## Community path (phase E)

> **Shipped 2026-08-28 as [`templates/module/`](../../templates/module)**, in
> the repo rather than as a separate `code-module-template` repo. A directory
> is copied with `cp -r`; a template repo is forked, which ties someone's
> module history to ours and needs a second repo kept in step with an ABI
> that lives here. Nothing is lost — GitHub's "use this template" button is
> the only thing a separate repo would have added.
>
> It is a *working* module, not a skeleton: a `Greet` handler, a fixture that
> runs in both output modes, and the publish workflow.
> `tests/module_template.rs` builds it and runs that fixture on every CI run,
> with `code-native` swapped to a path dependency so the check needs no
> published version — which also makes it sharper, since it catches a
> template that has drifted from the ABI *as it stands now*.

- A `code-module-template` repo: CI workflow (matrix + releases), fixture
  harness skeleton, README skeleton, and a prominent license notice.
- License reality, stated upfront: every native module embeds `runtime.c`
  (that is how the ABI's value-lifetime contract works), so modules are
  GPL-3.0 derivatives. Fine for most contributors; the template says so in
  bold.
- Publish flow: fork the template → implement → tag → CI publishes to your
  releases → share the URL. Nothing central to maintain until the index.

## Website (phase E) — still open

A third card was added to the Packages section on 2026-08-28, pointing at the
template and stating the GPL-3.0 consequence, so the publish path is
discoverable from the site. What is still missing is the listing below, which
now wants a data source of its own: the first-party set is a list compiled
into the CLI (`module_install::FIRST_PARTY`) rather than a JSON document the
site could render, and there is no community index at all.

A **Modules** page beside the Packages section: renders each module's
`module.json` (handlers, versions, platforms, install command), sourced from
the same data `code ls` reads. It doubles as the de-facto index.

## Phases

| # | deliverable | unblocks |
|---|---|---|
| A | core `Timestamp` (both backends, fixtures) | the core-handler pattern proven before any module ships |
| B | `crates/modules/{terminal,math,strings}` + cross-build CI → GitHub Releases, `console` npm package; dogfood by hand | proof the pipeline works |
| C | ~~`code install/remove/ls` + resolver fallback chain + lockfile/sha256~~ — shipped 2026-08-23 | users getting modules without copying files |
| D | ~~`net` module~~ — shipped 2026-08-28, renamed `http_client` 2026-08-29 | the flagship community-facing module |
| E | ~~template + publish guide~~ — shipped 2026-08-28 as `templates/module/`, in-tree rather than a separate `code-module-template` repo; website Modules page still open | other people publishing |
| F | (optional) Windows `.dll` support, name-based community index, artifact signing | wider reach / stronger trust |

## Still open

- `Print` operand policy (both `terminal` and `console`): strings only, or coerce numbers/booleans too?
- ~~Walk-up semantics for `.code/modules/`~~ — decided 2026-08-23 with the
  fallback chain: stop at the nearest `.code/` directory (shared helper
  `find_project_code_dir` in `src/loader.rs`); git-root detection deferred
  until it earns its complexity.
- ~~`net` API shape: one particle per verb (`Get`/`Post`) vs one `Request`
  particle carrying a method field~~ — decided 2026-08-28: **one particle
  per verb**, following `euglena-language`'s organelles. Dispatch in this
  language is already a `_class` switch, so a method field would make the
  module run a second switch on a string, re-implementing the dispatcher one
  level down. Sketched in the module's README before coding, as this entry
  asked.
