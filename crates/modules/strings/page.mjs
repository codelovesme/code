(_ctx) => {
  const exception = (message) => ({ _class: "Exception", source: "strings", message, innerException: null });
  const upper = (s) => s.replace(/[a-z]/g, c => c.toUpperCase());
  const lower = (s) => s.replace(/[A-Z]/g, c => c.toLowerCase());
  return ["strings", (p) => {
    const name = p._class;
    if (!["Shout", "Echo", "Split", "Join", "Trim", "Upper", "Lower", "Length"].includes(name)) return null;
    const text = () => {
      if (typeof p.value !== "string") throw `${name} requires a string 'value'`;
      return p.value;
    };
    const separator = () => {
      if (typeof p.separator !== "string") throw `${name} requires a string 'separator' field`;
      if ([...p.separator].length !== 1) throw `${name} requires a single-character 'separator' field`;
      return p.separator;
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
      }
      return { _class: `${name}Result`, value };
    } catch (error) { return exception(String(error)); }
  }];
}
