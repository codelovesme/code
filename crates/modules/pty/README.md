# `pty` — a shell in a pseudo-terminal

What an editor's integrated terminal is made of: a program (your shell, by
default) running on a pseudo-terminal, a terminal emulator keeping its
screen, and that screen handed out as rows of coloured spans — the shape the
`tty` module's `Draw` takes. Machine only.

```code
link "pty.so" as pty

emit Open { cols = 80, rows = 12, cwd = "/home/me/project" } to pty get t

TerminalOutput { id } =>
    emit Screen { id = id } to pty get s
    | s.lines: one list of { text, fg?, bg?, bold? } per row

emit Type { id = t.id, name = "enter", text = "" } to pty
```

## Handlers

```
Open   { cols?, rows?, command?, args?, cwd?, env? }  → TerminalOpened { id }
Type   { id, name, text }                             → Typed    { id, bytes }
Write  { id, data }                                   → Written  { id, bytes }
Resize { id, cols, rows }                             → Resized  { id }
Screen { id }                                         → TerminalScreen { id, lines, cursor_row, cursor_col, cursor_visible, alive, code }
Close  { id }                                         → Closed   { id }
```

Pushed, from the terminal's own thread:

```
TerminalOutput { id }         the screen changed
TerminalExited { id, code }   the program ended
```

- `Open` starts `command` (default `$SHELL`, else `/bin/sh`) with `args`
  in its own session, the pseudo-terminal as its controlling terminal — so
  Ctrl+C is a signal to it and job control works. `TERM=xterm-256color`.
  Size defaults to 80 × 24.
- `Type` takes a key the way `tty` names it (`enter`, `ctrl+c`, `up`,
  `alt+b`, `f5`, …) and sends the bytes a terminal would, in the program's
  cursor mode. `Write` sends text as it is.
- `Screen` is the whole screen: `lines` has one entry per row, each a list of
  spans that look the same. `fg` / `bg` are `[r, g, b]` — the 16 colours are
  VS Code's terminal palette, 256-colour and 24-bit are exact — and absent
  where the terminal said "default", so the drawer uses its own. Inverse
  video comes out swapped.
- `TerminalOutput` is coalesced: after one is pushed, the next waits until a
  `Screen` has read the screen. A flood of output is one redraw.
- `Close` hangs up on the program (SIGHUP), and kills it half a second later
  if it is still there. The program stays up while a terminal's thread runs.
- Not yet: scrollback, the window title, mouse reporting.
