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
const DEBUG_MODULE_CALLS = false; //__CODE_WEB_DEBUG__

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
/// A guard may additionally implement canUseLocal(name) returning true only
/// when the module may answer locally without calling the guard. Without that
/// explicit permission, pure wasm handlers use ordinary guarded dispatch too.
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
  flowParent = null,
  program = null,
} = {}) {
  const dec = new TextDecoder();
  const enc = new TextEncoder();
  let memory;
  let exports = null;
  let eventAt = 0;
  let fire = () => {};
  let ask = () => null;

  const str = (ptr, len) => dec.decode(new Uint8Array(memory.buffer, ptr, len));

  // Only instrumented wasm calls flow_on. Quiet programs keep their original
  // event path; no clocks, steps or root copies are used for them.
  let flowsOn = false;
  let enableFlowEvents = () => {};
  const handled = new Set();
  const graph = flowParent?.graph ?? { active: null, delivering: false };
  const owner = () => flowParent?.owner() ?? (flowsOn ? deliverTrace : null);
  const flows = { graph, owner };
  const mono = () => performance.now();
  const round = (n) => Math.round(n * 1000) / 1000;
  const beginFlow = (target, name, particle, from) => {
    if (graph.delivering) return -1;
    let flow = graph.active;
    if (!flow) {
      const deliver = owner();
      if (!from || !deliver) return -1;
      flow = graph.active = {
        root: { from, class: name, particle, program },
        started: Date.now(), clock: mono(), steps: [], open: [], dropped: 0, deliver,
      };
    }
    if (flow.steps.length >= 500) { flow.dropped++; return -2; }
    const id = flow.steps.length;
    flow.steps.push({ id, parent: flow.open.at(-1) ?? null, target, class: name,
      at: mono() - flow.clock, ms: 0, ok: true, program });
    flow.open.push(id);
    return id;
  };
  const finishFlow = (id, ok) => {
    const flow = graph.active;
    if (id < 0 || !flow) return;
    const now = mono() - flow.clock;
    while (flow.open.length) {
      const top = flow.open.pop();
      const step = flow.steps[top];
      step.ms = round(now - step.at);
      step.at = round(step.at);
      step.ok = top === id ? ok : false;
      if (top === id) break;
    }
    if (id !== 0) return;
    graph.active = null;
    const trace = { _class: "Trace", root: flow.root, started: flow.started,
      ms: flow.steps[0].ms, ok: flow.steps[0].ok, steps: flow.steps, dropped: flow.dropped };
    graph.delivering = true;
    try { flow.deliver(trace); } finally { graph.delivering = false; }
  };
  // Called directly rather than through ask's event wrapper: Trace cannot
  // trace itself, even when its handler calls a guest or a furnished module.
  const deliverTrace = (trace) => rawAsk(trace);
  let rawAsk = () => null;

  /// Writes `text` into a buffer the module owns and says how much landed.
  /// Cut rather than refused: the caller was told the capacity, and a
  /// truncated value beats a program that cannot start.
  const writeInto = (text, ptr, cap) => {
    const b = enc.encode(text);
    const n = Math.min(b.length, cap);
    new Uint8Array(memory.buffer).set(b.subarray(0, n), ptr);
    return n;
  };

  /// Writes an event into the runtime's buffer, asking for the room first,
  /// and says where it landed and how much: the buffer grows to fit and may
  /// move when it does. A runtime built before it could grow is written the
  /// old way, into what it has — a guest runs on its host's copy of this
  /// file, and the two need not be the same age.
  const writeEvent = (e, text) => {
    const b = enc.encode(text);
    const at = e.code_event_text_reserve
      ? e.code_event_text_reserve(BigInt(b.length))
      : e.code_event_text();
    const cap = Number(e.code_event_text_capacity());
    const n = Math.min(b.length, cap);
    new Uint8Array(memory.buffer).set(b.subarray(0, n), at);
    eventAt = at;
    return { at, n, cap };
  };

  const env = {
    // The language itself, on a host with no operating system: a clock, an
    // error sink, and turning a double into text and back — which a
    // freestanding build cannot compute and asks for.
    code_host_error(ptr, len) {
      throw new Error("wasm error: " + str(ptr, len));
    },
    code_host_now: () => Math.floor(Date.now() / 1000),
    code_host_flow_on: () => { flowsOn = true; enableFlowEvents(); },
    code_host_flow_handles: (ptr, len) => handled.add(str(ptr, len)),
    code_host_flow_begin(target, targetLen, name, nameLen, from, fromLen, json, jsonLen) {
      return BigInt(beginFlow(str(target, targetLen), str(name, nameLen),
        jsonLen ? JSON.parse(str(json, jsonLen)) : null, fromLen ? str(from, fromLen) : null));
    },
    code_host_flow_finish: (id, ok) => finishFlow(Number(id), !!ok),
    // Minutes to add to UTC to get this page's clock. `getTimezoneOffset`
    // counts the other way round — minutes to add to local to get UTC — so
    // the sign is flipped here, once, rather than in every caller.
    code_host_tz_offset: () => -new Date().getTimezoneOffset(),
    code_host_number_exact(value, ptr, cap) {
      const b = enc.encode(value.toExponential(40));
      if (b.length >= cap) return -1;
      new Uint8Array(memory.buffer).set(b, ptr);
      return b.length;
    },
    code_host_number_parse: (ptr, len) => Number(str(ptr, len)),
    // The significant digits of the shortest spelling that reads back as
    // `value`: `toExponential()`'s, without its sign, point and exponent.
    code_host_number_shortest(value, ptr, cap) {
      const b = enc.encode(value.toExponential().replace(/^-|e.*$|\./g, ""));
      if (b.length >= cap) return 0;
      new Uint8Array(memory.buffer).set(b, ptr);
      return b.length;
    },
  };

  // The world every module's half is given. `fire` and `ask` are passed as
  // wrappers because they cannot be wired until the instance exists, and the
  // parts are built before it does.
  const ctx = {
    doc,
    log,
    store,
    address,
    flows,
    fire: (particle) => fire(particle),
    ask: (particle) => ask(particle),
  };
  let currentInstance = () => 0;
  const answering = new Map();
  for (const part of PARTS) {
    const [name, answer] = part(ctx);
    answering.set(name, { part, instances: new Map([[0, answer]]) });
  }

  const answer_here = (name, particle, instance) => {
    const entry = answering.get(name);
    if (!entry) return undefined;
    if (!entry.instances.has(instance)) {
      entry.instances.set(instance, entry.part(ctx)[1]);
    }
    return entry.instances.get(instance)(particle);
  };
  // A guarded program's modules answer to its host first, which can carry the
  // particle out, refuse it, or answer in the module's place.
  const answer_for = (name, particle, instance) => {
    const local = (name, particle) => answer_here(name, particle, instance);
    return guard ? guard(name, particle, local) : local(name, particle);
  };

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
    // Optional preflight for pure handlers implemented by the wasm module.
    // A guard must explicitly allow it; custom guards keep seeing full calls.
    // No particle is serialized unless the host actually needs to receive it.
    if (cap === -1n) {
      try {
        return answering.has(name) && (!guard || guard.canUseLocal?.(name) === true) ? 0x1_4c4f4341n : 0n;
      } catch { return 0n; }
    }
    let result, particle;
    try {
      particle = JSON.parse(str(jsonPtr, Number(jsonLen)));
      result = answer_for(name, particle, currentInstance());
    } catch (e) {
      result = { _class: "Exception", source: name, message: String(e?.message ?? e) };
    }
    // Nothing answered: no half for this module, or a half that had nothing
    // to say to this class. The module reads a negative length as that, and
    // hands the program null.
    if (result === null || result === undefined) {
      const flow = graph.active;
      if (flow && !graph.delivering && flow.open.length) {
        flow.steps[flow.open.at(-1)].answered = false;
      }
      if (DEBUG_MODULE_CALLS) console.warn(`[code] unanswered module call: ${name} / ${typeof particle?._class === "string" ? particle._class : "<no class>"}`);
      return -1n;
    }
    const text = JSON.stringify(result);
    // A module built with a buffer of its own says how big it is. One that
    // left the buffer to the runtime passes none, and the room is reserved
    // here for exactly what the answer needs — so an answer is never cut.
    if (Number(cap) > 0) return BigInt(writeInto(text, outPtr, Number(cap) - 1));
    const b = enc.encode(text);
    const at = exports.code_web_answer_reserve(BigInt(b.length + 1));
    const room = Number(exports.code_web_answer_capacity());
    if (b.length + 1 > room) return -1n;
    new Uint8Array(memory.buffer).set(b, at);
    return BigInt(b.length);
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
      exports = e;
      memory = e.memory;
      currentInstance = e.code_web_instance ?? (() => 0);

      // The particle goes into a buffer the runtime owns, as JSON — one
      // that grows to fit, so a recording arrives whole.
      fire = (particle) => {
        const { n } = writeEvent(e, JSON.stringify(particle));
        e.code_event_fire(BigInt(n));
      };

      // The other kind. `fire` tells — a click has happened whether or not
      // the program has an opinion. This asks, and cannot go on without the
      // answer: it is what lets a page put the program in the middle of
      // something. Null when nothing answered.
      rawAsk = (particle) => {
        const { n, cap } = writeEvent(e, JSON.stringify(particle));
        const back = Number(e.code_event_ask(BigInt(n)));
        // Nested events update eventAt themselves. Only an answer larger
        // than the incoming capacity can move it without a JS writeEvent.
        // Avoid an extra wasm call on every quiet, ordinary-sized answer.
        if (back > cap) eventAt = e.code_event_text();
        return back === 0 ? null : JSON.parse(str(eventAt, back));
      };

      // An incoming page event is a root. A guest asking its host while
      // already running is a child under the guest's currently open step.
      const tracedAsk = (particle, from = program ? "host" : "page") => {
        if (graph.delivering || (!flowsOn && !flowParent?.owner() && !graph.active)) return rawAsk(particle);
        if (flowsOn && !handled.has(particle?._class)) return rawAsk(particle);
        const id = beginFlow("this", particle?._class ?? "", particle, from);
        try {
          const answer = rawAsk(particle);
          finishFlow(id, answer?._class !== "Exception");
          return answer;
        } catch (e) { finishFlow(id, false); throw e; }
      };
      enableFlowEvents = () => {
        ask = tracedAsk;
        fire = (particle) => { tracedAsk(particle, "page"); };
      };
      // A quiet standalone program uses the original raw paths. main's
      // flow_on import installs the traced paths before any statements run.
      ask = rawAsk;
      if (flowParent?.owner()) enableFlowEvents();
      const started = e.main();
      return started;
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
      ask = rawAsk = () => null;
      flowsOn = false;
      enableFlowEvents = () => {};
      handled.clear();
      memory = undefined;
      currentInstance = () => 0;
      answering.clear();
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
