//! The `mqtt` native module — a broker's messages as particles, and
//! particles onto the broker.
//!
//! Handlers:
//!
//! - `Config { url, username?, password?, client_id? }` → `ConfigResult
//!   { ok }`. `url` is `mqtt://host:port` (port 1883 when left out).
//!   Nothing is opened yet: the first `Subscribe` or `Publish` connects.
//! - `Subscribe { topic, then }` → `Subscribed { ok }`. Every message on
//!   that filter (`zigbee2mqtt/#`, `home/+/temperature`) arrives as `then`
//!   — a class name, or a whole particle written where the subscription is
//!   asked for — with `topic`, `payload` (text) and `retained` added. A
//!   subscription is remembered and asked for again after every reconnect.
//! - `Publish { topic, payload, retain? }` → `Published { ok }`, or an
//!   `Exception` when there is no connection to put it on: a command to a
//!   device that cannot be delivered now is refused now, not queued for a
//!   broker that may come back in an hour.
//! - `Unsubscribe { topic }` → `Unsubscribed { ok }`.
//! - `Status {}` → `BusStatus { connected, subscribed }` — `subscribed` is
//!   the filters the broker has acknowledged so far. A subscription is
//!   asked for as soon as there is a connection, but the broker only
//!   delivers messages published after it has registered the filter; a
//!   program that publishes something it expects to hear back waits for
//!   this.
//! - `Disconnect {}` → `Disconnected { ok }`. Ends the connection and
//!   **joins its thread**, so a host that unloads the application right
//!   after finds nothing of this module still running.
//!
//! ```text
//! emit Config { url = "mqtt://127.0.0.1:1883", username = "home", password = "…" } to bus
//! emit Subscribe { topic = "zigbee2mqtt/#", then = "Spoke" } to bus
//!
//! Spoke { topic, payload, retained } =>
//!     ...
//!
//! emit Publish { topic = "zigbee2mqtt/hall-light/set", payload = "{\"state\":\"ON\"}" } to bus get sent
//! ```
//!
//! The application names the particle it wants back, in advance — the rule
//! `timer` follows for `then` and `dom` for a click. A pushed class the
//! program has no handler for is dropped, so a `then` nobody handles is a
//! subscription nobody hears.
//!
//! The module speaks first about the connection too: `Log { source =
//! "mqtt", level, message }` when it is lost and when it is back — once
//! per change, not once per retry. A program with no `Log` handler never
//! hears it.
//!
//! # The thread
//!
//! One thread per linked instance holds the connection: it drains the
//! client's event loop, resubscribes on every `ConnAck`, and pushes each
//! inbound message onto the program's ring — the handler runs on the
//! program's own thread when its loop next drains, never here. Lost
//! connections are retried by the client itself, a second apart.
//!
//! While the thread is alive the module is serving (`code_module_serving`),
//! which keeps a standalone program up and tells a host not to unmap it;
//! `Disconnect` is how either lets go.

use code_native::*;
use rumqttc::{Client, Event, MqttOptions, Packet, QoS};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// What `Config` said, kept until the first `Subscribe` or `Publish`.
struct Settings {
    host: String,
    port: u16,
    username: Option<String>,
    password: Option<String>,
    client_id: String,
}

/// One remembered subscription: the filter, and the particle to push for
/// each message on it — owned by this module, since a particle literal's
/// storage does not outlive the dispatch that carried it.
struct Subscription {
    topic: String,
    then: CodeValue,
    /// The broker said yes (a `SubAck` for this filter, on this connection).
    acked: bool,
}

/// Shared between the dispatch and the connection thread.
struct Shared {
    subscriptions: Mutex<Vec<Subscription>>,
    connected: AtomicBool,
    stopping: AtomicBool,
    /// Filters handed to the client and not yet seen going out, in order:
    /// the client numbers a subscribe as it sends it, and `Outgoing(
    /// Subscribe(pkid))` events come in the order the requests were made,
    /// so the front of this line is the filter that number belongs to.
    sent: Mutex<Vec<String>>,
    /// Packet number → filter, until its `SubAck` arrives.
    in_flight: Mutex<Vec<(u16, String)>>,
}

