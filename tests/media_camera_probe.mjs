// The probe for tests/media_camera.rs: `media`'s camera under node, against
// a stand-in phone with three cameras (front, rear, rear wide), a
// viewfinder the probe pinches, and a canvas that records what was drawn.
import { readFileSync } from "node:fs";
const half = new Function("return (" + readFileSync(process.argv[2], "utf8") + ")")();

const cameras = [
  { kind: "videoinput", deviceId: "front", label: "camera2 1, facing front", getCapabilities: () => ({ facingMode: ["user"] }) },
  { kind: "videoinput", deviceId: "rear", label: "camera2 0, facing back", getCapabilities: () => ({ facingMode: ["environment"] }) },
  { kind: "videoinput", deviceId: "wide", label: "camera2 2, facing back" },
  { kind: "audioinput", deviceId: "mic", label: "Microphone" },
];
const asked = [];
let zoomed = null;
let applies = 0;
function streamFor(c) {
  const v = c.video === true ? {} : c.video;
  const id = v.deviceId?.exact ?? (v.facingMode?.exact === "user" ? "front" : "rear");
  const facing = id === "front" ? "user" : "environment";
  const track = {
    stop() {},
    getSettings: () => ({ deviceId: id, facingMode: facing, width: 4000, height: 3000, zoom: 1 }),
    getCapabilities: () => (id === "rear" ? { zoom: { min: 0.5, max: 8, step: 0.1 } } : {}),
    applyConstraints: (x) => { zoomed = x; applies += 1; return Promise.resolve(); },
  };
  return { id, getTracks: () => [track], getVideoTracks: () => [track] };
}
let listeners = {};
globalThis.addEventListener = (name, f) => { listeners[name] = f; };
globalThis.removeEventListener = (name) => { delete listeners[name]; };
Object.defineProperty(globalThis, "navigator", {
  get: () => ({
    mediaDevices: {
      getUserMedia: (c) => { asked.push(c); return Promise.resolve(streamFor(c)); },
      enumerateDevices: () => Promise.resolve(cameras),
    },
  }),
  configurable: true,
});
globalThis.FileReader = class { readAsDataURL() { this.result = "data:image/jpeg;base64,SlBH"; this.onload(); } };

let pens = [];
// The viewfinder the application shows: listeners the probe can call.
const finder = { style: {}, on: {}, srcObject: null, addEventListener(name, f) { this.on[name] = f; }, play() { return Promise.resolve(); } };
const touches = (d) => [{ clientX: 0, clientY: 0 }, { clientX: d, clientY: 0 }];
const pinch = (from, to) => {
  let stopped = 0;
  const ev = (t) => ({ touches: t, preventDefault() { stopped += 1; } });
  finder.on.touchstart(ev(touches(from)));
  finder.on.touchmove(ev(touches((from + to) / 2)));
  finder.on.touchmove(ev(touches(to)));
  finder.on.touchend(ev([]));
  return stopped;
};
const doc = {
  body: null,
  querySelector: () => finder,
  createElement: (tag) => {
    if (tag === "video") {
      return { srcObject: null, play() { return Promise.resolve(); }, addEventListener(_, f) { f(); },
        get videoWidth() { return this.srcObject ? 3000 : 0; }, get videoHeight() { return this.srcObject ? 4000 : 0; } };
    }
    const pen = { drawn: null, drawImage(_, ...box) { this.drawn = box.map((n) => Math.round(n)); } };
    pens.push(pen);
    return { width: 0, height: 0, getContext: () => pen, toBlob(f, type, q) { pen.quality = q; f({ type }); } };
  },
};
const fired = [];
const [, media] = half({ doc, fire: (p) => fired.push(p) });
const check = (what, got, want) => {
  if (JSON.stringify(got) !== JSON.stringify(want)) {
    console.error(`${what}: got ${JSON.stringify(got)}, wanted ${JSON.stringify(want)}`);
    process.exit(1);
  }
};
const wait = (ms) => new Promise((r) => setTimeout(r, ms));
const last = (k) => fired.filter((p) => p._class === k).at(-1);

