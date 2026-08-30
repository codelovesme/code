# `mailer` — send email over SMTP

SMTP, because every provider speaks it — Gmail, Amazon SES, Postmark,
Mailgun, Azure Communication Services, a relay of your own. One `Config`
with your provider's SMTP settings, then `Send`.

```code
link "mailer.so" as mail

emit Config {
    host = "email-smtp.us-east-1.amazonaws.com",
    username = "${SMTP_USER}",
    password = "${SMTP_PASS}",
    from = "no-reply@myapp.com"
} to mail get c
assert c.ok

emit Send {
    recipient = "user@example.com",
    subject = "Welcome",
    html = "<h1>Hi</h1><p>Thanks for signing up.</p>"
} to mail get _
```

## Handlers

```
Config { host, port?, username?, password?, from, tls? } → ConfigResult { ok }
Send   { recipient, subject?, text?, html?, from?, cc?, bcc? } → SendResult { ok }
```

`Config` is the setup particle — `Send` is an `Exception` until it has run.

| `Config` field | Default | Meaning |
|---|---|---|
| `host` | — | the SMTP server |
| `port` | `587`, or `465` when `tls = "wrapper"` | a whole number in `1..=65535` |
| `username` / `password` | — | SMTP auth. Both, or neither — one alone is an `Exception` |
| `from` | — | the default sender. Validated as an address now, not on the first `Send` |
| `tls` | `"starttls"` | `"starttls"` (upgrade on port 587), `"wrapper"` (implicit TLS on 465), or `"none"` (plaintext — a local relay or a test, never the internet) |

| `Send` field | Meaning |
|---|---|
| `recipient` / `cc` / `bcc` | a string, or an array of strings. `recipient` is `to` under another name (`to` is a keyword in the language) |
| `subject` | defaults to `""` |
| `text` / `html` | the body. Give `html` and it's sent as HTML; `text` otherwise; neither sends an empty body |
| `from` | overrides the `Config` default for this one message |

## Errors

A message the SMTP server **rejects** — a bad mailbox, a rate limit, auth
refused — is an `Exception` carrying the server's reply. So is a
**malformed address**, a **missing transport** (no `Config`), or a
**connection that won't open**. `Send` only answers `SendResult { ok }` when
the server has accepted the message for delivery.

Acceptance is not delivery: a later bounce is between the provider and the
recipient, and this module never sees it.

## The decisions, and why

**Ported from `euglena-language`'s `mailer` organelle**, with two changes:

- **SMTP, not Azure.** The organelle built HMAC-signed requests against
  Azure Communication Services' REST endpoint. SMTP reaches Azure too — and
  everyone else — through code that isn't tied to one vendor's API version.
- **A rejected send is an `Exception`**, where the organelle returned
  `SendResult { success = false, message }`. A caller that wants to treat a
  send failure as non-fatal catches the `Exception`; the default is that it
  propagates.

**No attachments, no templating, no queue.** A body is `text` or `html`. A
program that needs a retrying queue builds one on top; a program that needs
attachments has a well-defined feature to ask for.

**TLS is on by default.** `tls = "none"` exists for a loopback relay and for
`tests/mailer_module.rs`; using it against a real server sends credentials
in the clear.

## Build

```sh
cargo build --release        # -> target/release/libmailer.so
```