/// Hand a filter to the client, remembering it for the `Outgoing` that
/// follows.
fn ask_broker(client: &Client, shared: &Shared, topic: String) {
    shared
        .sent
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(topic.clone());
    if client.try_subscribe(topic, QoS::AtLeastOnce).is_err() {
        shared.sent.lock().unwrap_or_else(|e| e.into_inner()).pop();
    }
}

struct Bus {
    client: Client,
    shared: Arc<Shared>,
    thread: Option<thread::JoinHandle<()>>,
}

static SETTINGS: Mutex<Option<Settings>> = Mutex::new(None);
static BUS: Mutex<Option<Bus>> = Mutex::new(None);

fn settings() -> std::sync::MutexGuard<'static, Option<Settings>> {
    SETTINGS.lock().unwrap_or_else(|e| e.into_inner())
}

fn bus() -> std::sync::MutexGuard<'static, Option<Bus>> {
    BUS.lock().unwrap_or_else(|e| e.into_inner())
}

/// Non-zero while the connection thread is alive.
#[no_mangle]
pub extern "C" fn code_module_serving() -> std::ffi::c_int {
    i32::from(bus().as_ref().is_some_and(|b| b.thread.is_some()))
}

declare_inbound!();
declare_inbound_reply!(answered);

/// The program answered a message it was handed. Nothing waits on it.
fn answered(_particle: &CodeValue, _result: &CodeValue) {}

/// A value of the program's, made this module's own — every string, list
/// and object rebuilt, since `copy` shares the program's storage.
fn own(v: &CodeValue) -> CodeValue {
    let mut out = CodeValue::zeroed();
    match v.tag {
        CodeTag::Str => owned_str(&mut out, read_str(v).unwrap_or("")),
        CodeTag::Number => number(&mut out, read_number(v).unwrap_or(0.0)),
        CodeTag::Bool => boolean(&mut out, read_bool(v).unwrap_or(false)),
        CodeTag::Array => {
            let mut buf = SlotBuffer::new(v.len as usize);
            for i in 0..v.len {
                let item = unsafe { &*slot_at(v.items, i) };
                let mut owned = own(item);
                copy(buf.slot_mut(i), &owned);
                release(&mut owned);
            }
            array(&mut out, &mut buf);
            buf.release_all();
        }
        CodeTag::Object => {
            let entries: Vec<(String, CodeValue)> = object_entries(v)
                .map(|(k, x)| (k.to_string(), own(x)))
                .collect();
            let keys: Vec<&str> = entries.iter().map(|(k, _)| k.as_str()).collect();
            let mut buf = SlotBuffer::new(keys.len());
            for (i, (_, x)) in entries.iter().enumerate() {
                copy(buf.slot_mut(i as i64), x);
            }
            object_dyn(&mut out, &keys, &mut buf);
            buf.release_all();
            for (_, mut x) in entries {
                release(&mut x);
            }
        }
        CodeTag::Null => null(&mut out),
    }
    out
}

/// `then` as a template: a class name becomes `{ _class }`, an object with
/// a `_class` is taken whole (owned). Anything else is refused.
fn template_from_then(then: &CodeValue) -> Option<CodeValue> {
    if let Some(class) = read_str(then) {
        if class.is_empty() {
            return None;
        }
        let mut c = CodeValue::zeroed();
        owned_str(&mut c, class);
        let mut buf = SlotBuffer::new(1);
        copy(buf.slot_mut(0), &c);
        let mut particle = CodeValue::zeroed();
        object_dyn(&mut particle, &["_class"], &mut buf);
        buf.release_all();
        release(&mut c);
        return Some(particle);
    }
    if then.tag == CodeTag::Object && read_field_str(then, "_class").is_some_and(|c| !c.is_empty())
    {
        return Some(own(then));
    }
    None
}

