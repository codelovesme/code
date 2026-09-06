// The page's half of `console`: where a line goes when there is no stdout.
//
// **The rendering is here as well as in the module**, which is the one place
// in these halves where the same rule is written twice, and it is worth being
// plain about. The alternative was for the module to render and hand the page
// a finished line — but then the module needs to build a particle of its own
// to send, and a browser module that builds particles needs the whole
// machinery back that this door removed.
//
// So both sides render, and both must agree: `console_print_kinds.code` is
// what holds the machine side to the rule, and `console/README.md` states it
// once for both. If they ever disagree, this comment is where to look.
(ctx) => [
  "console",
  (particle) => {
    if (particle._class !== "Print") return null;

    // A field the particle does not carry is null, and null renders as
    // "null" like any other value — `Print { }` is `Print { value = null }`.
    const v = particle.value === undefined ? null : particle.value;
    const line =
      typeof v === "string"
        ? v // already text; quoting would make `Print "hi"` show `"hi"`
        : typeof v === "number" || typeof v === "boolean"
          ? String(v)
          : v === null
            ? "null"
            : Array.isArray(v)
              ? `[${v.length} items]` // a console, not a serializer
              : `{${Object.keys(v).length} fields}`;

    ctx.log(line);
    // Characters as rendered, which is what the machine half counts too, so
    // a program can `assert` that the print happened.
    return { _class: "PrintResult", value: line.length };
  },
]
