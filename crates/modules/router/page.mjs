// The page's half of `router`: where in the application the reader is.
(ctx) => {
  // The hash, because a page served as a file has nothing else it can change
  // without asking a server for a URL that does not exist.
  const currentRoute = () => {
    const hash = String(globalThis.location?.hash || "");
    return hash.replace(/^#/, "") || "/";
  };

  let watching = null;
  let armed = false;

  return [
    "router",
    (particle) => {
      switch (particle._class) {
        case "Route":
          return { _class: "RouteResult", value: currentRoute() };

        case "Navigate": {
          if (!globalThis.location || typeof particle.path !== "string") {
            return { _class: "NavigateResult", ok: false };
          }
          // Assigning the hash is what puts an entry in the reader's history,
          // so Back means what they expect. An unchanged path fires nothing,
          // which is also what they expect.
          const path = particle.path;
          globalThis.location.hash = path.startsWith("#") ? path.slice(1) : path;
          return { _class: "NavigateResult", ok: true };
        }

        case "Watch": {
          if (typeof particle.then !== "string" || !particle.then) {
            return { _class: "WatchResult", ok: false };
          }
          watching = particle.then;
          // Listened for once, however many times the application asks:
          // watching twice would deliver every change twice.
          if (!armed) {
            globalThis.addEventListener?.("hashchange", () => {
              if (watching) ctx.fire({ _class: watching, path: currentRoute() });
            });
            armed = true;
          }
          return { _class: "WatchResult", ok: true };
        }

        default:
          return null;
      }
    },
  ];
}
