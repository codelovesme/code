// The page's half of `net_client`: one POST, and the answer as a particle.
//
// `Send` cannot answer here — waiting for a reply means blocking, and
// blocking in a page freezes the reader. So the request goes out, the module
// answers `SendResult { ok, value }` at once, and when the reply comes it is
// fired as a particle at the program's own handlers, carrying `_request_id`
// so two exchanges that both answer `Pong` can be told apart.
(ctx) => {
  const { str, fire } = ctx;

  // What a sender is told when the exchange never produced a particle. The
  // same `Exception` shape the machine half returns and `net_server` sends,
  // so a program has one way to read a failure wherever it happened.
  const failed = (id, message) => ({
    _class: "Exception",
    source: "net_client",
    message,
    _request_id: id,
  });

  return {
    code_web_send(id, urlPtr, urlLen, bodyPtr, bodyLen, timeoutMs) {
      // Copied out now: these are the module's own buffers, and it will have
      // refilled them long before this reply comes back.
      const url = str(urlPtr, urlLen);
      const body = str(bodyPtr, bodyLen);
      const which = Number(id);

      if (!/^http:\/\//.test(url)) {
        // Refused rather than sent: `https://` would be TLS the machine half
        // does not speak either, and anything else is not an address.
        fire(failed(which, `url must start with \`http://\` — got '${url}'`));
        return 1;
      }

      // Aborted rather than left hanging: a request nobody will answer would
      // otherwise be a particle that never arrives, which is the one failure
      // an application cannot see.
      const stop = new AbortController();
      const timer = setTimeout(() => stop.abort(), timeoutMs);

      fetch(url, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body,
        signal: stop.signal,
      })
        .then((response) => response.text().then((text) => ({ response, text })))
        .then(({ response, text }) => {
          let answer;
          try {
            answer = JSON.parse(text);
          } catch {
            return fire(failed(which, `answer from '${url}' is not JSON`));
          }
          if (answer === null || typeof answer !== "object" || Array.isArray(answer)) {
            return fire(failed(which, `answer from '${url}' is not a particle`));
          }
          if (typeof answer._class !== "string") {
            // A far side that answered something, but not something with a
            // class — including anything that is not a `net_server` at all,
            // which is what a status line here usually means.
            return fire(
              failed(which, `answer from '${url}' has no \`_class\` (${response.status})`)
            );
          }
          // The far side's own particle, with the number this exchange is
          // known by added. Its own `_request_id`, if it had one, is not
          // ours to keep — this one names *this* request.
          fire({ ...answer, _request_id: which });
        })
        .catch((e) => {
          const why = e?.name === "AbortError" ? `no answer within ${timeoutMs}ms` : String(e);
          fire(failed(which, `cannot reach '${url}': ${why}`));
        })
        .finally(() => clearTimeout(timer));

      // The request is on its way. Whether it arrives is a later particle.
      return 1;
    },
  };
}
