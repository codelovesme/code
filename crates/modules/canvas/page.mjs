// The page's half of `canvas`: a bounded command list painted to Canvas 2D.
(ctx) => {
  const { doc, fire } = ctx;
  const attached = new WeakMap();
  // Named drawings a canvas keeps between frames: `Keep` stores one, a
  // `use` command paints it. Bounded, like everything else here.
  const keptBy = new WeakMap();
  const MAX_KEPT = 1024;
  const MAX_DIMENSION = 4096;
  const MAX_COMMANDS = 4000;
  const MAX_DEPTH = 8;
  const finite = (value) => typeof value === "number" && Number.isFinite(value);
  const number = (value, fallback = 0) => finite(value) ? value : fallback;
  const clamp = (value, low, high) => Math.max(low, Math.min(high, value));
  const result = (ok, fields = {}) => ({ _class: "CanvasResult", ok, ...fields });

  // A colour, or a gradient described as data: `linear` runs from (x0, y0) to
  // (x1, y1); `radial` from the circle (x0, y0, r0) to the circle (x1, y1, r1).
  function style(ctx2d, value) {
    if (typeof value === "string") return value;
    if (!value || typeof value !== "object" || !Array.isArray(value.stops)) return null;
    let gradient;
    try {
      if (value.type === "linear") {
        gradient = ctx2d.createLinearGradient(
          number(value.x0), number(value.y0), number(value.x1), number(value.y1),
        );
      } else if (value.type === "radial") {
        gradient = ctx2d.createRadialGradient(
          number(value.x0), number(value.y0), Math.max(0, number(value.r0)),
          number(value.x1), number(value.y1), Math.max(0, number(value.r1)),
        );
      } else {
        return null;
      }
    } catch {
      return null;
    }
    for (const stop of value.stops.slice(0, 16)) {
      if (!stop || typeof stop.color !== "string" || !finite(stop.at)) continue;
      try { gradient.addColorStop(clamp(stop.at, 0, 1), stop.color); } catch { /* ignore an invalid stop */ }
    }
    return gradient;
  }

  // `budget.left` counts every command painted in one frame, nested ones too,
  // so a group cannot carry more than a flat frame could.
  // A flat path: SVG's letters, each followed by its numbers —
  // M x y, L x y, Q cx cy x y, C cx1 cy1 cx2 cy2 x y, Z.
  function tracePath(ctx2d, d) {
    const n = Math.min(d.length, 16384);
    let i = 0;
    while (i < n) {
      const letter = d[i];
      const at = (k) => d[i + k];
      if (letter === "M" && finite(at(1)) && finite(at(2))) { ctx2d.moveTo(at(1), at(2)); i += 3; }
      else if (letter === "L" && finite(at(1)) && finite(at(2))) { ctx2d.lineTo(at(1), at(2)); i += 3; }
      else if (letter === "Q" && [at(1), at(2), at(3), at(4)].every(finite)) { ctx2d.quadraticCurveTo(at(1), at(2), at(3), at(4)); i += 5; }
      else if (letter === "C" && [at(1), at(2), at(3), at(4), at(5), at(6)].every(finite)) { ctx2d.bezierCurveTo(at(1), at(2), at(3), at(4), at(5), at(6)); i += 7; }
      else if (letter === "Z") { ctx2d.closePath(); i += 1; }
      else return;
    }
  }

  function paint(ctx2d, command, budget, depth, kept) {
    if (!command || typeof command !== "object" || typeof command.op !== "string") return;
    if (budget.left <= 0) return;
    budget.left -= 1;
    const op = command.op;
    ctx2d.save();
    const matrix = command.transform;
    if (Array.isArray(matrix) && matrix.length === 6 && matrix.every(finite)) {
      ctx2d.transform(matrix[0], matrix[1], matrix[2], matrix[3], matrix[4], matrix[5]);
    }
    ctx2d.globalAlpha *= clamp(number(command.alpha, 1), 0, 1);
    if (op === "group" || op === "use") {
      const children = op === "group" ? command.commands : kept?.get(String(command.name));
      if (Array.isArray(children) && depth < MAX_DEPTH) {
        for (const child of children) paint(ctx2d, child, budget, depth + 1, kept);
      }
      ctx2d.restore();
      return;
    }
    ctx2d.lineWidth = clamp(number(command.line_width, 1), 0.01, 1024);
    ctx2d.lineJoin = ["round", "bevel", "miter"].includes(command.line_join) ? command.line_join : "round";
    ctx2d.lineCap = ["round", "butt", "square"].includes(command.line_cap) ? command.line_cap : "round";
    const fill = style(ctx2d, command.fill);
    if (fill !== null) ctx2d.fillStyle = fill;
    const stroke = style(ctx2d, command.stroke);
    if (stroke !== null) ctx2d.strokeStyle = stroke;

    if (op === "rect" && [command.x, command.y, command.width, command.height].every(finite)) {
      if (fill !== null) ctx2d.fillRect(command.x, command.y, command.width, command.height);
      if (stroke !== null) ctx2d.strokeRect(command.x, command.y, command.width, command.height);
    } else if (op === "ellipse" && [command.x, command.y, command.rx, command.ry].every(finite)) {
      ctx2d.beginPath();
      ctx2d.ellipse(command.x, command.y, Math.max(0, command.rx), Math.max(0, command.ry), number(command.rotation), 0, Math.PI * 2);
      if (fill !== null) ctx2d.fill();
      if (stroke !== null) ctx2d.stroke();
    } else if (op === "poly" && Array.isArray(command.xy) && command.xy.length > 3) {
      const xy = command.xy;
      ctx2d.beginPath();
      ctx2d.moveTo(number(xy[0]), number(xy[1]));
      for (let i = 2; i + 1 < Math.min(xy.length, 8192); i += 2) ctx2d.lineTo(number(xy[i]), number(xy[i + 1]));
      if (command.closed === true) ctx2d.closePath();
      if (fill !== null && command.closed === true) ctx2d.fill();
      if (stroke !== null) ctx2d.stroke();
    } else if (op === "poly" && Array.isArray(command.points) && command.points.length > 1) {
      const points = command.points.slice(0, 2048).filter((point) => point && finite(point.x) && finite(point.y));
      if (points.length > 1) {
        ctx2d.beginPath();
        ctx2d.moveTo(points[0].x, points[0].y);
        for (const point of points.slice(1)) ctx2d.lineTo(point.x, point.y);
        if (command.closed === true) ctx2d.closePath();
        if (fill !== null && command.closed === true) ctx2d.fill();
        if (stroke !== null) ctx2d.stroke();
      }
    } else if (op === "path" && Array.isArray(command.d)) {
      ctx2d.beginPath();
      tracePath(ctx2d, command.d);
      if (fill !== null) ctx2d.fill();
      if (stroke !== null) ctx2d.stroke();
    } else if (op === "path" && Array.isArray(command.path)) {
      ctx2d.beginPath();
      for (const segment of command.path.slice(0, 2048)) {
        if (!segment || typeof segment.op !== "string") continue;
        if ((segment.op === "move" || segment.op === "line") && finite(segment.x) && finite(segment.y)) {
          if (segment.op === "move") ctx2d.moveTo(segment.x, segment.y);
          else ctx2d.lineTo(segment.x, segment.y);
        } else if (segment.op === "bezier" && [segment.cx1, segment.cy1, segment.cx2, segment.cy2, segment.x, segment.y].every(finite)) {
          ctx2d.bezierCurveTo(segment.cx1, segment.cy1, segment.cx2, segment.cy2, segment.x, segment.y);
        } else if (segment.op === "quad" && [segment.cx, segment.cy, segment.x, segment.y].every(finite)) {
          ctx2d.quadraticCurveTo(segment.cx, segment.cy, segment.x, segment.y);
        } else if (segment.op === "close") {
          ctx2d.closePath();
        }
      }
      if (fill !== null) ctx2d.fill();
      if (stroke !== null) ctx2d.stroke();
    } else if (op === "text" && typeof command.text === "string" && finite(command.x) && finite(command.y)) {
      ctx2d.font = `${clamp(number(command.size, 14), 6, 96)}px ${typeof command.font === "string" ? command.font.slice(0, 80) : "system-ui, sans-serif"}`;
      ctx2d.textAlign = ["left", "center", "right"].includes(command.align) ? command.align : "left";
      ctx2d.textBaseline = "alphabetic";
      if (fill !== null) ctx2d.fillText(command.text.slice(0, 512), command.x, command.y);
    }
    ctx2d.restore();
  }

  function keep(particle) {
    const selector = typeof particle.into === "string" && particle.into ? particle.into : "#aquarium-canvas";
    const canvas = typeof doc.querySelector === "function" ? doc.querySelector(selector) : null;
    if (!canvas) return result(false, { reason: "the canvas selector matched no canvas" });
    if (typeof particle.name !== "string" || !particle.name) return result(false, { reason: "a kept drawing needs a name" });
    if (!Array.isArray(particle.commands) || particle.commands.length > MAX_COMMANDS) {
      return result(false, { reason: `commands must be an array of at most ${MAX_COMMANDS} items` });
    }
    let kept = keptBy.get(canvas);
    if (!kept) { kept = new Map(); keptBy.set(canvas, kept); }
    if (!kept.has(particle.name) && kept.size >= MAX_KEPT) return result(false, { reason: `a canvas keeps at most ${MAX_KEPT} drawings` });
    if (particle.commands.length === 0) kept.delete(particle.name);
    else kept.set(particle.name, particle.commands);
    return result(true, { kept: kept.size });
  }

  return ["canvas", (particle) => {
    if (particle._class === "Keep") return keep(particle);
    if (particle._class !== "Draw") return null;
    const selector = typeof particle.into === "string" && particle.into ? particle.into : "#aquarium-canvas";
    const canvas = typeof doc.querySelector === "function" ? doc.querySelector(selector) : null;
    if (!canvas || typeof canvas.getContext !== "function") return result(false, { reason: "the canvas selector matched no canvas" });
    const width = Math.round(number(particle.width));
    const height = Math.round(number(particle.height));
    if (width < 1 || height < 1 || width > MAX_DIMENSION || height > MAX_DIMENSION) {
      return result(false, { reason: `logical size must be between 1 and ${MAX_DIMENSION}` });
    }
    if (!Array.isArray(particle.commands) || particle.commands.length > MAX_COMMANDS) {
      return result(false, { reason: `commands must be an array of at most ${MAX_COMMANDS} items` });
    }
    const rect = typeof canvas.getBoundingClientRect === "function" ? canvas.getBoundingClientRect() : null;
    const cssWidth = Math.max(1, Math.round(number(rect?.width, width)));
    const cssHeight = Math.max(1, Math.round(number(rect?.height, height)));
    // Backing pixels per CSS pixel: the device's ratio (at most 2) unless the
    // frame names its own — `density = 1` is a canvas sized in CSS pixels.
    const dpr = finite(particle.density) ? clamp(particle.density, 0.25, 2) : clamp(number(globalThis.devicePixelRatio, 1), 1, 2);
    const pixelWidth = Math.min(8192, Math.round(cssWidth * dpr));
    const pixelHeight = Math.min(8192, Math.round(cssHeight * dpr));
    if (canvas.width !== pixelWidth) canvas.width = pixelWidth;
    if (canvas.height !== pixelHeight) canvas.height = pixelHeight;
    let ctx2d;
    try { ctx2d = canvas.getContext("2d"); } catch { return result(false, { reason: "Canvas 2D is unavailable" }); }
    if (!ctx2d) return result(false, { reason: "Canvas 2D is unavailable" });
    ctx2d.setTransform(pixelWidth / width, 0, 0, pixelHeight / height, 0, 0);
    ctx2d.globalAlpha = 1;
    // `clear = false` paints over what is there, so one scene can be sent as
    // several smaller frames — a scene built a piece at a time never has to
    // assemble one large list.
    if (particle.clear !== false) ctx2d.clearRect(0, 0, width, height);
    const budget = { left: MAX_COMMANDS };
    const kept = keptBy.get(canvas);
    for (const command of particle.commands) paint(ctx2d, command, budget, 0, kept);

    const eventClass = typeof particle.event === "string" ? particle.event : "";
    const config = { eventClass, width, height };
    if (!attached.has(canvas)) {
      canvas.addEventListener("click", (event) => {
        const current = attached.get(canvas);
        if (!current?.eventClass || !event || !finite(event.clientX) || !finite(event.clientY)) return;
        const bounds = canvas.getBoundingClientRect();
        if (!bounds.width || !bounds.height) return;
        fire({
          _class: current.eventClass,
          x: (event.clientX - bounds.left) * current.width / bounds.width,
          y: (event.clientY - bounds.top) * current.height / bounds.height,
          pointer_type: typeof event.pointerType === "string" ? event.pointerType : "mouse",
        });
      });
    }
    attached.set(canvas, config);
    // The element's own size, so a scene can lay itself out for the screen it
    // is on, and the page's clock, so it can move by time rather than by frame.
    const time = typeof globalThis.performance?.now === "function" ? globalThis.performance.now() : Date.now();
    return result(true, { width, height, view_width: cssWidth, view_height: cssHeight, time });
  }];
}
