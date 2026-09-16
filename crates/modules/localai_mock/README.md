# `localai_mock` — `localai` without a model server

A drop-in for [`localai`](../localai/README.md): the same `Config`, `Chat`,
`ChatJson`, `Transcribe` / `TranscribeWithOptions` particles and the same
result shapes, with canned but deterministic replies.

```code
link "localai_mock.so" as ai

emit Config { endpoint = "mock", model = "tiny" } to ai get c

emit Chat { system = "be terse", user = "what is 2+2?" } to ai get r
assert r.content = "[mock tiny] what is 2+2?"

emit ChatJson { user = "give me json" } to ai get j
assert j.content = "{}"

emit Transcribe { audio_base64 = "…" } to ai get t
assert t.text = "[mock transcript]"
```

## The replies

- **`Chat`** → `"[mock <model>] <last user message>"`. `messages` is read
  the same way as in `localai` — the last `user`-role turn wins.
- **`ChatJson`** → `"{}"`, valid JSON so a downstream `Parse` succeeds.
- **`Transcribe`** → `"[mock transcript]"`, `language` echoed from the
  request.

Every field on `Config` is accepted; only `model` is used (it appears in
`Chat` replies).

## Answering later

`Chat` and `ChatJson` with `later = true` answer `Sent { value = id }` at
once and push the same `ChatResult` with `_request_id = id` onto the
program's inbound ring a moment later — the shape `localai` answers with
`later`, so a program's state machine around a slow model call is tested
without a model:

```code
ChatResult { content, _request_id as id } =>
    …
emit ChatJson { user = "…", later = true } to ai get sent
assert sent ∈ Sent
```

## Build

```sh
cargo build --release        # -> target/release/liblocalai_mock.so
```
