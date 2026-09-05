// The page's half of every browser module, in one file.
//
// A module for the browser is two pieces of code. One is compiled into the
// `.wasm` — it takes a particle, works out what was asked, and calls a
// function it left undefined. The other is that function, here, in the only
// language that can reach a page. Neither is a module on its own.
//
// So this file is not a framework and not a runtime: it is `console`, `dom`,
// `storage`, `router` and `timer`, page-side, plus the four functions the
// language itself needs from a host that has no operating system. Loading it
// is loading the halves of the modules the application linked; a module the
// application did not link costs nothing but an unused import.
//
// Two rules run through all of it:
//
//   - **Nothing here interprets.** No `innerHTML`, no `eval`, no property
//     reached by name, no handler built from text. A tree built out of
//     someone's name is data all the way to the page and cannot become code
//     on the way.
//   - **Nothing trusts an address or a length that came from outside.** When
//     something has to cross — a stored value, the current path, a particle
//     fired back — the module hands over a buffer of its own and how much
//     room it has. Containment of an honest mistake, not a boundary: this
//     file and the module share one memory.
//
// Usage:
//
//   import { runWasm } from "./host.mjs";
//   await runWasm("app.wasm");
//
// or, when the page wants to hold on to the instance:
//
//   const host = createHost();
//   const { instance } = await WebAssembly.instantiate(bytes, { env: host.env });
//   host.start(instance);

