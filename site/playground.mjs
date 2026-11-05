  const EXAMPLES = JSON.parse(document.getElementById("examples-data").textContent);
  const byName = Object.fromEntries(EXAMPLES.map((e) => [e.name, e]));
  const FEATURED = JSON.parse(document.getElementById("featured-data").textContent);
  const defaultExample = FEATURED[0].name;
  let engineState = "loading";
  let runEngine = null;

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
    updateExampleContext();
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
    if (e.key === "Tab" && !e.shiftKey) {
      e.preventDefault();
      const s = source.selectionStart, en = source.selectionEnd;
      source.value = source.value.slice(0, s) + "    " + source.value.slice(en);
      source.selectionStart = source.selectionEnd = s + 4;
      source.dispatchEvent(new Event("input", { bubbles: true }));
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
    clearTimeout(liveTimer);
    if (engineState !== "ready") return;
    const t0 = performance.now();
    let result;
    printed = [];
    try {
      result = runEngine(source.value, { console: consoleModule });
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
    copyOutBtn.disabled = false;
    document.getElementById("result-copy-status").textContent = "";
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
    clearTimeout(liveTimer);
    source.value = ex.code;
    syncEditor();
    updateExampleContext();
    // A deliberate selection replaces an incoming shared/named source. Reload
    // can then restore later edits from the existing draft storage key.
    const selectedURL = new URL(location.href);
    selectedURL.searchParams.delete("example");
    if (selectedURL.hash.startsWith("#src=")) selectedURL.hash = "#playground";
    history.replaceState(null, "", selectedURL);
    try { localStorage.setItem(STORE_KEY, ex.code); } catch {}
    doRun();
    if (scroll) document.getElementById("playground").scrollIntoView({
      behavior: matchMedia("(prefers-reduced-motion: reduce)").matches ? "instant" : "smooth",
    });
  }
  resetBtn.addEventListener("click", () => {
    // A saved draft is not an example, but Reset must never strand the
    // editor blank — the fallback is the same example a fresh visit opens.
    if (!EXAMPLES.length) { source.value = ""; syncEditor(); doRun(); return; }
    loadExample(currentExample || defaultExample);
  });
  copyOutBtn.addEventListener("click", async () => {
    const text = output.textContent;
    try {
      await navigator.clipboard.writeText(text);
      document.getElementById("result-copy-status").textContent = "Result copied.";
    } catch {
      const range = document.createRange();
      range.selectNodeContents(output);
      const selection = window.getSelection();
      selection.removeAllRanges(); selection.addRange(range);
      output.focus();
      document.getElementById("result-copy-status").textContent = "Result selected. Use your device’s copy command.";
    }
  });

  /* ---------- featured examples ---------- */
  const featuredEl = document.getElementById("featured-examples");
  for (const example of FEATURED) {
    const button = document.createElement("button");
    button.className = "pg-feature";
    button.dataset.example = example.name;
    button.setAttribute("aria-pressed", "false");
    button.textContent = example.title;
    button.addEventListener("click", () => loadExample(example.name));
    featuredEl.appendChild(button);
  }
  function updateExampleContext() {
    const example = byName[currentExample];
    const featured = FEATURED.find(item => item.name === currentExample);
    const edited = example && source.value !== example.code;
    document.getElementById("example-title").textContent = example
      ? (featured?.title ?? example.name) + (edited ? " · edited" : "")
      : "Your program";
    document.getElementById("example-description").textContent = featured?.description
      ?? example?.description ?? "Your saved draft or shared source. Run it, or choose a featured example.";
    document.getElementById("expected-panel").hidden = !featured;
    document.getElementById("expected-output").textContent = featured?.output ?? "";
    for (const button of featuredEl.children) {
      button.setAttribute("aria-pressed", String(button.dataset.example === currentExample));
    }
  }

  /* ---------- advanced collection: lazy, filtered and paged ---------- */
  const CAT_COLORS = {
    "Values & Expressions": "#5eead4", "Conditionals": "#a78bfa",
    "Loops": "#f472b6", "Scoping": "#fbbf24", "Assertions": "#86efac",
    "Particles": "#f87171", "Errors": "#f87171", "Printing": "#60a5fa",
  };
  const library = document.getElementById("all-examples");
  const chipsEl = document.getElementById("chips");
  const gridEl = document.getElementById("ex-grid");
  const searchEl = document.getElementById("example-search");
  const previous = document.getElementById("previous-examples");
  const next = document.getElementById("next-examples");
  searchEl.disabled = false;
  document.getElementById("collection-static-note").hidden = true;
  const PAGE_SIZE = 12;
  let category = "all", pageIndex = 0;
  function renderGallery() {
    if (!library.open) return;
    const query = searchEl.value.trim().toLowerCase();
    const matches = EXAMPLES.filter(example =>
      (category === "all" || example.category === category) &&
      `${example.name} ${example.description}`.toLowerCase().includes(query));
    pageIndex = Math.min(pageIndex, Math.max(0, Math.ceil(matches.length / PAGE_SIZE) - 1));
    const start = pageIndex * PAGE_SIZE;
    gridEl.replaceChildren();
    for (const example of matches.slice(start, start + PAGE_SIZE)) {
      const card = document.createElement("button");
      card.className = "ex-card";
      card.dataset.cat = example.category;
      card.dataset.example = example.name;
      card.innerHTML = `<span class="ex-top"><span class="ex-dot" style="background:${CAT_COLORS[example.category] ?? "#9aa3b5"}"></span><span class="ex-name">${esc(example.name)}</span></span>
        <span class="ex-desc">${esc(example.description || example.name)}</span>
        <code class="ex-code">${highlight(example.code.split("\n").slice(0, 6).join("\n"))}</code>
        <span class="try">Load and run →</span>`;
      card.addEventListener("click", () => {
        loadExample(example.name, { scroll: true });
        source.focus({ preventScroll: true });
      });
      gridEl.appendChild(card);
    }
    document.getElementById("example-range").textContent = matches.length
      ? `${start + 1}–${Math.min(start + PAGE_SIZE, matches.length)} of ${matches.length}`
      : "No matching examples. Try another name or category.";
    previous.disabled = pageIndex === 0;
    next.disabled = start + PAGE_SIZE >= matches.length;
  }
  for (const cat of ["all", ...new Set(EXAMPLES.map(example => example.category))]) {
    const button = document.createElement("button");
    button.className = "chip" + (cat === "all" ? " active" : "");
    button.textContent = cat === "all" ? "All" : cat;
    button.setAttribute("aria-pressed", String(cat === category));
    button.addEventListener("click", () => {
      category = cat; pageIndex = 0;
      for (const chip of chipsEl.children) {
        chip.classList.toggle("active", chip === button);
        chip.setAttribute("aria-pressed", String(chip === button));
      }
      renderGallery();
    });
    chipsEl.appendChild(button);
  }
  searchEl.addEventListener("input", () => { pageIndex = 0; renderGallery(); });
  previous.addEventListener("click", () => { pageIndex--; renderGallery(); });
  next.addEventListener("click", () => { pageIndex++; renderGallery(); });
  library.addEventListener("toggle", renderGallery);
  function openLegacyCollection() {
    if (location.hash === "#examples") { library.open = true; renderGallery(); }
  }
  window.addEventListener("hashchange", openLegacyCollection);
  openLegacyCollection();

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
      if (saved !== null) return saved;
    } catch {}
    return byName[defaultExample].code;
  }
  document.getElementById("playground-static-note").hidden = true;
  source.readOnly = false;
  source.classList.add("highlighted");
  // When a restored draft happens to be an unchanged example, remember it,
  // so Reset ("restore the current example") means what its label says.
  const bootSource = initialSource();
  source.value = bootSource;
  const bootedExample = EXAMPLES.find((e) => e.code === bootSource);
  if (bootedExample) currentExample = bootedExample.name;
  syncEditor();
  updateExampleContext();

  status.textContent = "loading…";
  output.textContent = "Loading the browser interpreter…";
  async function loadEngine() {
    try {
      const engine = await import("./pkg/code_wasm.js");
      await engine.default();
      runEngine = engine.run_with_modules;
      engineState = "ready";
      status.textContent = "";
      runBtn.disabled = false;
      liveChk.disabled = false;
      doRun();
    } catch (error) {
      engineState = "failed";
      status.textContent = "failed to load engine";
      status.classList.add("err");
      output.textContent = "error: Could not load the browser interpreter. Check your connection and reload the page.\n" + error;
      output.classList.add("error");
      copyOutBtn.disabled = false;
    }
  }
  loadEngine();
