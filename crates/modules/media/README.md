# `media` — the page's microphone and camera

```code
link "media.a" as media
link "dom.a" as dom

emit Record to media get r
assert r.ok

| …later, when the reader presses stop
emit StopRecording to media

| The recording arrives on its own, because it did not exist when it was
| asked for.
Recorded { audio_base64, format, ms } => {
    | …hand it to whatever wants it
}
```

## The handlers

```
Record {}          → RecordResult      { ok }   · later: Recorded { audio_base64, format, ms }
StopRecording {}   → StopResult        { ok }
StartCamera {}     → StartCameraResult { ok }   · later: CameraReady {}
SwitchCamera {}    → SwitchCameraResult { ok }  · later: CameraReady {}
TakePhoto {}       → TakePhotoResult   { ok }   · later: Captured { image_base64, width, height }
StopCamera {}      → StopResult        { ok }
Play { audio_base64, format } → PlayResult { ok }   · later: Played {}
StopPlaying {}     → StopResult        { ok }
```

`Play` is the speaker: bytes the program holds — a recording, or speech a
model made — as base64 of the named subtype (`wav`, `mp3`, `webm`). One
sound at a time; a new `Play` replaces the one playing, which then does not
get to say `Played`.

`ok = false` means the module was not in a state to do it — stopping a
recording that never started, photographing with the camera shut. It is not
an error and not a refusal.

## The answer comes later

A recording does not exist when `Record` is asked, and waiting for it would
mean blocking the page. So the handler answers *started* and the bytes arrive
as their own particle — the same shape `net_client` uses for a reply that
outlives its request.

## A refusal is an answer

Asking for a device is asking a **person**, and they may say no.

- `Denied { device, reason }` — they said no, or the page is not allowed to
  ask (a camera needs a secure context: `https`, or `localhost`).
- `Unavailable { device, reason }` — the browser has nothing to offer.

Neither is an `Exception`. A reader declining the microphone is the system
working, and an application should answer it rather than treat it as a fault.

## The viewfinder

A camera needs somewhere to show itself, and `dom` is what draws. `dom`
renders a tree that is **data** — no raw HTML, no property assignment — so
nothing an application writes can hand an element a live `MediaStream`.

So the application says *where*, and this module says *what*:

```code
{ tag = "video", media = "camera" }
```

`dom` turns `media = "camera"` into a plain attribute and does nothing else
with it — it does not know what a camera is. This module watches the page for
that attribute and attaches the stream wherever it appears.

**Watches, rather than attaches once.** A redraw replaces the element, and an
application redraws whenever anything changes. Attaching once would mean a
viewfinder that goes black on the next keystroke.

`TakePhoto` draws the frame from that same element, so a photo is what the
reader could see rather than what the camera happened to be sending. It is
cut down before it leaves — the longer side capped at 1600 pixels, JPEG at
0.85 — because a phone's frame is thousands of pixels across, and as base64
that is more than a service takes in one request. `width` and `height` on
`Captured` are the picture's as sent.

## Bytes

`audio_base64` and `image_base64` are base64 with no `data:` prefix — the
same shape every other module moves bytes in, so one can be handed straight
to `blob_storage`, `azure_blob` or an API without being unwrapped first.

## Where it works

**A browser.** On a machine every handler answers an `Exception` saying so: a
microphone belongs to whoever is sitting there, and a program with nobody
sitting there has no business inventing one.

For wasm it is built as an archive linked into the program:

```bash
cargo rustc --target wasm32-unknown-unknown --release --crate-type staticlib
```

## Testing it

A headless browser will not ask a person, so it has to be told to answer for
one. Chromium takes `--use-fake-device-for-media-stream` (a synthetic camera
and microphone) and `--use-fake-ui-for-media-capture` (permission granted
without a prompt), which is how `my-euglena-apps`' browser suite drives this
for real rather than with a stub.
