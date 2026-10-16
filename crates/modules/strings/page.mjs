(_ctx) => {
  const exception = (message) => ({ _class: "Exception", source: "strings", message, innerException: null });
  const upper = (s) => s.replace(/[a-z]/g, c => c.toUpperCase());
  const lower = (s) => s.replace(/[A-Z]/g, c => c.toLowerCase());
  return ["strings", (p) => {
    const name = p._class;
    if (!["Shout", "Echo", "Split", "Join", "Trim", "Upper", "Lower", "Length", "Contains", "StartsWith", "EndsWith", "IndexOf", "Replace", "Slice", "Pad"].includes(name)) return null;
    const text = () => {
      if (typeof p.value !== "string") throw `${name} requires a string 'value'`;
      return p.value;
    };
    const field = (key) => {
      if (typeof p[key] !== "string") throw `${name} requires a string '${key}' field`;
      return p[key];
    };
    const integer = (key, fallback) => {
      if (!Object.hasOwn(p, key) && fallback !== undefined) return fallback;
      if (!Number.isSafeInteger(p[key])) throw `${name} requires a safe integer '${key}' field`;
      return p[key];
    };
    const separator = () => {
      const sep = field("separator");
      if (name === "Split" && sep === "") throw "Split requires a non-empty 'separator' field";
      return sep;
    };
    try {
      let value;
      switch (name) {
        case "Echo": value = p.value ?? null; break;
        case "Shout": value = upper(text()) + "!"; break;
        case "Upper": value = upper(text()); break;
        case "Lower": value = lower(text()); break;
        case "Trim": value = text().replace(/^[ \t\n\r\f\v]+|[ \t\n\r\f\v]+$/g, ""); break;
        case "Length": value = [...text()].length; break;
        case "Split": { const s = text(); value = s.split(separator()); break; }
        case "Join": {
          if (!Array.isArray(p.value)) throw "Join requires an array 'value'";
          const sep = separator();
          if (!p.value.every(v => typeof v === "string")) throw "Join requires an array of strings";
          value = p.value.join(sep); break;
        }
        case "Contains": case "StartsWith": case "EndsWith": case "IndexOf": {
          let s = text(), needle = field("search");
          if (Object.hasOwn(p, "ignore_case") && typeof p.ignore_case !== "boolean") throw `${name} requires a boolean 'ignore_case' field`;
          if (p.ignore_case) { s = lower(s); needle = lower(needle); }
          if (name === "Contains") value = s.includes(needle);
          else if (name === "StartsWith") value = s.startsWith(needle);
          else if (name === "EndsWith") value = s.endsWith(needle);
          else { const index = s.indexOf(needle); value = index < 0 ? -1 : [...s.slice(0, index)].length; }
          break;
        }
        case "Replace": {
          const s = text(), needle = field("search"), replacement = field("replacement");
          if (needle === "") throw "Replace requires a non-empty 'search' field";
          value = s.split(needle).join(replacement); break;
        }
        case "Slice": {
          const chars = [...text()], start = integer("start", 0), end = integer("end", chars.length);
          value = chars.slice(start, end).join(""); break;
        }
        case "Pad": {
          const s = text(), width = integer("width");
          if (width < 0 || width > 1000000) throw "Pad requires 'width' between 0 and 1000000";
          const fill = Object.hasOwn(p, "fill") ? field("fill") : " ";
          if (fill === "") throw "Pad requires a non-empty 'fill' field";
          const side = Object.hasOwn(p, "side") ? field("side") : "left";
          if (side !== "left" && side !== "right") throw "Pad requires 'side' to be left or right";
          const needed = Math.max(0, width - [...s].length), chars = [...fill];
          const padding = fill.repeat(Math.floor(needed / chars.length)) + chars.slice(0, needed % chars.length).join("");
          value = side === "left" ? padding + s : s + padding; break;
        }
      }
      return { _class: `${name}Result`, value };
    } catch (error) { return exception(String(error)); }
  }];
}
