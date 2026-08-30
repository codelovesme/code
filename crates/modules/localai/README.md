# `localai` — chat and transcription over an OpenAI-compatible endpoint

Points at a local model server — LocalAI, llama.cpp's `server`, Ollama's
OpenAI shim, vLLM, anything that speaks `/v1/chat/completions` and
`/v1/audio/transcriptions`.

```code
link "localai.so" as ai

emit Config { endpoint = "${LOCALAI_ENDPOINT}", model = "${LOCALAI_MODEL}" } to ai get c
assert c.ok

emit Chat { system = "You are terse.", user = "Capital of France?" } to ai get r
assert r.content ≠ ""
```

## Handlers

```
Config   { endpoint, model?, max_tokens?, temperature?, timeout_seconds? }  → ConfigResult { ok }
Chat     { system?, user?, messages?, model?, temperature?, max_tokens? }   → ChatResult   { content }
ChatJson { … same … }                                                      → ChatResult   { content }
Transcribe { audio_base64, language?, model?, audio_format? }               → TranscribeResult { text, language }
```

`TranscribeWithOptions` is an alias for `Transcribe`. `Config` is the setup
particle — everything else is an `Exception` until it has run.

## Config

- **`endpoint`** — the server root (`http://host:8080`). `/v1` is appended
  unless it's already there.
- **`model` / `max_tokens` / `temperature`** — defaults (`gpt-4`, `4096`,
  `0.3`) that every `Chat` and `Transcribe` can override per call.
- **`timeout_seconds`** — whole-request budget, default `300`. Local models
  on CPU are slow; a language with no way to interrupt itself needs the
  ceiling to be generous but real.

## Chat

`messages` is an array of `{ role, content }` for a multi-turn conversation:

```code
emit Chat { messages = [
    { role = "system", content = "You are a helpful assistant." },
    { role = "user", content = "Remember the number 7." },
    { role = "assistant", content = "Got it — 7." },
    { role = "user", content = "What was the number?" }
] } to ai get r
```

Without `messages`, `system` + `user` are the whole conversation (the euglena
organelle took only these two). At least one of the three must be non-empty.

`<think>…</think>` blocks — reasoning models (Qwen3, DeepSeek-R1) emit them
ahead of the answer — are stripped from every reply.

## ChatJson

The same request, but `content` is the reply with one wrapping
```` ```json … ``` ```` fence removed and then **validated as JSON**. A reply
that isn't JSON is an `Exception` — if the model can't be trusted to return
JSON, that's a fact worth stopping on. `content` comes back as canonical
JSON text (re-serialised, so the model's whitespace doesn't leak through);
parse it with the `json` module if you want a value.

## Transcribe

`audio_base64` is the audio file, base64-encoded (the language holds text,
not bytes). `audio_format` names the container for the multipart part
(`webm` default, `mp3`, `wav`, `m4a`, …). `language` is an optional hint,
echoed back on the result.

## The decisions, and why

**Ported from `euglena-language`'s `localai` organelle**, with these
changes:

- **`Sap` became `Config`**, and it no longer pings `/models` on the way in —
  "connect" and "is it up?" are different questions, and the first `Chat`
  answers the second.
- **A failed call is an `Exception`**, where the organelle returned
  `{ ok: false, content: <message> }` on every result — so a caller that
  forgot to check `ok` fed an error string to the model as if it were a
  reply.
- **`Chat` takes a `messages` array** for multi-turn. The organelle's
  callers concatenated the history into one `user` string by hand.
- **`Transcribe` and `TranscribeWithOptions` are one handler.** The split
  bought nothing — the options were all optional.
- **Code fences come off only for `ChatJson`.** The organelle stripped them
  from plain `Chat` too, which mangled any reply that legitimately contained
  a code block.

**No streaming, no function calling, no embeddings, no image input.** Each is
a real feature to add when asked; the euglena apps needed a completion and a
transcript.

## Build

```sh
cargo build --release        # -> target/release/liblocalai.so
```
