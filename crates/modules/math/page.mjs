(_ctx) => {
  const exception = (message) => ({ _class: "Exception", source: "math", message, innerException: null });
  const known = new Set(["Double", "Sum", "Floor", "Ceil", "Round", "Trunc", "Div", "Mod", "Abs", "Min", "Max", "Parse", "Sqrt", "Pow", "Sin", "Cos", "Ln", "Exp"]);
  const decimal = /^[+-]?(?:[0-9]+(?:\.[0-9]*)?|\.[0-9]+)(?:[eE][+-]?[0-9]+)?$/;
  return ["math", (p) => {
    const name = p._class;
    if (!known.has(name)) return null;
    const operand = (field) => {
      const value = p[field];
      if (typeof value !== "number") throw `${name} requires a numeric '${field}'`;
      if (!Number.isFinite(value)) throw `${name} requires a numeric '${field}'`;
      return value;
    };
    try {
      let value;
      if (name === "Parse") {
        if (typeof p.text !== "string") throw "Parse requires a string 'text'";
        // RegExp '$' also matches before a final newline: require the entire match.
        if (p.text.match(decimal)?.[0] !== p.text) throw "Parse requires decimal text";
        value = Number(p.text);
      } else if (name === "Sum") {
        if (!Array.isArray(p.value)) throw "Sum requires an array 'value'";
        value = 0;
        for (const n of p.value) {
          if (typeof n !== "number") throw "Sum requires an array of numbers";
          if (!Number.isFinite(n)) throw "Sum requires an array of numbers";
          value += n;
        }
      } else {
        const n = operand("value");
        switch (name) {
          case "Double": value = n * 2; break;
          case "Floor": value = Math.floor(n); break;
          case "Ceil": value = Math.ceil(n); break;
          case "Round": value = n < 0 ? -Math.round(-n) : Math.round(n); break;
          case "Trunc": value = Math.trunc(n); break;
          case "Abs": value = Math.abs(n); break;
          case "Sqrt":
            if (n < 0) throw "Sqrt requires a non-negative 'value'";
            value = Math.sqrt(n); break;
          case "Ln":
            if (n <= 0) throw "Ln requires a positive 'value'";
            value = Math.log(n); break;
          case "Exp": value = Math.exp(n); break;
          case "Sin": value = Math.sin(n); break;
          case "Cos": value = Math.cos(n); break;
          case "Pow": value = Math.pow(n, operand("exponent")); break;
          case "Min": value = Math.min(n, operand("other")); break;
          case "Max": value = Math.max(n, operand("other")); break;
          case "Div": case "Mod": {
            const by = operand("by");
            if (by === 0) throw `${name} requires a non-zero 'by'`;
            if (name === "Div") value = Math.floor(n / by);
            else {
              const rem = n % by;
              value = rem !== 0 && (rem < 0) !== (by < 0) ? rem + by : rem;
            }
            break;
          }
        }
      }
      if (!Number.isFinite(value)) throw `${name} result must be finite`;
      return { _class: `${name}Result`, value: value === 0 ? 0 : value };
    } catch (error) { return exception(String(error)); }
  }];
}
