//! Cached, bounded search over a public OpenAPI registry.

use code_native::*;
use serde_json::Value as Json;
use std::cmp::Reverse;
use std::sync::Mutex;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

#[derive(Clone)]
struct ConfigState {
    endpoint: String,
    max_results: usize,
    cache_seconds: u64,
}

struct CacheState {
    fetched_at: Instant,
    catalog: Json,
}

static CONFIG: Mutex<Option<ConfigState>> = Mutex::new(None);
static CACHE: Mutex<Option<CacheState>> = Mutex::new(None);
static WARMING: Mutex<bool> = Mutex::new(false);
static WARM_THREAD: Mutex<Option<JoinHandle<()>>> = Mutex::new(None);

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
    guarded(&mut *out, "api_registry", |out| {
        let result = match read_field_str(particle, "_class").unwrap_or("") {
            "Config" => config(out, particle),
            "Warm" => warm(out, particle),
            "Search" => search(out, particle),
            "Stop" => stop(out),
            _ => {
                null(out);
                Ok(())
            }
        };
        if let Err(message) = result {
            exception(out, "api_registry", &message);
        }
    })
}

fn config(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let endpoint = find_field(particle, "endpoint")
        .and_then(read_str)
        .filter(|value| value.starts_with("https://"))
        .ok_or("Config requires an https endpoint")?;
    let max_results = number_field(particle, "max_results", 5).clamp(1, 10);
    let cache_seconds = number_field(particle, "cache_seconds", 86_400).clamp(300, 604_800);
    *CONFIG
        .lock()
        .map_err(|_| "api_registry config lock poisoned".to_string())? = Some(ConfigState {
        endpoint: endpoint.trim_end_matches('/').to_string(),
        max_results,
        cache_seconds: cache_seconds as u64,
    });
    *CACHE
        .lock()
        .map_err(|_| "api_registry cache lock poisoned".to_string())? = None;
    *WARMING
        .lock()
        .map_err(|_| "api_registry warm lock poisoned".to_string())? = false;
    let mut fields = SlotBuffer::new(2);
    borrowed_str(fields.slot_mut(0), c"ConfigResult");
    boolean(fields.slot_mut(1), true);
    object(out, &[c"_class", c"ok"], &mut fields);
    fields.release_all();
    Ok(())
}

fn warm(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let config = configured()?;
    let wait = read_field_bool(particle, "wait").unwrap_or(false);
    if wait {
        let catalog = fetch_catalog(&config)?;
        store_catalog(catalog)?;
        write_warm_result(out, true, false);
        return Ok(());
    }
    let started = start_warm(config)?;
    write_warm_result(out, cache_ready()?, started);
    Ok(())
}

fn stop(out: &mut CodeValue) -> Result<(), String> {
    let handle = WARM_THREAD
        .lock()
        .map_err(|_| "api_registry thread lock poisoned".to_string())?
        .take();
    if let Some(handle) = handle {
        handle
            .join()
            .map_err(|_| "api_registry warm thread panicked".to_string())?;
    }
    *WARMING
        .lock()
        .map_err(|_| "api_registry warm lock poisoned".to_string())? = false;
    let mut fields = SlotBuffer::new(2);
    borrowed_str(fields.slot_mut(0), c"Stopped");
    boolean(fields.slot_mut(1), true);
    object(out, &[c"_class", c"ok"], &mut fields);
    fields.release_all();
    Ok(())
}

