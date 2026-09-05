// The page's half of `storage`: what the browser remembers between visits.
(ctx) => {
  const { str, writeInto } = ctx;

  // Reached through a function rather than held: a browser can refuse storage
  // outright — a private window, a reader's setting — and that throws on the
  // *first* touch rather than returning null.
  const store = () => {
    try {
      return globalThis.localStorage ?? null;
    } catch {
      return null;
    }
  };

  return {
    code_web_storage_get(keyPtr, keyLen, outPtr, cap) {
      const held = store();
      if (!held) return -1;
      let value;
      try {
        value = held.getItem(str(keyPtr, keyLen));
      } catch {
        return -1;
      }
      // A negative answer means nothing is stored there. Null rather than "":
      // a key never set and one holding an empty string are different
      // answers, and the module keeps them apart.
      if (value === null || value === undefined) return -1;
      return writeInto(String(value), outPtr, cap);
    },

    code_web_storage_set(keyPtr, keyLen, valPtr, valLen) {
      const held = store();
      if (!held) return 0;
      try {
        held.setItem(str(keyPtr, keyLen), str(valPtr, valLen));
        return 1;
      } catch {
        // Full, or refused. The module answers `ok = false` and the
        // application decides what that means.
        return 0;
      }
    },

    code_web_storage_remove(keyPtr, keyLen) {
      const held = store();
      if (!held) return 0;
      try {
        held.removeItem(str(keyPtr, keyLen));
        return 1;
      } catch {
        return 0;
      }
    },
  };
}
