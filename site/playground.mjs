  import init, { run_with_modules } from "./pkg/code_wasm.js";

  const EXAMPLES = JSON.parse(document.getElementById("examples-data").textContent);
  const byName = Object.fromEntries(EXAMPLES.map((e) => [e.name, e]));

  const source = document.getElementById("source");
  const hl = document.getElementById("hl");
  const output = document.getElementById("output");
  const runBtn = document.getElementById("run");
  const resetBtn = document.getElementById("reset");
  const copyOutBtn = document.getElementById("copy-out");
  const liveChk = document.getElementById("live");
  const status = document.getElementById("status");
  const lineCount = document.getElementById("line-count");

  const STORE_KEY = "code-playground-src";
  let currentExample = null;
  let liveTimer = null;

  /* ---------- syntax highlighting ---------- */
  const esc = (s) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  const TOKEN_RE =
    /(\|[^\n]*)|("(?:[^"\\]|\\.)*")|(\b\d+(?:\.\d+)?\b)|\b(assert|if|loop|over|get|break|continue|link|unlink|as|emit|to|core|this|base|return|and|or|not|is|type|of)\b|\b(true|false|null)\b|([A-Z][A-Za-z0-9_]*)/g;
  function highlight(src) {
    let out = "", last = 0, m;
    TOKEN_RE.lastIndex = 0;
    while ((m = TOKEN_RE.exec(src))) {
      out += esc(src.slice(last, m.index));
      const [full, comment, str, num, kw, bool, particle] = m;
      if (comment) out += `<span class="tok-c">${esc(full)}</span>`;
      else if (str) out += `<span class="tok-s">${esc(full)}</span>`;
      else if (num) out += `<span class="tok-n">${esc(full)}</span>`;
      else if (kw) out += `<span class="tok-k">${esc(full)}</span>`;
      else if (bool) out += `<span class="tok-b">${esc(full)}</span>`;
      else if (particle) out += `<span class="tok-p">${esc(full)}</span>`;
      last = TOKEN_RE.lastIndex;
    }
    out += esc(src.slice(last));
    return out;
  }
  // The engine answers "" or "error: …" and nothing else. It used to answer
  // a dump of the program's final bindings, one `name = value` per line,
  // which is what the `name = value` splitting here used to be for — the
  // dump is gone, so the splitting only waited to mangle the first output
  // line that happened to contain " = ".
  function renderOutput(text) {
    if (text.startsWith("error:")) return `<span class="out-err">${esc(text)}</span>`;
    return `<span>${esc(text)}</span>`;
  }

  /* ---------- editor ---------- */
  function syncEditor() {
    hl.innerHTML = highlight(source.value) + "\n";
    lineCount.textContent = source.value.split("\n").length + " lines";
  }
  source.addEventListener("scroll", () => {
    hl.parentElement.scrollTop = source.scrollTop;
    hl.parentElement.scrollLeft = source.scrollLeft;
  });
  source.addEventListener("input", () => {
    syncEditor();
    try { localStorage.setItem(STORE_KEY, source.value); } catch {}
    if (liveChk.checked) {
      clearTimeout(liveTimer);
      liveTimer = setTimeout(doRun, 380);
    }
  });
  source.addEventListener("keydown", (e) => {
    if (e.key === "Escape") {
      e.preventDefault();
      (runBtn.disabled ? resetBtn : runBtn).focus();
      return;
    }
    if ((e.metaKey || e.ctrlKey) && e.key === "Enter") { e.preventDefault(); doRun(); return; }
    if (e.key === "Tab") {
      e.preventDefault();
      const s = source.selectionStart, en = source.selectionEnd;
      source.value = source.value.slice(0, s) + "    " + source.value.slice(en);
      source.selectionStart = source.selectionEnd = s + 4;
      syncEditor();
    }
  });

  /* ---------- the console module ----------
   *
   * A browser module is whatever object the host hands over — see
   * `run_with_modules` in crates/code-wasm. So the page does not *capture*
   * anything: it supplies its own `console`, whose `dispatch` writes into the
   * panel below instead of the browser's devtools. A program cannot tell the
   * difference, and should not be able to: `console` writes to stdout on the
   * native side without caring whether stdout is a terminal or a pipe.
   *
   * The particle contract is `console`'s, so a program written here runs
   * unchanged against the real thing: `Print { value }` in, and
   * `PrintResult { value }` back carrying how many characters were
   * written. A String prints as itself and every other type renders as
   * compact JSON — the same rule string interpolation follows.
   */
  let printed = [];

  function renderPrinted(value) {
    return typeof value === "string" ? value : JSON.stringify(value);
  }

  const consoleModule = {
    dispatch(particleJson) {
      const particle = JSON.parse(particleJson);
      if (particle._class !== "Print") {
        // A class this module does not handle is null, like every module.
        return "null";
      }
      const text = renderPrinted(particle.value);
      printed.push(text);
      return JSON.stringify({ _class: "PrintResult", value: [...text].length });
    },
  };

  /* ---------- run ---------- */
  function doRun() {
    const t0 = performance.now();
    let result;
    printed = [];
    try {
      result = run_with_modules(source.value, { console: consoleModule });
    } catch (e) {
      result = "error: " + e;
    }
    const ms = Math.max(1, Math.round(performance.now() - t0));
    const isError = result.startsWith("error:");
    // What the program said, then how it ended. An error after some output is
    // the interesting case — the printing that happened before it still
    // happened, and hiding it would lose the half of the story that says
    // where the program got to.
    const said = printed.join("\n");
    output.innerHTML = [
      said ? renderOutput(said) : "",
      result ? renderOutput(result) : "",
      said || result
        ? ""
        : '(no output — a program is silent unless it emits to a module)',
    ]
      .filter((part) => part.length)
      .join("\n");
    output.classList.toggle("error", isError);
    status.textContent = isError ? "error" : `ran in ${ms} ms`;
    status.classList.toggle("err", isError);
  }
  runBtn.addEventListener("click", doRun);

  /* ---------- examples: load / reset / share ---------- */
  function loadExample(name, { scroll = false } = {}) {
    const ex = byName[name];
    if (!ex) return;
    currentExample = name;
    source.value = ex.code;
    syncEditor();
    try { localStorage.setItem(STORE_KEY, ex.code); } catch {}
    doRun();
    if (scroll) document.getElementById("playground").scrollIntoView({ behavior: "smooth" });
  }
  resetBtn.addEventListener("click", () => {
    // A saved draft is not an example, but Reset must never strand the
    // editor blank — the fallback is the same example a fresh visit opens.
    if (!EXAMPLES.length) { source.value = ""; syncEditor(); doRun(); return; }
    loadExample(currentExample || EXAMPLES[0].name);
  });
  copyOutBtn.addEventListener("click", async () => {
    const text = output.textContent;
    try {
      await navigator.clipboard.writeText(text);
      flash(copyOutBtn, "copied");
    } catch {}
  });
  function flash(btn, label) {
    const old = btn.innerHTML;
    btn.textContent = label;
    setTimeout(() => { btn.innerHTML = old; }, 1200);
  }

  /* ---------- examples gallery ---------- */
  const CAT_COLORS = {
    "Values & Expressions": "#5eead4",
    "Conditionals": "#a78bfa",
    "Loops": "#f472b6",
    "Scoping": "#fbbf24",
    "Assertions": "#86efac",
    "Particles": "#f87171",
    "Errors": "#f87171",
    "Printing": "#60a5fa",
  };
  const catColor = (c) => CAT_COLORS[c] || "#9aa3b5";
  const cats = [...new Set(EXAMPLES.map((e) => e.category))];
  const chipsEl = document.getElementById("chips");
  const gridEl = document.getElementById("ex-grid");

  function makeChip(label, cat) {
    const b = document.createElement("button");
    b.className = "chip" + (cat === "all" ? " active" : "");
    b.textContent = label;
    b.dataset.cat = cat;
    b.addEventListener("click", () => {
      chipsEl.querySelectorAll(".chip").forEach((c) => c.classList.toggle("active", c === b));
      filterGrid(cat);
    });
    chipsEl.appendChild(b);
  }
  function filterGrid(cat) {
    gridEl.querySelectorAll(".ex-card").forEach((card) => {
      card.style.display = cat === "all" || card.dataset.cat === cat ? "" : "none";
    });
  }
  makeChip("All", "all");
  cats.forEach((c) => makeChip(c, c));

  for (const ex of EXAMPLES) {
    const card = document.createElement("button");
    card.className = "ex-card reveal";
    card.dataset.cat = ex.category;
    card.innerHTML = `
      <div class="ex-top">
        <span class="ex-dot" style="background:${catColor(ex.category)}"></span>
        <span class="ex-name">${esc(ex.name)}</span>
      </div>
      <p class="ex-desc">${esc(ex.description || ex.name)}</p>
      <pre class="ex-code">${highlight(ex.code.split("\n").slice(0, 6).join("\n"))}</pre>
      <span class="try">try it in the playground →</span>`;
    card.addEventListener("click", () => loadExample(ex.name, { scroll: true }));
    gridEl.appendChild(card);
  }

  /* ---------- boot ---------- */
  function initialSource() {
    // A shared link (#src=…) always wins over a locally saved draft —
    // otherwise the recipient would see the sharer's stale editor state.
    const m = location.hash.match(/^#src=(.*)$/);
    if (m) {
      const src = decodeURIComponent(m[1]);
      try { localStorage.setItem(STORE_KEY, src); } catch {}
      return src;
    }
    const requested = byName[new URLSearchParams(location.search).get("example")];
    if (requested) return requested.code;
    try {
      const saved = localStorage.getItem(STORE_KEY);
      if (saved) return saved;
    } catch {}
    return EXAMPLES.length ? EXAMPLES[0].code : "";
  }
  // When a restored draft happens to be an unchanged example, remember it,
  // so Reset ("restore the current example") means what its label says.
  const bootSource = initialSource();
  source.value = bootSource;
  const bootedExample = EXAMPLES.find((e) => e.code === bootSource);
  if (bootedExample) currentExample = bootedExample.name;
  syncEditor();

  status.textContent = "loading…";
  init()
    .then(() => {
      status.textContent = "";
      runBtn.disabled = false;
      doRun();
    })
    .catch((e) => {
      status.textContent = "failed to load wasm";
      status.classList.add("err");
      output.textContent = "error: " + e;
      output.classList.add("error");
    });
  runBtn.disabled = true;
