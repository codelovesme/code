// The page's half of `dom`: a tree of nodes, and events fired back.
//
// The whole vocabulary is a tag, flat attributes, children and `on`. There is
// no `innerHTML`, no property set by name, no handler built from text — so a
// tree built out of someone's name is data all the way here and cannot become
// code on the way. The module's own half is held to the same rule.
(ctx) => {
  const { doc, str, fire } = ctx;

  /// `"Add"` or `{ _class: "Add", ... }` — both are how an application says
  /// what an event means, and anything else is not one.
  function asParticle(wanted) {
    if (typeof wanted === "string") return wanted ? { _class: wanted } : null;
    if (wanted && typeof wanted === "object" && typeof wanted._class === "string") {
      return wanted;
    }
    return null;
  }

  // What an event carries, as one piece of text: what the reader typed, or
  // what the application wrote on the element. Anything else carries nothing
  // and says so, so a plain button makes a particle with no `value` field.
  const VALUED = new Set(["input", "select", "textarea"]);
  function eventValue(el) {
    const tag = String(el.tagName || "").toLowerCase();
    if (VALUED.has(tag)) return String(el.value ?? "");
    if (el.hasAttribute && el.hasAttribute("value")) return String(el.getAttribute("value"));
    return null;
  }

  function node(spec) {
    if (typeof spec === "string") return doc.createTextNode(spec);
    if (spec === null || typeof spec !== "object" || Array.isArray(spec)) {
      return doc.createTextNode(String(spec));
    }
    const el = doc.createElement(spec.tag || "div");
    for (const [k, v] of Object.entries(spec.attrs || {})) {
      if (/^on/i.test(k)) continue; // never an event handler
      el.setAttribute(k, String(v));
    }
    // `on` maps an event name to the *particle* the application wants back —
    // a whole one, written in the tree, or just its class when there is
    // nothing else to say. Nothing is registered and nothing is held: it
    // travels out in the payload and comes back in when the event happens.
    for (const [event, wanted] of Object.entries(spec.on || {})) {
      const particle = asParticle(wanted);
      if (!particle) continue;
      el.addEventListener(event, (e) => {
        const value = eventValue(e.target || el);
        // What the element holds, added only when the application did not
        // say it itself — an `on` that names `value` means that value.
        fire(value === null || "value" in particle ? particle : { ...particle, value });
      });
    }
    for (const child of spec.children || []) el.appendChild(node(child));
    return el;
  }

  // `styles` arrives as selector -> property -> value, so there is no CSS to
  // parse. Braces and angle brackets are dropped anyway: nothing built here
  // may end a rule early and start a different one.
  const clean = (s) => String(s).replace(/[{}<>]/g, "");
  function sheetText(styles) {
    if (!styles || typeof styles !== "object") return "";
    return Object.entries(styles)
      .map(([sel, props]) => {
        const body = Object.entries(props || {})
          .map(([k, v]) => `  ${clean(k)}: ${clean(v)};`)
          .join("\n");
        return `${clean(sel)} {\n${body}\n}`;
      })
      .join("\n");
  }

  return {
    code_web_render(jsonPtr, jsonLen, intoPtr, intoLen) {
      const target = doc.querySelector(str(intoPtr, intoLen));
      if (!target) return 0;
      const payload = JSON.parse(str(jsonPtr, jsonLen));
      if (payload.styles) {
        // One sheet per page, replaced rather than stacked: an application
        // restyling itself should not leave its old rules behind.
        let sheet = doc.getElementById("code-style");
        if (!sheet) {
          sheet = doc.createElement("style");
          sheet.id = "code-style";
          (doc.head || doc.body).appendChild(sheet);
        }
        sheet.textContent = sheetText(payload.styles);
      }
      target.replaceChildren(node(payload.tree));
      return 1;
    },
  };
}
