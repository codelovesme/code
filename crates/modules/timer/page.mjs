// The page's half of `timer`: a particle, later.
(ctx) => {
  const { str, fire } = ctx;

  // Kept so `Cancel` has something to name. A fired delay drops out of it —
  // nothing repeats on its own, so nothing here outlives its own firing.
  const pending = new Map();
  let next = 1;

  return {
    code_web_timer_set(ms, classPtr, classLen, valuePtr, valueLen) {
      // Copied out now: these pointers are the module's own memory and the
      // wait is longer than the call.
      const className = str(classPtr, classLen);
      if (!className) return -1;
      const value = valueLen < 0 ? null : str(valuePtr, valueLen);
      const id = next++;
      const handle = setTimeout(() => {
        pending.delete(id);
        fire(value === null ? { _class: className } : { _class: className, value });
      }, ms);
      pending.set(id, handle);
      return id;
    },

    code_web_timer_clear(id) {
      const handle = pending.get(id);
      if (handle === undefined) return 0;
      clearTimeout(handle);
      pending.delete(id);
      return 1;
    },
  };
}
