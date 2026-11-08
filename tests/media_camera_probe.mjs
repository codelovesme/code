// The probe for tests/media_camera.rs: `media`'s camera under node, against
// a stand-in phone with three cameras (front, rear, rear wide), a motion
// sensor the probe tilts, and a canvas that records how it was turned.
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
function streamFor(c) {
  const v = c.video === true ? {} : c.video;
  const id = v.deviceId?.exact ?? (v.facingMode?.exact === "user" ? "front" : "rear");
  const facing = id === "front" ? "user" : "environment";
  const track = {
    stop() {},
    getSettings: () => ({ deviceId: id, facingMode: facing, width: 4000, height: 3000, zoom: 1 }),
    getCapabilities: () => (id === "rear" ? { zoom: { min: 0.5, max: 8, step: 0.1 } } : {}),
    applyConstraints: (x) => { zoomed = x; return Promise.resolve(); },
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
const doc = {
  body: null,
  querySelector: () => null,
  createElement: (tag) => {
    if (tag === "video") {
      return { srcObject: null, play() { return Promise.resolve(); }, addEventListener(_, f) { f(); },
        get videoWidth() { return this.srcObject ? 3000 : 0; }, get videoHeight() { return this.srcObject ? 4000 : 0; } };
    }
    const pen = { turns: [], drawn: null, translate(x, y) { this.turns.push(["t", x, y]); }, rotate(a) { this.turns.push(["r", Math.round(a * 180 / Math.PI)]); }, drawImage(_, x, y, w, h) { this.drawn = [w, h]; } };
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
check("the zoom", ready.zoom, { min: 0.5, max: 8, step: 0.1, value: 1 });
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
check("upright is portrait", [last("Captured").orientation, pens.at(-1).turns], ["portrait", []]);

// Held on its side, counter-clockwise, with the screen still upright: a
// landscape photo, turned a quarter back.
listeners.devicemotion({ accelerationIncludingGravity: { x: 9.6, y: 0.8, z: 1 } });
media({ _class: "TakePhoto", max_side: 4096 });
await wait(20);
check("held sideways", [last("Captured").width, last("Captured").height, last("Captured").orientation], [4000, 3000, "landscape"]);
check("turned back", pens.at(-1).turns, [["t", 0, 3000], ["r", -90]]);
check("the frame drawn whole", pens.at(-1).drawn, [3000, 4000]);
// The other way round.
listeners.devicemotion({ accelerationIncludingGravity: { x: -9.6, y: 0.5, z: 1 } });
media({ _class: "TakePhoto", max_side: 4096 });
await wait(20);
check("held the other way", [last("Captured").orientation, pens.at(-1).turns], ["landscape", [["t", 4000, 0], ["r", 90]]]);
// Lying flat says nothing: the screen's way up.
listeners.devicemotion({ accelerationIncludingGravity: { x: 0.3, y: 0.2, z: 9.8 } });
media({ _class: "TakePhoto", max_side: 4096 });
await wait(20);
check("flat keeps the frame", last("Captured").orientation, "portrait");
// Asked for a shape, it gets it whatever the sensor says.
media({ _class: "TakePhoto", max_side: 4096, orientation: "landscape" });
await wait(20);
check("asked landscape", last("Captured").orientation, "landscape");

// Shut: the sensor is let go.
media({ _class: "StopCamera" });
check("the sensor was let go", listeners.devicemotion, undefined);
process.exit(0);
