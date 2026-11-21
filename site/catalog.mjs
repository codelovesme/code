// The module catalog: every module of the newest release, from the
// modules-index.json the release itself published (copied in when the site
// is built). Nothing is asked of anyone while the page is open.

const query = document.getElementById("catalog-query");
const list = document.getElementById("catalog-list");
const detail = document.getElementById("catalog-detail");
const count = document.getElementById("catalog-count");
const filters = [...document.querySelectorAll(".catalog-filters button")];

let index = { version: "", modules: [] };
let filter = "all";
let chosen = null;

const el = (tag, attrs = {}, ...children) => {
  const node = document.createElement(tag);
  for (const [key, value] of Object.entries(attrs)) {
    if (key === "class") node.className = value;
    else if (key.startsWith("on")) node.addEventListener(key.slice(2), value);
    else node.setAttribute(key, value);
  }
  for (const child of children.flat()) {
    if (child !== null && child !== undefined && child !== false) node.append(child);
  }
  return node;
};

const where = (m) => {
  if (m.test_double) return "Test double";
  if (m.runs === "browser") return "Browser";
  if (m.runs === "both") return "Machine and browser";
  return "Machine";
};

// A README's Markdown, drawn with the few shapes module READMEs use:
// headings, fenced code, lists, tables as text, paragraphs, `code`,
// **bold**, [links](…). Built from nodes, never from HTML strings.
const inline = (text) => {
  const out = [];
  const pattern = /(`[^`]+`|\*\*[^*]+\*\*|\[[^\]]+\]\([^)]+\))/g;
  let last = 0;
  for (const found of text.matchAll(pattern)) {
    if (found.index > last) out.push(text.slice(last, found.index));
    const piece = found[0];
    if (piece.startsWith("`")) out.push(el("code", {}, piece.slice(1, -1)));
    else if (piece.startsWith("**")) out.push(el("strong", {}, piece.slice(2, -2)));
    else {
      const [, label, href] = piece.match(/\[([^\]]+)\]\(([^)]+)\)/);
      const safe = /^(https?:|#|\.{0,2}\/)/.test(href) ? href : null;
      out.push(safe ? el("a", { href: safe }, label) : label);
    }
    last = found.index + piece.length;
  }
  if (last < text.length) out.push(text.slice(last));
  return out;
};

