//! The `localai` native module — chat completions and audio transcription
//! over an OpenAI-compatible endpoint, for the Code programming language,
//! written in Rust on [`code-native`] over `ureq`.
//!
//! Points at a local model server (LocalAI, llama.cpp's server, Ollama's
//! OpenAI shim, vLLM, …) — anything that speaks `/v1/chat/completions` and
//! `/v1/audio/transcriptions`.
//!
//! Handlers:
//!
//! - `Config { endpoint, api_key?, model?, max_tokens?, temperature?, timeout_seconds? }`
//!   → `ConfigResult { ok }` — the setup particle. `endpoint` is the server
//!   root (`http://host:8080` or `.../v1`); the rest are defaults every
//!   `Chat` can override.
//! - `Chat { system?, user?, messages?, model?, temperature?, max_tokens? }`
//!   → `ChatResult { content }` — one completion. `messages` is an array of
//!   `{ role, content }` for multi-turn; without it, `system` + `user` are
//!   the conversation.
//! - `ChatJson { … same … }` → `ChatResult { content }` — `content` is the
//!   model's reply with a surrounding ```-fence stripped, validated as JSON.
//!   Not valid JSON → `Exception`.
//! - `Transcribe { audio_base64, language?, model?, audio_format? }` →
//!   `TranscribeResult { text, language }` — Whisper-style transcription.
//!   `TranscribeWithOptions` is an alias.
//! - `Speak { text, model?, voice?, format?, later? }` → `SpeakResult {
//!   audio_base64, format }` — text to speech through `/audio/speech`. The
//!   bytes come back base64, `format` is the audio subtype the server chose
//!   ("wav", "mp3"). With `later = true`, `Sent { value = id }` at once and
//!   the `SpeakResult` (or `Exception`) afterwards with `_request_id = id`,
//!   as for `Chat`.
//!
//! `<think>…</think>` blocks (some reasoning models emit them) are stripped
//! from every reply.
//!
//! A failed call is an `Exception { source = "localai" }`, never `{ ok:
//! false }` and never the end of the program.
//!
//! `code_release` needs no code here — `code-native` links the vendored
//! `runtime.c` into the cdylib and re-exports it.

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use code_native::*;
use serde_json::{json, Value as Json};
use std::sync::Mutex;
use std::time::Duration;

static CONFIG: Mutex<Option<Config>> = Mutex::new(None);

struct Config {
    base: String,
    /// Sent as `Authorization: Bearer …` on every request when non-empty.
    /// A LocalAI with `API_KEY` set refuses everything without it.
    api_key: String,
    model: String,
    max_tokens: u32,
    temperature: f64,
    timeout: Duration,
}

const DEFAULT_MODEL: &str = "gpt-4";
const DEFAULT_MAX_TOKENS: u32 = 4096;
const DEFAULT_TEMPERATURE: f64 = 0.3;
const DEFAULT_TIMEOUT_SECONDS: f64 = 300.0;
const NOT_CONFIGURED: &str = "localai has no endpoint — send Config { endpoint, … } first";

/// The ABI version this module speaks. Must equal `CODE_ABI_VERSION` or the
/// host refuses to load us.
#[no_mangle]
pub extern "C" fn code_module_abi_version() -> u32 {
    CODE_ABI_VERSION
}

