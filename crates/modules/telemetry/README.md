# `telemetry` — what went wrong on a page

```code
link "telemetry.a" as telemetry

| Once, by the program that is the page, after sign-in:
emit Configure { url = "/api/logs", token = token, app = "host-web" } to telemetry

| Anywhere, by any program on the page:
emit Note { level = "error", kind = "sync", message = "the upload was refused" } to telemetry
emit Note { message = "opened the draft" } to telemetry
```

## The handlers

```
Configure { url, token, app? } → ConfigureResult { ok }
Note { level, message, kind? } → NoteResult { ok }
Flush                          → FlushResult { ok }
Kept                           → KeptResult { items }
```

A page on somebody's phone has no console anyone reads. This keeps the
trouble on it — script errors nothing caught, a module half that threw, a
request that never came back as a particle (`net_client` writes these),
an application's own `Note` at `warn` or `error` — each with the last ten
steps before it (every exchange's classes and time, and `Note`s at other
levels). It is kept on the device (`localStorage`, at most 200 lines; the
same line again within a minute is counted, not repeated) and sent in
batches of 50 every 30 seconds, when the network comes back, and with
`sendBeacon` as the page closes:

```
Impulse { token, particle = PageReport { device, items } }  →  Reported
```

A line leaves the device only once the far side answers `Reported`.

**Never kept:** what was said. Class names, times, messages of errors, the
page's path and the device's kind — not particles, not anything typed.

## One collector per page

A shell and every guest it runs each link their own copy, and all of them
write into one collector, each line naming the program it came from. The
shell configures it; a guest only says what it has to say.

## Where it works

**A browser.** On a machine every handler answers an `Exception`: a held
program there has `Log` through its membrane, which the host keeps.
