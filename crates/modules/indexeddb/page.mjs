// The page's half of `indexeddb`: what a page keeps that is too large for
// `storage` — photos, recordings, an offline copy of an application's
// records.
//
// IndexedDB answers later, so every handler answers "asked" at once with a
// number, and the real answer arrives as its own particle carrying that
// number as `id` — the shape `net_client` and `media` use for an answer that
// outlives its request. Nothing here throws: a browser refusing to keep
// something is `DbFailed`, with `full = true` when the origin is out of
// space, because a page must never claim to have kept what it did not.
//
// One database, one object store, per origin: applications keep their keys
// under their own prefix, as they do in `storage`.
(ctx) => {
  const { fire } = ctx;
  const DB = "code-indexeddb";
  const STORE = "kept";
  let opened = null;
  let next = 1;

  const factory = () => {
    try {
      return globalThis.indexedDB ?? null;
    } catch {
      return null;
    }
  };

  // Opened once and kept: every handler waits on the same promise.
  const database = () => {
    if (opened) return opened;
    opened = new Promise((resolve, reject) => {
      const idb = factory();
      if (!idb) {
        reject(new Error("this browser keeps nothing in IndexedDB"));
        return;
      }
      const asked = idb.open(DB, 1);
      asked.onupgradeneeded = () => {
        if (!asked.result.objectStoreNames.contains(STORE)) asked.result.createObjectStore(STORE);
      };
      asked.onsuccess = () => resolve(asked.result);
      asked.onerror = () => reject(asked.error || new Error("IndexedDB would not open"));
      asked.onblocked = () => reject(new Error("IndexedDB is held open by another tab at an older version"));
    });
    opened.catch(() => {
      opened = null;
    });
    return opened;
  };

  const isFull = (error) => {
    const name = String(error?.name ?? "");
    return name === "QuotaExceededError" || /quota|space/i.test(String(error?.message ?? ""));
  };
  const failed = (id, key, error) =>
    fire({
      _class: "DbFailed",
      id,
      key: key ?? null,
      reason: String(error?.message ?? error ?? "IndexedDB refused"),
      full: isFull(error),
    });

  // One request in one transaction; `done` is told the request's result once
  // the transaction has committed, so `DbStored` means kept, not queued.
  const run = (id, key, mode, make, done) => {
    database()
      .then((db) => {
        let tx;
        try {
          tx = db.transaction(STORE, mode);
        } catch (error) {
          failed(id, key, error);
          return;
        }
        let request;
        try {
          request = make(tx.objectStore(STORE));
        } catch (error) {
          failed(id, key, error);
          return;
        }
        tx.oncomplete = () => done(request.result);
        tx.onerror = () => failed(id, key, tx.error || request.error);
        tx.onabort = () => failed(id, key, tx.error || request.error || new Error("aborted"));
      })
      .catch((error) => failed(id, key, error));
  };

  const space = (id, persisted) => {
    const storage = globalThis.navigator?.storage;
    if (!storage || typeof storage.estimate !== "function") {
      fire({ _class: "DbSpace", id, usage: null, quota: null, persisted });
      return;
    }
    storage
      .estimate()
      .then((e) => fire({ _class: "DbSpace", id, usage: e.usage ?? null, quota: e.quota ?? null, persisted }))
      .catch(() => fire({ _class: "DbSpace", id, usage: null, quota: null, persisted }));
  };

  const asked = (klass, ok, id) => ({ _class: klass, ok, id: ok ? id : 0 });

  return [
    "indexeddb",
    (particle) => {
      const key = typeof particle.key === "string" && particle.key !== "" ? particle.key : null;
      switch (particle._class) {
        case "Put": {
          if (key === null || particle.value === undefined) return asked("PutResult", false, 0);
          const id = next++;
          const value = particle.value;
          run(id, key, "readwrite", (s) => s.put(value, key), () => fire({ _class: "DbStored", id, key }));
          return asked("PutResult", true, id);
        }
        case "Get": {
          if (key === null) return asked("GetResult", false, 0);
          const id = next++;
          run(id, key, "readonly", (s) => s.get(key), (value) =>
            fire({ _class: "DbFound", id, key, value: value === undefined ? null : value }));
          return asked("GetResult", true, id);
        }
        case "Delete": {
          if (key === null) return asked("DeleteResult", false, 0);
          const id = next++;
          run(id, key, "readwrite", (s) => s.delete(key), () => fire({ _class: "DbDeleted", id, key }));
          return asked("DeleteResult", true, id);
        }
        case "Keys": {
          const prefix = typeof particle.prefix === "string" ? particle.prefix : "";
          const id = next++;
          const Range = globalThis.IDBKeyRange;
          run(id, null, "readonly", (s) =>
            prefix && Range ? s.getAllKeys(Range.bound(prefix, prefix + "￿")) : s.getAllKeys(),
          (keys) => {
            const found = (keys || []).filter((k) => typeof k === "string" && k.startsWith(prefix)).sort();
            fire({ _class: "DbKeys", id, prefix, keys: found });
          });
          return asked("KeysResult", true, id);
        }
        case "Space": {
          const id = next++;
          const storage = globalThis.navigator?.storage;
          if (storage && typeof storage.persisted === "function") {
            storage.persisted().then((p) => space(id, p === true)).catch(() => space(id, false));
          } else {
            space(id, false);
          }
          return asked("SpaceResult", true, id);
        }
        case "Persist": {
          const id = next++;
          const storage = globalThis.navigator?.storage;
          if (storage && typeof storage.persist === "function") {
            storage.persist().then((p) => space(id, p === true)).catch(() => space(id, false));
          } else {
            space(id, false);
          }
          return asked("PersistResult", true, id);
        }
        default:
          return null;
      }
    },
  ];
}
