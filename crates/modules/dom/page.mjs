(ctx) => {
  const { doc, fire } = ctx;

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

  // A file box: the one element whose value is not what it holds.
  const isFileBox = (el) =>
    String(el.tagName || "").toLowerCase() === "input" &&
    String(el.getAttribute && el.getAttribute("type") || "").toLowerCase() === "file";
  // How much of a file is carried into the program. Base64 makes it a third
  // larger again, and it crosses as one event.
  const FILE_CAP = 12 * 1024 * 1024;
  const FILE_CAP_SAID = "12 MB";

  // The particle a gesture on `el` means, with what the element holds — the
  // same rule an ordinary event follows.
  const meant = (el, particle) => {
    const value = eventValue(el);
    return value === null || "value" in particle ? particle : { ...particle, value };
  };

  // Names in `on` the page reads as gestures rather than forwarding as
  // events. Nothing is held between renders here either: what a gesture
  // needs to remember lives on the node, and goes when the node does.
  const GESTURES = new Set(["swipeleft", "swiperight", "doubletap", "reorder"]);

  // How far a finger goes before it is a swipe and not a tap, how long it
  // may take, how much slower the other axis must be, and how close two
  // taps are before they are one double.
  const SWIPE_PX = 40;
  const SWIPE_MS = 800;
  const TAP_PX = 10;
  const DOUBLE_MS = 350;
  const DRAG_PX = 8;

  function gesture(el, name, particle) {
    if (name === "swipeleft" || name === "swiperight") {
      // A press, and a release far enough along the one axis, soon enough.
      // `pointercancel` is the browser taking the pointer for a scroll — a
      // list that wants a swipe across it says `touch-action: pan-y`.
      let start = null;
      el.addEventListener("pointerdown", (e) => {
        start = { x: e.clientX, y: e.clientY, t: e.timeStamp, id: e.pointerId };
      });
      el.addEventListener("pointercancel", () => {
        start = null;
      });
      el.addEventListener("pointerup", (e) => {
        if (!start || e.pointerId !== start.id) return;
        const dx = e.clientX - start.x;
        const dy = e.clientY - start.y;
        const dt = e.timeStamp - start.t;
        start = null;
        if (dt > SWIPE_MS || Math.abs(dx) < SWIPE_PX || Math.abs(dx) < Math.abs(dy) * 2) return;
        if ((name === "swipeleft") === (dx < 0)) fire(meant(el, particle));
      });
      return;
    }
    if (name === "doubletap") {
      // Two releases close together, neither having travelled: a swipe that
      // ends where a tap was is not half of a double.
      let last = null;
      let down = null;
      el.addEventListener("pointerdown", (e) => {
        down = { x: e.clientX, y: e.clientY };
      });
      el.addEventListener("pointerup", (e) => {
        const from = down;
        down = null;
        if (!from || Math.abs(e.clientX - from.x) > TAP_PX || Math.abs(e.clientY - from.y) > TAP_PX) {
          last = null;
          return;
        }
        if (last !== null && e.timeStamp - last < DOUBLE_MS) {
          last = null;
          fire(meant(el, particle));
        } else {
          last = e.timeStamp;
        }
      });
      return;
    }
    if (name === "reorder") {
      // On a container: a child pressed and carried among its siblings. What
      // is sent is where it was and where it was let go — `from` and `at`,
      // positions among the children (`to` is a word the language keeps for
      // itself) — and nothing is moved here: the
      // application holds the list, so the application reorders it and
      // draws. While it is carried the child wears `data-code-dragging`, for
      // the application's own styles to pick up.
      let drag = null;
      const childAt = (target) => {
        let n = target;
        while (n && n.parentNode !== el) n = n.parentNode;
        return n && n.nodeType === 1 ? n : null;
      };
      const indexOf = (child) => Array.prototype.indexOf.call(el.children, child);
      // Where the child would land: how many of the *others* have their
      // middle above the pointer. That is its index once it is put back.
      const slotAt = (child, y) => {
        let slot = 0;
        for (const other of el.children) {
          if (other === child) continue;
          const r = other.getBoundingClientRect();
          if (y > r.top + r.height / 2) slot += 1;
        }
        return slot;
      };
      const letGo = () => {
        if (drag && drag.child.removeAttribute) drag.child.removeAttribute("data-code-dragging");
        drag = null;
      };
      el.addEventListener("pointerdown", (e) => {
        const child = childAt(e.target);
        if (!child) return;
        drag = { child, from: indexOf(child), id: e.pointerId, x: e.clientX, y: e.clientY, moved: false };
      });
      el.addEventListener("pointermove", (e) => {
        if (!drag || e.pointerId !== drag.id || drag.moved) return;
        const dx = Math.abs(e.clientX - drag.x);
        const dy = Math.abs(e.clientY - drag.y);
        if (dx + dy < DRAG_PX) return;
        // A list is reordered up and down. A press that sets off sideways is
        // something else — a swipe on the child, most likely — and taking the
        // pointer here would be the end of it, since a captured pointer's
        // release never reaches the child. So it is left alone.
        if (dx > dy) {
          drag = null;
          return;
        }
        drag.moved = true;
        drag.child.setAttribute("data-code-dragging", "");
        // The pointer stays this container's until it is released, so a
        // finger that leaves the list still ends the drag here.
        try {
          el.setPointerCapture(e.pointerId);
        } catch {
          // A page without pointer capture still gets the drag when the
          // pointer is let go over the list.
        }
      });
      el.addEventListener("pointercancel", letGo);
      el.addEventListener("pointerup", (e) => {
        if (!drag || e.pointerId !== drag.id) return;
        const carried = drag;
        letGo();
        if (!carried.moved) return;
        const at = slotAt(carried.child, e.clientY);
        if (at === carried.from) return;
        fire({ ...particle, from: carried.from, at });
      });
    }
  }

  function node(spec) {
    if (typeof spec === "string") return doc.createTextNode(spec);
    if (spec === null || typeof spec !== "object" || Array.isArray(spec)) {
      return doc.createTextNode(String(spec));
    }
    const el = doc.createElement(spec.tag || "div");
    // `media = "camera"` marks a node as somewhere a device may show itself.
    // It becomes a plain attribute and nothing more: this module does not
    // know what a camera is, and the tree stays data. Whatever owns the
    // device finds the node by this attribute and fills it in — a live
    // `MediaStream` is a property, and properties are what a tree cannot
    // carry, which is the whole reason the mark exists.
    if (typeof spec.media === "string" && spec.media) {
      el.setAttribute("data-code-media", spec.media);
    }
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
      // A gesture is several events read as one; the page reads them, since
      // a program should not be woken for every point a finger passes.
      if (GESTURES.has(event)) {
        gesture(el, event, particle);
        continue;
      }
      el.addEventListener(event, (e) => {
        const target = e.target || el;
        // A file box holds a file, and its `value` is a path the browser
        // made up. What the application wants is the bytes, so they are
        // read and sent as `file` — name, type, size, and the data as
        // base64 — with `value` the file's name, the way every other box
        // carries text. Too large to carry, and the data is empty and
        // `refused` says why: the program is told, never left waiting.
        if (isFileBox(target)) {
          const chosen = target.files && target.files[0];
          if (!chosen) return;
          const said = { name: chosen.name, type: chosen.type || "", size: chosen.size };
          if (chosen.size > FILE_CAP) {
            fire({ ...particle, value: chosen.name, file: { ...said, data_base64: "", refused: `larger than ${FILE_CAP_SAID}` } });
            return;
          }
          const reader = new FileReader();
          reader.onload = () => {
            const url = String(reader.result || "");
            const data_base64 = url.slice(url.indexOf(",") + 1);
            fire({ ...particle, value: chosen.name, file: { ...said, data_base64 } });
          };
          reader.onerror = () => {
            fire({ ...particle, value: chosen.name, file: { ...said, data_base64: "", refused: "could not be read" } });
          };
          reader.readAsDataURL(chosen);
          return;
        }
        const value = eventValue(target);
        // What the element holds, added only when the application did not
        // say it itself — an `on` that names `value` means that value.
        let sent = value === null || "value" in particle ? particle : { ...particle, value };
        // A key event says which key, since "a key was pressed" is never
        // what an application wanted to know.
        if (typeof e.key === "string" && !("key" in sent)) sent = { ...sent, key: e.key };
        fire(sent);
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

  return [
    "dom",
    (particle) => {
      if (particle._class !== "Render") return null;

      const into = typeof particle.into === "string" ? particle.into : "body";
      const target = doc.querySelector(into);
      // Not an exception: a selector that matches nothing is an application
      // drawing before its page has the node, which it can act on.
      if (!target) return { _class: "RenderResult", ok: false };

      if (particle.styles) {
        // One sheet per page, replaced rather than stacked: an application
        // restyling itself should not leave its old rules behind.
        let sheet = doc.getElementById("code-style");
        if (!sheet) {
          sheet = doc.createElement("style");
          sheet.id = "code-style";
          (doc.head || doc.body).appendChild(sheet);
        }
        sheet.textContent = sheetText(particle.styles);
      }

      // A tree already written as JSON is taken as one — for an application
      // that built the text itself rather than handing over a value.
      const tree = typeof particle.tree === "string" ? JSON.parse(particle.tree) : particle.tree;
      target.replaceChildren(node(tree));
      // A node the tree marked `autofocus` is focused now that it is on the
      // page. Every render is a new tree, so the application says which
      // render: leaving the mark on would pull the caret back on each redraw.
      const wanted = target.querySelector && target.querySelector("[autofocus]");
      if (wanted && typeof wanted.focus === "function") wanted.focus();
      return { _class: "RenderResult", ok: true };
    },
  ];
}
