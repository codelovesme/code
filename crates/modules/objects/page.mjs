(_ctx) => {
  const exception = message => ({ _class: "Exception", source: "objects", message, innerException: null });
  // The same answers as the native half: an object is a value whose keys
  // keep the order they were written in, and an array is not an object.
  const isObject = v => v !== null && typeof v === "object" && !Array.isArray(v);
  return ["objects", p => {
    const name = p._class;
    if (!["Keys", "Values", "Has", "Delete"].includes(name)) return null;
    if (!isObject(p.value)) return exception(`${name} requires an object 'value'`);
    const entries = Object.entries(p.value);
    switch (name) {
      case "Keys": return { _class: "KeysResult", value: entries.map(([k]) => k) };
      case "Values": return { _class: "ValuesResult", value: entries.map(([, v]) => v) };
    }
    if (typeof p.key !== "string") return exception(`${name} requires a text 'key'`);
    if (name === "Has") return { _class: "HasResult", value: Object.hasOwn(p.value, p.key) };
    return { _class: "DeleteResult", value: Object.fromEntries(entries.filter(([k]) => k !== p.key)) };
  }];
}
