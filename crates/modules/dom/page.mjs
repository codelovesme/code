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

  // A file goes to the application as `file` — name, type, size and the
  // bytes as base64 — with `value` its name. Too large to carry, or
  // unreadable, the data is empty and `refused` says why.
  // What a JPEG says about itself: when it was taken (the camera's wall
  // clock, and its offset when the camera wrote one) and where (GPS). Read
  // from the Exif block, only the few tags a person's memory needs; any
  // other file, or a photo without them, says nothing rather than guessing.
  const photoFacts = (bytes) => {
    try {
      const v = new DataView(bytes);
      if (v.byteLength < 4 || v.getUint16(0) !== 0xffd8) return null;
      let at = 2;
      while (at + 4 <= v.byteLength) {
        if (v.getUint8(at) !== 0xff) return null;
        const marker = v.getUint8(at + 1);
        const length = v.getUint16(at + 2);
        if (marker === 0xe1 && at + 10 <= v.byteLength && v.getUint32(at + 4) === 0x45786966) {
          return exifFacts(v, at + 10, Math.min(v.byteLength, at + 2 + length));
        }
        if (marker === 0xda || marker === 0xd9) return null;
        at += 2 + length;
      }
    } catch {
      // A broken header is a photo with nothing to say.
    }
    return null;
  };
  const exifFacts = (v, tiff, end) => {
    const little = v.getUint16(tiff) === 0x4949;
    const u16 = (o) => v.getUint16(o, little);
    const u32 = (o) => v.getUint32(o, little);
    const entries = (ifd) => {
      const found = new Map();
      if (!ifd || tiff + ifd + 2 > end) return found;
      const count = u16(tiff + ifd);
      for (let i = 0; i < count; i++) {
        const e = tiff + ifd + 2 + i * 12;
        if (e + 12 > end) break;
        found.set(u16(e), { type: u16(e + 2), count: u32(e + 4), at: e + 8 });
      }
      return found;
    };
    const text = (entry) => {
      if (!entry || entry.type !== 2) return null;
      const from = entry.count > 4 ? tiff + u32(entry.at) : entry.at;
      let s = "";
      for (let i = 0; i < entry.count && from + i < end; i++) {
        const c = v.getUint8(from + i);
        if (c === 0) break;
        s += String.fromCharCode(c);
      }
      return s;
    };
    const rationals = (entry) => {
      if (!entry || entry.type !== 5) return null;
      const from = tiff + u32(entry.at);
      const out = [];
      for (let i = 0; i < entry.count; i++) {
        const o = from + i * 8;
        if (o + 8 > end) return null;
        const d = u32(o + 4);
        out.push(d === 0 ? 0 : u32(o) / d);
      }
      return out;
    };
    const top = entries(u32(tiff + 4));
    const pointer = (tag) => (top.get(tag) ? u32(top.get(tag).at) : 0);
    const exif = entries(pointer(0x8769));
    const gps = entries(pointer(0x8825));
    const facts = {};
    const taken = text(exif.get(0x9003)) || text(top.get(0x0132));
    const shape = /^(\d{4}):(\d{2}):(\d{2}) (\d{2}):(\d{2}):(\d{2})$/.exec(taken || "");
    if (shape && shape[1] !== "0000") {
      facts.taken_local = `${shape[1]}-${shape[2]}-${shape[3]} ${shape[4]}:${shape[5]}:${shape[6]}`;
      const zone = /^([+-])(\d{2}):(\d{2})$/.exec(text(exif.get(0x9011)) || "");
      facts.offset = zone ? (zone[1] === "-" ? -1 : 1) * (Number(zone[2]) * 60 + Number(zone[3])) : null;
    }
    const lat = rationals(gps.get(2));
    const lon = rationals(gps.get(4));
    if (lat && lon && lat.length === 3 && lon.length === 3) {
      const deg = (p) => p[0] + p[1] / 60 + p[2] / 3600;
      const la = deg(lat) * (text(gps.get(1)) === "S" ? -1 : 1);
      const lo = deg(lon) * (text(gps.get(3)) === "W" ? -1 : 1);
      if (Number.isFinite(la) && Number.isFinite(lo) && !(la === 0 && lo === 0) && Math.abs(la) <= 90 && Math.abs(lo) <= 180) {
        facts.lat = Math.round(la * 1e6) / 1e6;
        facts.lon = Math.round(lo * 1e6) / 1e6;
      }
    }
    return Object.keys(facts).length ? facts : null;
  };

  const sendFile = (particle, chosen) => {
    const name = chosen.name || "pasted";
    const said = { name, type: chosen.type || "", size: chosen.size };
    // When the file says it was last changed: the browser's own field, so
    // a pasted image (made just now) says now. Seconds, like every clock here.
    if (Number.isFinite(chosen.lastModified)) said.last_modified = Math.floor(chosen.lastModified / 1000);
    if (chosen.size > FILE_CAP) {
      fire({ ...particle, value: name, file: { ...said, data_base64: "", refused: `larger than ${FILE_CAP_SAID}` } });
      return;
    }
    const reader = new FileReader();
    reader.onload = () => {
      const url = String(reader.result || "");
      const data_base64 = url.slice(url.indexOf(",") + 1);
      if (!/^image\/jpe?g$/i.test(said.type) || typeof chosen.slice !== "function") {
        fire({ ...particle, value: name, file: { ...said, data_base64 } });
        return;
      }
      // A JPEG's Exif block is in its first 128 KB.
      const head = new FileReader();
      head.onload = () => {
        const photo = photoFacts(head.result);
        fire({ ...particle, value: name, file: photo ? { ...said, photo, data_base64 } : { ...said, data_base64 } });
      };
      head.onerror = () => fire({ ...particle, value: name, file: { ...said, data_base64 } });
      head.readAsArrayBuffer(chosen.slice(0, 131072));
    };
    reader.onerror = () => {
      fire({ ...particle, value: name, file: { ...said, data_base64: "", refused: "could not be read" } });
    };
    reader.readAsDataURL(chosen);
  };

  // The first file a paste carries, if it carries one.
  const pastedFile = (e) => {
    const data = e && e.clipboardData;
    if (!data) return null;
    if (data.files && data.files.length) return data.files[0];
    for (const item of Array.from(data.items || [])) {
      if (item && item.kind === "file" && typeof item.getAsFile === "function") {
        const got = item.getAsFile();
        if (got) return got;
      }
    }
    return null;
  };

  // The particle a gesture on `el` means, with what the element holds — the
  // same rule an ordinary event follows.
  const meant = (el, particle) => {
    const value = eventValue(el);
    return value === null || "value" in particle ? particle : { ...particle, value };
  };

  // Names in `on` the page reads as gestures rather than forwarding as
  // events. Nothing is held between renders here either: what a gesture
  // needs to remember lives on the node, and goes when the node does.
  const GESTURES = new Set(["swipeleft", "swiperight", "doubletap", "drag", "edgeclose", "reorder", "longreorder", "viewport"]);

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
  const LONG_REORDER_MS = 450;

  function gesture(el, name, particle) {
    if (name === "viewport") {
      // A map or a timeline: one finger (or a mouse) carries it, two fingers
      // pinch it, a wheel zooms it. While it moves the page moves the first
      // child itself — a program woken for every point would be too slow —
      // and when it is let go one particle says what happened:
      //   { dx, dy, scale, x, y, width, height }
      // `dx`/`dy` the pixels it was carried, `scale` how much bigger it got
      // (0.5 = half), `x`/`y` the point it was zoomed about and `width`/
      // `height` the element's size, all in the element's own pixels. The
      // program redraws at its new view; the page lets go of its own
      // transform shortly after. A press that hardly moved is a tap, left
      // to the child's own `click`.
      const points = new Map();
      let begun = null;
      let moved = false;
      let suppressClick = false;
      let surface = null;
      let view = { dx: 0, dy: 0, scale: 1, x: 0, y: 0 };
      let wheelTimer = null;
      let releaseTimer = null;
      const local = (e) => {
        const box = el.getBoundingClientRect();
        return { x: e.clientX - box.left, y: e.clientY - box.top };
      };
      const both = () => [...points.values()];
      const middle = (ps) => ({ x: (ps[0].x + ps[1].x) / 2, y: (ps[0].y + ps[1].y) / 2 });
      const apart = (ps) => Math.hypot(ps[0].x - ps[1].x, ps[0].y - ps[1].y) || 1;
      const show = () => {
        surface = surface || el.firstElementChild || (el.children && el.children[0]) || null;
        if (!surface || !surface.style) return;
        if (releaseTimer !== null) { clearTimeout(releaseTimer); releaseTimer = null; }
        surface.style.transformOrigin = `${view.x}px ${view.y}px`;
        surface.style.transform = `translate(${view.dx}px, ${view.dy}px) scale(${view.scale})`;
      };
      const letGo = () => {
        const held = surface;
        surface = null;
        if (!held || !held.style) return;
        releaseTimer = setTimeout(() => {
          releaseTimer = null;
          held.style.transform = "";
          held.style.transformOrigin = "";
        }, 250);
      };
      const send = () => {
        const box = el.getBoundingClientRect();
        fire({
          ...particle,
          dx: Math.round(view.dx), dy: Math.round(view.dy),
          scale: Math.round(view.scale * 1000) / 1000,
          x: Math.round(view.x), y: Math.round(view.y),
          width: Math.round(box.width), height: Math.round(box.height),
        });
        letGo();
      };
      const restart = () => {
        const ps = both();
        if (ps.length >= 2) {
          const m = middle(ps);
          begun = { mid: m, apart: apart(ps), view: { ...view } };
        } else if (ps.length === 1) {
          begun = { mid: ps[0], apart: 1, view: { ...view } };
        }
      };
      el.addEventListener("click", (e) => {
        if (!suppressClick) return;
        suppressClick = false;
        e.preventDefault();
        e.stopImmediatePropagation();
      }, true);
      el.addEventListener("pointerdown", (e) => {
        if (e.pointerType === "mouse" && e.button !== 0) return;
        if (points.size === 0) {
          moved = false;
          suppressClick = false;
          const at = local(e);
          view = { dx: 0, dy: 0, scale: 1, x: at.x, y: at.y };
        }
        points.set(e.pointerId, local(e));
        restart();
      });
      const follow = (e) => {
        if (!points.has(e.pointerId) || !begun) return;
        points.set(e.pointerId, local(e));
        const ps = both();
        if (ps.length >= 2) {
          const m = middle(ps);
          const s = Math.max(0.05, Math.min(40, begun.view.scale * apart(ps) / begun.apart));
          view = { ...view, scale: s, x: begun.mid.x - begun.view.dx, y: begun.mid.y - begun.view.dy, dx: begun.view.dx + m.x - begun.mid.x, dy: begun.view.dy + m.y - begun.mid.y };
        } else {
          view = { ...view, dx: begun.view.dx + ps[0].x - begun.mid.x, dy: begun.view.dy + ps[0].y - begun.mid.y };
        }
        if (!moved && (Math.hypot(view.dx, view.dy) > TAP_PX / 2 || Math.abs(view.scale - 1) > 0.02)) {
          moved = true;
          try { el.setPointerCapture(e.pointerId); } catch { /* a release still comes */ }
        }
        if (moved) {
          e.preventDefault();
          show();
        }
      };
      el.addEventListener("pointermove", follow);
      const up = (e, cancelled) => {
        if (!points.has(e.pointerId)) return;
        if (!cancelled) follow(e);
        points.delete(e.pointerId);
        if (points.size > 0) { restart(); return; }
        begun = null;
        if (!moved) return;
        suppressClick = true;
        send();
      };
      el.addEventListener("pointerup", (e) => up(e, false));
      el.addEventListener("pointercancel", (e) => up(e, true));
      el.addEventListener("wheel", (e) => {
        e.preventDefault();
        const at = local(e);
        if (wheelTimer === null) view = { dx: 0, dy: 0, scale: 1, x: at.x, y: at.y };
        const step = Math.exp(-Math.max(-200, Math.min(200, e.deltaY)) * 0.002);
        view = { ...view, scale: Math.max(0.05, Math.min(40, view.scale * step)) };
        show();
        if (wheelTimer !== null) clearTimeout(wheelTimer);
        wheelTimer = setTimeout(() => { wheelTimer = null; send(); }, 180);
      }, { passive: false });
      return;
    }
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
          // A node may also expose the live drag gesture. Mark this release
          // so drag does not commit the same action a second time after the
          // swipe handler has already fired.
          el.__euglenaSwipeHandledPointer = e.pointerId;
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
    if (name === "drag") {
      // A horizontal drag moves the card surface directly in the DOM, then
      // reports one final event. The application decides whether the release
      // crossed its action threshold. Vertical movement remains available to
      // the page for scrolling.
      let start = null;
      let horizontal = false;
      let suppressClick = false;
      let surface = null;
      let surfaceTransition = "";
      let surfaceTransform = "";
      let surfaceTimer = null;
      const restoreSurface = (finalTransform = "0px") => {
        if (!surface) return;
        if (surfaceTimer !== null) clearTimeout(surfaceTimer);
        surface.style.transition = "transform .16s ease";
        surface.style.transform = "translateX(" + finalTransform + ")";
        const released = surface;
        surfaceTimer = setTimeout(() => {
          if (released === surface) {
            released.style.transition = surfaceTransition;
            released.style.transform = surfaceTransform;
            surface = null;
          }
          surfaceTimer = null;
        }, 180);
      };
      el.addEventListener("click", (e) => {
        if (!suppressClick) return;
        suppressClick = false;
        e.preventDefault();
        e.stopImmediatePropagation();
      }, true);
      el.addEventListener("pointerdown", (e) => {
        suppressClick = false;
        if (surfaceTimer !== null) {
          clearTimeout(surfaceTimer);
          if (surface) {
            surface.style.transition = surfaceTransition;
            surface.style.transform = surfaceTransform;
          }
          surfaceTimer = null;
        }
        start = { x: e.clientX, y: e.clientY, id: e.pointerId };
        horizontal = false;
        surface = el.firstElementChild || null;
        if (surface) {
          surfaceTransition = surface.style.transition;
          surfaceTransform = surface.style.transform;
        }
      });
      const cancel = () => {
        restoreSurface();
        start = null;
        horizontal = false;
      };
      el.addEventListener("pointercancel", cancel);
      el.addEventListener("pointermove", (e) => {
        if (!start || e.pointerId !== start.id) return;
        const dx = e.clientX - start.x;
        const dy = e.clientY - start.y;
        if (!horizontal) {
          if (Math.abs(dx) < DRAG_PX) return;
          if (Math.abs(dy) > Math.abs(dx) * 1.2) {
            cancel();
            return;
          }
          horizontal = true;
          try {
            el.setPointerCapture(e.pointerId);
          } catch {
            // A browser without pointer capture still delivers the release.
          }
        }
        e.preventDefault();
        if (surface) {
          const limited = Math.max(-180, Math.min(180, dx));
          surface.style.transition = "none";
          surface.style.transform = "translateX(" + limited + "px)";
        }
      });
      el.addEventListener("pointerup", (e) => {
        if (!start || e.pointerId !== start.id) return;
        const dx = e.clientX - start.x;
        const wasHorizontal = horizontal;
        start = null;
        horizontal = false;
        if (el.__euglenaSwipeHandledPointer === e.pointerId) {
          delete el.__euglenaSwipeHandledPointer;
          restoreSurface(dx < 0 ? "-2.8rem" : "0px");
          return;
        }
        if (!wasHorizontal) {
          restoreSurface();
          return;
        }
        suppressClick = true;
        restoreSurface(dx <= -56 ? "-2.8rem" : "0px");
        fire({ ...particle, phase: "end", dx });
      });
      return;
    }
    if (name === "edgeclose") {
      // A modal page owns vertical scrolling. When a touch starts at one of
      // its two scroll boundaries, a second pull in that same direction is a
      // dismissal gesture instead of the browser's rubber-band bounce. The
      // surface follows the finger while it is pulled, then either settles
      // back or slides away after release. A touch that starts in the middle
      // is left entirely to native scrolling; reaching an edge never closes
      // the dialog by itself.
      const EDGE_PX = 96;
      const MAX_DRAG = 180;
      const ANIMATION_MS = 200;
      const atTop = () => el.scrollTop <= 0;
      const atBottom = () => el.scrollTop + el.clientHeight >= el.scrollHeight - 1;
      const outward = (dy, edge) => (edge === "top" ? dy > 0 : dy < 0);
      // The scroll host may contain a fixed backdrop followed by the modal
      // surface. Transform only the first non-backdrop child so the scrim
      // stays fixed while the dialog follows the pull.
      const findSurface = () => {
        const children = el.children ? Array.from(el.children) : [];
        for (const child of children) {
          const klass = child.getAttribute && child.getAttribute("class");
          if (!String(klass || "").split(/\s+/).includes("backdrop")) return child;
        }
        return el;
      };
      let surface = el;
      let closeQueued = false;
      let settleTimer = null;
      let baseTransition = "";
      let baseTransform = "";
      let baseWillChange = "";
      let input = null;

      const rememberBase = () => {
        if (settleTimer !== null) {
          clearTimeout(settleTimer);
          settleTimer = null;
        }
        surface = findSurface();
        baseTransition = surface.style ? surface.style.transition || "" : "";
        baseTransform = surface.style ? surface.style.transform || "" : "";
        baseWillChange = surface.style ? surface.style.willChange || "" : "";
        if (surface.style) surface.style.willChange = "transform";
      };
      const restoreBase = () => {
        if (!surface.style) return;
        surface.style.transition = baseTransition;
        surface.style.transform = baseTransform;
        surface.style.willChange = baseWillChange;
      };
      const offset = (dy) => Math.max(-MAX_DRAG, Math.min(MAX_DRAG, dy));
      const moveSurface = (dy) => {
        if (!surface.style) return;
        surface.style.transition = "none";
        surface.style.transform = "translate3d(0, " + offset(dy) + "px, 0)";
      };
      const settleBack = () => {
        if (!surface.style || closeQueued) return;
        surface.style.transition = "transform " + ANIMATION_MS + "ms ease-out";
        surface.style.transform = baseTransform || "translate3d(0, 0, 0)";
        settleTimer = setTimeout(() => {
          settleTimer = null;
          if (!closeQueued) restoreBase();
        }, ANIMATION_MS);
      };
      const close = (edge) => {
        if (closeQueued) return;
        closeQueued = true;
        if (settleTimer !== null) {
          clearTimeout(settleTimer);
          settleTimer = null;
        }
        if (surface.style) {
          surface.style.transition = "transform " + ANIMATION_MS + "ms ease-in";
          surface.style.transform = edge === "top" ? "translate3d(0, 100%, 0)" : "translate3d(0, -100%, 0)";
        }
        setTimeout(() => fire(meant(el, particle)), ANIMATION_MS);
      };
      let touch = null;
      // Pointer events cover browsers that do not expose a TouchEvent stream.
      // Mouse drags are deliberately excluded: desktop wheel scrolling has a
      // separate boundary path, while a mouse drag should never dismiss a
      // dialog accidentally.
      let pointer = null;
      el.addEventListener("touchstart", (e) => {
        // On browsers that expose both APIs, pointerdown precedes touchstart.
        // Prefer the touch stream once it appears: pointer events may be
        // cancelled when the browser begins its native vertical-pan decision.
        if (input === "pointer") {
          pointer = null;
          input = null;
        } else if (input && input !== "touch") return;
        if (!e.touches || e.touches.length !== 1) {
          touch = null;
          input = null;
          return;
        }
        if (closeQueued) return;
        const edge = atTop() ? "top" : atBottom() ? "bottom" : null;
        if (!edge) {
          touch = null;
          return;
        }
        input = "touch";
        rememberBase();
        touch = { y: e.touches[0].clientY, lastY: e.touches[0].clientY, edge, cancelled: false };
      }, { passive: true });
      el.addEventListener("touchmove", (e) => {
        if (!touch || !e.touches || e.touches.length !== 1) return;
        const y = e.touches[0].clientY;
        const dy = y - touch.y;
        touch.lastY = y;
        if (!outward(dy, touch.edge)) {
          if (Math.abs(dy) > 2) touch.cancelled = true;
          return;
        }
        if (touch.cancelled) return;
        // Suppress the platform bounce as soon as this is an outward pull.
        e.preventDefault();
        moveSurface(dy);
      }, { passive: false });
      const finishTouch = (e, cancelled = false) => {
        if (!touch) return;
        const gesture = touch;
        touch = null;
        pointer = null;
        input = null;
        const changed = e && e.changedTouches && e.changedTouches[0];
        const y = changed && typeof changed.clientY === "number" ? changed.clientY : gesture.lastY;
        const dy = y - gesture.y;
        if (cancelled || gesture.cancelled || !outward(dy, gesture.edge)) {
          settleBack();
        } else if (Math.abs(dy) >= EDGE_PX) {
          close(gesture.edge);
        } else {
          settleBack();
        }
      };
      el.addEventListener("touchend", (e) => finishTouch(e), { passive: true });
      el.addEventListener("touchcancel", (e) => finishTouch(e, true), { passive: true });

      el.addEventListener("pointerdown", (e) => {
        if (e.pointerType === "mouse") {
          pointer = null;
          return;
        }
        if (input) return;
        if (closeQueued) return;
        const edge = atTop() ? "top" : atBottom() ? "bottom" : null;
        if (!edge) {
          pointer = null;
          return;
        }
        input = "pointer";
        rememberBase();
        pointer = { y: e.clientY, lastY: e.clientY, id: e.pointerId, edge, cancelled: false };
      });
      el.addEventListener("pointermove", (e) => {
        if (!pointer || e.pointerId !== pointer.id) return;
        const dy = e.clientY - pointer.y;
        pointer.lastY = e.clientY;
        if (!outward(dy, pointer.edge)) {
          if (Math.abs(dy) > 2) pointer.cancelled = true;
          return;
        }
        if (pointer.cancelled) return;
        e.preventDefault();
        moveSurface(dy);
      });
      const finishPointer = (e, cancelled = false) => {
        if (!pointer || (e && e.pointerId !== pointer.id)) return;
        const gesture = pointer;
        pointer = null;
        touch = null;
        input = null;
        const y = e && typeof e.clientY === "number" ? e.clientY : gesture.lastY;
        const dy = y - gesture.y;
        if (cancelled || gesture.cancelled || !outward(dy, gesture.edge)) {
          settleBack();
        } else if (Math.abs(dy) >= EDGE_PX) {
          close(gesture.edge);
        } else {
          settleBack();
        }
      };
      el.addEventListener("pointerup", (e) => finishPointer(e));
      el.addEventListener("pointercancel", (e) => finishPointer(e, true));

      // A wheel event has no continuation state: one outward wheel tick at a
      // boundary is the extra scroll the user asked for. Preventing its
      // default action avoids a desktop overscroll animation.
      el.addEventListener("wheel", (e) => {
        const edge = e.deltaY < 0 && atTop() ? "top" : e.deltaY > 0 && atBottom() ? "bottom" : null;
        if (!edge) return;
        e.preventDefault();
        rememberBase();
        close(edge);
      }, { passive: false });
      return;
    }
    if (name === "reorder" || name === "longreorder") {
      const needsHold = name === "longreorder";
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
        if (drag && drag.timer !== null) clearTimeout(drag.timer);
        if (drag && drag.child.removeAttribute) drag.child.removeAttribute("data-code-dragging");
        drag = null;
      };
      el.addEventListener("pointerdown", (e) => {
        const child = childAt(e.target);
        if (!child) return;
        const candidate = { child, from: indexOf(child), id: e.pointerId, x: e.clientX, y: e.clientY, moved: false, armed: !needsHold, timer: null };
        if (needsHold) {
          candidate.timer = setTimeout(() => {
            if (!drag || drag !== candidate) return;
            drag.armed = true;
            drag.child.setAttribute("data-code-dragging", "");
            try {
              el.setPointerCapture(e.pointerId);
            } catch {
              // A page without pointer capture still receives the release.
            }
          }, LONG_REORDER_MS);
        }
        drag = candidate;
      });
      el.addEventListener("pointermove", (e) => {
        if (!drag || e.pointerId !== drag.id || drag.moved || !drag.armed) return;
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
        if (!carried.armed || !carried.moved) return;
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
          sendFile(particle, chosen);
          return;
        }
        // A paste carries a file when something like a screenshot was
        // copied: it is read and sent as `file`, exactly as a file box sends
        // one. Pasted text is not an event here — it changes the box, and
        // the box's own `input` says so.
        if (event === "paste") {
          const pasted = pastedFile(e);
          if (!pasted) return;
          e.preventDefault();
          sendFile(particle, pasted);
          return;
        }
        const value = eventValue(target);
        // What the element holds, added only when the application did not
        // say it itself — an `on` that names `value` means that value.
        let sent = value === null || "value" in particle ? particle : { ...particle, value };
        // A key event says which key, since "a key was pressed" is never
        // what an application wanted to know.
        if (typeof e.key === "string" && !("key" in sent)) sent = { ...sent, key: e.key };
        // And which modifiers were held, so Shift+Enter can mean a new line
        // where Enter alone sends. Only for key events, and only when held.
        if (typeof e.key === "string") {
          for (const [name, held] of [["shift", e.shiftKey], ["ctrl", e.ctrlKey], ["alt", e.altKey], ["meta", e.metaKey]]) {
            if (held && !(name in sent)) sent = { ...sent, [name]: true };
          }
        }
        fire(sent);
      });
    }
    for (const child of spec.children || []) el.appendChild(node(child));
    return el;
  }

  // A render tree is small data, so keeping its previous shape lets the DOM
  // keep the useful parts of the page alive. `data-focus-key` is the stable
  // identity the applications already put on rows; a tree may also provide a
  // plain `key` when it has one.
  const renderChildren = (spec) =>
    spec && typeof spec === "object" && !Array.isArray(spec) && Array.isArray(spec.children)
      ? spec.children
      : [];

  const isElementSpec = (spec) =>
    spec !== null && typeof spec === "object" && !Array.isArray(spec);

  const specTag = (spec) => isElementSpec(spec) ? String(spec.tag || "div") : null;

  const specKey = (spec) => {
    if (!isElementSpec(spec)) return null;
    if (spec.key !== undefined && spec.key !== null) return `key:${String(spec.key)}`;
    const attrs = spec.attrs || {};
    if (attrs["data-focus-key"] !== undefined && attrs["data-focus-key"] !== null) {
      return `focus:${String(attrs["data-focus-key"])}`;
    }
    if (attrs["data-code-key"] !== undefined && attrs["data-code-key"] !== null) {
      return `code:${String(attrs["data-code-key"])}`;
    }
    return null;
  };

  const eventShape = (spec) => JSON.stringify(spec?.on || {});

  const mediaValue = (spec) =>
    isElementSpec(spec) && typeof spec.media === "string" && spec.media ? spec.media : null;

  const attrValues = (spec) => {
    const attrs = { ...(isElementSpec(spec) ? spec.attrs || {} : {}) };
    const media = mediaValue(spec);
    if (media) attrs["data-code-media"] = media;
    return attrs;
  };

  const canReuse = (dom, previous, next) => {
    if (!dom || isElementSpec(previous) !== isElementSpec(next)) return false;
    if (!isElementSpec(next)) return dom.nodeType === 3;
    if (specTag(previous) !== specTag(next)) return false;
    if (specKey(previous) !== specKey(next)) return false;
    if (eventShape(previous) !== eventShape(next)) return false;
    return mediaValue(previous) === mediaValue(next);
  };

  function updateAttrs(el, previous, next) {
    const before = attrValues(previous);
    const after = attrValues(next);
    for (const key of Object.keys(before)) {
      if (!(key in after) && typeof el.removeAttribute === "function") el.removeAttribute(key);
    }
    for (const [key, value] of Object.entries(after)) {
      if (!(key in before) || String(before[key]) !== String(value)) {
        el.setAttribute(key, String(value));
        // A box someone has typed in keeps its live value whatever its
        // `value` attribute says — the attribute is only the default. Before
        // trees were reconciled in place every render made a fresh element,
        // so the attribute *was* the value; now the element is kept, and a
        // program that clears a search box by drawing `value = ""` must
        // reach the live value too, or the reader keeps seeing their words.
        if (key === "value" && VALUED.has(String(el.tagName || "").toLowerCase()) && el.value !== String(value)) {
          el.value = String(value);
        }
      }
    }
  }

  function childNodesOf(parent) {
    if (parent && parent.childNodes) return Array.from(parent.childNodes);
    return parent && parent.children ? Array.from(parent.children) : [];
  }

  function patchChildren(parent, previous, next) {
    const oldSpecs = renderChildren(previous);
    const newSpecs = renderChildren(next);
    const current = childNodesOf(parent);
    const slots = current.map((dom, index) => ({ dom, spec: oldSpecs[index], used: false, replaced: false }));
    const keyed = new Map();
    for (const slot of slots) {
      const key = specKey(slot.spec);
      if (key !== null && !keyed.has(key)) keyed.set(key, slot);
    }

    const wanted = [];
    for (let index = 0; index < newSpecs.length; index += 1) {
      const spec = newSpecs[index];
      const key = specKey(spec);
      let slot = key === null ? null : keyed.get(key);
      if (slot?.used) slot = null;
      if (!slot && key === null) {
        const at = slots[index];
        if (at && !at.used && specKey(at.spec) === null) slot = at;
      }
      if (slot) {
        slot.used = true;
        const patched = patchNode(slot.dom, slot.spec, spec);
        if (patched !== slot.dom) slot.replaced = true;
        wanted.push(patched);
      } else {
        wanted.push(node(spec));
      }
    }

    // What went goes first. Left in place while the rest is put in order,
    // a child that went makes every kept child after it move — and a moved
    // node loses focus, so a dialog after a notice that went lost its caret.
    for (const slot of slots) {
      if ((!slot.used || slot.replaced) && slot.dom.parentNode === parent && typeof parent.removeChild === "function") {
        parent.removeChild(slot.dom);
      }
    }
    for (let index = 0; index < wanted.length; index += 1) {
      const currentAt = childNodesOf(parent)[index] || null;
      if (currentAt === wanted[index]) continue;
      if (typeof parent.insertBefore === "function") parent.insertBefore(wanted[index], currentAt);
      else if (!wanted[index].parentNode || wanted[index].parentNode !== parent) parent.appendChild(wanted[index]);
    }
  }

  function patchNode(dom, previous, next) {
    if (!canReuse(dom, previous, next)) return node(next);
    if (!isElementSpec(next)) {
      const text = String(next);
      if (dom.nodeValue !== text) dom.nodeValue = text;
      return dom;
    }
    updateAttrs(dom, previous, next);
    patchChildren(dom, previous, next);
    return dom;
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
  const rendered = new WeakMap();

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

  // Replacing a drawn tree can make the browser discard its scroll anchor,
  // especially when the focused node is one of the children being replaced.
  // Keep the scroll positions of the target's ancestors (and the viewport)
  // across the replacement so a small row action does not send a reader back
  // to the top of a long list.
  function scrollState(root) {
    const entries = [];
    const seen = new Set();
    const remember = (el) => {
      if (!el || seen.has(el) || typeof el.scrollTop !== "number") return;
      seen.add(el);
      entries.push({ el, top: el.scrollTop, left: el.scrollLeft });
    };
    for (let el = root; el; el = el.parentNode) remember(el);
    remember(doc.scrollingElement);
    remember(doc.documentElement);
    remember(doc.body);
    const viewport = typeof globalThis.scrollX === "number" && typeof globalThis.scrollY === "number"
      ? { x: globalThis.scrollX, y: globalThis.scrollY }
      : null;
    return { entries, viewport };
  }

  function restoreScroll(was) {
    for (const one of was.entries) {
      if (!one.el || one.el.isConnected === false) continue;
      if (one.el.scrollTop !== one.top) one.el.scrollTop = one.top;
      if (one.el.scrollLeft !== one.left) one.el.scrollLeft = one.left;
    }
    if (was.viewport && typeof globalThis.scrollTo === "function"
      && (globalThis.scrollX !== was.viewport.x || globalThis.scrollY !== was.viewport.y)) {
      globalThis.scrollTo(was.viewport.x, was.viewport.y);
    }
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
      const scroll = scrollState(target);
      const activeBefore = doc.activeElement;
      const oldModal = openModal();
      const previous = rendered.get(target);
      const canKeepRoot = previous && previous.dom && previous.dom.parentNode === target
        && canReuse(previous.dom, previous.tree, tree);
      const nextRoot = canKeepRoot ? patchNode(previous.dom, previous.tree, tree) : node(tree);
      if (!canKeepRoot || nextRoot !== previous.dom) target.replaceChildren(nextRoot);
      rendered.set(target, { tree, dom: nextRoot });
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
      restoreScroll(scroll);
      return { _class: "RenderResult", ok: true };
    },
  ];
}