/// The particle for one message: the template's fields, then `topic`,
/// `payload` and `retained` on top.
fn message_particle(template: &CodeValue, topic: &str, payload: &str, retained: bool) -> CodeValue {
    let mut entries: Vec<(String, CodeValue)> = Vec::new();
    for (k, v) in object_entries(template) {
        if k == "topic" || k == "payload" || k == "retained" {
            continue;
        }
        entries.push((k.to_string(), own(v)));
    }
    let mut t = CodeValue::zeroed();
    owned_str(&mut t, topic);
    entries.push(("topic".to_string(), t));
    let mut p = CodeValue::zeroed();
    owned_str(&mut p, payload);
    entries.push(("payload".to_string(), p));
    let mut r = CodeValue::zeroed();
    boolean(&mut r, retained);
    entries.push(("retained".to_string(), r));

    let keys: Vec<&str> = entries.iter().map(|(k, _)| k.as_str()).collect();
    let mut buf = SlotBuffer::new(keys.len());
    for (i, (_, v)) in entries.iter().enumerate() {
        copy(buf.slot_mut(i as i64), v);
    }
    let mut particle = CodeValue::zeroed();
    object_dyn(&mut particle, &keys, &mut buf);
    buf.release_all();
    for (_, mut v) in entries {
        release(&mut v);
    }
    particle
}

/// Push `Log { source, level, message }` into the program — the same field
/// names as every organelle's, so one handler reads them all. Best effort:
/// a program with no handler simply never hears it.
fn report_log(level: &str, message: &str) {
    let mut particle = CodeValue::zeroed();
    let mut buf = SlotBuffer::new(4);
    borrowed_str(buf.slot_mut(0), c"Log");
    borrowed_str(buf.slot_mut(1), c"mqtt");
    owned_str(buf.slot_mut(2), level);
    owned_str(buf.slot_mut(3), message);
    object(
        &mut particle,
        &[c"_class", c"source", c"level", c"message"],
        &mut buf,
    );
    buf.release_all();
    emit_inbound(&particle);
    release(&mut particle);
}

/// Does `filter` (with `+` and `#`) match `topic`? The broker already
/// decided; this picks which subscription's `then` a message is for when
/// several overlap.
fn filter_matches(filter: &str, topic: &str) -> bool {
    let mut f = filter.split('/');
    let mut t = topic.split('/');
    loop {
        match (f.next(), t.next()) {
            (Some("#"), _) => return true,
            (Some("+"), Some(_)) => continue,
            (Some(a), Some(b)) if a == b => continue,
            (None, None) => return true,
            _ => return false,
        }
    }
}

fn simple_ok(out: &mut CodeValue, class: &'static std::ffi::CStr, ok: bool) {
    let mut buf = SlotBuffer::new(2);
    borrowed_str(buf.slot_mut(0), class);
    boolean(buf.slot_mut(1), ok);
    object(out, &[c"_class", c"ok"], &mut buf);
    buf.release_all();
}

fn handle_config(out: &mut CodeValue, particle: &CodeValue) {
    let url = match read_field_str(particle, "url") {
        Some(u) if !u.is_empty() => u,
        _ => {
            exception(
                out,
                "mqtt",
                "Config needs a `url` string, like mqtt://127.0.0.1:1883",
            );
            return;
        }
    };
    let rest = url
        .strip_prefix("mqtt://")
        .or_else(|| url.strip_prefix("tcp://"))
        .unwrap_or(url);
    let rest = rest.trim_end_matches('/');
    let (host, port) = match rest.rsplit_once(':') {
        Some((h, p)) if !h.is_empty() => match p.parse::<u16>() {
            Ok(p) => (h.to_string(), p),
            Err(_) => {
                exception(out, "mqtt", &format!("Config: '{p}' is not a port"));
                return;
            }
        },
        _ => (rest.to_string(), 1883),
    };
    if host.is_empty() {
        exception(out, "mqtt", "Config: the url names no host");
        return;
    }
    let client_id = read_field_str(particle, "client_id")
        .filter(|c| !c.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("code-{}", std::process::id()));
    *settings() = Some(Settings {
        host,
        port,
        username: read_field_str(particle, "username")
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        password: read_field_str(particle, "password").map(str::to_string),
        client_id,
    });
    simple_ok(out, c"ConfigResult", true);
}

