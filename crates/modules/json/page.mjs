// The page's half of `json`: text in and out, the way the machine half does it.
//
// A browser has `JSON` already, so there is nothing to port — only the
// module's contract to keep: `_class` is dropped from every object on the way
// out and nothing else is, a whole number writes without a fractional part,
// a number JSON cannot spell writes as `null`, and text that is not JSON is
// an `Exception` rather than a value.
(ctx) => {
  const exception = (message) => ({ _class: "Exception", source: "json", message });

  // `_class` off every object, at any depth. The plumbing the language adds
  // is not the data the program is carrying. Arrays keep their shape.
  const bare = (v) => {
    if (Array.isArray(v)) return v.map(bare);
    if (v === null || typeof v !== "object") return v;
    const out = {};
    for (const [k, inner] of Object.entries(v)) {
      if (k !== "_class") out[k] = bare(inner);
    }
    return out;
  };

  return [
    "json",
    (particle) => {
      switch (particle._class) {
        case "Parse": {
          if (typeof particle.text !== "string") {
            return exception("Parse requires a string 'text'");
          }
          let value;
          try {
            value = JSON.parse(particle.text);
          } catch (e) {
            return exception(`invalid JSON: ${e?.message ?? e}`);
          }
          return { _class: "ParseResult", value };
        }
        case "Stringify": {
          // `Stringify { }` stringifies null — there is no value this cannot
          // render. `JSON.stringify` already writes `1` for 1.0 and `null`
          // for a number it cannot spell, which is the machine half's rule.
          const value = Object.hasOwn(particle, "value") ? bare(particle.value) : null;
          const pretty = particle.pretty === true;
          const text = JSON.stringify(value === undefined ? null : value, null, pretty ? 2 : 0);
          return { _class: "StringifyResult", value: text };
        }
        // A class this module does not handle is null, not an error: the
        // particle may have been meant for something else entirely.
        default:
          return null;
      }
    },
  ];
}
