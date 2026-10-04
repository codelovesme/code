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

`Draw { into?, width, height, commands, event? } → CanvasResult { ok, width,
height, reason? }` clears the canvas, executes one frame, and answers the
logical dimensions. `into` is a canvas selector and defaults to
`#aquarium-canvas`. `width` and `height` define a logical coordinate space
from 1 through 4096. The backing surface tracks the selected canvas's current
size and device pixel ratio, capped at 2, so one frame uses the same
coordinates on a phone and a desktop.

`event` may name the class that should receive a primary click. It arrives as
`event { x, y, pointer_type }`, where x/y are converted into logical frame
coordinates. The listener is installed once for each canvas element and reads
the latest frame dimensions and event class.

## Drawing commands

`commands` is limited to 4,000 items per frame. Each command has an `op` and
uses logical coordinates:

- `rect`: `x`, `y`, `width`, `height`, optional `fill`, `stroke`, `line_width`,
  and `alpha`.
- `ellipse`: `x`, `y`, `rx`, `ry`, optional `rotation` in radians, `fill`,
  `stroke`, `line_width`, and `alpha`.
- `poly`: `points = [{ x, y }, …]`, optional `closed`, `fill`, `stroke`,
  `line_width`, and `alpha`.
- `path`: `path = [{ op = "move" | "line", x, y }, { op = "bezier", cx1,
  cy1, cx2, cy2, x, y }, { op = "close" }]`, with optional `fill`, `stroke`,
  `line_width`, and `alpha`.
- `text`: `text`, `x`, `y`, optional `size`, `font`, `align`, `fill`, and
  `alpha`.

`fill` may be a CSS color or a linear gradient value:
`{ type = "linear", x0, y0, x1, y1, stops = [{ at, color }, …] }`.
Unsupported commands are ignored; malformed frames return `ok = false` with a
short reason. Values are data sent to the Canvas 2D API, never executable
JavaScript or HTML.

## Where it works

This module draws only in a browser. A machine build answers `Exception` with
`source = "canvas"` and explains that a page is required.

## Building

```bash
cargo rustc --target wasm32-unknown-unknown --release --crate-type staticlib
```
