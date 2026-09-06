// The page's half of `guest`: one application running inside another.
//
// A guest is one `.wasm` and nothing else — its own code, its modules and the
// language runtime are already inside it — so running one is a fetch, an
// instantiate, and a world of its own to run in. It does not know it is a
// guest: the same file runs on its own page, and every module it links
// answers there exactly as it answers here.
//
// **What a guest may reach, the host decides — by answering.** The same two
// questions a host answers on a machine, in the same words: `Offer { app,
// name }` the first time a guest reaches for a module, and `Module { app,
// name, particle }` for every particle to one the host took:
//
//   - `Offered { }` — the host furnishes it. Every particle arrives at the
//     host's own `Module` handler, which answers in the module's place,
//     forwards to its own copy, or anything else it likes.
//   - `Denied { }` — the module refuses. The guest gets an `Exception` on
//     first use, the way it would from a network that is not there.
//   - Nothing at all — the host stays out of it, and the guest reaches the
//     page's own half in a world of its own. A host that writes no handler
//     hosts an application without taking anything from it, which is what
//     writing no `Offer` means on a machine too.
//
// One thing means something different here, and it is the reason this module
// exists at all. A held `.so` that its host says nothing about opens *its
// own* modules — its own file, its own settings — and the host never sees
// them. A page has no dlopen: every half a guest can reach is this page's,
// out of the host's own build. So "its own" here is the same half given a
// world of its own — a document that stops at its container, and its own
// slice of the address.
//
// The decision belongs in the language, where it can be read and tested,
// rather than here.
//
// **What a guest draws stays the guest's.** The nodes are made by the same
// `dom` half, out of the real document, so a click on them fires at the
// guest's handlers and not at the host's. Only the world they are made in is
// narrowed.
//
// None of that is a boundary — a guest shares this page's memory like
// everything else here, and anything on the page can read all of it. It is
// containment of an honest application, which is what a shell of one's own
// applications needs.
//
// Two things are worth knowing before writing a shell:
//
//   - **A guest reaches only the modules its host linked.** The halves it
//     asks are the ones pasted into this page at the *host's* build, so a
//     guest that links `storage` inside a host that never did asks a page
//     with nobody to answer, and is handed null. Link what you mean to offer.
//   - **`Load` answers when the guest is on its way, not when it is running.**
//     Fetching cannot be waited for in a page without freezing the reader. The
//     arrival comes back as a particle at the host's own handlers — `Loaded
//     { app }`, or an `Exception` from `guest` saying what went wrong — the
//     same way `net_client` answers a request it has sent.
(ctx) => {
  // One instance per name, held while it runs: a guest keeps going long after
  // `Load` has answered, and `Unload` has to be able to let go of all of it.
  const running = new Map();

  // A name is a key in two places at once — the container's attribute and
  // the head of its path — so it is held to what is literal in both. Nothing
  // here is quoted or escaped.
  const nameOf = (app) =>
    typeof app === "string" && /^[A-Za-z0-9_-]{1,64}$/.test(app) ? app : null;

  const isParticle = (v) =>
    v !== null && typeof v === "object" && !Array.isArray(v) && typeof v._class === "string";

  const refused = (reason) => ({ _class: "LoadResult", ok: false, reason });

  // `body`, `html` and `:root` all mean the whole page. A guest's whole page
  // is its container; `#app` is the starter template mount point every euglena
  // web application targets in standalone mode.
  const WHOLE_PAGE = new Set(["body", "html", ":root", "#app"]);

  // What the guest's container is known by in a rule. The name is already
  // letters, digits, `-` and `_`, so there is nothing in here to quote.
  const rootSelector = (app) => `[data-code-guest="${app}"]`;

  /// Moves every rule of a sheet under `root`.
  ///
  /// `dom` writes a sheet as `selector {\n  prop: value;\n}` per rule, out of
  /// a value whose selectors cannot contain a brace — so the rules come apart
  /// on the braces and there is no CSS to parse. A selector that meant the
  /// whole page becomes the container itself: a guest styling `body` is
  /// styling its own root, which is the only body it has.
  const scopedCss = (css, root) =>
    String(css)
      .split("}")
      .map((rule) => {
        const brace = rule.indexOf("{");
        if (brace < 0) return "";
        const selectors = rule
          .slice(0, brace)
          .split(",")
          .map((one) => one.trim())
          .filter(Boolean)
          .map((one) => (WHOLE_PAGE.has(one) ? root : `${root} ${one}`));
        return selectors.length ? `${selectors.join(", ")} {${rule.slice(brace + 1)}}` : "";
      })
      .filter(Boolean)
      .join("\n");

  /// The guest's one stylesheet: a real element in the page's head, and the
  /// stand-in `dom` is handed in its place.
  ///
  /// In the head rather than in the container, because `Render` replaces
  /// everything in the container — a sheet kept inside would be drawn away by
  /// the first tree. The stand-in exists so the text can be moved under the
  /// container on its way through; `dom` sets `textContent` on what it was
  /// given and cannot tell that something happened to it.
  ///
  /// A shell that is itself a guest has no page head to reach — its own
  /// document stops at its own container — so its guests' sheets live one
  /// level out, which is as far out as anything here can honestly go.
  const sheetFor = (app, root) => {
    const real = ctx.doc.createElement("style");
    real.setAttribute("data-code-guest", app);
    (ctx.doc.head || ctx.doc.body)?.appendChild(real);
    return {
      real,
      standIn: {
        id: "code-style",
        set textContent(css) {
          real.textContent = scopedCss(css, root);
        },
        get textContent() {
          return real.textContent;
        },
      },
    };
  };

  // An id is the guest's own text, and nothing here turns text into a
  // selector — so the one element with that id is found by walking what the
  // guest drew, which is also what keeps the search inside the container.
  const idInside = (container, id) => {
    for (const el of container.querySelectorAll("[id]")) {
      if (el.id === String(id)) return el;
    }
    return null;
  };

  /// A document that stops at `container`.
  ///
  /// Everything `dom` reaches for is here: nodes are made by the real
  /// document, because what a guest draws has to be part of the page, but
  /// nothing found through this one is outside the container. `body` is the
  /// container, a selector searches within it, and the stylesheet it asks for
  /// is the stand-in above.
  const documentFor = (container, sheet) => ({
    createElement: (tag) => ctx.doc.createElement(tag),
    createTextNode: (text) => ctx.doc.createTextNode(text),
    querySelector: (selector) =>
      WHOLE_PAGE.has(String(selector).trim()) ? container : container.querySelector(selector),
    getElementById: (id) => (id === "code-style" ? sheet.standIn : idInside(container, id)),
    // Where `dom` would put a stylesheet of its own making. It never gets
    // that far — `getElementById` always answers — and both are the container
    // so that nothing reached through this document is ever outside it.
    head: container,
    body: container,
  });

  /// The address, minus the guest's own name.
  ///
  /// The address bar stays the host's: a guest called `mail`, at `#/mail/inbox`,
  /// reads `/inbox`; navigating to `/sent` puts the page at `#/mail/sent`. A
  /// host that goes somewhere else entirely is not a change of route for the
  /// guest, and it is not told about one.
  const addressFor = (app, whileRunning) => {
    const head = `/${app}`;
    const hash = () => String(globalThis.location?.hash || "").replace(/^#/, "");
    const mine = () => {
      const path = hash();
      if (path === head) return "/";
      return path.startsWith(`${head}/`) ? path.slice(head.length) : "/";
    };
    return {
      read: mine,
      write: (path) => {
        if (!globalThis.location) return false;
        const bare = path.startsWith("#") ? path.slice(1) : path;
        globalThis.location.hash = head + (bare.startsWith("/") ? bare : `/${bare}`);
        return true;
      },
      watch: (then) => {
        let last = mine();
        const listener = () => {
          const now = mine();
          if (now === last) return;
          last = now;
          then();
        };
        globalThis.addEventListener?.("hashchange", listener);
        whileRunning(() => globalThis.removeEventListener?.("hashchange", listener));
      },
    };
  };

  /// What stands between one guest and the modules it reaches for.
  ///
  /// The host is asked `Offer { app, name }` once per module — the first time
  /// the guest actually reaches for it, so one it never uses is one the host
  /// is never asked about — and the answer holds for as long as that guest
  /// runs. `Offered` sends every particle to the host's `Module` handler,
  /// `Denied` refuses them all, and no answer leaves the guest with the
  /// page's own half.
  ///
  /// A host that offers a module and writes no `Module` handler has offered
  /// one that answers nothing. That is a program saying two things that do
  /// not agree, and the guest hears the second: null, the way any module
  /// answers a class it does not handle.
  const guardFor = (app) => {
    const settled = new Map();
    return (module, particle, answerHere) => {
      let verdict = settled.get(module);
      if (verdict === undefined) {
        const said = ctx.ask({ _class: "Offer", app, name: module });
        const which = isParticle(said) ? said._class : null;
        // Compared, not looked up: a class is a name that came from outside,
        // and nothing here reaches a property by one.
        verdict = which === "Offered" ? "the host's" : which === "Denied" ? "nobody's" : "its own";
        settled.set(module, verdict);
      }
      if (verdict === "its own") return answerHere(module, particle);
      if (verdict === "nobody's") {
        return {
          _class: "Exception",
          source: module,
          message: `'${module}' is not offered to '${app}' here`,
        };
      }
      const answer = ctx.ask({ _class: "Module", app, name: module, particle });
      return isParticle(answer) ? answer : null;
    };
  };

  /// Lets go of everything a guest was: its instance and its memory, the
  /// nodes it drew, its stylesheet, and the listeners that would otherwise
  /// fire into an application that is not there any more.
  const drop = (app) => {
    const held = running.get(app);
    if (!held) return;
    running.delete(app);
    held.host.stop();
    for (const off of held.leaving) off();
    held.sheet.real.remove?.();
    held.container.replaceChildren?.();
    held.container.removeAttribute?.("data-code-guest");
  };

  return [
    "guest",
    (particle) => {
      switch (particle._class) {
        case "Load": {
          const app = nameOf(particle.app);
          if (!app) return refused("a guest needs a name of letters, digits, '-' or '_'");
          // One instance per name — two of the same application drawing into
          // two containers would still share every stored key and the path.
          if (running.has(app)) return refused(`'${app}' is already running`);

          const url = typeof particle.url === "string" && particle.url ? particle.url : null;
          if (!url) return refused("a guest is one `.wasm`, and `url` is where it is");

          const into = typeof particle.into === "string" && particle.into ? particle.into : "body";
          const container = ctx.doc.querySelector(into);
          // Not an exception: a host loading before its own page has the node
          // is a mistake it can act on.
          if (!container) return refused(`nothing at '${into}' to run '${app}' inside`);
          // One application to a container. Two would draw over each other's
          // trees and answer to each other's rules, which is not two
          // applications running — it is one of them losing.
          const taken = container.getAttribute?.("data-code-guest");
          if (taken) return refused(`'${into}' is already running '${taken}'`);

          container.setAttribute("data-code-guest", app);
          const sheet = sheetFor(app, rootSelector(app));
          const leaving = [];
          const host = createHost({
            doc: documentFor(container, sheet),
            // Whose line it is, in a console that now has more than one
            // application printing to it.
            log: (line) => ctx.log(`${app}: ${line}`),
            // The page's own store, whatever the host's is. **Not narrowed,
            // deliberately**: one origin is one store, applications served
            // from the same place already share it when they run alone, and
            // one that could not see what it wrote on its own page would be a
            // different application for being hosted. It is also how a shell
            // signs in once for everything it runs. A shell that wants a
            // guest kept apart offers `storage` and namespaces it in its own
            // handlers, where that is a decision rather than a rule.
            store: ctx.store,
            address: addressFor(app, (off) => leaving.push(off)),
            guard: guardFor(app),
          });
          const held = { host, container, sheet, leaving, started: false };
          // Held before it exists, so that a second `Load` of the same name
          // while the first is still arriving is refused rather than run.
          running.set(app, held);

          // Fetched and then instantiated, rather than streamed: a `.wasm`
          // served as a plain file arrives with the wrong content type often
          // enough, and streaming refuses it outright.
          fetch(url)
            .then((answer) =>
              answer.ok
                ? answer.arrayBuffer()
                : Promise.reject(new Error(`'${url}' answered ${answer.status}`))
            )
            .then((bytes) => WebAssembly.instantiate(bytes, { env: host.env }))
            .then(({ instance }) => {
              // Unloaded while it was still on its way: start nothing, and
              // say nothing — the host already knows it let this one go.
              if (running.get(app) !== held) return;
              held.started = true;
              held.host.start(instance);
              ctx.fire({ _class: "Loaded", app });
            })
            .catch((e) => {
              if (running.get(app) === held) drop(app);
              ctx.fire({
                _class: "Exception",
                source: "guest",
                app,
                message: `cannot run '${app}': ${String(e?.message ?? e)}`,
              });
            });

          // On its way. Whether it arrives is a later particle.
          return { _class: "LoadResult", ok: true, reason: null };
        }

        case "Unload": {
          const app = nameOf(particle.app);
          if (!app || !running.has(app)) return { _class: "UnloadResult", ok: false };
          drop(app);
          return { _class: "UnloadResult", ok: true };
        }

        case "Tell": {
          const app = nameOf(particle.app);
          const held = app ? running.get(app) : null;
          // False rather than an exception, and the same false for all three:
          // a guest that was never loaded, one still arriving, and a particle
          // that is not one are the same thing to the host — nobody heard it.
          if (!held || !held.started || !isParticle(particle.particle)) {
            return { _class: "TellResult", ok: false };
          }
          const said = particle.particle;
          // Handed over, not carried out here — which is the whole reason
          // `Tell` answers `ok` rather than the guest's own answer.
          //
          // A host says something to its guest from inside one of its own
          // handlers, almost always. Running the guest right there would run
          // it *inside* that handler, and the guest's first question to its
          // modules comes straight back as a question to the host — a program
          // asked one thing while it is still answering another, over the one
          // buffer both answers are written into. It survives that today, by
          // construction: each question is read out of the buffer before its
          // handler runs, and each answer written after. But `runtime.c` does
          // not enforce one-at-a-time, it *assumes* it ("a handler cannot
          // re-enter one that is already running"), and this module would be
          // the one thing on the page making that untrue. So the guest runs
          // once the call it was told in has returned.
          //
          // The machine's host has no such rule and tells its guest there and
          // then: dispatch inside one program is an ordinary call, and the
          // page's one-particle door is not in it.
          queueMicrotask(() => {
            // Let go of in the meantime: a particle for an application that
            // is no longer there is dropped, not delivered to the next one to
            // take its name.
            if (running.get(app) !== held) return;
            // Asked rather than told, for one reason: a handler that trips
            // over something answers an `Exception`, and a told particle
            // throws the answer away. On a machine the host would have that
            // in its hand — `emit ... to app get r` — so here it arrives as a
            // particle, the way a guest that could not be fetched does. What
            // a handler answers when it *works* is still let go of: this is a
            // telling, and the host did not ask for an answer.
            const answer = held.host.ask(said);
            if (isParticle(answer) && answer._class === "Exception") {
              ctx.fire({ ...answer, app });
            }
          });
          return { _class: "TellResult", ok: true };
        }

        default:
          return null;
      }
    },
  ];
}
