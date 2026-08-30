# `mailer_mock` — `mailer` without an SMTP server

A drop-in for [`mailer`](../mailer/README.md): the same `Config` and `Send`
particles, the same `ConfigResult { ok }` / `SendResult { ok }` results, but
`Send` files the message into an in-memory outbox instead of delivering it.

```code
link "mailer_mock.so" as mailer

emit Config { host = "mock", from = "app@example.com" } to mailer get c
emit Send { recipient = "user@example.com", subject = "Hi", text = "hello" } to mailer get _

emit Outbox { } to mailer get box
assert box.count = 1
assert box.messages[0].subject = "Hi"
```

## Beyond `mailer`'s surface

```
Outbox { clear? } → Outbox { messages, count }
```

Every message `Send` has captured since the module loaded, oldest first.
Each is `SentMessage { from, recipient, cc, bcc, subject, body }` —
`recipient`/`cc`/`bcc` are comma-joined when an array was passed.
`Outbox { clear = true }` empties the outbox after returning it.

The outbox is process-global (one copy per euglena cell), so a test drives
the app and then reads `Outbox` to assert what would have been sent.

## Build

```sh
cargo build --release        # -> target/release/libmailer_mock.so
```
