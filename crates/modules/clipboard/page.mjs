// The page's half of `clipboard`: what the reader copies.
//
// A particle in, a particle out. The write is a promise the browser may
// refuse — outside a click, or on an insecure page — and by then the answer
// has gone, so the refusal comes back as its own particle.
(ctx) => {
  const { fire } = ctx;
  return [
    "clipboard",
    (particle) => {
      if (particle._class !== "Copy") return null;
      const text = typeof particle.text === "string" ? particle.text : "";
      const clipboard = globalThis.navigator?.clipboard;
      if (typeof clipboard?.writeText !== "function") return { _class: "CopyResult", ok: false };
      try {
        const written = clipboard.writeText(text);
        if (written && typeof written.catch === "function") {
          written.catch((e) => fire({ _class: "CopyFailed", reason: String(e?.message ?? e ?? "refused") }));
        }
      } catch {
        return { _class: "CopyResult", ok: false };
      }
      return { _class: "CopyResult", ok: true };
    },
  ];
}
