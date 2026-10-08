# `canvas` — bounded 2D drawing in a browser page

```code
link "canvas.a" as canvas

emit Draw {
    into = "#aquarium-canvas"
    width = 960
    height = 600
    event = "AquariumTapped"
    commands = [
        { op = "rect", x = 0, y = 0, width = 960, height = 600, fill = "#137aa2" }
        { op = "ellipse", x = 480, y = 280, rx = 44, ry = 22, fill = "#ffad47" }
    ]
} to canvas get frame
```

## Handler

`Draw { into?, width, height, commands, event?, clear?, density? } → CanvasResult { ok, width,
height, view_width, view_height, time, reason? }` clears the canvas, executes
one frame, and answers the logical dimensions, the element's size on the page
in CSS pixels, and the page's clock in milliseconds when the frame was
painted. A scene that should fill the screen without stretching draws its next
frame at `view_width` × `view_height`; one that moves by time rather than by
frame reads the difference between two `time`s. `into` is a canvas selector and defaults to
`#aquarium-canvas`. `width` and `height` define a logical coordinate space
from 1 through 4096. The backing surface tracks the selected canvas's current
size and device pixel ratio, capped at 2, so one frame uses the same
coordinates on a phone and a desktop. `density` overrides that ratio (0.25
to 2): `density = 1` gives one backing pixel per CSS pixel, a softer and
cheaper canvas.

`event` may name the class that should receive a primary click. It arrives as
`event { x, y, pointer_type }`, where x/y are converted into logical frame
coordinates. The listener is installed once for each canvas element and reads
the latest frame dimensions and event class.

## Drawing commands

`commands` is limited to 4,000 painted items per frame, counting the commands
inside groups. Each command has an `op` and uses logical coordinates:

- `rect`: `x`, `y`, `width`, `height`, optional `fill`, `stroke`, `line_width`,
  and `alpha`.
- `ellipse`: `x`, `y`, `rx`, `ry`, optional `rotation` in radians, `fill`,
  `stroke`, `line_width`, and `alpha`.
- `poly`: `points = [{ x, y }, …]`, optional `closed`, `fill`, `stroke`,
  `line_width`, and `alpha`.
- `path`: `path = [{ op = "move" | "line", x, y }, { op = "bezier", cx1,
  cy1, cx2, cy2, x, y }, { op = "quad", cx, cy, x, y }, { op = "close" }]`,
  with optional `fill`, `stroke`, `line_width`, and `alpha`.
- `text`: `text`, `x`, `y`, optional `size`, `font`, `align`, `fill`, and
  `alpha`.
- `group`: `commands = [ … ]`, painted in order inside the group's
  `transform` and `alpha`; groups nest up to eight deep.
- `use`: `name`, a drawing kept earlier with `Keep`.

Any command may carry `transform = [a, b, c, d, e, f]`, the matrix Canvas 2D's
`transform()` takes: a shape can be drawn in its own coordinates and placed,
scaled or turned as a whole, and line widths scale with it. A command's
`alpha` multiplies the alpha of the group it is in.

`Keep { into?, name, commands } → CanvasResult { ok, kept }` stores a drawing
under a name for that canvas, without painting it; `{ op = "use", name }`
paints it inside any later frame, with the command's own `transform` and
`alpha`. Keeping an empty list forgets the name. A canvas keeps at most 1,024
drawings. A scene sends what changes and refers to what does not — a frame
can be a short list of `use`s.

Geometry may also be sent flat, which is cheaper to build: a `poly` takes
`xy = [x0, y0, x1, y1, …]` in place of `points`, and a `path` takes
`d = ["M", x, y, "L", x, y, "Q", cx, cy, x, y, "C", cx1, cy1, cx2, cy2, x, y,
"Z"]` — SVG's letters — in place of `path`.

`clear = false` on `Draw` paints over the canvas instead of clearing it first,
so a scene can be sent as several frames, back to front, rather than as one
large list.

`fill` and `stroke` may each be a CSS color or a gradient value:
`{ type = "linear", x0, y0, x1, y1, stops = [{ at, color }, …] }`, or
`{ type = "radial", x0, y0, r0, x1, y1, r1, stops = [{ at, color }, …] }`
between two circles. Every shape also takes `line_cap` (`round`, `butt`,
`square`) and `line_join` (`round`, `bevel`, `miter`); both are `round` when
not given.
Unsupported commands are ignored; malformed frames return `ok = false` with a
short reason. Values are data sent to the Canvas 2D API, never executable
JavaScript or HTML.

## Where it works

This module draws only in a browser. A machine build answers `Draw` and `Keep`
with an `Exception` whose `source` is `"canvas"` and whose message explains
that a page is required.

## Building

```bash
cargo rustc --target wasm32-unknown-unknown --release --crate-type staticlib
```