// Opened: the rear camera, asked for at the most pixels it has, and the
// answer lists every camera with the way it faces.
media({ _class: "StartCamera" });
await wait(20);
check("the camera was not asked for its pixels", asked[0].video.width, { ideal: 4096 });
check("the rear camera was not the default", asked[0].video.facingMode, { exact: "environment" });
const ready = last("CameraReady");
check("the open camera", [ready.camera_id, ready.facing], ["rear", "environment"]);
check("the cameras", ready.cameras.map((c) => [c.camera_id, c.facing]), [["front", "user"], ["rear", "environment"], ["wide", "environment"]]);
check("the zoom", ready.zoom, { kind: "track", min: 0.5, max: 8, value: 1 });
check("the size", [ready.width, ready.height], [3000, 4000]);

// A zoom is clamped to what the camera allows.
check("zoom answer", media({ _class: "ZoomCamera", zoom: 0.2 }), { _class: "ZoomCameraResult", ok: true, zoom: 0.5 });
check("zoom applied", zoomed, { advanced: [{ zoom: 0.5 }] });

// A camera picked by id is that camera; no id is the other side's.
media({ _class: "SwitchCamera", camera_id: "wide" });
await wait(20);
check("switched by id", asked.at(-1).video.deviceId, { exact: "wide" });
check("switched to the wide one", last("CameraReady").camera_id, "wide");
media({ _class: "SwitchCamera" });
await wait(20);
check("switched sides", asked.at(-1).video.facingMode, { exact: "user" });
media({ _class: "SwitchCamera", camera_id: "rear" });
await wait(20);

// A photo by default stays task-sized; asked for, it keeps its pixels.
media({ _class: "TakePhoto" });
await wait(20);
check("default photo", [last("Captured").width, last("Captured").height, pens.at(-1).quality], [1200, 1600, 0.85]);
media({ _class: "TakePhoto", max_side: 4096, quality: 0.92 });
await wait(20);
check("large photo", [last("Captured").width, last("Captured").height, pens.at(-1).quality], [3000, 4000, 0.92]);
check("the whole frame", pens.at(-1).drawn, [0, 0, 3000, 4000, 0, 0, 3000, 4000]);
check("upright is portrait", last("Captured").orientation, "portrait");

// Landscape, picked by hand: the frame's middle, in its own proportions
// turned — what a viewfinder of that shape shows. No sensor is asked.
media({ _class: "TakePhoto", max_side: 4096, orientation: "landscape" });
await wait(20);
check("landscape", [last("Captured").width, last("Captured").height, last("Captured").orientation], [3000, 2250, "landscape"]);
check("landscape is the middle", pens.at(-1).drawn, [0, 875, 3000, 2250, 0, 0, 3000, 2250]);
check("no sensor", listeners.devicemotion, undefined);
media({ _class: "TakePhoto", max_side: 4096, orientation: "portrait" });
await wait(20);
check("portrait stays whole", [last("Captured").width, last("Captured").height], [3000, 4000]);

// Pinching the viewfinder zooms the rear camera itself, live, and says
// where it ended; the page does not zoom with it.
const stopped = pinch(100, 300);
await wait(20);
check("the page's own pinch was stopped", stopped > 0, true);
check("the camera zoomed", zoomed, { advanced: [{ zoom: 3 }] });
check("where it ended", last("CameraZoomed").zoom, 3);
check("a camera zoom leaves the viewfinder alone", finder.style.transform ?? "", "");

// The front camera cannot zoom: the viewfinder is scaled and the photo
// cropped to match.
media({ _class: "SwitchCamera", facing: "user" });
await wait(20);
check("switched to the front by side", asked.at(-1).video.facingMode, { exact: "user" });
check("its zoom is the module's", last("CameraReady").zoom, { kind: "digital", min: 1, max: 5, value: 1 });
pinch(100, 200);
check("the viewfinder scaled", finder.style.transform, "scale(2)");
media({ _class: "TakePhoto", max_side: 4096 });
await wait(20);
check("the photo cropped to the zoom", pens.at(-1).drawn, [750, 1000, 1500, 2000, 0, 0, 1500, 2000]);
check("ZoomCamera answers where it went", media({ _class: "ZoomCamera", zoom: 9 }), { _class: "ZoomCameraResult", ok: true, zoom: 5 });
check("the viewfinder follows", finder.style.transform, "scale(5)");
media({ _class: "SwitchCamera", facing: "environment" });
await wait(20);
check("a new camera starts unzoomed", [finder.style.transform, last("CameraReady").zoom.value], ["", 1]);

media({ _class: "StopCamera" });
process.exit(0);
