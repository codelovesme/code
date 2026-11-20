// The page's half of `telemetry`: what went wrong on this page, kept until
// someone who reads it has it.
//
// A page has no console anyone sees once it runs on somebody's phone. So the
// trouble on it — a script error nothing caught, a module half that threw, a
// request that never came back, an application saying something failed — is
// written down here, kept on the device through a closed page and no
// network, and sent in batches to the destination a program configured.
//
// **One collector per page.** A shell and every guest it runs each get this
// half, and all of them write into the same one (`globalThis.__code_telemetry`),
// each line carrying the program it came from. `net_client` and the runtime
// write into it too when it is there — that is how a request every application
// makes is seen without any of them asking.
//
// **What is never kept:** what was said. Class names, times, error messages,
// the page's path and the device's kind — not particles, not text a person
// typed. A line is cut at 2000 characters.
(ctx) => {
  const g = globalThis;
  const KEY = "code:telemetry";
  const MAX_KEPT = 200;
  const MAX_CRUMBS = 30;
  const MAX_MESSAGE = 2000;
  const BATCH = 50;
  const SAME_WITHIN_S = 60;
  const EVERY_MS = 30_000;

  const now = () => Math.floor(Date.now() / 1000);
  const cut = (text, n) => {
    const s = String(text ?? "");
    return s.length > n ? s.slice(0, n) + "…" : s;
  };

  const createHub = () => {
    const hub = {
      items: [],
      crumbs: [],
      dest: null,
      self: null,
      sending: false,
      timer: null,
    };
    try {
      const kept = JSON.parse(g.localStorage?.getItem(KEY) ?? "[]");
      if (Array.isArray(kept)) hub.items = kept.slice(-MAX_KEPT);
    } catch {
      // Storage refused or garbled: start from nothing, which is what a page
      // with nothing kept is.
    }
    const save = () => {
      try {
        g.localStorage?.setItem(KEY, JSON.stringify(hub.items));
      } catch {
        // Full or refused: what is in memory still goes when it can.
      }
    };
    const device = () => {
      const nav = g.navigator ?? {};
      return {
        agent: cut(nav.userAgent ?? "", 300),
        language: cut(nav.language ?? "", 20),
        online: nav.onLine !== false,
        width: g.innerWidth ?? null,
        height: g.innerHeight ?? null,
        installed: Boolean(g.matchMedia?.("(display-mode: standalone)")?.matches),
      };
    };
    const later = (ms) => {
      if (hub.timer !== null) return;
      hub.timer = setTimeout(() => {
        hub.timer = null;
        hub.flush(false);
      }, ms);
      hub.timer?.unref?.();
    };

    hub.crumb = ({ app, message }) => {
      hub.crumbs.push({ app: app ?? hub.self, message: cut(message, 200), at: now() });
      if (hub.crumbs.length > MAX_CRUMBS) hub.crumbs.splice(0, hub.crumbs.length - MAX_CRUMBS);
    };

    hub.trouble = ({ app, level, kind, message, ms }) => {
      const line = {
        app: app ?? hub.self ?? "page",
        level: level === "error" ? "error" : "warn",
        kind: cut(kind ?? "note", 40),
        message: cut(message, MAX_MESSAGE),
        at: now(),
        count: 1,
        page: cut(g.location?.pathname ?? "", 200),
        online: g.navigator?.onLine !== false,
        ms: typeof ms === "number" ? ms : null,
        trail: hub.crumbs.slice(-10).map((c) => `${c.app ?? "page"}: ${c.message}`),
      };
      // The same thing again within a minute is one line said more times:
      // a page offline for an hour is not three hundred lines.
      const last = hub.items.findLast?.((one) => one.app === line.app && one.kind === line.kind && one.message === line.message);
      if (last && line.at - last.at <= SAME_WITHIN_S && !last.sent) {
        last.count += 1;
        last.at = line.at;
      } else {
        hub.items.push(line);
        if (hub.items.length > MAX_KEPT) hub.items.splice(0, hub.items.length - MAX_KEPT);
      }
      hub.crumb({ app: line.app, message: `${line.kind}: ${cut(line.message, 120)}` });
      save();
      later(5_000);
    };

    // Sent as `Impulse { token, particle = PageReport { device, items } }`
    // to the configured door, with `fetch` rather than `net_client`, so that
    // sending trouble is never itself trouble written down here. Taken off
    // the device only once the far side answers `Reported`.
    hub.flush = (leaving) => {
      if (!hub.dest || !hub.dest.token || hub.items.length === 0) return false;
      if (hub.sending && !leaving) return false;
      if (g.navigator?.onLine === false) return false;
      const batch = hub.items.slice(0, BATCH);
      const body = JSON.stringify({
        _class: "Impulse",
        token: hub.dest.token,
        particle: { _class: "PageReport", device: device(), items: batch.map(({ sent, ...one }) => one) },
      });
      const done = () => {
        hub.items = hub.items.filter((one) => !batch.includes(one));
        save();
        if (hub.items.length > 0) later(1_000);
      };
      // Leaving the page: the one send a closing page still makes.
      if (leaving && typeof g.navigator?.sendBeacon === "function") {
        try {
          if (g.navigator.sendBeacon(hub.dest.url, new Blob([body], { type: "application/json" }))) done();
        } catch {
          // Kept for the next visit.
        }
        return true;
      }
      if (typeof g.fetch !== "function") return false;
      hub.sending = true;
      batch.forEach((one) => (one.sent = true));
      const stop = typeof AbortController === "function" ? new AbortController() : null;
      const timer = setTimeout(() => stop?.abort(), 15_000);
      timer?.unref?.();
      g.fetch(hub.dest.url, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body,
        keepalive: body.length < 60_000,
        signal: stop?.signal,
      })
        .then((r) => r.json())
        .then((answer) => {
          if (answer?._class === "Reported") done();
          else batch.forEach((one) => delete one.sent);
        })
        .catch(() => batch.forEach((one) => delete one.sent))
        .finally(() => {
          clearTimeout(timer);
          hub.sending = false;
        });
      return true;
    };

    g.addEventListener?.("error", (e) => {
      const where = e?.filename ? ` (${cut(e.filename, 200)}:${e.lineno ?? 0})` : "";
      hub.trouble({ app: null, level: "error", kind: "uncaught", message: `${e?.message ?? e?.error ?? "error"}${where}` });
    });
    g.addEventListener?.("unhandledrejection", (e) => {
      const why = e?.reason?.message ?? e?.reason ?? "rejected";
      hub.trouble({ app: null, level: "error", kind: "uncaught", message: `unhandled rejection: ${why}` });
    });
    g.addEventListener?.("online", () => hub.flush(false));
    g.addEventListener?.("pagehide", () => hub.flush(true));
    g.document?.addEventListener?.("visibilitychange", () => {
      if (g.document.visibilityState === "hidden") hub.flush(true);
    });
    const every = setInterval(() => hub.flush(false), EVERY_MS);
    every?.unref?.();
    return hub;
  };

  const hub = g.__code_telemetry ?? (g.__code_telemetry = createHub());
  const me = ctx.program ?? null;

  const sameOrigin = (url) => typeof url === "string" && /^\/[^/?#\s][^?#\s]*$/.test(url);

  return [
    "telemetry",
    (particle) => {
      switch (particle._class) {
        // Where trouble goes, and as whom. A same-origin path only: a page
        // sends what went wrong on it to its own server, nowhere else. A
        // null token stops the sending (signed out) and keeps what is there.
        case "Configure": {
          if (!sameOrigin(particle.url)) {
            return { _class: "Exception", source: "telemetry", message: "Configure needs a same-origin path, like /api/logs" };
          }
          const token = typeof particle.token === "string" && particle.token ? particle.token : null;
          hub.dest = { url: particle.url, token };
          if (typeof particle.app === "string" && particle.app) hub.self = particle.app;
          else if (me) hub.self = me;
          if (token) hub.flush(false);
          return { _class: "ConfigureResult", ok: true };
        }
        // An application's own word: "warn" or "error" is a line of trouble,
        // anything else a step on the trail that goes with the next one.
        case "Note": {
          const message = typeof particle.message === "string" ? particle.message : "";
          if (!message) return { _class: "NoteResult", ok: false };
          const level = particle.level;
          if (level === "warn" || level === "error") {
            const kind = typeof particle.kind === "string" && particle.kind ? particle.kind : "note";
            hub.trouble({ app: me, level, kind, message });
          } else {
            hub.crumb({ app: me, message });
          }
          return { _class: "NoteResult", ok: true };
        }
        case "Flush":
          return { _class: "FlushResult", ok: hub.flush(false) };
        // What is waiting to be sent, oldest first.
        case "Kept":
          return { _class: "KeptResult", items: hub.items.map(({ sent, ...one }) => one) };
        default:
          return null;
      }
    },
  ];
}