/// The single dispatch point: read `_class`, route to a handler. An
/// unhandled class is null; a handler that cannot do the work returns an
/// `Exception`. Neither ends the program.
///
/// # Safety
///
/// Both pointers must be valid for reads/writes for the duration of the
/// call and laid out per `code_abi.h` — the host guarantees this.
#[no_mangle]
pub unsafe extern "C" fn code_module_dispatch(out: *mut CodeValue, particle: *const CodeValue) {
    let particle = &*particle;
    guarded(&mut *out, "localai", |out| {
        let outcome = match read_field_str(particle, "_class").unwrap_or("") {
            "Config" => config(out, particle),
            "Chat" => chat(out, particle, false),
            "ChatJson" => chat(out, particle, true),
            "Transcribe" | "TranscribeWithOptions" => transcribe(out, particle),
            "Speak" => speak(out, particle),
            _ => {
                null(out);
                Ok(())
            }
        };
        if let Err(message) = outcome {
            exception(out, "localai", &message);
        }
    })
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

fn config(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let endpoint = find_field(particle, "endpoint")
        .and_then(read_str)
        .filter(|s| !s.is_empty())
        .ok_or("Config requires a non-empty string 'endpoint'")?;

    let cfg = Config {
        base: v1_base(endpoint),
        api_key: opt_str(particle, "api_key").unwrap_or_default(),
        model: opt_str(particle, "model").unwrap_or_else(|| DEFAULT_MODEL.to_string()),
        max_tokens: opt_number(particle, "max_tokens")
            .map(|n| n as u32)
            .filter(|n| *n > 0)
            .unwrap_or(DEFAULT_MAX_TOKENS),
        temperature: opt_number(particle, "temperature")
            .filter(|n| *n >= 0.0)
            .unwrap_or(DEFAULT_TEMPERATURE),
        timeout: Duration::from_secs_f64(
            opt_number(particle, "timeout_seconds")
                .filter(|n| *n > 0.0 && n.is_finite())
                .unwrap_or(DEFAULT_TIMEOUT_SECONDS),
        ),
    };
    *CONFIG.lock().unwrap_or_else(|e| e.into_inner()) = Some(cfg);

    let mut b = SlotBuffer::new(2);
    borrowed_str(b.slot_mut(0), c"ConfigResult");
    boolean(b.slot_mut(1), true);
    object(out, &[c"_class", c"ok"], &mut b);
    b.release_all();
    Ok(())
}

fn chat(out: &mut CodeValue, particle: &CodeValue, json_mode: bool) -> Result<(), String> {
    let (url, body, timeout, api_key) = {
        let guard = CONFIG.lock().unwrap_or_else(|e| e.into_inner());
        let cfg = guard.as_ref().ok_or(NOT_CONFIGURED)?;

        let messages = build_messages(particle)?;
        if messages.is_empty() {
            return Err("Chat needs 'system', 'user' or a non-empty 'messages'".to_string());
        }
        let model = opt_str(particle, "model").unwrap_or_else(|| cfg.model.clone());
        let temperature = opt_number(particle, "temperature")
            .filter(|n| *n >= 0.0)
            .unwrap_or(cfg.temperature);
        let max_tokens = opt_number(particle, "max_tokens")
            .map(|n| n as u32)
            .filter(|n| *n > 0)
            .unwrap_or(cfg.max_tokens);

        (
            format!("{}/chat/completions", cfg.base),
            json!({
                "model": model,
                "messages": messages,
                "temperature": temperature,
                "max_tokens": max_tokens,
            }),
            cfg.timeout,
            cfg.api_key.clone(),
        )
    };

    // `later = true`: the model is asked on a thread of its own, this
    // answers `Sent { id }` at once, and the `ChatResult` — or the
    // `Exception` — arrives afterwards as a particle carrying
    // `_request_id = id`. A minute-long answer then never holds the
    // program's thread. Without `later`, nothing changes.
    if read_field_bool(particle, "later") == Some(true) {
        let id = NEXT_REQUEST_ID.fetch_add(1, Ordering::SeqCst);
        std::thread::spawn(move || {
            let outcome = chat_perform(&url, body, timeout, &api_key, json_mode);
            push_later(id, c"ChatResult", c"content", outcome);
        });
        make_result(out, c"Sent", |slot| number(slot, id as f64));
        return Ok(());
    }

    let content = chat_perform(&url, body, timeout, &api_key, json_mode)?;
    one_str(out, c"ChatResult", c"content", &content);
    Ok(())
}

/// The one exchange with the model: the request, the read, the checks —
/// and for `ChatJson`, the fence stripped and the JSON made canonical.
/// Blocking, which is why a `later` caller runs it on a thread.
fn chat_perform(url: &str, body: Json, timeout: Duration, api_key: &str, json_mode: bool) -> Result<String, String> {
    let mut resp = authorized(agent(timeout).post(url), api_key)
        .header("Content-Type", "application/json")
        .send(body.to_string())
        .map_err(|e| format!("chat request to '{url}' failed: {e}"))?;
    let status = resp.status().as_u16();
    let text = resp
        .body_mut()
        .with_config()
        .limit(16 * 1024 * 1024)
        .read_to_string()
        .map_err(|e| format!("reading the chat response failed: {e}"))?;
    if !(200..300).contains(&status) {
        return Err(format!("chat rejected: HTTP {status}: {}", trim(&text)));
    }
    let parsed: Json =
        serde_json::from_str(&text).map_err(|e| format!("chat response was not JSON: {e}"))?;
    let raw = parsed
        .pointer("/choices/0/message/content")
        .and_then(Json::as_str)
        .ok_or("chat response carried no choices/0/message/content")?;

    let content = strip_think(raw);
    if json_mode {
        let content = strip_fence(&content);
        let value: Json = serde_json::from_str(&content).map_err(|_| {
            format!(
                "ChatJson: the model did not return valid JSON: {}",
                trim(&content)
            )
        })?;
        // Re-serialise so `content` is canonical text, not the model's
        // whitespace — the euglena apps carry it as a string and parse it
        // downstream.
        Ok(serde_json::to_string(&value).unwrap_or(content))
    } else {
        Ok(content.trim().to_string())
    }
}

/// Push a `later` outcome into the program: the result class with its one
/// text field and `_request_id`, or an `Exception` carrying the same id.
fn push_later(id: u64, class: &'static std::ffi::CStr, key: &'static std::ffi::CStr, outcome: Result<String, String>) {
    push_later_fields(id, class, outcome.map(|text| vec![(key, text)]));
}

/// The same, for a result with several text fields.
fn push_later_fields(
    id: u64,
    class: &'static std::ffi::CStr,
    outcome: Result<Vec<(&'static std::ffi::CStr, String)>, String>,
) {
    let mut particle = CodeValue::zeroed();
    match outcome {
        Ok(fields) => {
            let mut keys: Vec<&std::ffi::CStr> = vec![c"_class"];
            let mut b = SlotBuffer::new(fields.len() + 2);
            borrowed_str(b.slot_mut(0), class);
            for (i, (key, text)) in fields.iter().enumerate() {
                keys.push(key);
                owned_str(b.slot_mut(i as i64 + 1), text);
            }
            keys.push(c"_request_id");
            number(b.slot_mut(fields.len() as i64 + 1), id as f64);
            object(&mut particle, &keys, &mut b);
            b.release_all();
        }
        Err(message) => {
            let mut b = SlotBuffer::new(5);
            borrowed_str(b.slot_mut(0), c"Exception");
            owned_str(b.slot_mut(1), "localai");
            owned_str(b.slot_mut(2), &message);
            null(b.slot_mut(3));
            number(b.slot_mut(4), id as f64);
            object(&mut particle, &[c"_class", c"source", c"message", c"innerException", c"_request_id"], &mut b);
            b.release_all();
        }
    }
    emit_inbound(&particle);
    release(&mut particle);
}

/// One number per `later` ask, so an answer finds its question.
static NEXT_REQUEST_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
use std::sync::atomic::Ordering;

code_native::declare_inbound!();
code_native::declare_inbound_reply!(answered);

/// The program answered a particle pushed by `later`; nothing waits on it.
fn answered(_particle: &CodeValue, _result: &CodeValue) {}

fn transcribe(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let audio = find_field(particle, "audio_base64")
        .and_then(read_str)
        .filter(|s| !s.is_empty())
        .ok_or("Transcribe requires a base64 string 'audio_base64'")?;
    let bytes = B64
        .decode(audio.trim())
        .map_err(|e| format!("'audio_base64' is not valid base64: {e}"))?;
    let language = opt_str(particle, "language").unwrap_or_default();
    let format = opt_str(particle, "audio_format").unwrap_or_else(|| "webm".to_string());

    let (url, model, timeout, api_key) = {
        let guard = CONFIG.lock().unwrap_or_else(|e| e.into_inner());
        let cfg = guard.as_ref().ok_or(NOT_CONFIGURED)?;
        (
            format!("{}/audio/transcriptions", cfg.base),
            opt_str(particle, "model").unwrap_or_else(|| cfg.model.clone()),
            cfg.timeout,
            cfg.api_key.clone(),
        )
    };

    let boundary = "codeLocalaiBoundary8b21f0";
    let mut body: Vec<u8> = Vec::with_capacity(bytes.len() + 512);
    part_file(&mut body, boundary, &format, &bytes);
    part_field(&mut body, boundary, "model", &model);
    if !language.is_empty() {
        part_field(&mut body, boundary, "language", &language);
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());

    let mut resp = authorized(agent(timeout).post(&url), &api_key)
        .header(
            "Content-Type",
            &format!("multipart/form-data; boundary={boundary}"),
        )
        .send(&body[..])
        .map_err(|e| format!("transcription request to '{url}' failed: {e}"))?;
    let status = resp.status().as_u16();
    let text = resp
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("reading the transcription response failed: {e}"))?;
    if !(200..300).contains(&status) {
        return Err(format!(
            "transcription rejected: HTTP {status}: {}",
            trim(&text)
        ));
    }
    let parsed: Json = serde_json::from_str(&text)
        .map_err(|e| format!("transcription response was not JSON: {e}"))?;
    let transcript = parsed
        .get("text")
        .and_then(Json::as_str)
        .ok_or("transcription response carried no 'text'")?;

    let mut b = SlotBuffer::new(3);
    borrowed_str(b.slot_mut(0), c"TranscribeResult");
    owned_str(b.slot_mut(1), transcript.trim());
    owned_str(b.slot_mut(2), &language);
    object(out, &[c"_class", c"text", c"language"], &mut b);
    b.release_all();
    Ok(())
}

// ---------------------------------------------------------------------------
// Request shaping
// ---------------------------------------------------------------------------

/// The chat `messages` array: an explicit `messages` field if the particle
/// carries one, otherwise `[system?, user?]`.
fn build_messages(particle: &CodeValue) -> Result<Vec<Json>, String> {
    if let Some(field) = find_field(particle, "messages") {
        if field.tag != CodeTag::Array {
            return Err("'messages' must be an array of { role, content }".to_string());
        }
        let mut out = Vec::new();
        for m in array_elems(field) {
            let role = find_field(m, "role")
                .and_then(read_str)
                .filter(|s| !s.is_empty())
                .ok_or("every 'messages' entry needs a 'role'")?;
            let content = find_field(m, "content")
                .and_then(read_str)
                .ok_or("every 'messages' entry needs a string 'content'")?;
            out.push(json!({ "role": role, "content": content }));
        }
        return Ok(out);
    }

    let mut out = Vec::new();
    if let Some(system) = opt_str(particle, "system") {
        out.push(json!({ "role": "system", "content": system }));
    }
    if let Some(user) = opt_str(particle, "user") {
        out.push(json!({ "role": "user", "content": user }));
    }
    Ok(out)
}

fn part_file(body: &mut Vec<u8>, boundary: &str, format: &str, bytes: &[u8]) {
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        format!("Content-Disposition: form-data; name=\"file\"; filename=\"audio.{format}\"\r\n")
            .as_bytes(),
    );
    body.extend_from_slice(format!("Content-Type: audio/{format}\r\n\r\n").as_bytes());
    body.extend_from_slice(bytes);
    body.extend_from_slice(b"\r\n");
}

