// The page's half of `media`: the microphone and the camera.
//
// A particle in, a particle out — but the interesting answers cannot be the
// return value. A recording does not exist when `Record` is asked, and
// waiting for it would mean blocking the page, so the handler answers
// "started" and the bytes arrive later as their own particle. The same shape
// `net_client` uses for a reply that outlives its request.
//
// Asking for a device is asking a person. A refusal is `Denied`, not an
// exception: a reader saying no is an answer.
(ctx) => {
  const { doc, fire } = ctx;

  // The attribute `dom` writes for `media = "camera"`. Read here rather than
  // agreed in prose: the two modules never call each other, and this string
  // is the whole of what passes between them.
  const SINK = "data-code-media";

  let recorder = null;      // MediaRecorder, while recording
  let opening = false;      // the microphone asked for and not yet given
  let stopEarly = false;    // Stop came while it was still being asked for
  let chunks = [];          // what it has handed over so far
  let startedAt = 0;
  let micStream = null;
  let camStream = null;
  let watching = null;      // MutationObserver, while the camera is open

  // The module's own view of the camera, off the page and never drawn.
  //
  // A photo is taken from this and never from the element the application is
  // showing. That element belongs to `dom`, which replaces it on every
  // redraw — and an application redraws whenever anything changes, so a
  // photo taken just after one would be of an element that had existed for a
  // millisecond and seen no frames. This one is attached once, when the
  // camera opens, and outlives every redraw.
  let ownView = null;

  const ok = (klass, value = true) => ({ _class: klass, ok: value });

  /// A device that could not be had. `Denied` when the person said no,
  /// `Unavailable` when the browser has nothing to offer — different
  /// answers because they call for different next moves.
  function refused(device, error) {
    const name = String(error?.name ?? "");
    const klass =
      name === "NotAllowedError" || name === "SecurityError" ? "Denied" : "Unavailable";
    fire({ _class: klass, device, reason: String(error?.message ?? name ?? "refused") });
  }

  /// Bytes as base64, without a data: prefix — the same shape every other
  /// module moves bytes in, so an application can hand one to another.
  function toBase64(blob) {
    return new Promise((resolve, reject) => {
      const reader = new FileReader();
      reader.onerror = () => reject(reader.error);
      reader.onload = () => {
        const text = String(reader.result || "");
        const comma = text.indexOf(",");
        resolve(comma < 0 ? text : text.slice(comma + 1));
      };
      reader.readAsDataURL(blob);
    });
  }

  /// The viewfinder the application is showing, if it is showing one.
  ///
  /// `querySelector` and never `querySelectorAll`: hosted inside another
  /// application, `doc` is not the page but a stand-in scoped to this
  /// guest's own container, and it offers exactly what `dom` reaches for —
  /// which is the singular. Reaching for more works standing alone and
  /// fails the moment the application is hosted, which is a bad way to find
  /// out. One viewfinder is also all a camera has to show.
  const sink = () => doc.querySelector(`[${SINK}="camera"]`);

  /// Put the live stream into it.
  ///
  /// `srcObject` is a property, not an attribute, which is exactly why this
  /// is here and not in `dom`: a tree that is data cannot carry a live
  /// object. The application says *where*, this says *what*.
  function fillSinks() {
    if (!camStream) return;
    const el = sink();
    if (!el || el.srcObject === camStream) return;
    el.srcObject = camStream;
    // A viewfinder nobody asked to hear, and one that plays without being
    // told to: an application should not have to know either.
    el.muted = true;
    el.playsInline = true;
    const played = el.play?.();
    if (played && typeof played.catch === "function") played.catch(() => {});
  }

  /// A redraw replaces the element the stream was attached to, so attaching
  /// once is attaching until the next keystroke. Watching the page costs
  /// nothing while the camera is shut and is the only way to survive a
  /// redraw the application did not tell us about.
  function watchForSinks() {
    if (watching || !doc.body) return;
    watching = new MutationObserver(() => fillSinks());
    watching.observe(doc.body, { childList: true, subtree: true });
  }

  function stopWatching() {
    watching?.disconnect();
    watching = null;
  }

  function letGo(stream) {
    for (const track of stream?.getTracks?.() ?? []) track.stop();
  }

  /// Attach the stream to the module's own element and wait until it is
  /// actually showing something. `videoWidth` is 0 until the first frame
  /// lands, and a canvas drawn from it before then is a blank rectangle —
  /// which encodes to a perfectly valid, perfectly empty photograph.
  function ownViewReady(stream) {
    if (!ownView) {
      ownView = doc.createElement("video");
      ownView.muted = true;
      ownView.playsInline = true;
    }
    if (ownView.srcObject !== stream) ownView.srcObject = stream;
    const played = ownView.play?.();
    if (played && typeof played.catch === "function") played.catch(() => {});
    if (ownView.videoWidth > 0) return Promise.resolve(ownView);
    return new Promise((resolve) => {
      const done = () => resolve(ownView);
      ownView.addEventListener("loadeddata", done, { once: true });
      // Belt and braces: some browsers have the frame before the event.
      setTimeout(done, 2000);
    });
  }

  const devices = () => globalThis.navigator?.mediaDevices ?? null;

  return [
    "media",
    (particle) => {
      switch (particle._class) {
        case "Record": {
          if (recorder || opening) return ok("RecordResult", false);
          const media = devices();
          if (!media) {
            fire({
              _class: "Unavailable",
              device: "microphone",
              reason: "this browser offers no media devices",
            });
            return ok("RecordResult", false);
          }
          opening = true;
          stopEarly = false;
          media
            .getUserMedia({ audio: true })
            .then((stream) => {
              opening = false;
              // Stopped before the microphone was given — a press and a
              // release quicker than a permission prompt. Nothing was
              // recorded, and the answer says so rather than never coming;
              // and the microphone is let go, not left on.
              if (stopEarly) {
                stopEarly = false;
                letGo(stream);
                fire({ _class: "Recorded", audio_base64: "", format: "none", ms: 0 });
                return;
              }
              micStream = stream;
              chunks = [];
              startedAt = Date.now();
              recorder = new globalThis.MediaRecorder(stream);
              recorder.ondataavailable = (e) => {
                if (e.data && e.data.size) chunks.push(e.data);
              };
              // A recorder that fails mid-way says so through here and then
              // stops; `onstop` still runs, with whatever was handed over.
              recorder.onerror = (e) => {
                fire({
                  _class: "Unavailable",
                  device: "microphone",
                  reason: `the recording failed: ${e?.error?.message ?? e?.error ?? "unknown"}`,
                });
              };
              recorder.onstop = async () => {
                const type = recorder?.mimeType || "audio/webm";
                const blob = new Blob(chunks, { type });
                const ms = Date.now() - startedAt;
                recorder = null;
                chunks = [];
                letGo(micStream);
                micStream = null;
                try {
                  fire({
                    _class: "Recorded",
                    audio_base64: await toBase64(blob),
                    // The subtype is what a caller needs — "webm", "mp4" —
                    // and every browser spells the rest of it differently.
                    format: String(type).split(";")[0].split("/")[1] || "webm",
                    ms,
                  });
                } catch (e) {
                  fire({
                    _class: "Unavailable",
                    device: "microphone",
                    reason: `the recording could not be read: ${e}`,
                  });
                }
              };
              recorder.start();
            })
            .catch((e) => {
              opening = false;
              stopEarly = false;
              refused("microphone", e);
            });
          return ok("RecordResult");
        }

        case "StopRecording": {
          // Asked for and not yet given: the stop is kept and honoured the
          // moment the microphone arrives. `ok` — a `Recorded` will follow.
          if (opening) {
            stopEarly = true;
            return ok("StopResult");
          }
          if (!recorder) return ok("StopResult", false);
          // `onstop` above is what fires `Recorded`; stopping is all this
          // has to do. A recorder already inactive throws on `stop`, and
          // that is not an error worth ending a handler over.
          try {
            recorder.stop();
          } catch {
            // Already stopped; `onstop` has fired or is about to.
          }
          return ok("StopResult");
        }

        case "StartCamera": {
          if (camStream) {
            fillSinks();
            return ok("StartCameraResult");
          }
          const media = devices();
          if (!media) {
            fire({
              _class: "Unavailable",
              device: "camera",
              reason: "this browser offers no media devices",
            });
            return ok("StartCameraResult", false);
          }
          media
            .getUserMedia({ video: true })
            .then(async (stream) => {
              camStream = stream;
              watchForSinks();
              fillSinks();
              // Ready means there is a frame to photograph, not merely that
              // permission was given — otherwise the first `TakePhoto` after
              // it is a race the application cannot see.
              await ownViewReady(stream);
              fire({ _class: "CameraReady" });
            })
            .catch((e) => refused("camera", e));
          return ok("StartCameraResult");
        }

        case "TakePhoto": {
          if (!camStream) return ok("TakePhotoResult", false);
          const stream = camStream;
          ownViewReady(stream).then((view) => {
            const width = view.videoWidth;
            const height = view.videoHeight;
            if (!width || !height) {
              fire({
                _class: "Unavailable",
                device: "camera",
                reason: "the camera has not produced a frame yet",
              });
              return;
            }
            const canvas = doc.createElement("canvas");
            canvas.width = width;
            canvas.height = height;
            const pen = canvas.getContext("2d");
            if (!pen) {
              fire({
                _class: "Unavailable",
                device: "camera",
                reason: "this browser has no 2d canvas to draw the frame on",
              });
              return;
            }
            pen.drawImage(view, 0, 0, width, height);
            canvas.toBlob(async (blob) => {
            if (!blob) {
              fire({
                _class: "Unavailable",
                device: "camera",
                reason: "the frame could not be encoded",
              });
              return;
            }
            try {
              fire({
                _class: "Captured",
                image_base64: await toBase64(blob),
                width,
                height,
              });
            } catch (e) {
              fire({
                _class: "Unavailable",
                device: "camera",
                reason: `the frame could not be read: ${e}`,
              });
            }
            }, "image/jpeg", 0.9);
          });
          return ok("TakePhotoResult");
        }

        case "StopCamera": {
          if (!camStream) return ok("StopResult", false);
          const shown = sink();
          if (shown) shown.srcObject = null;
          if (ownView) ownView.srcObject = null;
          letGo(camStream);
          camStream = null;
          stopWatching();
          return ok("StopResult");
        }

        default:
          return null;
      }
    },
  ];
}
