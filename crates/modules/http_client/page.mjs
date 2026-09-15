// The page's half of `http_client`: public HTTP from a browser.
//
// The machine half is synchronous and returns HttpResponse directly. A page
// cannot wait inside a handler without freezing the UI, so the browser half
// returns HttpResult immediately and fires HttpResponse later with the same
// response fields plus `_request_id`. The response body stays text on both
// backends; callers that need JSON link the `json` module and Parse it.
(ctx) => {
  const nextRequestId = () => {
    const key = "__euglena_http_client_next_request_id";
    const current = Number(globalThis[key] ?? 1);
    globalThis[key] = current + 1;
    return current;
  };

  const exception = (message, id) => ({
    _class: "Exception",
    source: "http_client",
    message,
    ...(id === undefined ? {} : { _request_id: id }),
  });

  const text = (value) => {
    if (typeof value === "string") return value;
    if (typeof value === "number" || typeof value === "boolean") return String(value);
    return "";
  };

  const headers = (value) => {
    if (value === null || typeof value !== "object" || Array.isArray(value)) return {};
    const out = {};
    for (const [name, header] of Object.entries(value)) out[name] = text(header);
    return out;
  };

  const request = (particle, method) => {
    if (typeof particle.url !== "string") {
      return exception("request needs a `url` string");
    }
    let url;
    try {
      // Relative URLs are useful when the same app is served locally and
      // through a public host; absolute HTTP(S) URLs support public APIs such
      // as GitHub Releases. `fetch` remains responsible for CORS and TLS.
      url = new URL(particle.url, typeof location === "undefined" ? "http://localhost/" : location.href).href;
      if (!/^https?:$/.test(new URL(url).protocol)) throw new Error("URL must use http or https");
    } catch (error) {
      return exception(`invalid url: ${error?.message ?? error}`);
    }

    const timeoutSeconds =
      typeof particle.timeout_seconds === "number" &&
      Number.isFinite(particle.timeout_seconds) &&
      particle.timeout_seconds > 0
        ? particle.timeout_seconds
        : 10;
    const maxBodyBytes =
      typeof particle.max_body_bytes === "number" &&
      Number.isFinite(particle.max_body_bytes) &&
      particle.max_body_bytes > 0
        ? particle.max_body_bytes
        : 1_048_576;
    const id = nextRequestId();
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), timeoutSeconds * 1000);
    const init = {
      method,
      headers: headers(particle.headers),
      signal: controller.signal,
    };
    if (method === "POST" || method === "PUT" || method === "PATCH") {
      init.body = text(particle.body);
      init.headers["Content-Type"] =
        typeof particle.content_type === "string"
          ? particle.content_type
          : "application/octet-stream";
    }

    fetch(url, init)
      .then(async (response) => {
        const body = await response.text();
        const bytes = new TextEncoder().encode(body).byteLength;
        if (bytes > maxBodyBytes) {
          throw new Error(`response body exceeds max_body_bytes (${maxBodyBytes})`);
        }
        ctx.fire({
          _class: "HttpResponse",
          ok: true,
          status: response.status,
          body: method === "HEAD" ? "" : body,
          _request_id: id,
        });
      })
      .catch((error) => {
        const why = error?.name === "AbortError" ? `no answer within ${timeoutSeconds}s` : String(error);
        ctx.fire(exception(`cannot reach '${url}': ${why}`, id));
      })
      .finally(() => clearTimeout(timer));

    return { _class: "HttpResult", ok: true, value: id };
  };

  return [
    "http_client",
    (particle) => {
      switch (particle._class) {
        case "Get":
          return request(particle, "GET");
        case "Post":
          return request(particle, "POST");
        case "Put":
          return request(particle, "PUT");
        case "Patch":
          return request(particle, "PATCH");
        case "Delete":
          return request(particle, "DELETE");
        case "Head":
          return request(particle, "HEAD");
        case "Options":
          return request(particle, "OPTIONS");
        default:
          return null;
      }
    },
  ];
}
