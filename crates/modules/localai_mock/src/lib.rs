//! The `localai_mock` native module — a drop-in for `localai` that reaches
//! no model server, for the Code programming language, written in Rust on
//! [`code-native`].
//!
//! Same particles and result shapes as `localai`. Replies are canned but
//! deterministic — echoing the prompt back — so a program's flow around an
//! AI call can be exercised without a GPU or a network.
//!
//! - `Config { endpoint, model?, … }` → `ConfigResult { ok }` — every field
//!   is accepted and ignored bar `model`, which is echoed in replies.
//! - `Chat { system?, user?, messages? }` → `ChatResult { content }` —
//!   `content` is `"[mock <model>] <last user message>"`.
//! - `ChatJson { … }` → `ChatResult { content }` — `content` is `"{}"`,
//!   valid JSON so a downstream `Parse` succeeds.
//! - `Transcribe { audio_base64, language? }` / `TranscribeWithOptions` →
//!   `TranscribeResult { text, language }` — `text` is `"[mock transcript]"`.
//!
//! `code_release` needs no code here — `code-native` links the vendored
//! `runtime.c` into the cdylib and re-exports it.

use code_native::*;
use std::sync::Mutex;

static MODEL: Mutex<Option<String>> = Mutex::new(None);

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
    guarded(&mut *out, "localai_mock", |out| {
        let outcome = match read_field_str(particle, "_class").unwrap_or("") {
            "Config" => config(out, particle),
            "Chat" => chat(out, particle, false),
            "ChatJson" => chat(out, particle, true),
            "Transcribe" | "TranscribeWithOptions" => transcribe(out, particle),
            _ => {
                null(out);
                Ok(())
            }
        };
        if let Err(message) = outcome {
            exception(out, "localai_mock", &message);
        }
    })
}

fn config(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    find_field(particle, "endpoint")
        .and_then(read_str)
        .filter(|s| !s.is_empty())
        .ok_or("Config requires a non-empty string 'endpoint'")?;
    *MODEL.lock().unwrap_or_else(|e| e.into_inner()) =
        Some(opt(particle, "model").unwrap_or_else(|| "mock-model".to_string()));

    let mut b = SlotBuffer::new(2);
    borrowed_str(b.slot_mut(0), c"ConfigResult");
    boolean(b.slot_mut(1), true);
    object(out, &[c"_class", c"ok"], &mut b);
    b.release_all();
    Ok(())
}

fn chat(out: &mut CodeValue, particle: &CodeValue, json_mode: bool) -> Result<(), String> {
    let model = MODEL
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .ok_or("localai_mock has no endpoint — send Config { endpoint } first")?;

    let last_user = last_user_message(particle);
    if last_user.is_none() && opt(particle, "system").is_none() {
        return Err("Chat needs 'system', 'user' or a non-empty 'messages'".to_string());
    }

    let content = if json_mode {
        "{}".to_string()
    } else {
        format!("[mock {model}] {}", last_user.unwrap_or_default())
    };
    one_str(out, c"ChatResult", c"content", &content);
    Ok(())
}

fn transcribe(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    find_field(particle, "audio_base64")
        .and_then(read_str)
        .filter(|s| !s.is_empty())
        .ok_or("Transcribe requires a base64 string 'audio_base64'")?;
    let language = opt(particle, "language").unwrap_or_default();

    let mut b = SlotBuffer::new(3);
    borrowed_str(b.slot_mut(0), c"TranscribeResult");
    borrowed_str(b.slot_mut(1), c"[mock transcript]");
    owned_str(b.slot_mut(2), &language);
    object(out, &[c"_class", c"text", c"language"], &mut b);
    b.release_all();
    Ok(())
}

// ---------------------------------------------------------------------------
// Shared
// ---------------------------------------------------------------------------

/// The content of the last `user`-role message — from `messages` if present,
/// otherwise the `user` field.
fn last_user_message(particle: &CodeValue) -> Option<String> {
    if let Some(field) = find_field(particle, "messages") {
        if field.tag == CodeTag::Array {
            return array_elems(field)
                .filter(|m| find_field(m, "role").and_then(read_str) == Some("user"))
                .filter_map(|m| find_field(m, "content").and_then(read_str))
                .last()
                .map(str::to_string);
        }
    }
    opt(particle, "user")
}

fn opt(particle: &CodeValue, name: &str) -> Option<String> {
    find_field(particle, name)
        .and_then(read_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
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
