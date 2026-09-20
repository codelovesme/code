# `mqtt` — a broker's messages as particles

```code
link "mqtt.so" as bus

emit Config { url = "mqtt://192.168.1.172:1883", username = "home", password = "…" } to bus
emit Subscribe { topic = "zigbee2mqtt/#", then = "Spoke" } to bus

Spoke { topic, payload, retained } =>
    | payload is text: `{"temperature":22.5,"humidity":48}` — Parse it.
    ...

emit Publish { topic = "zigbee2mqtt/hall-light/set", payload = "{\"state\":\"ON\"}" } to bus get sent
| sent ∈ Published, or an Exception when there is no connection to put it on
```

## The handlers

```
Config { url, username?, password?, client_id? }  → ConfigResult { ok }
Subscribe { topic, then }                          → Subscribed { ok }
Unsubscribe { topic }                              → Unsubscribed { ok }   | ok = false when it was not subscribed
Publish { topic, payload, retain? }                → Published { ok }      | Exception when not connected
Status {}                                          → BusStatus { connected, subscribed }
Disconnect {}                                      → Disconnected { ok }   | ok = false when there was nothing to end
```

`url` is `mqtt://host:port`; port 1883 when left out. Nothing is opened at
`Config`: the first `Subscribe` or `Publish` starts the connection, on a
thread of this module's own, and that thread keeps reconnecting — a second
between tries — for as long as the program has not said `Disconnect`.

**`then` names the particle every message on that filter arrives as** — a
class name, or a whole particle written where the subscription is asked
for, whose fields ride along with each message. `topic`, `payload` (the
bytes as text) and `retained` are added on top. The handler runs on the
program's own thread, between its statements, never on this module's. A
pushed class the program has no handler for is dropped — a `then` nobody
handles is a subscription nobody hears.

The module speaks first about the connection too: `Log { source = "mqtt",
level, message }` when the broker is lost and when it is back, once per
change. A program with no `Log` handler never hears it.

A subscription is remembered: asked for the moment there is a connection,
and asked for again after every reconnect. `Status.subscribed` lists the
filters the broker has **acknowledged** on the current connection — a
program that publishes something it expects to hear back waits for that,
because a broker only delivers what was published after it registered the
filter.

**A publish with no connection is refused now**, as an `Exception`, not
queued: a command to a device that cannot be delivered is something the
program should know about now, not something a broker that comes back in
an hour should carry out then. `payload` is text (`Stringify` an object
first); a number or a boolean is written as its text.

`Disconnect` ends the connection and **joins the thread**, so a host that
unloads the application right after finds nothing of this module still
running. While the thread is alive the module is serving
(`code_module_serving`), which keeps a standalone program up.

## Where it works

A machine. There is no browser half: a page has no socket to hold a
broker connection on.

## Testing it

`tests/mqtt_unreachable.code` needs no network — port 1 on loopback.
`tests/mqtt_module.rs` carries a broker of its own, a few dozen lines of
MQTT 3.1.1 on loopback, and drives the whole round trip in both output
modes: subscribe before there is a connection, `Status` saying when the
broker has it, a retained message arriving marked so, a publish coming
back as the particle the program named, and `Disconnect` letting the
program end.
