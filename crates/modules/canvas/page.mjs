// The page's half of `canvas`: a bounded command list painted to Canvas 2D.
(ctx) => {
  const { doc, fire } = ctx;
  const attached = new WeakMap();
  const MAX_DIMENSION = 4096;
  const MAX_COMMANDS = 4000;
  const finite = (value) => typeof value === "number" && Number.isFinite(value);
  const number = (value, fallback = 0) => finite(value) ? value : fallback;
  const clamp = (value, low, high) => Math.max(low, Math.min(high, value));
  const result = (ok, fields = {}) => ({ _class: "CanvasResult", ok, ...fields });

  function style(ctx2d, value) {
    if (typeof value === "string") return value;
    if (!value || typeof value !== "object" || value.type !== "linear" || !Array.isArray(value.stops)) return null;
    const gradient = ctx2d.createLinearGradient(
      number(value.x0), number(value.y0), number(value.x1), number(value.y1),
    );
    for (const stop of value.stops.slice(0, 16)) {
      if (!stop || typeof stop.color !== "string" || !finite(stop.at)) continue;
      try { gradient.addColorStop(clamp(stop.at, 0, 1), stop.color); } catch { /* ignore an invalid stop */ }
    }
    return gradient;
  }

  function paint(ctx2d, command) {
    if (!command || typeof command !== "object" || typeof command.op !== "string") return;
    const op = command.op;
    ctx2d.save();
    ctx2d.globalAlpha = clamp(number(command.alpha, 1), 0, 1);
    ctx2d.lineWidth = clamp(number(command.line_width, 1), 0.1, 64);
    ctx2d.lineJoin = "round";
    ctx2d.lineCap = "round";
    const fill = style(ctx2d, command.fill);
    if (fill !== null) ctx2d.fillStyle = fill;
    if (typeof command.stroke === "string") ctx2d.strokeStyle = command.stroke;

    if (op === "rect" && [command.x, command.y, command.width, command.height].every(finite)) {
      if (fill !== null) ctx2d.fillRect(command.x, command.y, command.width, command.height);
      if (typeof command.stroke === "string") ctx2d.strokeRect(command.x, command.y, command.width, command.height);
    } else if (op === "ellipse" && [command.x, command.y, command.rx, command.ry].every(finite)) {
      ctx2d.beginPath();
      ctx2d.ellipse(command.x, command.y, Math.max(0, command.rx), Math.max(0, command.ry), number(command.rotation), 0, Math.PI * 2);
      if (fill !== null) ctx2d.fill();
      if (typeof command.stroke === "string") ctx2d.stroke();
    } else if (op === "poly" && Array.isArray(command.points) && command.points.length > 1) {
      const points = command.points.slice(0, 2048).filter((point) => point && finite(point.x) && finite(point.y));
      if (points.length > 1) {
        ctx2d.beginPath();
        ctx2d.moveTo(points[0].x, points[0].y);
        for (const point of points.slice(1)) ctx2d.lineTo(point.x, point.y);
        if (command.closed === true) ctx2d.closePath();
        if (fill !== null && command.closed === true) ctx2d.fill();
        if (typeof command.stroke === "string") ctx2d.stroke();
      }
    } else if (op === "path" && Array.isArray(command.path)) {
      ctx2d.beginPath();
      for (const segment of command.path.slice(0, 2048)) {
        if (!segment || typeof segment.op !== "string") continue;
        if ((segment.op === "move" || segment.op === "line") && finite(segment.x) && finite(segment.y)) {
          if (segment.op === "move") ctx2d.moveTo(segment.x, segment.y);
          else ctx2d.lineTo(segment.x, segment.y);
        } else if (segment.op === "bezier" && [segment.cx1, segment.cy1, segment.cx2, segment.cy2, segment.x, segment.y].every(finite)) {
          ctx2d.bezierCurveTo(segment.cx1, segment.cy1, segment.cx2, segment.cy2, segment.x, segment.y);
        } else if (segment.op === "close") {
          ctx2d.closePath();
        }
      }
      if (fill !== null) ctx2d.fill();
      if (typeof command.stroke === "string") ctx2d.stroke();
    } else if (op === "text" && typeof command.text === "string" && finite(command.x) && finite(command.y)) {
      ctx2d.font = `${clamp(number(command.size, 14), 6, 96)}px ${typeof command.font === "string" ? command.font.slice(0, 80) : "system-ui, sans-serif"}`;
      ctx2d.textAlign = ["left", "center", "right"].includes(command.align) ? command.align : "left";
      ctx2d.textBaseline = "alphabetic";
      if (fill !== null) ctx2d.fillText(command.text.slice(0, 512), command.x, command.y);
    }
    ctx2d.restore();
  }

  return ["canvas", (particle) => {
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
    const dpr = clamp(number(globalThis.devicePixelRatio, 1), 1, 2);
    const pixelWidth = Math.min(8192, Math.round(cssWidth * dpr));
    const pixelHeight = Math.min(8192, Math.round(cssHeight * dpr));
    if (canvas.width !== pixelWidth) canvas.width = pixelWidth;
    if (canvas.height !== pixelHeight) canvas.height = pixelHeight;
    let ctx2d;
    try { ctx2d = canvas.getContext("2d"); } catch { return result(false, { reason: "Canvas 2D is unavailable" }); }
    if (!ctx2d) return result(false, { reason: "Canvas 2D is unavailable" });
    ctx2d.setTransform(pixelWidth / width, 0, 0, pixelHeight / height, 0, 0);
    ctx2d.clearRect(0, 0, width, height);
    for (const command of particle.commands) paint(ctx2d, command);

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
    return result(true, { width, height });
  }];
}