fn part_field(body: &mut Vec<u8>, boundary: &str, name: &str, value: &str) {
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes(),
    );
    body.extend_from_slice(value.as_bytes());
    body.extend_from_slice(b"\r\n");
}

/// `Speak`: text in, audio out. The server answers raw bytes with a
/// content type, so what comes back is base64 and the subtype it named.
fn speak(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let text = find_field(particle, "text")
        .and_then(read_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or("Speak requires a non-empty string 'text'")?
        .to_string();
    let (url, timeout, api_key, body) = {
        let guard = CONFIG.lock().unwrap_or_else(|e| e.into_inner());
        let cfg = guard.as_ref().ok_or(NOT_CONFIGURED)?;
        let mut body = json!({
            "model": opt_str(particle, "model").unwrap_or_else(|| cfg.model.clone()),
            "input": text,
        });
        if let Some(voice) = opt_str(particle, "voice") {
            body["voice"] = json!(voice);
        }
        if let Some(format) = opt_str(particle, "format") {
            body["response_format"] = json!(format);
        }
        (
            format!("{}/audio/speech", cfg.base),
            cfg.timeout,
            cfg.api_key.clone(),
            body,
        )
    };

    if read_field_bool(particle, "later") == Some(true) {
        let id = NEXT_REQUEST_ID.fetch_add(1, Ordering::SeqCst);
        std::thread::spawn(move || {
            let outcome = speak_perform(&url, body, timeout, &api_key)
                .map(|(audio, format)| vec![(c"audio_base64", audio), (c"format", format)]);
            push_later_fields(id, c"SpeakResult", outcome);
        });
        make_result(out, c"Sent", |slot| number(slot, id as f64));
        return Ok(());
    }

    let (audio, format) = speak_perform(&url, body, timeout, &api_key)?;
    let mut b = SlotBuffer::new(3);
    borrowed_str(b.slot_mut(0), c"SpeakResult");
    owned_str(b.slot_mut(1), &audio);
    owned_str(b.slot_mut(2), &format);
    object(out, &[c"_class", c"audio_base64", c"format"], &mut b);
    b.release_all();
    Ok(())
}

/// The one exchange with the speech model: the bytes and the audio subtype
/// from the content type ("audio/wav" → "wav"). Blocking, as `chat_perform`.
fn speak_perform(url: &str, body: Json, timeout: Duration, api_key: &str) -> Result<(String, String), String> {
    let mut resp = authorized(agent(timeout).post(url), api_key)
        .header("Content-Type", "application/json")
        .send(body.to_string())
        .map_err(|e| format!("speech request to '{url}' failed: {e}"))?;
    let status = resp.status().as_u16();
    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let bytes = resp
        .body_mut()
        .with_config()
        .limit(64 * 1024 * 1024)
        .read_to_vec()
        .map_err(|e| format!("reading the speech response failed: {e}"))?;
    if !(200..300).contains(&status) {
        return Err(format!(
            "speech rejected: HTTP {status}: {}",
            trim(&String::from_utf8_lossy(&bytes))
        ));
    }
    if bytes.is_empty() {
        return Err("speech response carried no audio".to_string());
    }
    let format = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .strip_prefix("audio/")
        .map(|s| match s {
            "wave" | "x-wav" => "wav",
            "mpeg" => "mp3",
            other => other,
        })
        .filter(|s| !s.is_empty())
        .unwrap_or("wav")
        .to_string();
    Ok((B64.encode(&bytes), format))
}

// ---------------------------------------------------------------------------
// Reply cleanup
// ---------------------------------------------------------------------------

/// Remove `<think>…</think>` blocks — reasoning models (Qwen3, R1) emit them
/// ahead of the answer, and they are never what the caller wants.
fn strip_think(s: &str) -> String {
    let mut out = s.to_string();
    while let Some(start) = out.find("<think>") {
        match out[start..].find("</think>") {
            Some(rel) => {
                let end = start + rel + "</think>".len();
                out.replace_range(start..end, "");
            }
            None => {
                out.truncate(start);
                break;
            }
        }
    }
    out
}

/// Strip one wrapping ```` ```lang … ``` ```` fence, if the whole string is
/// one — for `ChatJson`, where the payload is meant to be bare JSON.
fn strip_fence(s: &str) -> String {
    let t = s.trim();
    let Some(rest) = t.strip_prefix("```") else {
        return t.to_string();
    };
    // Drop the rest of the opening fence line (an optional language tag).
    let rest = match rest.find('\n') {
        Some(nl) => &rest[nl + 1..],
        None => rest,
    };
    rest.trim()
        .strip_suffix("```")
        .unwrap_or(rest)
        .trim()
        .to_string()
}

// ---------------------------------------------------------------------------
// Shared
// ---------------------------------------------------------------------------

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .http_status_as_error(false)
        .build()
        .into()
}

