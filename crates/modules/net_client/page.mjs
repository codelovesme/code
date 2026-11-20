// The page's half of `net_client`: one POST, and the answer as a particle.
//
// `Send` cannot answer with the reply here — waiting means blocking, and
// blocking in a page freezes the reader. So it answers as soon as the request
// is on its way, and the reply arrives later as a particle at the program's
// own handlers, carrying `_request_id` so two exchanges that both answer
// `Pong` can be told apart.
(ctx) => {
  let next = 1;
  let destination = null;
  const exception = (message) => ({ _class: "Exception", source: "net_client", message });

  // What a sender is told when the exchange never produced a particle. The
  // same `Exception` shape the machine half returns and `net_server` sends,
  // so a program has one way to read a failure wherever it happened.
  const failed = (id, message) => ({
    _class: "Exception",
    source: "net_client",
    message,
    _request_id: id,
  });

  // What `telemetry` keeps of an exchange, when the page links it: a step on
  // the trail for every one, and a line of trouble for one that never came
  // back as a particle. Class names and times only — never what was said.
  const said = (payload) =>
    payload.particle && typeof payload.particle._class === "string"
      ? `${payload._class}/${payload.particle._class}`
      : payload._class;
  const step = (message) => globalThis.__code_telemetry?.crumb?.({ app: ctx.program, message });
  const trouble = (message, ms) =>
    globalThis.__code_telemetry?.trouble?.({ app: ctx.program, level: "warn", kind: "net", message, ms });

  return [
    "net_client",
    (particle) => {
      if (particle._class === "Config") {
        const url = particle.url;
        if (typeof url !== "string") return exception("Config needs a `url` string");
        // Three spellings, and the first is the one a published application
        // wants. A **same-origin path** (`/api/todo`) names a destination
        // without naming a host: the browser resolves it against the page, so
        // one build runs on a laptop and behind a public domain alike — and a
        // page served over https cannot reach an `http://` destination at all,
        // which is what makes a hard-coded host a dead end once anything is
        // published. An **absolute** `http(s)://host[:port]/app` is the other
        // origins. Either way there is no query and no fragment: the body is
        // the particle, so a destination is a destination, not a request.
        const relative = url === "/" || /^\/[^/?#\s][^?#\s]*$/.test(url);
        const absolute = /^https?:\/\/[^/?#\s]+(?:\/[^?#\s]*)?$/.test(url);
        if (!relative && !absolute) {
          return exception(
            "Config url must be a same-origin path (/app) or http(s)://host[:port]/app, with no query or fragment"
          );
        }
        try {
          // A relative path needs a base to resolve against; an absolute one
          // ignores it. `location` is the page's in a browser and absent in a
          // test harness, so a placeholder stands in to check the shape.
          const base =
            typeof location !== "undefined" && location.origin ? location.origin : "http://localhost";
          new URL(url, base);
        } catch {
          return exception("Config url is not a valid HTTP destination");
        }
        destination = url;
        return { _class: "ConfigResult", ok: true };
      }
      if (particle._class !== "Send") return null;
      if (Object.hasOwn(particle, "url")) return exception("url belongs in Config, not Send");
      if (destination === null) return exception("net_client needs Config before Send");

      const url = destination;
      const payload = particle.particle;
      if (
        payload === null ||
        typeof payload !== "object" ||
        Array.isArray(payload) ||
        typeof payload._class !== "string"
      ) {
        return { _class: "SendResult", ok: false, value: null };
      }

      const timeoutMs =
        typeof particle.timeout_ms === "number" && particle.timeout_ms > 0
          ? particle.timeout_ms
          : 10_000;
      const which = next++;

      // Aborted rather than left hanging: a request nobody will answer would
      // otherwise be a particle that never arrives, which is the one failure
      // an application cannot see.
      const stop = new AbortController();
      const timer = setTimeout(() => stop.abort(), timeoutMs);
      const began = Date.now();
      const what = said(payload);

      fetch(url, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(payload),
        signal: stop.signal,
      })
        .then((response) => response.text().then((text) => ({ response, text })))
        .then(({ response, text }) => {
          let answer;
          try {
            answer = JSON.parse(text);
          } catch {
            trouble(`${what} to ${url}: the answer is not JSON (${response.status})`, Date.now() - began);
            return ctx.fire(failed(which, `answer from '${url}' is not JSON`));
          }
          if (answer === null || typeof answer !== "object" || Array.isArray(answer)) {
            trouble(`${what} to ${url}: the answer is not a particle (${response.status})`, Date.now() - began);
            return ctx.fire(failed(which, `answer from '${url}' is not a particle`));
          }
          if (typeof answer._class !== "string") {
            trouble(`${what} to ${url}: the answer has no class (${response.status})`, Date.now() - began);
            // A far side that answered something, but not something with a
            // class — including anything that is not a `net_server` at all,
            // which is what a status line here usually means.
            return ctx.fire(
              failed(which, `answer from '${url}' has no \`_class\` (${response.status})`)
            );
          }
          // The far side's own particle, with the number this exchange is
          // known by added. Its own `_request_id`, if it had one, is not ours
          // to keep — this one names *this* request.
          step(`${what} → ${answer._class} ${Date.now() - began}ms`);
          // The far side failing in its own words is trouble too, said by it.
          if (answer._class === "Exception" || answer._class === "Unavailable") {
            const why = String(answer.message ?? answer.reason ?? "").slice(0, 300);
            globalThis.__code_telemetry?.trouble?.({ app: ctx.program, level: "error", kind: "service", message: `${what} to ${url}: ${answer._class} ${why}`, ms: Date.now() - began });
          }
          ctx.fire({ ...answer, _request_id: which });
        })
        .catch((e) => {
          const why = e?.name === "AbortError" ? `no answer within ${timeoutMs}ms` : String(e);
          trouble(`${what} to ${url}: ${why}`, Date.now() - began);
          ctx.fire(failed(which, `cannot reach '${url}': ${why}`));
        })
        .finally(() => clearTimeout(timer));

      // The request is on its way. Whether it arrives is a later particle.
      return { _class: "SendResult", ok: true, value: which };
    },
  ];
}
