# `ntfy` — a push notification to a phone

```code
link "ntfy.so" as push

emit Config { topic = "$NTFY_TOPIC" } to push
| ntfy.sh unless `url` says otherwise; a `token` or `username`+`password`
| when the server asks for one.

emit Notify {
    title = "Change a battery"
    message = "The bedroom sensor is at 12%."
    priority = "high"
    tags = ["battery", "warning"]
    click = "https://apps.codeloves.me/home"
} to push get sent
| sent ∈ Notified { ok, id }, or an Exception carrying the server's words
```

[ntfy](https://ntfy.sh) is a topic on an HTTP server: whoever is subscribed
to the topic in the ntfy app gets the message on their phone, at once, with
no account and no vendor. A server of your own speaks the same protocol
(`url = "https://ntfy.example.org"`). **A topic's name is its password** —
anyone who knows it can read and post to it — so it lives in the
environment (`${NTFY_TOPIC}` in a manifest), never in the source.

## The handlers

```
Config { topic, url?, token?, username?, password?, timeout_seconds? }  → ConfigResult { ok }
Notify { message, title?, priority?, tags?, click?, topic?, markdown? } → Notified { ok, id }
```

`Config` is the setup particle. `url` is `https://ntfy.sh` unless said;
`timeout_seconds` is 10. A `topic` holds letters, digits, `-` and `_`.

`Notify` is one `POST` to `<url>/<topic>` with `message` as the body and
the rest as ntfy's headers:

- `title` — shown above the message.
- `priority` — `1`–`5`, or `min` / `low` / `default` / `high` / `urgent`
  (`max`). `high` and above make the phone sound; `min` is silent.
- `tags` — a string or an array of strings; an emoji short code (`warning`,
  `battery`, `tada`, `house`) becomes the icon.
- `click` — a URL the notification opens when tapped.
- `topic` — this one notification to another topic than Config's.
- `markdown = true` — the message is Markdown.

The answer carries the message's `id` from the server. A server that
refuses (a topic that needs auth, a rate limit) is an `Exception` with the
status and the server's words; a server that does not answer within the
timeout is one too. Nothing is queued: a notification about now is worth
nothing in an hour.

## A euglena organelle

```json
"push": { "module": "ntfy", "config": { "topic": "${NTFY_TOPIC}" } }
```

## Tests

`tests/ntfy_unreachable.code` needs no network: every check of the
particle, and a server that is not there. `tests/ntfy_module.rs` carries a
few lines of HTTP as a server of its own and reads the one request back —
path, body, every header — in both output modes.
