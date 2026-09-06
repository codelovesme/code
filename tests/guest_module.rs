//! `guest` — one application running inside another, in a page.
//!
//! The machine's side of hosting is `tests/hosted_app.rs`: a host `link`s a
//! `.so` while it runs, furnishes what it chooses to, and unlinks. This is
//! the same design where there is no dlopen and no thread — a shell fetches
//! another application's `.wasm`, gives it a container to draw in, and stands
//! behind the modules it reaches for.
//!
//! What is held here, end to end and under Node:
//!
//!   - a guest draws **in its container**, and its stylesheet is moved under
//!     it, so two applications on one page cannot restyle each other;
//!   - the host is asked **`Offer` once per module** and **`Module` for every
//!     particle** to one it took — the same two questions, in the same words,
//!     a machine host answers;
//!   - a host that answers **neither** leaves the guest with the page's own
//!     half in a world of its own, which is what saying nothing means on a
//!     machine too;
//!   - `Denied` reaches the guest as an ordinary `Exception` **from the
//!     module it asked**, not as a silence;
//!   - **one instance per name** and one application to a container, and both
//!     come free again when it goes;
//!   - **stopping means stopped**: a delay a guest set before it was let go
//!     fires into nothing, and the container, the stylesheet and the mark on
//!     it are all gone;
//!   - a **told** particle whose handler fails is not silence: the answer
//!     nobody is holding reaches the host as an `Exception` particle;
//!   - a guest **let go and started again** gets a clean world, which is the
//!     one the machine's host got wrong twice (see `euglena`'s notes on the
//!     dirty byte).
//!
//! Skipped where the wasm toolchain or Node is missing — this test is the
//! only place both halves of a browser module meet, so there is nothing left
//! to check without them.

#![cfg(feature = "llvm")]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The shell. It never draws anything itself; the page drives it, the way a
/// reader clicking on an application in a launcher would.
const SHELL: &str = r##"
| A shell: it runs other applications inside itself, and decides what they
| may reach.

link "guest.a" as guest
link "dom.a" as dom
link "storage.a" as store
link "router.a" as router
link "console.a" as out
link "timer.a" as clock

| Asked once, the first time a guest reaches for a module. `notes` is not
| mentioned at all, so it keeps its own of everything.
Offer { app, name } => {
    if app = "mail" {
        if name = "storage" {
            return Denied { }
        }
        if name = "router" {
            return Offered { }
        }
    }
}

| Everything sent to a module this host took.
Module { app, name, particle } => {
    return RouteResult { value = "/from-the-host" }
}

Loaded { app } => {
    emit Print { value = "loaded $app" } to out
}

Exception { source, message } => {
    emit Print { value = "heard from $source: $message" } to out
}

Open { app, url, into } => {
    emit Load { app = app, url = url, into = into } to guest get r
    return r
}

Close { app } => {
    emit Unload { app = app } to guest get r
    return r
}

Say { app, text } => {
    emit Tell { app = app, particle = Show { text = text } } to guest get r
    return r
}

Trip { app } => {
    emit Tell { app = app, particle = Break { } } to guest get r
    return r
}
"##;

/// The application. Nothing in it knows whether it is the page or is running
/// inside another application — it is built once and run both ways.
const MAIL: &str = r##"
| An ordinary application.

link "dom.a" as dom
link "storage.a" as store
link "router.a" as router
link "console.a" as out
link "timer.a" as clock

| Drawn later, so that letting the guest go in between proves that a stopped
| guest is really stopped.
Later { text } => {
    emit Render { into = "body", tree = { tag = "p", children = [text] } } to dom get r
    return Drawn { ok = r.ok }
}

Show { text } => {
    emit Delay { ms = 30, then = Later { text = text } } to clock get d
    return Shown { }
}

| A handler that trips over nothing in particular. Told, not asked — so the
| answer nobody is holding has to reach the host some other way.
Break { } => {
    emit Whatever { } to out get nobody
    return Nothing { value = nobody.value }
}

emit Set { key = "token", value = "abc" } to store get s
if s ∈ Exception {
    emit Print { value = "storage: $s.message" } to out
}

emit Route { } to router get where
emit Print { value = "route: $where.value" } to out

emit Render {
    into = "body",
    styles = { "p" = { color = "red" } },
    tree = { tag = "p", attrs = { class = "hi" }, children = ["hello"] }
} to dom get r
emit Print { value = "drew: $r.ok" } to out
"##;

/// The page. A document small enough to read, with only what `dom` and
/// `guest` actually touch — the point of `createHost` taking one rather than
/// reaching for `globalThis.document`.
const PROBE: &str = r##"
import { readFileSync } from "node:fs";
import { createHost } from "./shell/host.mjs";

