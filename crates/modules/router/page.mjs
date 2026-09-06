// The page's half of `router`: where in the application the reader is.
(ctx) => {
  // The hash, because a page served as a file has nothing else it can change
  // without asking a server for a URL that does not exist.
  const theBrowsers = {
    read: () => String(globalThis.location?.hash || "").replace(/^#/, "") || "/",
    write: (path) => {
      if (!globalThis.location) return false;
      // Assigning the hash is what puts an entry in the reader's history, so
      // Back means what they expect. An unchanged path fires nothing, which
      // is also what they expect.
      globalThis.location.hash = path.startsWith("#") ? path.slice(1) : path;
      return true;
    },
    watch: (then) => globalThis.addEventListener?.("hashchange", () => then()),
  };

  // An address of one's own, when something gave this program one: an
  // application running inside another reads the path after its own name, so
  // one page keeps one address bar and every application on it still starts
  // at its own root.
  const where = ctx.address ?? theBrowsers;

  let watching = null;
  let armed = false;

  return [
    "router",
    (particle) => {
      switch (particle._class) {
        case "Route":
          return { _class: "RouteResult", value: where.read() };

        case "Navigate": {
          if (typeof particle.path !== "string") {
            return { _class: "NavigateResult", ok: false };
          }
          return { _class: "NavigateResult", ok: where.write(particle.path) === true };
        }

        case "Watch": {
          if (typeof particle.then !== "string" || !particle.then) {
            return { _class: "WatchResult", ok: false };
          }
          watching = particle.then;
          // Listened for once, however many times the application asks:
          // watching twice would deliver every change twice.
          if (!armed) {
            where.watch(() => {
              if (watching) ctx.fire({ _class: watching, path: where.read() });
            });
            armed = true;
          }
          return { _class: "WatchResult", ok: true };
        }

        // Reached straight from `globalThis.location`, not through `where` —
        // deliberately. `where` is the per-application slice a guest's own
        // address is scoped to; the scheme, the host and the port belong to
        // the one real page underneath every application on it, and answer
        // the same thing asked by any of them.
        case "Where": {
          const loc = globalThis.location;
          if (!loc || !loc.origin || loc.origin === "null") {
            // A page with no address at all — opened straight off disk. Not
            // an exception: this module answers what the page is, and a
            // page with no address is a fact, not a failure.
            return {
              _class: "WhereResult",
              origin: null,
              protocol: null,
              hostname: null,
              port: null,
            };
          }
          return {
            _class: "WhereResult",
            origin: loc.origin,
            protocol: loc.protocol.replace(/:$/, ""),
            hostname: loc.hostname,
            port: loc.port || null,
          };
        }

        // Splitting a path is not a text problem this module reaches
        // outside itself for — a path is what `router` is about, and the
        // one thing an application built to be hosted needs is its own
        // name back out of a path it did not build alone (`guest` scopes a
        // guest's own `Route`/`Navigate`, but a shell deciding *which*
        // guest a reload's address names has to read the whole thing).
        case "Segment": {
          const path = typeof particle.path === "string" ? particle.path : "";
          const at = typeof particle.at === "number" ? particle.at : 0;
          // A leading slash means the first real segment is index 1 of the
          // split, not 0 — dropped here so `at` counts the way a reader
          // would say it: "auth-web" is segment 0 of "/auth-web/account".
          const parts = path.replace(/^\//, "").split("/");
          return { _class: "SegmentResult", value: parts[at] ?? "" };
        }

        default:
          return null;
      }
    },
  ];
}
