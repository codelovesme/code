# `azure_mailer` — send email through Azure Communication Services

[`mailer`](../mailer) speaks SMTP, which every provider understands. This
speaks one provider's REST API, and exists for the case where that is the
credential you have: Azure hands out a **connection string**, so an account
can send without a mailbox and a password existing anywhere.

```code
link "azure_mailer.so" as mail

emit Config {
    connection_string = "${AZURE_EMAIL_CONNECTION_STRING}",
    from = "DoNotReply@example.com"
} to mail get c
assert c.ok

emit Send {
    recipient = "reader@example.com",
    subject = "Welcome",
    html = "<h1>Hi</h1><p>Thanks for signing up.</p>"
} to mail get r
assert r.ok
```

## Handlers

```
Config { connection_string, from }                             → ConfigResult { ok }
Send   { recipient, subject?, text?, html?, from?, cc?, bcc? }  → SendResult   { ok, operation }
```

`Config` is the setup particle — `Send` is an `Exception` until it has run.

| `Config` field | Meaning |
|---|---|
| `connection_string` | exactly what Azure gives you: `endpoint=https://<name>.<region>.communication.azure.com/;accesskey=<base64>`. Either order, any spacing, and the key names are matched case-insensitively |
| `from` | the default sender. Must be an address on a domain **linked to that Communication Service**, or Azure refuses every message |

`Send`'s fields are [`mailer`](../mailer)'s, and that is the point — see
below. `operation` on the result is Azure's id for the send, which is how a
delivery is followed up afterwards.

## `Send` is `mailer`'s, field for field

Moving between them is **one word in a manifest** and not a line of any
gene — the same promise [`membrane`](../membrane) makes to
[`net_server`](../net_server). `recipient` is `to` under another name (`to`
is a keyword in the language); `recipient`/`cc`/`bcc` each take a string or
an array of strings; `html` wins over `text`, and neither sends an empty
body.

Only `Config` differs, because SMTP and a REST API are configured by
genuinely different things — a host, a port and a password on one side, a
connection string on the other. Everything a program *writes* is the same.

[`mailer_mock`](../mailer_mock) is the mock twin for this module too, for
the same reason: it captures the same `Send` and files it in an `Outbox`. Its
`Config` is SMTP-shaped, so a manifest's mock overlay gives it `host` and
`from` while the real organelle gets a connection string.

## How a request is signed

Azure's HMAC-SHA256 scheme. The access key signs the verb, the path, and the
date, host and **a hash of the body** it was sent with:

```
POST\n/emails:send?api-version=…\n<date>;<host>;<sha256 of body>
```

Signing the body's hash is what stops a captured signature being replayed
against a different message. The key is base64 *in* the connection string and
raw bytes once decoded — signing with the text produces a signature the
service rejects with nothing to say why, which is worth knowing if you ever
port this.

The API version is **pinned** (`2023-03-31`) rather than floating: a provider
changing its contract under a running program is the failure that avoids.

## Accepted is not delivered

`ok` means Azure took the message for delivery. A later bounce is between
Azure and the recipient, and this module never sees it — the same honesty
[`mailer`](../mailer) keeps about SMTP. Everything Azure refuses outright —
an unlinked sender domain, a bad key, a malformed address, a rate limit —
is an `Exception` carrying Azure's own explanation, because a status code
alone is not something a program can act on.

## What you need on the Azure side

A Communication Service, an Email Service, and a **domain linked to the
Communication Service** with its verification passing. An Azure-managed
domain (`<guid>.azurecomm.net`) works immediately; your own needs SPF and
DKIM records before it will send.

## The decisions, and why

**Ported from `euglena-platform`'s `mailer` organelle**, which spoke this
same API under the old ABI. Changed:

- **`Sap` became `Config`**, and a failure is an `Exception` where the
  organelle returned `Error { status_code }`.
- **`from_address` became `from`, and `body_html` became `html`**, so the
  `Send` surface is `mailer`'s exactly. The old names were this provider's
  spelling leaking into every program that used it.
- **The refusal is passed through.** The organelle logged and returned a
  generic failure; Azure explains itself in the response body, and that is
  the part an operator needs.

**No attachments, no templates, no delivery-status polling.** Each is a real
feature to add when asked; the applications here needed a verification code
to arrive.

## Build

```sh
cargo build --release        # -> target/release/libazure_mailer.so
```
