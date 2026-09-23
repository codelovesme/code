# `tty` — the terminal as a screen

Keys pushed in as particles, rows of styled text drawn out. What a
full-screen terminal program (an editor, a dashboard) is written on. Machine
only: a browser has no terminal.

```code
link "tty.so" as tty

emit Start {} to tty get size        | StartResult { cols, rows }

Key { name, text } =>
  if name = "ctrl+q", emit Stop {} to tty get _
  emit Draw { rows = [[{ text = "you pressed $name", style = "status" }]] } to tty
```

## Handlers

```
Start { key?, resize? }                     → StartResult { cols, rows }
Draw  { rows?, overlays?, cursor_row?, cursor_col? }
                                            → DrawResult  { rows }      rows written
Size  {}                                    → SizeResult  { cols, rows }
Stop  {}                                    → StopResult  { ok }        ok = it was started
```

Pushed in, from the module's own thread, after `Start`:

```
Key    { name, text }
Resize { cols, rows }
```

- **`Start`** takes the terminal over: the alternate screen, raw mode (so
  `ctrl+c` is a key, not a signal), the cursor hidden. The program stays up
  while started. `key` / `resize` rename the two pushed classes.
- **Key names** are VS Code's: `a`, `shift+a`, `ctrl+s`, `ctrl+shift+e`,
  `alt+f`, `enter`, `escape`, `backspace`, `delete`, `tab`, `shift+tab`,
  `up`/`down`/`left`/`right` (with `ctrl+`/`shift+`), `home`, `end`,
  `pageup`, `pagedown`, `f1`…`f12`. `text` is what a printable key types
  (`A` for `shift+a`, `∈` for `∈`), `""` otherwise. `Start` asks the
  terminal for xterm's *modifyOtherKeys* and kitty's keyboard protocol,
  which is what tells `ctrl+1` from `1` and `ctrl+shift+e` from `ctrl+e`; a
  terminal that knows neither sends the classic encoding, where those pairs
  are the same key.
- **`Draw`** takes a whole screen. `rows` is laid from the top at column
  0, one entry per row; `overlays` then go on top, in order, each `{ row,
  col, spans }` — panes side by side, a menu dropped over them. A row (and
  an overlay's `spans`) is a list of spans `{ text, style, width?, align?, fg?, bg?, bold? }`,
  or a bare string. `width` pads or cuts a span to exactly that many
  characters (`align = "right"` pads on the left), so a program lays out
  columns without counting characters. `fg` / `bg` (`[r, g, b]`) and
  `bold` win over the style's own — how a terminal's colours are shown (see
  `pty`). Everything is clipped at the edge;
  only rows that changed since the last `Draw` are written. The cursor shows
  at `cursor_row`/`cursor_col` (zero-based), hidden if they are absent. Tabs
  become spaces and control characters are dropped: a program cannot move
  the cursor or change the terminal by printing.
- **Styles** are names, the colours this module's (VS Code's dark theme):
  `plain menu menu_active menu_item menu_item_active menu_key
  menu_key_active title pane_title pane_title_focus sidebar selected
  selected_focus folder dim line_number line_number_active status
  status_warn prompt border`, and the `syntax` module's kinds `comment
  string number keyword class property variable operator`. An unknown name
  is `plain`.
- **`Stop`** gives the terminal back as it was found. So does the process
  exiting without it, and `SIGTERM`/`SIGHUP`.
- Before `Start`, `Draw` is an `Exception`; `Start` without a terminal on
  stdin and stdout is an `Exception`.
