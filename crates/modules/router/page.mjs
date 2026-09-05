// The page's half of `router`: where in the application the reader is.
(ctx) => {
  const { str, writeInto, fire } = ctx;

  // The hash, because a page served as a file has nothing else it can change
  // without asking a server for a URL that does not exist.
  const currentRoute = () => {
    const hash = String(globalThis.location?.hash || "");
    return hash.replace(/^#/, "") || "/";
  };

  let watching = null;
  let armed = false;

  return {
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
      // Listened for once, however many times the application asks: watching
      // twice would deliver every change twice.
      if (!armed) {
        globalThis.addEventListener?.("hashchange", () => {
          if (watching) fire({ _class: watching, path: currentRoute() });
        });
        armed = true;
      }
      return 1;
    },
  };
}