fn search(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let query = find_field(particle, "query")
        .and_then(read_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or("Search requires a non-empty string 'query'")?;
    let config = configured()?;
    let requested = number_field(particle, "limit", config.max_results)
        .clamp(1, config.max_results)
        .min(10);
    let query_normalized = normalize(query);
    let query_tokens: Vec<&str> = query_normalized
        .split_whitespace()
        .filter(|token| token.len() > 1)
        .collect();
    if query_tokens.is_empty() {
        return Err("Search query has no searchable terms".to_string());
    }

    let cache_guard = CACHE
        .lock()
        .map_err(|_| "api_registry cache lock poisoned".to_string())?;
    let Some(cache) = cache_guard.as_ref() else {
        drop(cache_guard);
        let started = start_warm(config)?;
        write_unready_results(out, started);
        return Ok(());
    };
    let root = cache
        .catalog
        .as_object()
        .ok_or("registry catalog is not an object")?;
    let mut matches: Vec<(usize, String, String, String, String)> = Vec::new();
    for (provider, entry) in root {
        let Some(preferred) = entry.get("preferred").and_then(Json::as_str) else {
            continue;
        };
        let Some(version) = entry
            .get("versions")
            .and_then(Json::as_object)
            .and_then(|versions| versions.get(preferred))
        else {
            continue;
        };
        let Some(url) = version
            .get("swaggerUrl")
            .or_else(|| version.get("openapiUrl"))
            .and_then(Json::as_str)
            .filter(|url| is_safe_json_spec(url))
        else {
            continue;
        };
        let info = version.get("info").unwrap_or(&Json::Null);
        let title = info.get("title").and_then(Json::as_str).unwrap_or(provider);
        let description = info.get("description").and_then(Json::as_str).unwrap_or("");
        let categories = info
            .get("x-apisguru-categories")
            .and_then(Json::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Json::as_str)
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_default();
        let provider_text = normalize(provider);
        let title_text = normalize(title);
        let description_text = normalize(description);
        let category_text = normalize(&categories);
        let mut score = 0usize;
        if title_text == query_normalized || provider_text == query_normalized {
            score += 100;
        }
        if title_text.contains(&query_normalized) || provider_text.contains(&query_normalized) {
            score += 40;
        }
        for token in &query_tokens {
            if title_text.contains(token) {
                score += 20;
            }
            if provider_text.contains(token) {
                score += 12;
            }
            if category_text.contains(token) {
                score += 10;
            }
            if description_text.contains(token) {
                score += 2;
            }
        }
        if score > 0 {
            matches.push((
                score,
                provider.to_string(),
                title.to_string(),
                truncate(description, 500),
                url.to_string(),
            ));
        }
    }
    let stale = cache.fetched_at.elapsed() >= Duration::from_secs(config.cache_seconds);
    drop(cache_guard);
    if stale {
        let _ = start_warm(config);
    }
    matches.sort_by_key(|item| (Reverse(item.0), item.2.to_lowercase()));
    matches.truncate(requested);
    write_results(out, &matches);
    Ok(())
}

fn configured() -> Result<ConfigState, String> {
    CONFIG
        .lock()
        .map_err(|_| "api_registry config lock poisoned".to_string())?
        .clone()
        .ok_or_else(|| "api_registry is not configured".to_string())
}

fn cache_ready() -> Result<bool, String> {
    Ok(CACHE
        .lock()
        .map_err(|_| "api_registry cache lock poisoned".to_string())?
        .is_some())
}

fn start_warm(config: ConfigState) -> Result<bool, String> {
    let mut thread_slot = WARM_THREAD
        .lock()
        .map_err(|_| "api_registry thread lock poisoned".to_string())?;
    if thread_slot
        .as_ref()
        .is_some_and(|handle| handle.is_finished())
    {
        if let Some(handle) = thread_slot.take() {
            let _ = handle.join();
        }
    }
    if thread_slot.is_some() {
        return Ok(false);
    }
    let mut warming = WARMING
        .lock()
        .map_err(|_| "api_registry warm lock poisoned".to_string())?;
    if *warming {
        return Ok(false);
    }
    *warming = true;
    let handle = thread::spawn(move || {
        if let Ok(catalog) = fetch_catalog(&config) {
            let _ = store_catalog(catalog);
        }
        if let Ok(mut warming) = WARMING.lock() {
            *warming = false;
        }
    });
    *thread_slot = Some(handle);
    Ok(true)
}

fn fetch_catalog(config: &ConfigState) -> Result<Json, String> {
    let url = format!("{}/list.json", config.endpoint);
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(30)))
        .http_status_as_error(false)
        .max_redirects(0)
        .proxy(None)
        .build()
        .into();
    let mut response = agent
        .get(&url)
        .call()
        .map_err(|error| format!("registry request failed: {error}"))?;
    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(format!("registry returned status {status}"));
    }
    let body = response
        .body_mut()
        .with_config()
        .limit(12 * 1024 * 1024)
        .read_to_string()
        .map_err(|error| format!("registry response failed: {error}"))?;
    let catalog: Json = serde_json::from_str(&body)
        .map_err(|error| format!("registry returned invalid JSON: {error}"))?;
    Ok(catalog)
}