/// The connection thread, started on first use. Returns the reason when
/// it cannot be.
fn ensure_bus() -> Result<(), String> {
    let mut slot = bus();
    if slot.as_ref().is_some_and(|b| b.thread.is_some()) {
        return Ok(());
    }
    let guard = settings();
    let s = guard
        .as_ref()
        .ok_or_else(|| "mqtt needs Config before Subscribe or Publish".to_string())?;
    let mut options = MqttOptions::new(s.client_id.clone(), s.host.clone(), s.port);
    options.set_keep_alive(Duration::from_secs(30));
    options.set_clean_session(true);
    // The client's default is 10 KB a packet; Zigbee2MQTT's retained device
    // list alone is more, and its definitions run to megabytes.
    options.set_max_packet_size(16 * 1024 * 1024, 16 * 1024 * 1024);
    if let Some(user) = &s.username {
        options.set_credentials(user.clone(), s.password.clone().unwrap_or_default());
    }
    drop(guard);

    let (client, mut connection) = Client::new(options, 64);
    let shared = Arc::new(Shared {
        subscriptions: Mutex::new(Vec::new()),
        connected: AtomicBool::new(false),
        stopping: AtomicBool::new(false),
        sent: Mutex::new(Vec::new()),
        in_flight: Mutex::new(Vec::new()),
    });
    let on_thread = Arc::clone(&shared);
    let resubscribe = client.clone();
    let handle = thread::spawn(move || {
        // Said once per change: a broker that is down is one line, not one
        // a second until it is back.
        let mut lost: Option<String> = None;
        for event in connection.iter() {
            if on_thread.stopping.load(Ordering::SeqCst) {
                break;
            }
            match event {
                Ok(Event::Incoming(Packet::ConnAck(_))) => {
                    on_thread.connected.store(true, Ordering::SeqCst);
                    if lost.take().is_some() {
                        report_log("Info", "connected to the broker again");
                    }
                    on_thread
                        .sent
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .clear();
                    on_thread
                        .in_flight
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .clear();
                    let topics: Vec<String> = {
                        let mut subs = on_thread
                            .subscriptions
                            .lock()
                            .unwrap_or_else(|e| e.into_inner());
                        for s in subs.iter_mut() {
                            s.acked = false;
                        }
                        subs.iter().map(|s| s.topic.clone()).collect()
                    };
                    for topic in topics {
                        ask_broker(&resubscribe, &on_thread, topic);
                    }
                }
                Ok(Event::Outgoing(rumqttc::Outgoing::Subscribe(pkid))) => {
                    let mut sent = on_thread.sent.lock().unwrap_or_else(|e| e.into_inner());
                    if !sent.is_empty() {
                        let topic = sent.remove(0);
                        on_thread
                            .in_flight
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .push((pkid, topic));
                    }
                }
                Ok(Event::Incoming(Packet::SubAck(ack))) => {
                    let mut in_flight = on_thread
                        .in_flight
                        .lock()
                        .unwrap_or_else(|e| e.into_inner());
                    if let Some(at) = in_flight.iter().position(|(pkid, _)| *pkid == ack.pkid) {
                        let (_, topic) = in_flight.remove(at);
                        let mut subs = on_thread
                            .subscriptions
                            .lock()
                            .unwrap_or_else(|e| e.into_inner());
                        if let Some(s) = subs.iter_mut().find(|s| s.topic == topic) {
                            s.acked = true;
                        }
                    }
                }
                Ok(Event::Incoming(Packet::Publish(publish))) => {
                    let payload = String::from_utf8_lossy(&publish.payload).into_owned();
                    let subs = on_thread
                        .subscriptions
                        .lock()
                        .unwrap_or_else(|e| e.into_inner());
                    // Every subscription the topic falls under hears it —
                    // two filters are two things the program asked for.
                    for sub in subs
                        .iter()
                        .filter(|s| filter_matches(&s.topic, &publish.topic))
                    {
                        let mut particle =
                            message_particle(&sub.then, &publish.topic, &payload, publish.retain);
                        emit_inbound(&particle);
                        release(&mut particle);
                    }
                }
                Ok(Event::Incoming(Packet::Disconnect))
                | Ok(Event::Outgoing(rumqttc::Outgoing::Disconnect)) => {
                    on_thread.connected.store(false, Ordering::SeqCst);
                }
                Ok(_) => {}
                Err(e) => {
                    on_thread.connected.store(false, Ordering::SeqCst);
                    if on_thread.stopping.load(Ordering::SeqCst) {
                        break;
                    }
                    let reason = e.to_string();
                    if lost.as_deref() != Some(reason.as_str()) {
                        report_log("Warn", &format!("the broker connection is down: {reason}"));
                        lost = Some(reason);
                    }
                    // The client reconnects on the next poll; a second
                    // between tries keeps a dead broker from a hot loop.
                    thread::sleep(Duration::from_secs(1));
                }
            }
        }
        on_thread.connected.store(false, Ordering::SeqCst);
    });
    *slot = Some(Bus {
        client,
        shared,
        thread: Some(handle),
    });
    Ok(())
}

