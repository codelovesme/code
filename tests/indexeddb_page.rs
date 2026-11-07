//! `indexeddb`'s page half keeps values and says so only once they are
//! kept: `Put` answers at once with a number, `DbStored` follows when the
//! transaction has committed; `Get`, `Keys` and `Delete` answer the same
//! way; a full origin is `DbFailed { full = true }`, never a `DbStored`.
//! Run under node against a small in-memory stand-in for IndexedDB.

use std::fs;
use std::path::Path;
use std::process::Command;

fn tool_exists(name: &str) -> bool {
    Command::new(name)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[test]
fn a_page_keeps_values_and_says_when_it_could_not() {
    if !tool_exists("node") {
        eprintln!("skipped: needs node");
        return;
    }
    let half = Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/modules/indexeddb/page.mjs");
    let dir = std::env::temp_dir().join(format!("code-indexeddb-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("create the probe dir");
    let probe = dir.join("probe.mjs");
    fs::write(
        &probe,
        format!(
            r#"import {{ readFileSync }} from "node:fs";
const half = new Function("return (" + readFileSync({half:?}, "utf8") + ")")();
const check = (what, got, want) => {{
  if (JSON.stringify(got) !== JSON.stringify(want)) {{
    console.error(`${{what}}: got ${{JSON.stringify(got)}}, wanted ${{JSON.stringify(want)}}`);
    process.exit(1);
  }}
}};
const later = (f) => setTimeout(f, 0);
// IndexedDB, as little of it as the half uses: one store, requests whose
// result is set before the transaction completes, and a quota.
const data = new Map();
let quota = Infinity;
const sizeOf = (v) => JSON.stringify(v).length;
globalThis.IDBKeyRange = {{ bound: (lo, hi) => ({{ lo, hi }}) }};
globalThis.indexedDB = {{
  open() {{
    const req = {{}};
    const db = {{
      objectStoreNames: {{ contains: () => false }},
      createObjectStore() {{}},
      transaction() {{
        const tx = {{}};
        const pending = [];
        const store = {{
          put(value, key) {{ const r = {{}}; pending.push(() => {{
            const used = [...data.values()].reduce((n, v) => n + sizeOf(v), 0);
            if (used + sizeOf(value) > quota) {{ tx.error = Object.assign(new Error("QuotaExceededError"), {{ name: "QuotaExceededError" }}); return false; }}
            data.set(key, structuredClone(value)); r.result = key; return true; }}); return r; }},
          get(key) {{ const r = {{}}; pending.push(() => {{ r.result = data.has(key) ? structuredClone(data.get(key)) : undefined; return true; }}); return r; }},
          delete(key) {{ const r = {{}}; pending.push(() => {{ data.delete(key); r.result = undefined; return true; }}); return r; }},
          getAllKeys(range) {{ const r = {{}}; pending.push(() => {{ r.result = [...data.keys()].filter((k) => !range || (k >= range.lo && k <= range.hi)); return true; }}); return r; }},
        }};
        tx.objectStore = () => store;
        later(() => {{ const ok = pending.every((f) => f()); ok ? tx.oncomplete() : tx.onabort(); }});
        return tx;
      }},
    }};
    later(() => {{ req.result = db; req.onupgradeneeded?.(); req.onsuccess(); }});
    return req;
  }},
}};
Object.defineProperty(globalThis, "navigator", {{ value: {{ storage: {{
  estimate: async () => ({{ usage: 1234, quota: 99999 }}),
  persisted: async () => false,
  persist: async () => true,
}} }}, configurable: true }});
const fired = [];
const [, kept] = half({{ fire: (p) => fired.push(p) }});
const settle = () => new Promise((r) => setTimeout(r, 10));

const put = kept({{ _class: "Put", key: "app:a", value: {{ type: "image/jpeg", data: "QUJD" }} }});
check("a put did not answer at once", put, {{ _class: "PutResult", ok: true, id: 1 }});
check("stored before the transaction committed", fired, []);
await settle();
check("the put was not said", fired, [{{ _class: "DbStored", id: 1, key: "app:a" }}]);
fired.length = 0;

kept({{ _class: "Put", key: "app:b", value: "text" }});
kept({{ _class: "Put", key: "other:c", value: 1 }});
await settle();
fired.length = 0;
kept({{ _class: "Get", key: "app:a" }});
kept({{ _class: "Get", key: "app:none" }});
kept({{ _class: "Keys", prefix: "app:" }});
await settle();
check("get, a missing get, keys", fired, [
  {{ _class: "DbFound", id: 4, key: "app:a", value: {{ type: "image/jpeg", data: "QUJD" }} }},
  {{ _class: "DbFound", id: 5, key: "app:none", value: null }},
  {{ _class: "DbKeys", id: 6, prefix: "app:", keys: ["app:a", "app:b"] }},
]);
fired.length = 0;

kept({{ _class: "Delete", key: "app:b" }});
await settle();
kept({{ _class: "Keys", prefix: "app:" }});
await settle();
check("delete", fired, [{{ _class: "DbDeleted", id: 7, key: "app:b" }}, {{ _class: "DbKeys", id: 8, prefix: "app:", keys: ["app:a"] }}]);
fired.length = 0;

quota = 10;
kept({{ _class: "Put", key: "app:big", value: "x".repeat(100) }});
await settle();
check("a full origin", fired.map((p) => [p._class, p.key, p.full]), [["DbFailed", "app:big", true]]);
fired.length = 0;
kept({{ _class: "Get", key: "app:big" }});
await settle();
check("what failed was not kept", fired[0].value, null);
fired.length = 0;

check("a put without a key claimed to start", kept({{ _class: "Put", value: 1 }}), {{ _class: "PutResult", ok: false, id: 0 }});
kept({{ _class: "Persist" }});
await settle();
check("persist", fired, [{{ _class: "DbSpace", id: 11, usage: 1234, quota: 99999, persisted: true }}]);
"#
        ),
    )
    .expect("write the probe");
    let output = Command::new("node")
        .arg(&probe)
        .output()
        .expect("run the probe under node");
    assert!(
        output.status.success(),
        "indexeddb's page half did not hold: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let _ = fs::remove_dir_all(&dir);
}
