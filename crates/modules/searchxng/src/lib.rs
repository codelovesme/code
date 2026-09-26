//! Native SearXNG search for bounded, interest-driven discovery.

use code_native::*;
use serde_json::Value as Json;
use std::sync::Mutex;
use std::time::Duration;

static CONFIG: Mutex<Option<(String, usize)>> = Mutex::new(None);

#[no_mangle]
pub extern "C" fn code_module_abi_version() -> u32 {
    CODE_ABI_VERSION
}

#[no_mangle]
/// # Safety
///
/// The host must pass valid, non-null pointers to writable output and input
/// particle values for the duration of this call.
pub unsafe extern "C" fn code_module_dispatch(out: *mut CodeValue, particle: *const CodeValue) {
    let particle = &*particle;
    guarded(&mut *out, "searchxng", |out| {
        let result = match read_field_str(particle, "_class").unwrap_or("") {
            "Config" => config(out, particle),
            "Search" => search(out, particle),
            _ => {
                null(out);
                Ok(())
            }
        };
        if let Err(message) = result {
            exception(out, "searchxng", &message);
        }
    })
}

fn config(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let endpoint = find_field(particle, "endpoint")
        .and_then(read_str)
        .filter(|s| s.starts_with("http://") || s.starts_with("https://"))
        .ok_or("Config requires an http(s) endpoint")?;
    let max = find_field(particle, "max_results")
        .and_then(read_number)
        .filter(|n| n.is_finite() && *n >= 1.0)
        .map(|n| n as usize)
        .unwrap_or(10)
        .min(10);
    *CONFIG
        .lock()
        .map_err(|_| "searchxng config lock poisoned".to_string())? =
        Some((endpoint.trim_end_matches('/').to_string(), max));
    let mut fields = SlotBuffer::new(2);
    borrowed_str(fields.slot_mut(0), c"ConfigResult");
    boolean(fields.slot_mut(1), true);
    object(out, &[c"_class", c"ok"], &mut fields);
    fields.release_all();
    Ok(())
}

fn search(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let query = find_field(particle, "query")
        .and_then(read_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or("Search requires a non-empty string 'query'")?
        .to_string();
    let (endpoint, configured_max) = CONFIG
        .lock()
        .map_err(|_| "searchxng config lock poisoned".to_string())?
        .clone()
        .ok_or("searchxng is not configured")?;
    let requested = find_field(particle, "limit")
        .and_then(read_number)
        .filter(|n| n.is_finite() && *n >= 1.0)
        .map(|n| n as usize)
        .unwrap_or(configured_max)
        .min(configured_max)
        .min(10);
    let language = find_field(particle, "language")
        .and_then(read_str)
        .unwrap_or("all");
    let categories = find_field(particle, "categories")
        .and_then(read_str)
        .unwrap_or("general");
    let url = format!(
        "{endpoint}/search?q={}&format=json&language={}&categories={}",
        encode(&query),
        encode(language),
        encode(categories)
    );
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(10)))
        .http_status_as_error(false)
        .max_redirects(0)
        .proxy(None)
        .build()
        .into();
    let mut response = agent
        .get(&url)
        .call()
        .map_err(|e| format!("SearXNG request failed: {e}"))?;
    let status = response.status().as_u16() as f64;
    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("SearXNG response failed: {e}"))?;
    if !(200.0..300.0).contains(&status) {
        return Err(format!("SearXNG returned status {status}"));
    }
    let root: Json =
        serde_json::from_str(&body).map_err(|e| format!("SearXNG returned invalid JSON: {e}"))?;
    let items = root
        .get("results")
        .and_then(Json::as_array)
        .ok_or("SearXNG JSON has no results array")?;
    let mut slots = SlotBuffer::new(items.len().min(requested));
    for (i, item) in items.iter().take(requested).enumerate() {
        let mut fields = SlotBuffer::new(5);
        owned_str(
            fields.slot_mut(0),
            item.get("title").and_then(Json::as_str).unwrap_or(""),
        );
        owned_str(
            fields.slot_mut(1),
            item.get("url").and_then(Json::as_str).unwrap_or(""),
        );
        owned_str(
            fields.slot_mut(2),
            item.get("content").and_then(Json::as_str).unwrap_or(""),
        );
        owned_str(
            fields.slot_mut(3),
            item.get("engine").and_then(Json::as_str).unwrap_or(""),
        );
        owned_str(
            fields.slot_mut(4),
            item.get("publishedDate")
                .and_then(Json::as_str)
                .unwrap_or(""),
        );
        let mut item = CodeValue::zeroed();
        object_dyn(
            &mut item,
            &["title", "url", "content", "engine", "published_at"],
            &mut fields,
        );
        copy(slots.slot_mut(i as i64), &item);
        release(&mut item);
        fields.release_all();
    }
    let mut fields = SlotBuffer::new(4);
    borrowed_str(fields.slot_mut(0), c"SearchResult");
    boolean(fields.slot_mut(1), true);
    number(fields.slot_mut(2), status);
    array(fields.slot_mut(3), &mut slots);
    object(out, &[c"_class", c"ok", c"status", c"results"], &mut fields);
    fields.release_all();
    slots.release_all();
    Ok(())
}

fn encode(value: &str) -> String {
    value
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}
