// The probe for tests/media_shrink.rs: `media`'s Shrink under node, with a
// stand-in decoder (the "picture" is its size written as text, "4032x3024")
// and a canvas that records what was drawn.
import { readFileSync } from "node:fs";
const half = new Function("return (" + readFileSync(process.argv[2], "utf8") + ")")();

const decodedWith = [];
globalThis.createImageBitmap = async (blob, options) => {
  decodedWith.push(options ?? null);
  const text = await blob.text();
  const m = /^(\d+)x(\d+)$/.exec(text);
  if (!m) throw new Error("The source image could not be decoded.");
  return { width: Number(m[1]), height: Number(m[2]), close() {} };
};
// What the browser's reader does: the blob's bytes as a data: URL.
globalThis.FileReader = class {
  readAsDataURL(blob) {
    blob.arrayBuffer().then((b) => { this.result = `data:${blob.type};base64,` + Buffer.from(b).toString("base64"); this.onload(); });
  }
};
const drawn = [];
const doc = {
  body: null,
  querySelector: () => null,
  createElement: (tag) => {
    if (tag !== "canvas") return {};
    const canvas = {
      width: 0,
      height: 0,
      getContext: () => ({ drawImage: (_p, x, y, w, h) => drawn.push([x, y, w, h]) }),
      toBlob: (done, type, quality) => done(new Blob([`${canvas.width}x${canvas.height}@${quality}`], { type })),
    };
    return canvas;
  },
};
const fired = [];
const [, media] = half({ doc, fire: (p) => fired.push(p) });
const b64 = (s) => Buffer.from(s).toString("base64");
const check = (what, got, want) => {
  if (JSON.stringify(got) !== JSON.stringify(want)) {
    console.error(`${what}: got ${JSON.stringify(got)}, wanted ${JSON.stringify(want)}`);
    process.exit(1);
  }
};
const settle = () => new Promise((r) => setTimeout(r, 20));

check("nothing to shrink claimed success", media({ _class: "Shrink" }), { _class: "ShrinkResult", ok: false });

// A phone photo, landscape: 400 on its longer side, by default at 0.8.
check("a shrink did not answer at once", media({ _class: "Shrink", image_base64: b64("4032x3024"), content_type: "image/jpeg", ref: "m:1" }), { _class: "ShrinkResult", ok: true });
await settle();
check("the photo was not decoded the way the browser shows it", decodedWith[0], { imageOrientation: "from-image" });
check("the small one", fired.shift(), { _class: "Shrunk", ref: "m:1", image_base64: b64("400x300@0.8"), content_type: "image/jpeg", width: 400, height: 300 });
check("it was drawn whole", drawn.shift(), [0, 0, 400, 300]);

// Portrait, with its own size and quality asked for.
media({ _class: "Shrink", image_base64: b64("3024x4032"), max_side: 200, quality: 0.6, ref: { id: 7 } });
await settle();
check("a portrait asked for 200", fired.shift(), { _class: "Shrunk", ref: { id: 7 }, image_base64: b64("150x200@0.6"), content_type: "image/jpeg", width: 150, height: 200 });

// Already small: kept at its size, never made larger.
media({ _class: "Shrink", image_base64: b64("120x90") });
await settle();
check("a small picture", fired.shift(), { _class: "Shrunk", ref: null, image_base64: b64("120x90@0.8"), content_type: "image/jpeg", width: 120, height: 90 });

// Not a picture: refused with the ref and a reason.
media({ _class: "Shrink", image_base64: b64("a video, really"), ref: "m:2" });
await settle();
const refused = fired.shift();
check("a non-picture was not refused", [refused._class, refused.ref], ["Unshrinkable", "m:2"]);
if (!refused.reason) { console.error("the refusal gave no reason"); process.exit(1); }
check("something else was fired", fired, []);
console.log("ok");