/// Builds the import object a `code` wasm module needs, and the one call that
/// starts it.
///
/// `doc` is the document to draw into — passed in rather than reached for, so
/// a test can hand over a small stand-in and check what the module did
/// without a browser. `log` is where `console`'s lines go.
export function createHost({ doc = globalThis.document, log = (s) => console.log(s) } = {}) {
  const dec = new TextDecoder();
  const enc = new TextEncoder();
  let memory;
  let fire = () => {};

  const str = (ptr, len) => dec.decode(new Uint8Array(memory.buffer, ptr, len));
  const bytes = (ptr, len) => new Uint8Array(memory.buffer, ptr, len);

  /// Writes `text` into a buffer the module owns and says how much landed.
  /// Cut rather than refused: the caller was told the capacity, and a
  /// truncated value beats a program that cannot start.
  const writeInto = (text, ptr, cap) => {
    const b = enc.encode(text);
    const n = Math.min(b.length, cap);
    new Uint8Array(memory.buffer).set(b.subarray(0, n), ptr);
    return n;
  };

  // ---- dom ----------------------------------------------------------------

  /// `"Add"` or `{ _class: "Add", ... }` — both are how an application says
  /// what an event means, and anything else is not one.
  function asParticle(wanted) {
    if (typeof wanted === "string") return wanted ? { _class: wanted } : null;
    if (wanted && typeof wanted === "object" && typeof wanted._class === "string") {
      return wanted;
    }
    return null;
  }

  // What an event carries, as one piece of text: what the reader typed, or
  // what the application wrote on the element. Anything else carries nothing
  // and says so, so a plain button makes a particle with no `value` field.
  const VALUED = new Set(["input", "select", "textarea"]);
  function eventValue(el) {
    const tag = String(el.tagName || "").toLowerCase();
    if (VALUED.has(tag)) return String(el.value ?? "");
    if (el.hasAttribute && el.hasAttribute("value")) return String(el.getAttribute("value"));
    return null;
  }

  function node(spec) {
    if (typeof spec === "string") return doc.createTextNode(spec);
    if (spec === null || typeof spec !== "object" || Array.isArray(spec)) {
      return doc.createTextNode(String(spec));
    }
    const el = doc.createElement(spec.tag || "div");
    for (const [k, v] of Object.entries(spec.attrs || {})) {
      if (/^on/i.test(k)) continue; // never an event handler
      el.setAttribute(k, String(v));
    }
    // `on` maps an event name to the *particle* the application wants back —
    // a whole one, written in the tree, or just its class when there is
    // nothing else to say. Nothing is registered and nothing is held: it
    // travels out in the payload and comes back in when the event happens.
    for (const [event, wanted] of Object.entries(spec.on || {})) {
      const particle = asParticle(wanted);
      if (!particle) continue;
      el.addEventListener(event, (e) => {
        const value = eventValue(e.target || el);
        // What the element holds, added only when the application did not
        // say it itself — an `on` that names `value` means that value.
        fire(value === null || "value" in particle ? particle : { ...particle, value });
      });
    }
    for (const child of spec.children || []) el.appendChild(node(child));
    return el;
  }

  // `styles` arrives as selector -> property -> value, so there is no CSS to
  // parse. Braces and angle brackets are dropped anyway: nothing built here
  // may end a rule early and start a different one.
  const clean = (s) => String(s).replace(/[{}<>]/g, "");
  function sheetText(styles) {
    if (!styles || typeof styles !== "object") return "";
    return Object.entries(styles)
      .map(([sel, props]) => {
        const body = Object.entries(props || {})
          .map(([k, v]) => `  ${clean(k)}: ${clean(v)};`)
          .join("\n");
        return `${clean(sel)} {\n${body}\n}`;
      })
      .join("\n");
  }

  // ---- router -------------------------------------------------------------

  // The hash, because a page served as a file has nothing else it can change
  // without asking a server for a URL that does not exist.
  const currentRoute = () => {
    const hash = String(globalThis.location?.hash || "");
    return hash.replace(/^#/, "") || "/";
  };
  let watching = null;

  // ---- storage ------------------------------------------------------------

  // Reached through a function rather than held: a browser can refuse
  // storage outright (a private window, a reader's setting), and that throws
  // on the *first* touch rather than returning null.
  const store = () => {
    try {
      return globalThis.localStorage ?? null;
    } catch {
      return null;
    }
  };

  // ---- timer --------------------------------------------------------------

  // Kept so `Cancel` has something to name. A fired delay drops out of it —
  // nothing repeats on its own, so nothing here outlives its own firing.
  const pending = new Map();
  let nextTimer = 1;

  const env = {
    // --- the language itself, on a host with no operating system ---
    code_host_error(ptr, len) {
      throw new Error("wasm error: " + str(ptr, len));
    },
    code_host_now: () => Date.now() / 1000,
    code_host_number_exact(value, ptr, cap) {
      const b = enc.encode(value.toExponential(40));
      if (b.length >= cap) return -1;
      new Uint8Array(memory.buffer).set(b, ptr);
      return b.length;
    },
    code_host_number_parse: (ptr, len) => Number(str(ptr, len)),

    // --- console ---
    code_web_log: (ptr, len) => log(str(ptr, len)),

    // --- dom ---
    code_web_render(jsonPtr, jsonLen, intoPtr, intoLen) {
      const target = doc.querySelector(str(intoPtr, intoLen));
      if (!target) return 0;
      const payload = JSON.parse(str(jsonPtr, jsonLen));
      if (payload.styles) {
        // One sheet per page, replaced rather than stacked: an application
        // restyling itself should not leave its old rules behind.
        let sheet = doc.getElementById("code-style");
        if (!sheet) {
          sheet = doc.createElement("style");
          sheet.id = "code-style";
          (doc.head || doc.body).appendChild(sheet);
        }
        sheet.textContent = sheetText(payload.styles);
      }
      target.replaceChildren(node(payload.tree));
      return 1;
    },

    // --- storage ---
    code_web_storage_get(keyPtr, keyLen, outPtr, cap) {
      const held = store();
      if (!held) return -1;
      let value;
      try {
        value = held.getItem(str(keyPtr, keyLen));
      } catch {
        return -1;
      }
      // Null rather than "": a key never set and one holding an empty
      // string are different answers, and the module keeps them apart.
      if (value === null || value === undefined) return -1;
      return writeInto(String(value), outPtr, cap);
    },
    code_web_storage_set(keyPtr, keyLen, valPtr, valLen) {
      const held = store();
      if (!held) return 0;
      try {
        held.setItem(str(keyPtr, keyLen), str(valPtr, valLen));
        return 1;
      } catch {
        // Full, or refused. The module answers `ok = false` and the
        // application decides what that means.
        return 0;
      }
    },
    code_web_storage_remove(keyPtr, keyLen) {
      const held = store();
      if (!held) return 0;
      try {
        held.removeItem(str(keyPtr, keyLen));
        return 1;
      } catch {
        return 0;
      }
    },

    // --- router ---
    code_web_route_get: (outPtr, cap) => writeInto(currentRoute(), outPtr, cap),
    code_web_route_set(pathPtr, pathLen) {
      if (!globalThis.location) return 0;
      const path = str(pathPtr, pathLen);
      // Assigning the hash is what puts an entry in the reader's history, so
      // Back means what they expect. An unchanged path fires nothing, which
      // is also what they expect.
      globalThis.location.hash = path.startsWith("#") ? path.slice(1) : path;
      return 1;
    },
    code_web_route_watch(classPtr, classLen) {
      const className = str(classPtr, classLen);
      if (!className) return 0;
      watching = className;
      if (!env.code_web_route_watch.armed) {
        globalThis.addEventListener?.("hashchange", () => {
          if (watching) fire({ _class: watching, path: currentRoute() });
        });
        env.code_web_route_watch.armed = true;
      }
      return 1;
    },

    // --- timer ---
    code_web_timer_set(ms, classPtr, classLen, valuePtr, valueLen) {
      // Copied out now: these pointers are the module's own memory and the
      // wait is longer than the call.
      const className = str(classPtr, classLen);
      if (!className) return -1;
      const value = valueLen < 0 ? null : str(valuePtr, valueLen);
      const id = nextTimer++;
      const handle = setTimeout(() => {
        pending.delete(id);
        fire(value === null ? { _class: className } : { _class: className, value });
      }, ms);
      pending.set(id, handle);
      return id;
    },
    code_web_timer_clear(id) {
      const handle = pending.get(id);
      if (handle === undefined) return 0;
      clearTimeout(handle);
      pending.delete(id);
      return 1;
    },
  };

  return {
    env,

    /// Wires the instance up and runs it.
    ///
    /// The event path is wired *before* `main`, because the first thing an
    /// application usually does is draw, and a listener fired before this
    /// would reach nothing.
    start(instance) {
      const e = instance.exports;
      memory = e.memory;

      // The particle goes into a buffer the runtime owns, as JSON, and it
      // says how much room there is.
      const at = e.code_event_text();
      const cap = Number(e.code_event_text_capacity());

      fire = (particle) => {
        const n = writeInto(JSON.stringify(particle), at, cap);
        e.code_event_fire(BigInt(n));
      };

      return e.main();
    },
  };
}

/// Fetch, instantiate and start a `code` module — the whole of what a page
/// has to do.
export async function runWasm(url, options) {
  const host = createHost(options);
  const { instance } = await WebAssembly.instantiateStreaming(fetch(url), { env: host.env });
  return host.start(instance);
}
