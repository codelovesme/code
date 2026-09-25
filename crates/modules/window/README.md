# `window` — a desktop window as a screen of letters

A window of its own for a full-screen program: the same rows of styled
spans as the `tty` module's `Draw`, laid on a grid of cells in a monospace
font; keys named as `tty` names them — but whole, since a window loses
nothing a terminal does (ctrl+tab is not tab, ctrl+j is not enter, ctrl+1
is not 1). Machine only; X11 or Wayland, whichever the desktop runs.

```code
link "window.so" as win

emit Open { title = "hello", cols = 80, rows = 24 } to win get w
emit Draw { id = w.id, rows = [[{ text = "Hello", style = "keyword" }, " world"]] } to win

Key { id, name, text } =>
    | "ctrl+tab", "shift+a" (text "A"), "enter", "f5" …
    return Seen
```

## Handlers

```
Open   { title?, cols?, rows?, font?, bold_font?, font_size?, headless? } → WindowOpened { id, cols, rows, cell_width, cell_height }
Copy   { text }                        → Copied { ok }                 (the desktop clipboard)
Paste  {}                              → Pasted { text }
Draw   { id, rows?, overlays?, cursor_row?, cursor_col? }                 → Drawn { id }
Title  { id, text }                                                        → Titled { id }
Size   { id }                          → WindowSize { id, cols, rows, cell_width, cell_height, width, height }
Text   { id }                          → WindowText { id, rows }       (what is drawn, as plain text)
Pixel  { id, x, y }                    → WindowPixel { id, rgb }       (one pixel, drawn)
Close  { id }                          → Closed { id }
```

Pushed to the program, from the window's own thread:

```
Key            { id, name, text }
Resize         { id, cols, rows }
Mouse          { id, kind, button, row, col, mods, lines? }   kind: press release drag move wheel
Focus          { id, focused }
CloseRequested { id }                                         the close button; the program decides
```

`font` is a font file (TTF/OTF); without one, the system's monospace
(`fc-match monospace`) and its bold. `font_size` is in pixels (15), times
the display's scale. `headless` draws into memory only — no display — which
is what tests use, with `Text` and `Pixel` to look at the result.

`Draw` is `tty`'s: `rows` from the top, then `overlays` (`{ row, col,
spans }`) over them in order; a span is `{ text, style?, fg?, bg?, bold?,
width?, align? }` or a bare string; style names and their colours are
`tty`'s, so a screen drawn for the terminal looks the same here.

## The decisions, and why

**The CPU draws.** Letters on a grid are cheap to draw, and `softbuffer`
plus `fontdue` need no GPU, no driver and no system library at build time
(X11 and Wayland are opened at run time). A GPU can come later if a screen
ever draws slowly.

**One window thread, off the main one.** winit's event loop runs on a
thread of its own (allowed on Linux), so the program's own thread stays
the program's. The loop can be made once per program: when its last window
closes the thread ends — so the program can end — and a program cannot
open windows again after.