/// The bearer header, when `Config` gave a key. Without one the request
/// goes as it did before, so a server with no auth sees no change.
fn authorized(
    request: ureq::RequestBuilder<ureq::typestate::WithBody>,
    api_key: &str,
) -> ureq::RequestBuilder<ureq::typestate::WithBody> {
    if api_key.is_empty() {
        request
    } else {
        request.header("Authorization", &format!("Bearer {api_key}"))
    }
}

/// The `/v1` API root for a configured endpoint: appended unless it is
/// already there, trailing slash trimmed either way.
fn v1_base(endpoint: &str) -> String {
    let trimmed = endpoint.trim_end_matches('/');
    if trimmed.ends_with("/v1") {
        trimmed.to_string()
    } else {
        format!("{trimmed}/v1")
    }
}

fn opt_str(particle: &CodeValue, name: &str) -> Option<String> {
    find_field(particle, name)
        .and_then(read_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn opt_number(particle: &CodeValue, name: &str) -> Option<f64> {
    find_field(particle, name).and_then(read_number)
}

fn one_str(
    out: &mut CodeValue,
    class: &'static std::ffi::CStr,
    key: &'static std::ffi::CStr,
    value: &str,
) {
    let mut b = SlotBuffer::new(2);
    borrowed_str(b.slot_mut(0), class);
    owned_str(b.slot_mut(1), value);
    object(out, &[c"_class", key], &mut b);
    b.release_all();
}

fn trim(s: &str) -> &str {
    let s = s.trim();
    if s.len() > 200 {
        &s[..200]
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::{strip_fence, strip_think, v1_base};

    #[test]
    fn think_blocks_go() {
        assert_eq!(strip_think("<think>hmm</think>answer"), "answer");
        assert_eq!(strip_think("a<think>x</think>b<think>y</think>c"), "abc");
        assert_eq!(strip_think("lead <think>never closed"), "lead ");
    }

    #[test]
    fn one_fence_comes_off() {
        assert_eq!(strip_fence("```json\n{\"a\":1}\n```"), "{\"a\":1}");
        assert_eq!(strip_fence("```\n{}\n```"), "{}");
        assert_eq!(strip_fence("{\"a\":1}"), "{\"a\":1}");
    }

    #[test]
    fn v1_is_appended_once() {
        assert_eq!(v1_base("http://h:8080"), "http://h:8080/v1");
        assert_eq!(v1_base("http://h:8080/"), "http://h:8080/v1");
        assert_eq!(v1_base("http://h:8080/v1"), "http://h:8080/v1");
    }
}
