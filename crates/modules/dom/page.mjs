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

  // SVG needs its own namespace when it is created through the DOM API. A
  // plain `createElement("svg")` looks like an element in the HTML namespace
  // and its paths render as an empty box in browsers.
  const SVG_NAMESPACE = "http://www.w3.org/2000/svg";
  const SVG_TAGS = new Set([
    "svg", "path", "circle", "ellipse", "line", "polyline", "polygon",
    "rect", "g", "defs", "clipPath", "mask", "use", "symbol", "title", "desc",
  ]);

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
      let suppressClick = false;
      // A swipe can finish over a child button. Browsers may still synthesize
      // a click for that release; consume that one click so a swipe action
      // cannot also activate the button underneath the finger.
      el.addEventListener("click", (e) => {
        if (!suppressClick) return;
        suppressClick = false;
        e.preventDefault();
        e.stopImmediatePropagation();
      }, true);
      el.addEventListener("pointerdown", (e) => {
        // A later deliberate press is a fresh gesture. This also means that
        // on browsers which do not synthesize a click after a swipe, the
        // visible confirmation button remains usable on its next tap.
        suppressClick = false;
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
        if ((name === "swipeleft") === (dx < 0)) {
          suppressClick = true;
          fire(meant(el, particle));
        }
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
    const tag = spec.tag || "div";
    const isSvg = SVG_TAGS.has(String(tag));
    const el = isSvg && typeof doc.createElementNS === "function"
      ? doc.createElementNS(SVG_NAMESPACE, tag)
      : doc.createElement(tag);
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

  // Where the caret is, as a path of child positions under `root`, with the
  // node's kind and its selection — or null when it is not under `root`.
  function caretPath(root) {
    const active = doc.activeElement;
    if (!active || active === root || !(root.contains && root.contains(active))) return null;
    const path = [];
    let n = active;
    while (n && n !== root) {
      const parent = n.parentNode;
      if (!parent) return null;
      path.unshift(Array.prototype.indexOf.call(parent.children, n));
      n = parent;
    }
    const sel = typeof active.selectionStart === "number"
      ? { start: active.selectionStart, end: active.selectionEnd }
      : null;
    return { path, tag: active.tagName, sel };
  }

  function giveBack(root, was) {
    let n = root;
    for (const i of was.path) {
      n = n.children && n.children[i];
      if (!n) return;
    }
    if (n.tagName !== was.tag || typeof n.focus !== "function") return;
    n.focus();
    if (was.sel && typeof n.setSelectionRange === "function") {
      try { n.setSelectionRange(was.sel.start, was.sel.end); } catch { /* a box that has no caret */ }
    }
  }

  // A page-level dialog is one focus scope. The tree is redrawn on every
  // particle, so the element that opened it may be replaced along with the
  // old tree; keep the element and a small hint for finding its replacement.
  const MODAL_SELECTOR = 'dialog[aria-modal="true"][open]';
  const FOCUSABLE_SELECTOR = 'button, input, textarea, select, a[href], [tabindex]:not([tabindex="-1"])';
  let modalFocus = { dialog: null, opener: null, hint: null };

  function openModal() {
    if (typeof doc.querySelectorAll === "function") {
      const all = Array.from(doc.querySelectorAll(MODAL_SELECTOR));
      return all.length ? all[all.length - 1] : null;
    }
    return typeof doc.querySelector === "function" ? doc.querySelector(MODAL_SELECTOR) : null;
  }

  function modalControls(dialog) {
    if (!dialog || typeof dialog.querySelectorAll !== "function") return [];
    return Array.from(dialog.querySelectorAll(FOCUSABLE_SELECTOR)).filter((el) => {
      if (!el || el.disabled || (el.getAttribute && el.getAttribute("aria-hidden") === "true")) return false;
      if (el.hidden || (el.getAttribute && el.getAttribute("hidden") !== null)) return false;
      if (el.getAttribute && String(el.getAttribute("type") || "").toLowerCase() === "hidden") return false;
      if (typeof el.getClientRects === "function" && el.getClientRects().length === 0) return false;
      return typeof el.focus === "function";
    });
  }

  function openerHint(el) {
    if (!el) return null;
    const id = el.id || (el.getAttribute && el.getAttribute("id"));
    const klass = el.className || (el.getAttribute && el.getAttribute("class")) || "";
    const key = el.getAttribute && el.getAttribute("data-focus-key");
    return { id: id ? String(id) : "", className: String(klass), key: key ? String(key) : "" };
  }

  function focusOpener(target, opener, hint) {
    if (opener && opener.isConnected !== false && typeof opener.focus === "function") {
      opener.focus();
      return;
    }
    // Render creates a fresh node for the opener. Keep the common, important
    // case (the bottom add button) accessible after that replacement.
    if (hint && hint.className.includes("fab") && target.querySelector) {
      const replacement = target.querySelector(".fab");
      if (replacement && typeof replacement.focus === "function") {
        replacement.focus();
        return;
      }
    }
    if (hint && hint.key && typeof target.querySelectorAll === "function") {
      const replacement = Array.from(target.querySelectorAll("[data-focus-key]"))
        .find((el) => el.getAttribute && el.getAttribute("data-focus-key") === hint.key);
      if (replacement && typeof replacement.focus === "function") {
        replacement.focus();
        return;
      }
    }
    if (hint && hint.id && typeof doc.getElementById === "function") {
      const replacement = doc.getElementById(hint.id);
      if (replacement && typeof replacement.focus === "function") {
        replacement.focus();
        return;
      }
    }
    if (doc.body && typeof doc.body.focus === "function") doc.body.focus();
  }

  // Capture before the application handles the keydown. This works for
  // controls created by the tree and for controls supplied by the shell.
  if (doc && typeof doc.addEventListener === "function") {
    doc.addEventListener("keydown", (e) => {
      if (!e || e.key !== "Tab") return;
      const dialog = openModal();
      if (!dialog) return;
      const controls = modalControls(dialog);
      if (!controls.length) {
        if (e.preventDefault) e.preventDefault();
        if (typeof dialog.focus === "function") dialog.focus();
        return;
      }
      const current = doc.activeElement;
      const index = controls.indexOf(current);
      if (e.shiftKey) {
        if (index <= 0) {
          if (e.preventDefault) e.preventDefault();
          controls[controls.length - 1].focus();
        }
      } else if (index < 0 || index === controls.length - 1) {
        if (e.preventDefault) e.preventDefault();
        controls[0].focus();
      }
    }, true);
  }

  return [
    "dom",
    (particle) => {
      if (particle._class === "Download") {
        const uri = typeof particle.uri === "string" ? particle.uri : "";
        const name = typeof particle.name === "string" && particle.name ? particle.name : "attachment";
        const parent = doc.body || doc.documentElement;
        if (!uri || !parent || typeof doc.createElement !== "function") {
          return { _class: "DownloadResult", ok: false };
        }

        // A data/blob URI has already been fetched by the application. A
        // temporary anchor keeps the download in the same user gesture that
        // caused this particle, then disappears before the next render.
        const link = doc.createElement("a");
        link.setAttribute("href", uri);
        link.setAttribute("download", name);
        link.setAttribute("rel", "noopener");
        if (typeof link.click !== "function") {
          return { _class: "DownloadResult", ok: false };
        }
        parent.appendChild(link);
        link.click();
        if (typeof parent.removeChild === "function") parent.removeChild(link);
        else if (typeof link.remove === "function") link.remove();
        return { _class: "DownloadResult", ok: true };
      }
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
      // Every render is a new tree, and the node the reader was typing in
      // goes with the old one. So where the caret was is remembered as a
      // path of child positions from the root, and after the render the
      // node at the same path — the same kind of node — gets it back,
      // caret and all. A redraw while someone types is then nothing they
      // notice, which is what lets an application redraw whenever it likes.
      const was = caretPath(target);
      const activeBefore = doc.activeElement;
      const oldModal = openModal();
      target.replaceChildren(node(tree));
      const nextModal = openModal();
      if (nextModal && !modalFocus.dialog) {
        const opener = activeBefore && (!oldModal || !(oldModal.contains && oldModal.contains(activeBefore)))
          ? activeBefore
          : null;
        modalFocus = { dialog: nextModal, opener, hint: openerHint(opener) };
      } else if (nextModal) {
        modalFocus.dialog = nextModal;
      }
      // A node the tree marked `autofocus` is focused now that it is on the
      // page, ahead of the caret coming back: the application said so for
      // this render, and says nothing on the ones after.
      const wanted = target.querySelector && target.querySelector("[autofocus]");
      if (wanted && typeof wanted.focus === "function") wanted.focus();
      else if (was) giveBack(target, was);
      if (!nextModal && modalFocus.dialog) {
        const previous = modalFocus;
        modalFocus = { dialog: null, opener: null, hint: null };
        focusOpener(target, previous.opener, previous.hint);
      }
      return { _class: "RenderResult", ok: true };
    },
  ];
}