fn handle_subscribe(out: &mut CodeValue, particle: &CodeValue) {
    let topic = match read_field_str(particle, "topic") {
        Some(t) if !t.is_empty() => t.to_string(),
        _ => {
            exception(out, "mqtt", "Subscribe needs a `topic` filter");
            return;
        }
    };
    let then = match find_field(particle, "then").and_then(template_from_then) {
        Some(t) => t,
        None => {
            exception(
                out,
                "mqtt",
                "Subscribe needs `then`: a class name, or a particle with `_class`",
            );
            return;
        }
    };
    if let Err(reason) = ensure_bus() {
        exception(out, "mqtt", &reason);
        return;
    }
    let slot = bus();
    let b = slot.as_ref().expect("ensure_bus filled it");
    {
        let mut subs = b
            .shared
            .subscriptions
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(existing) = subs.iter_mut().find(|s| s.topic == topic) {
            release(&mut existing.then);
            existing.then = then;
            // Already asked for, or about to be by the ConnAck.
            simple_ok(out, c"Subscribed", true);
            return;
        }
        subs.push(Subscription {
            topic: topic.clone(),
            then,
            acked: false,
        });
    }
    // Asked for now when connected; otherwise the ConnAck will ask.
    if b.shared.connected.load(Ordering::SeqCst) {
        ask_broker(&b.client, &b.shared, topic);
    }
    simple_ok(out, c"Subscribed", true);
}

fn handle_unsubscribe(out: &mut CodeValue, particle: &CodeValue) {
    let topic = match read_field_str(particle, "topic") {
        Some(t) if !t.is_empty() => t.to_string(),
        _ => {
            exception(out, "mqtt", "Unsubscribe needs a `topic`");
            return;
        }
    };
    let slot = bus();
    let existed = match slot.as_ref() {
        Some(b) => {
            let mut subs = b
                .shared
                .subscriptions
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let before = subs.len();
            subs.retain_mut(|s| {
                if s.topic == topic {
                    release(&mut s.then);
                    false
                } else {
                    true
                }
            });
            if b.shared.connected.load(Ordering::SeqCst) {
                let _ = b.client.try_unsubscribe(topic);
            }
            subs.len() != before
        }
        None => false,
    };
    simple_ok(out, c"Unsubscribed", existed);
}