fn store_catalog(catalog: Json) -> Result<(), String> {
    *CACHE
        .lock()
        .map_err(|_| "api_registry cache lock poisoned".to_string())? = Some(CacheState {
        fetched_at: Instant::now(),
        catalog,
    });
    Ok(())
}

fn write_warm_result(out: &mut CodeValue, ready: bool, started: bool) {
    let mut fields = SlotBuffer::new(3);
    borrowed_str(fields.slot_mut(0), c"WarmResult");
    boolean(fields.slot_mut(1), ready);
    boolean(fields.slot_mut(2), started);
    object(out, &[c"_class", c"ready", c"started"], &mut fields);
    fields.release_all();
}

fn write_unready_results(out: &mut CodeValue, started: bool) {
    let mut items = SlotBuffer::new(0);
    let mut fields = SlotBuffer::new(5);
    borrowed_str(fields.slot_mut(0), c"SearchResult");
    boolean(fields.slot_mut(1), false);
    boolean(fields.slot_mut(2), false);
    boolean(fields.slot_mut(3), started);
    array(fields.slot_mut(4), &mut items);
    object(
        out,
        &[c"_class", c"ok", c"ready", c"warming", c"results"],
        &mut fields,
    );
    fields.release_all();
    items.release_all();
}

fn write_results(out: &mut CodeValue, matches: &[(usize, String, String, String, String)]) {
    let mut items = SlotBuffer::new(matches.len());
    for (index, (score, provider, title, description, url)) in matches.iter().enumerate() {
        let mut fields = SlotBuffer::new(5);
        owned_str(fields.slot_mut(0), provider);
        owned_str(fields.slot_mut(1), title);
        owned_str(fields.slot_mut(2), description);
        owned_str(fields.slot_mut(3), url);
        number(fields.slot_mut(4), *score as f64);
        let mut item = CodeValue::zeroed();
        object_dyn(
            &mut item,
            &["provider", "title", "description", "url", "score"],
            &mut fields,
        );
        copy(items.slot_mut(index as i64), &item);
        release(&mut item);
        fields.release_all();
    }
    let mut fields = SlotBuffer::new(3);
    borrowed_str(fields.slot_mut(0), c"SearchResult");
    boolean(fields.slot_mut(1), true);
    array(fields.slot_mut(2), &mut items);
    object(out, &[c"_class", c"ok", c"results"], &mut fields);
    fields.release_all();
    items.release_all();
}

fn number_field(particle: &CodeValue, name: &str, default: usize) -> usize {
    find_field(particle, name)
        .and_then(read_number)
        .filter(|value| value.is_finite() && *value >= 1.0)
        .map(|value| value as usize)
        .unwrap_or(default)
}

fn normalize(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_safe_json_spec(url: &str) -> bool {
    if !url.starts_with("https://") {
        return false;
    }
    let path = url.split(['?', '#']).next().unwrap_or(url);
    path.ends_with(".json") || path.ends_with("/openapi.json") || path.ends_with("/swagger.json")
}

fn truncate(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}
