// The page's half of `net_client`: one POST, and the answer as a particle.
//
// `Send` cannot answer with the reply here — waiting means blocking, and
// blocking in a page freezes the reader. So it answers as soon as the request
// is on its way, and the reply arrives later as a particle at the program's
// own handlers, carrying `_request_id` so two exchanges that both answer
// `Pong` can be told apart.
(ctx) => {
  let next = 1;

  // What a sender is told when the exchange never produced a particle. The
  // same `Exception` shape the machine half returns and `net_server` sends,
  // so a program has one way to read a failure wherever it happened.
  const failed = (id, message) => ({
    _class: "Exception",
    source: "net_client",
    message,
    _request_id: id,
  });

  return [
    "net_client",
    (particle) => {
      if (particle._class !== "Send") return null;

      const url = particle.url;
      const payload = particle.particle;
      if (typeof url !== "string" || !/^http:\/\//.test(url)) {
        // Refused rather than sent: `https://` would be TLS the machine half
        // does not speak either, and anything else is not an address.
        return { _class: "SendResult", ok: false, value: null };
      }
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
            return ctx.fire(failed(which, `answer from '${url}' is not JSON`));
          }
          if (answer === null || typeof answer !== "object" || Array.isArray(answer)) {
            return ctx.fire(failed(which, `answer from '${url}' is not a particle`));
          }
          if (typeof answer._class !== "string") {
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
          ctx.fire({ ...answer, _request_id: which });
        })
        .catch((e) => {
          const why = e?.name === "AbortError" ? `no answer within ${timeoutMs}ms` : String(e);
          ctx.fire(failed(which, `cannot reach '${url}': ${why}`));
        })
        .finally(() => clearTimeout(timer));

      // The request is on its way. Whether it arrives is a later particle.
      return { _class: "SendResult", ok: true, value: which };
    },
  ];
}