fn handle_publish(out: &mut CodeValue, particle: &CodeValue) {
    let topic = match read_field_str(particle, "topic") {
        Some(t) if !t.is_empty() => t.to_string(),
        _ => {
            exception(out, "mqtt", "Publish needs a `topic`");
            return;
        }
    };
    let payload = match find_field(particle, "payload") {
        Some(p) if p.tag == CodeTag::Str => read_str(p).unwrap_or("").to_string(),
        Some(p) if p.tag == CodeTag::Number => {
            read_number(p).map(|n| format!("{n}")).unwrap_or_default()
        }
        Some(p) if p.tag == CodeTag::Bool => {
            read_bool(p).map(|b| b.to_string()).unwrap_or_default()
        }
        None => String::new(),
        _ => {
            exception(
                out,
                "mqtt",
                "Publish takes `payload` as text — Stringify an object first",
            );
            return;
        }
    };
    let retain = read_field_bool(particle, "retain").unwrap_or(false);
    if let Err(reason) = ensure_bus() {
        exception(out, "mqtt", &reason);
        return;
    }
    let slot = bus();
    let b = slot.as_ref().expect("ensure_bus filled it");
    if !b.shared.connected.load(Ordering::SeqCst) {
        exception(out, "mqtt", "not connected to the broker");
        return;
    }
    match b
        .client
        .try_publish(topic, QoS::AtLeastOnce, retain, payload.into_bytes())
    {
        Ok(()) => simple_ok(out, c"Published", true),
        Err(e) => exception(out, "mqtt", &format!("could not publish: {e}")),
    }
}

fn handle_status(out: &mut CodeValue) {
    let slot = bus();
    let (connected, acked): (bool, Vec<String>) = match slot.as_ref() {
        Some(b) => (
            b.shared.connected.load(Ordering::SeqCst),
            b.shared
                .subscriptions
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .filter(|s| s.acked)
                .map(|s| s.topic.clone())
                .collect(),
        ),
        None => (false, Vec::new()),
    };
    let mut list = SlotBuffer::new(acked.len());
    for (i, topic) in acked.iter().enumerate() {
        owned_str(list.slot_mut(i as i64), topic);
    }
    let mut subscribed = CodeValue::zeroed();
    array(&mut subscribed, &mut list);
    list.release_all();
    let mut buf = SlotBuffer::new(3);
    borrowed_str(buf.slot_mut(0), c"BusStatus");
    boolean(buf.slot_mut(1), connected);
    copy(buf.slot_mut(2), &subscribed);
    object(out, &[c"_class", c"connected", c"subscribed"], &mut buf);
    buf.release_all();
    release(&mut subscribed);
}

fn handle_disconnect(out: &mut CodeValue) {
    let taken = bus().take();
    let ok = match taken {
        Some(mut b) => {
            b.shared.stopping.store(true, Ordering::SeqCst);
            let _ = b.client.try_disconnect();
            if let Some(handle) = b.thread.take() {
                let _ = handle.join();
            }
            let mut subs = b
                .shared
                .subscriptions
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            for s in subs.iter_mut() {
                release(&mut s.then);
            }
            subs.clear();
            true
        }
        None => false,
    };
    simple_ok(out, c"Disconnected", ok);
}

#[no_mangle]
pub extern "C" fn code_module_abi_version() -> u32 {
    CODE_ABI_VERSION
}

/// # Safety
///
/// Both pointers must be valid for the duration of the call and laid out
/// per `code_abi.h` — the host guarantees this on every dispatch.
#[no_mangle]
pub unsafe extern "C" fn code_module_dispatch(out: *mut CodeValue, particle: *const CodeValue) {
    let particle = &*particle;
    guarded(&mut *out, "mqtt", |out| {
        match read_field_str(particle, "_class") {
            Some("Config") => handle_config(out, particle),
            Some("Subscribe") => handle_subscribe(out, particle),
            Some("Unsubscribe") => handle_unsubscribe(out, particle),
            Some("Publish") => handle_publish(out, particle),
            Some("Status") => handle_status(out),
            Some("Disconnect") => handle_disconnect(out),
            _ => null(out),
        }
    });
}