const markdown = (source) => {
  const root = el("div", { class: "catalog-readme" });
  const lines = source.split("\n");
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    if (line.startsWith("```")) {
      const code = [];
      i++;
      while (i < lines.length && !lines[i].startsWith("```")) code.push(lines[i++]);
      i++;
      root.append(el("pre", { class: "codeblock" }, el("code", {}, code.join("\n"))));
      continue;
    }
    const heading = line.match(/^(#{1,4})\s+(.*)$/);
    if (heading) {
      // The README's own title is the detail's heading already.
      const level = Math.min(6, heading[1].length + 2);
      root.append(el("h" + level, {}, inline(heading[2])));
      i++;
      continue;
    }
    if (/^\s*[-*]\s+/.test(line)) {
      const items = [];
      while (i < lines.length && /^\s*[-*]\s+/.test(lines[i])) {
        let item = lines[i++].replace(/^\s*[-*]\s+/, "");
        while (i < lines.length && /^\s{2,}\S/.test(lines[i]) && !/^\s*[-*]\s+/.test(lines[i])) item += " " + lines[i++].trim();
        items.push(el("li", {}, inline(item)));
      }
      root.append(el("ul", {}, items));
      continue;
    }
    if (line.startsWith("|")) {
      const rows = [];
      while (i < lines.length && lines[i].startsWith("|")) rows.push(lines[i++]);
      root.append(el("pre", { class: "codeblock" }, el("code", {}, rows.join("\n"))));
      continue;
    }
    if (line.trim() === "") {
      i++;
      continue;
    }
    const para = [];
    while (i < lines.length && lines[i].trim() !== "" && !/^(#{1,4}\s|```|\s*[-*]\s|\|)/.test(lines[i])) para.push(lines[i++]);
    root.append(el("p", {}, inline(para.join(" "))));
  }
  return root;
};

const copyButton = (text) =>
  el("button", {
    type: "button",
    class: "catalog-copy",
    onclick: async (e) => {
      try {
        await navigator.clipboard.writeText(text);
        e.target.textContent = "Copied";
      } catch {
        e.target.textContent = "Select and copy";
      }
      setTimeout(() => (e.target.textContent = "Copy"), 1500);
    },
  }, "Copy");

const showDetail = (m) => {
  chosen = m.name;
  detail.hidden = false;
  detail.replaceChildren();
  const browserOnly = m.runs === "browser";
  const install = browserOnly
    ? `"$code_cli" install ${m.name} --platform wasm32`
    : `"$code_cli" install ${m.name}`;
  const link = browserOnly ? `link "${m.name}.a" as ${m.name}` : `link "${m.name}.so" as ${m.name}`;
  detail.append(...[
    el("h2", {}, m.name),
    el("p", { class: "catalog-lede" }, inline(m.description || "")),
    m.runs === "both" ? el("p", { class: "catalog-note" }, "On a machine install as below; for a browser build add ", el("code", {}, "--platform wasm32"), " and link the ", el("code", {}, ".a"), ".") : null,
    el("dl", { class: "catalog-facts" },
      el("dt", {}, "Version"), el("dd", {}, m.version),
      el("dt", {}, "Runs"), el("dd", {}, where(m)),
      el("dt", {}, "Platforms"), el("dd", {}, m.platforms.join(", ")),
      m.setup ? [el("dt", {}, "First send"), el("dd", {}, el("code", {}, m.setup))] : null,
    ),
    el("h3", {}, "Install"),
    el("div", { class: "catalog-command" }, el("pre", { class: "codeblock" }, el("code", {}, install)), copyButton(install)),
    el("div", { class: "catalog-command" }, el("pre", { class: "codeblock" }, el("code", {}, link)), copyButton(link)),
    el("h3", {}, `Handlers (${m.handlers.length})`),
    el("ul", { class: "catalog-handlers" }, m.handlers.map((h) => el("li", {}, el("code", {}, h)))),
    m.readme ? [el("h3", {}, "README"), markdown(m.readme.replace(/^#[^\n]*\n/, ""))] : null,
    el("p", { class: "catalog-source" },
      el("a", { href: `https://github.com/codelovesme/code/tree/v${m.version}/crates/modules/${m.name}` }, "Source on GitHub"),
      " · ",
      el("a", { href: `https://github.com/codelovesme/code/releases/tag/v${m.version}` }, `Release v${m.version}`),
    ),
  ].flat().filter(Boolean));
  for (const item of list.querySelectorAll("button")) item.setAttribute("aria-current", item.dataset.name === m.name ? "true" : "false");
  try { history.replaceState(null, "", "#" + m.name); } catch {}
  if (window.matchMedia("(max-width: 860px)").matches) detail.scrollIntoView({ behavior: "smooth", block: "start" });
};

const matches = (m, words) => {
  if (filter === "machine" && m.runs === "browser") return false;
  if (filter === "machine" && m.test_double) return false;
  if (filter === "browser" && m.runs === "machine") return false;
  if (filter === "test" && !m.test_double) return false;
  const hay = (m.name + " " + m.description + " " + m.handlers.join(" ")).toLowerCase();
  return words.every((w) => hay.includes(w));
};

const draw = () => {
  const words = query.value.toLowerCase().split(/\s+/).filter(Boolean);
  const shown = index.modules.filter((m) => matches(m, words));
  list.replaceChildren(
    ...shown.map((m) =>
      el("li", {},
        el("button", { type: "button", "data-name": m.name, "aria-current": m.name === chosen ? "true" : "false", onclick: () => showDetail(m) },
          el("span", { class: "catalog-name" }, m.name),
          el("span", { class: "catalog-kind" }, where(m)),
          el("span", { class: "catalog-desc" }, inline(m.description || "")),
        ),
      ),
    ),
  );
  count.textContent = shown.length === index.modules.length
    ? `${shown.length} modules in Code v${index.version}`
    : `${shown.length} of ${index.modules.length} modules in Code v${index.version}`;
  if (shown.length === 0) list.append(el("li", { class: "catalog-empty" }, "No module matches. Try a handler name, like Send or Parse."));
};

query.addEventListener("input", draw);
for (const button of filters) {
  button.addEventListener("click", () => {
    filter = button.dataset.filter;
    for (const other of filters) other.setAttribute("aria-pressed", other === button ? "true" : "false");
    draw();
  });
}

// `catalog.html#net_client` opens that module, on arrival and when the
// address changes to another one.
const openAsked = () => {
  const asked = decodeURIComponent(location.hash.slice(1));
  const found = index.modules.find((m) => m.name === asked);
  if (found && found.name !== chosen) showDetail(found);
};
window.addEventListener("hashchange", openAsked);

fetch("modules-index.json")
  .then((r) => (r.ok ? r.json() : Promise.reject(new Error(String(r.status)))))
  .then((loaded) => {
    index = loaded;
    draw();
    openAsked();
  })
  .catch(() => {
    count.textContent = "The catalog could not be loaded. Every module is also listed on the release page on GitHub.";
  });
