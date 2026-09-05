// The page's half — the language's own, and a slot for each browser module's.
//
// A module for the browser is two pieces of code. One is compiled into the
// `.wasm`: it takes a particle, works out what was asked, and calls a function
// it deliberately left undefined. The other is that function, written in the
// only language that can reach a page. Neither is a module on its own, so a
// module keeps both halves together — its `page.mjs` sits beside its Rust.
//
// This file is the part that belongs to nobody in particular: the four
// functions the language itself needs from a host with no operating system,
// and the wiring that lets a page fire a particle back. `code build --target
// wasm` writes it out with the halves of the modules the application actually
// linked pasted into `PARTS` below — so an application carries what it linked
// and not one line more, and both halves come out of the same build.
//
// Two rules run through all of it, this file and every module's half:
//
//   - **Nothing here interprets.** No `innerHTML`, no `eval`, no property
//     reached by name, no handler built from text. A tree built out of
//     someone's name is data all the way to the page and cannot become code
//     on the way.
//   - **Nothing trusts an address or a length that came from outside.** When
//     something has to cross, the module hands over a buffer of its own and
//     how much room it has. Containment of an honest mistake, not a boundary:
//     this file and the module share one memory.
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

/// Every linked module's half, pasted in at build time. Each is a function of
/// one context and returns the imports it supplies.
const PARTS = [
  //__CODE_WEB_PARTS__
];

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

  /// Writes `text` into a buffer the module owns and says how much landed.
  /// Cut rather than refused: the caller was told the capacity, and a
  /// truncated value beats a program that cannot start.
  const writeInto = (text, ptr, cap) => {
    const b = enc.encode(text);
    const n = Math.min(b.length, cap);
    new Uint8Array(memory.buffer).set(b.subarray(0, n), ptr);
    return n;
  };

  const env = {
    // The language itself, on a host with no operating system: a clock, an
    // error sink, and turning a double into text and back — which a
    // freestanding build cannot compute and asks for.
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
  };

  // What every module's half is given. `fire` is passed as a wrapper because
  // it cannot be wired until the instance exists, and the parts are built
  // before it does.
  const ctx = { doc, log, str, writeInto, fire: (particle) => fire(particle) };
  for (const part of PARTS) {
    Object.assign(env, part(ctx));
  }

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