const node = (tag) => ({
  tag, attrs: {}, children: [], listeners: {}, text: "",
  setAttribute(k, v) { this.attrs[k] = String(v); },
  removeAttribute(k) { delete this.attrs[k]; },
  hasAttribute(k) { return k in this.attrs; },
  getAttribute(k) { return this.attrs[k] ?? null; },
  get id() { return this.attrs.id ?? ""; },
  set id(v) { this.attrs.id = String(v); },
  addEventListener(name, fn) { (this.listeners[name] ||= []).push(fn); },
  appendChild(child) { this.children.push(child); child.parent = this; return child; },
  replaceChildren(...kids) { for (const k of kids) k.parent = this; this.children = kids; },
  remove() {
    if (!this.parent) return;
    this.parent.children = this.parent.children.filter((c) => c !== this);
    this.parent = null;
  },
  get textContent() { return this.text; },
  set textContent(t) { this.text = String(t); },
  every() { return this.children.flatMap((c) => (c.every ? [c, ...c.every()] : [c])); },
  matches(sel) {
    if (sel.startsWith("#")) return this.attrs.id === sel.slice(1);
    if (sel.startsWith("[")) return sel.slice(1, sel.indexOf("]")).split("=")[0] in this.attrs;
    return this.tag === sel;
  },
  querySelectorAll(sel) { return this.every().filter((c) => c.matches && c.matches(sel)); },
  querySelector(sel) { return this.querySelectorAll(sel)[0] ?? null; },
});

const body = node("body");
const head = node("head");
const panel = node("div");
panel.setAttribute("id", "panel");
const side = node("div");
side.setAttribute("id", "side");
const spare = node("div");
spare.setAttribute("id", "spare");
body.appendChild(panel);
body.appendChild(side);
body.appendChild(spare);
const doc = {
  head, body,
  createElement: (tag) => node(tag),
  createTextNode: (text) => ({ text, every: () => [] }),
  getElementById: (id) => body.querySelector("#" + id),
  querySelector: (sel) => (sel === "body" ? body : body.querySelector(sel)),
  querySelectorAll: (sel) => body.querySelectorAll(sel),
};

// A page with one file to serve.
globalThis.fetch = async (url) =>
  url === "mail.wasm"
    ? { ok: true, status: 200, arrayBuffer: async () => readFileSync("./mail/mail.wasm") }
    : { ok: false, status: 404 };

const lines = [];
const host = createHost({ doc, log: (s) => lines.push(s) });
const { instance } = await WebAssembly.instantiate(readFileSync("./shell/shell.wasm"), { env: host.env });
if (host.start(instance) !== 0) throw new Error("the shell did not start");

const settle = (ms = 40) => new Promise((r) => setTimeout(r, ms));
const drawn = (where) => where.children.map((c) => (c.children ?? []).map((t) => t.text).join("")).join("|");
const check = (what, got, want) => {
  if (JSON.stringify(got) !== JSON.stringify(want)) {
    throw new Error(`${what}: got ${JSON.stringify(got)}, wanted ${JSON.stringify(want)}`);
  }
};

check("a guest was not accepted",
  host.ask({ _class: "Open", app: "mail", url: "mail.wasm", into: "#panel" }),
  { _class: "LoadResult", ok: true, reason: null });
await settle();

check("the guest did not draw where it was put", drawn(panel), "hello");
check("the guest's sheet was not moved under its container",
  head.children.map((c) => c.text),
  ['[data-code-guest="mail"] p {\n  color: red;\n}']);
check("what the guest reached did not go the way the host said", lines, [
  "mail: storage: 'storage' is not offered to 'mail' here",
  "mail: route: /from-the-host",
  "mail: drew: true",
  "loaded mail",
]);

check("a second copy of the same guest was not refused",
  host.ask({ _class: "Open", app: "mail", url: "mail.wasm", into: "#panel" }),
  { _class: "LoadResult", ok: false, reason: "'mail' is already running" });

check("a second application in one container was not refused",
  host.ask({ _class: "Open", app: "other", url: "mail.wasm", into: "#panel" }),
  { _class: "LoadResult", ok: false, reason: "'#panel' is already running 'mail'" });

// The same application under a name the host says nothing about: it keeps
// its own of everything, in a container of its own.
lines.length = 0;
check("a guest the host has no opinion about was not accepted",
  host.ask({ _class: "Open", app: "notes", url: "mail.wasm", into: "#side" }),
  { _class: "LoadResult", ok: true, reason: null });
await settle();
check("a guest the host said nothing about did not get its own", lines, [
  "notes: route: /",
  "notes: drew: true",
  "loaded notes",
]);
check("two guests did not draw in two containers", [drawn(panel), drawn(side)], ["hello", "hello"]);
check("a second guest's sheet did not come with it", head.children.length, 2);

lines.length = 0;
check("a guest that cannot be fetched was not accepted first",
  host.ask({ _class: "Open", app: "nope", url: "gone.wasm", into: "#spare" }),
  { _class: "LoadResult", ok: true, reason: null });
await settle();
check("the host was not told the guest never arrived", lines,
  ["heard from guest: cannot run 'nope': 'gone.wasm' answered 404"]);

check("the host could not say anything to its guest",
  host.ask({ _class: "Say", app: "mail", text: "second" }),
  { _class: "TellResult", ok: true });
await settle();
check("what the host said did not reach the guest's handlers", drawn(panel), "second");

