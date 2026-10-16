(_ctx) => {
  const exception = message => ({ _class: "Exception", source: "arrays", message, innerException: null });
  // Iterative structural comparison: object field order is immaterial, array
  // order is significant. No stringify, coercion or recursive JS stack walk.
  const equal = (a, b) => {
    const pending = [[a, b]];
    while (pending.length) {
      const [x, y] = pending.pop();
      if (x === y) continue;
      if (x === null || y === null || typeof x !== typeof y || typeof x !== "object") return false;
      if (Array.isArray(x) !== Array.isArray(y)) return false;
      const keys = Object.keys(x);
      if (keys.length !== Object.keys(y).length) return false;
      for (const key of keys) {
        if (!Object.hasOwn(y, key)) return false;
        pending.push([x[key], y[key]]);
      }
    }
    return true;
  };
  // JS compares UTF-16 units by default; Code/Rust order valid text by scalars.
  const compareText = (a, b) => {
    const x = a[Symbol.iterator](), y = b[Symbol.iterator]();
    while (true) {
      const l = x.next(), r = y.next();
      if (l.done || r.done) return l.done === r.done ? 0 : l.done ? -1 : 1;
      const d = l.value.codePointAt(0) - r.value.codePointAt(0);
      if (d) return d < 0 ? -1 : 1;
    }
  };
  return ["arrays", p => {
    const name = p._class;
    if (!["Length", "Contains", "IndexOf", "Reverse", "Slice", "Unique", "Sort", "Sum", "Min", "Max"].includes(name)) return null;
    try {
      if (!Array.isArray(p.value)) throw `${name} requires an array 'value'`;
      const items = p.value;
      let value;
      switch (name) {
        case "Length": value = items.length; break;
        case "Contains": case "IndexOf": {
          const item = Object.hasOwn(p, "item") ? p.item : null;
          const index = items.findIndex(v => equal(v, item));
          value = name === "Contains" ? index >= 0 : index; break;
        }
        case "Reverse": value = [...items].reverse(); break;
        case "Slice": {
          const index = (key, fallback) => {
            if (!Object.hasOwn(p, key)) return fallback;
            if (!Number.isSafeInteger(p[key])) throw `Slice requires a safe integer '${key}' field`;
            return p[key];
          };
          value = items.slice(index("start", 0), index("end", items.length)); break;
        }
        case "Unique": {
          value = [];
          for (const item of items) if (!value.some(v => equal(v, item))) value.push(item);
          break;
        }
        case "Sort": {
          const by = Object.hasOwn(p, "by");
          if (by && typeof p.by !== "string") throw "Sort requires a string 'by' field";
          if (Object.hasOwn(p, "descending") && typeof p.descending !== "boolean") throw "Sort requires a boolean 'descending' field";
          let kind;
          const keyed = items.map((item, position) => {
            if (by && (item === null || typeof item !== "object" || Array.isArray(item) || !Object.hasOwn(item, p.by))) throw "Sort requires object items with the 'by' field";
            const key = by ? item[p.by] : item, type = typeof key;
            if (!(type === "string" || (type === "number" && Number.isFinite(key)))) throw "Sort requires finite number or string keys";
            if (kind !== undefined && kind !== type) throw "Sort requires keys of one type";
            kind = type;
            return { item, key, position };
          });
          keyed.sort((a, b) => {
            const order = typeof a.key === "string" ? compareText(a.key, b.key) : a.key < b.key ? -1 : a.key > b.key ? 1 : 0;
            return (p.descending ? -order : order) || a.position - b.position;
          });
          value = keyed.map(v => v.item); break;
        }
        default: {
          value = name === "Sum" ? 0 : null;
          for (const n of items) {
            if (typeof n !== "number" || !Number.isFinite(n)) throw `${name} requires an array of finite numbers`;
            value = name === "Sum" ? value + n : value === null ? n : name === "Min" ? Math.min(value, n) : Math.max(value, n);
          }
          if (value !== null && !Number.isFinite(value)) throw `${name} result must be finite`;
          if (value === 0) value = 0;
        }
      }
      return { _class: `${name}Result`, value };
    } catch (error) { return exception(String(error)); }
  }];
}
