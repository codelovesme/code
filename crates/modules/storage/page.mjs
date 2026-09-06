// The page's half of `storage`: what the browser remembers between visits.
//
// A particle in, a particle out. Nothing here throws: the runtime catches
// what does and turns it into an `Exception`, but a browser refusing storage
// is ordinary rather than exceptional, so it is answered as `ok = false`.
(ctx) => [
  "storage",
  (particle) => {
    // Reached through a function rather than held: a browser can refuse
    // storage outright — a private window, a reader's setting — and that
    // throws on the *first* touch rather than returning null.
    const store = () => {
      try {
        return globalThis.localStorage ?? null;
      } catch {
        return null;
      }
    };
    // A store of one's own, when something gave this program one: an
    // application running inside another keeps its keys under a prefix, and
    // nothing here has to know that it does.
    const held = ctx.store ?? store();
    const key = typeof particle.key === "string" ? particle.key : null;

    switch (particle._class) {
      case "Get": {
        if (!held || key === null) return { _class: "GetResult", value: null };
        let value;
        try {
          value = held.getItem(key);
        } catch {
          return { _class: "GetResult", value: null };
        }
        // Null rather than "": a key never set and one holding an empty
        // string are different answers, and this keeps them apart.
        return { _class: "GetResult", value: value === null ? null : String(value) };
      }
      case "Set": {
        // Text, and only text. An application with an object to keep turns it
        // into text with the `json` module and stores that — two modules each
        // doing one thing, rather than a store that quietly serialises.
        if (!held || key === null || typeof particle.value !== "string") {
          return { _class: "SetResult", ok: false };
        }
        try {
          held.setItem(key, particle.value);
          return { _class: "SetResult", ok: true };
        } catch {
          // Full, or refused. The application decides what that means.
          return { _class: "SetResult", ok: false };
        }
      }
      case "Remove": {
        if (!held || key === null) return { _class: "RemoveResult", ok: false };
        try {
          held.removeItem(key);
          return { _class: "RemoveResult", ok: true };
        } catch {
          return { _class: "RemoveResult", ok: false };
        }
      }
      // A class this module does not handle is null, not an error: the
      // particle may have been meant for something else entirely.
      default:
        return null;
    }
  },
]
