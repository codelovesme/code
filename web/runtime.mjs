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

/// Every linked module's half, pasted in at build time.
///
/// Each is a function of one context returning `[name, answer]`: the module's
/// own name, and what it does with a particle. **A module speaks particles in
/// both directions** — toward the language and toward the page — so a half
/// here takes the particle its module was sent and returns the one it
/// answers. Nothing crosses as a pointer, a length, or a shape invented for
/// one module.
const PARTS = [
  //__CODE_WEB_PARTS__
];

/// Builds the import object a `code` wasm module needs, and the one call that
/// starts it.
///
/// `doc` is the document to draw into — passed in rather than reached for, so
/// a test can hand over a small stand-in and check what the module did
/// without a browser. `log` is where `console`'s lines go.
///
/// `store` and `address` are the same idea for what is remembered and for
/// where the reader is. Null means the browser's own, which the half that
/// needs one knows how to reach — this file does not.
///
/// `guard` stands between a module's half and the particle it was sent. Null
/// for a program that *is* the page: nothing comes between it and its own
/// modules. A program running inside another is given one, which is how its
/// host decides what it may reach.
///
/// All four together are what makes a second world on one page possible: the
/// `guest` module calls this again, with a document that stops at a
/// container, keys under a prefix, a slice of the address, and a guard that
/// asks the host program. The halves it builds cannot tell the difference.
export function createHost({
  doc = globalThis.document,
  log = (s) => console.log(s),
  store = null,
  address = null,
  guard = null,
} = {}) {
  const dec = new TextDecoder();
  const enc = new TextEncoder();
  let memory;
  let fire = () => {};
  let ask = () => null;

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

  // The world every module's half is given. `fire` and `ask` are passed as
  // wrappers because they cannot be wired until the instance exists, and the
  // parts are built before it does.
  const ctx = {
    doc,
    log,
    store,
    address,
    fire: (particle) => fire(particle),
    ask: (particle) => ask(particle),
  };
  const answering = new Map();
  for (const part of PARTS) {
    const [name, answer] = part(ctx);
    answering.set(name, answer);
  }

  const answer_here = (name, particle) => {
    const answer = answering.get(name);
    return answer ? answer(particle) : undefined;
  };
  // A guarded program's modules answer to its host first, which can carry the
  // particle out, refuse it, or answer in the module's place.
  const answer_for = guard ? (name, particle) => guard(name, particle, answer_here) : answer_here;

  // The one door from any browser module to its half here: a particle in as
  // JSON, a particle out as JSON, under the module's name.
  //
  // **Nothing thrown escapes.** A half that throws would take the whole
  // program's dispatch down with it, from inside a handler, over something
  // as ordinary as a browser refusing storage. So every answer is caught here
  // and becomes an `Exception` particle — which is what the language reads a
  // failure as anyway, and what the same module's machine half returns.
  env.code_web_ask = (namePtr, nameLen, jsonPtr, jsonLen, outPtr, cap) => {
    const name = str(namePtr, Number(nameLen));
    let result;
    try {
      result = answer_for(name, JSON.parse(str(jsonPtr, Number(jsonLen))));
    } catch (e) {
      result = { _class: "Exception", source: name, message: String(e?.message ?? e) };
    }
    // Nothing answered: no half for this module, or a half that had nothing
    // to say to this class. The module reads a negative length as that, and
    // hands the program null.
    if (result === null || result === undefined) return -1n;
    return BigInt(writeInto(JSON.stringify(result), outPtr, Number(cap) - 1));
  };

  return {
    env,

    /// Tells the program something happened. Nothing comes back.
    fire: (particle) => fire(particle),

    /// Asks the program something and returns the particle it answered, or
    /// null when nothing did. Only meaningful after `start`.
    ask: (particle) => ask(particle),

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

      // The other kind. `fire` tells — a click has happened whether or not
      // the program has an opinion. This asks, and cannot go on without the
      // answer: it is what lets a page put the program in the middle of
      // something. Null when nothing answered.
      ask = (particle) => {
        const n = writeInto(JSON.stringify(particle), at, cap);
        const back = Number(e.code_event_ask(BigInt(n)));
        return back === 0 ? null : JSON.parse(str(at, back));
      };

      return e.main();
    },

    /// Lets the instance go. Nothing told or asked of it afterwards reaches
    /// it, and this host stops holding its memory — which is what lets the
    /// memory of a stopped program actually come back.
    ///
    /// There is nothing to stop for a program that is the page: it ends when
    /// the page does. It matters for one running inside another, where a
    /// timer set before it was unloaded would otherwise fire into an
    /// application that is no longer there.
    stop() {
      fire = () => {};
      ask = () => null;
      memory = undefined;
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