lines.length = 0;
check("the host could not tell its guest to trip",
  host.ask({ _class: "Trip", app: "mail" }),
  { _class: "TellResult", ok: true });
await settle();
check("a guest's handler failed and nobody heard", lines,
  ["heard from core: cannot read field 'value' of a null — '.' requires an object"]);

// Told, and let go before the delay it set has elapsed.
host.ask({ _class: "Say", app: "mail", text: "third" });
await settle(5);
check("the guest was not let go",
  host.ask({ _class: "Close", app: "mail" }),
  { _class: "UnloadResult", ok: true });
await settle();
check("a stopped guest still drew", drawn(panel), "");
check("a stopped guest left its sheet behind", head.children.length, 1);
check("a stopped guest left its mark on the container", panel.attrs, { id: "panel" });
check("something could still be said to a guest that is gone",
  host.ask({ _class: "Say", app: "mail", text: "fourth" }),
  { _class: "TellResult", ok: false });

lines.length = 0;
check("the name did not come free again",
  host.ask({ _class: "Open", app: "mail", url: "mail.wasm", into: "#panel" }),
  { _class: "LoadResult", ok: true, reason: null });
await settle();
check("the second life did not start clean", drawn(panel), "hello");
check("the second life did not say what the first did", lines, [
  "mail: storage: 'storage' is not offered to 'mail' here",
  "mail: route: /from-the-host",
  "mail: drew: true",
  "loaded mail",
]);
"##;

fn repo(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(path)
}

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("code-guest-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create test directory");
    dir
}

fn tool_exists(name: &str) -> bool {
    Command::new(name).arg("--version").output().is_ok()
}

/// Whether this toolchain can build for the browser at all. Asked of the
/// sysroot rather than of `rustup`, which is not always the thing that
/// installed the target.
fn wasm_target_installed() -> bool {
    let sysroot = match Command::new("rustc").args(["--print", "sysroot"]).output() {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).trim().to_owned(),
        _ => return false,
    };
    Path::new(&sysroot)
        .join("lib/rustlib/wasm32-unknown-unknown")
        .is_dir()
}

/// Builds one first-party module's browser half into `dir` as `<name>.a`.
///
/// A `cdylib` is a whole module and fails to link on the very imports an
/// archive is supposed to leave open, so the crate type is asked for on the
/// command line — the same line the release workflow runs.
fn archive(dir: &Path, module: &str) {
    let crate_dir = repo("crates/modules").join(module);
    let built = Command::new("cargo")
        .args([
            "rustc",
            "--target",
            "wasm32-unknown-unknown",
            "--release",
            "--crate-type",
            "staticlib",
        ])
        .current_dir(&crate_dir)
        .output()
        .unwrap_or_else(|e| panic!("failed to run cargo for {module}: {e}"));
    assert!(
        built.status.success(),
        "building {module} for wasm32 failed: {}",
        String::from_utf8_lossy(&built.stderr)
    );
    let from = crate_dir
        .join("target/wasm32-unknown-unknown/release")
        .join(format!("lib{module}.a"));
    fs::copy(&from, dir.join(format!("{module}.a")))
        .unwrap_or_else(|e| panic!("copy {}: {e}", from.display()));
}

#[test]
fn a_shell_runs_another_application_inside_itself() {
    if !tool_exists("node") {
        eprintln!("skipped: needs node to run a page");
        return;
    }
    if !wasm_target_installed() {
        eprintln!("skipped: needs the wasm32-unknown-unknown target");
        return;
    }

    let dir = temp_dir("shell");
    // Everything the shell offers, plus the one that does the offering. A
    // guest reaches only the halves its host's build carried, so this list is
    // the menu — `mail` links exactly the same five and gets them all.
    for module in ["guest", "dom", "storage", "router", "console", "timer"] {
        archive(&dir, module);
    }

    let shell_src = dir.join("shell.code");
    let mail_src = dir.join("mail.code");
    fs::write(&shell_src, SHELL).expect("write the shell");
    fs::write(&mail_src, MAIL).expect("write the application");

    // Each in its own directory: `host.mjs` is written beside the module it
    // belongs to, and two builds sharing a directory would leave one page's
    // half answering the other page's module.
    fs::create_dir_all(dir.join("shell")).expect("create shell/");
    fs::create_dir_all(dir.join("mail")).expect("create mail/");
    code::compile_file(
        &shell_src,
        code::BuildTarget::Wasm,
        &dir.join("shell/shell.wasm"),
        false,
    )
    .expect("build the shell for wasm");
    code::compile_file(
        &mail_src,
        code::BuildTarget::Wasm,
        &dir.join("mail/mail.wasm"),
        false,
    )
    .expect("build the application for wasm");

    let probe = dir.join("page.mjs");
    fs::write(&probe, PROBE).expect("write the page");
    let output = Command::new("node")
        .arg("page.mjs")
        .current_dir(&dir)
        .output()
        .expect("run the page under node");
    assert!(
        output.status.success(),
        "the page did not hold: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let _ = fs::remove_dir_all(&dir);
}
