//! The `window` native module — a desktop window as a screen of letters.
//! Machine only (X11 or Wayland).
//!
//! What a terminal app, or any full-screen program, draws on when it wants
//! a window of its own instead of a terminal: the same rows of styled spans
//! as the `tty` module's `Draw`, laid on a grid of cells in a monospace
//! font; and keys named as `tty` names them — but whole, since a window
//! loses nothing a terminal does (ctrl+tab is not tab, ctrl+j not enter).
//!
//! Handlers:
//!
//! - `Open { title?, cols?, rows?, font?, bold_font?, font_size?, headless? }`
//!   → `WindowOpened { id, cols, rows, cell_width, cell_height }`. `font` is
//!   a font file (TTF/OTF); without one, the system's monospace (fontconfig's
//!   `fc-match monospace`), and its bold for `bold_font`. `font_size` in
//!   pixels, 15 by default, times the display's scale. `headless` draws into
//!   memory only — no window, no display needed (tests).
//! - `Draw { id, rows?, overlays?, cursor_row?, cursor_col? }` → `Drawn { id }`
//!   — exactly the `tty` module's `Draw`: `rows` from the top, then
//!   `overlays` (`{ row, col, spans }`) on top, spans `{ text, style?, fg?,
//!   bg?, bold?, width?, align? }`; the cursor a block where it is given.
//! - `Title { id, text }` → `Titled { id }`.
//! - `Size { id }` → `WindowSize { id, cols, rows, cell_width, cell_height,
//!   width, height }`.
//! - `Text { id }` → `WindowText { id, rows }`: what is drawn, as plain text.
//! - `Pixel { id, x, y }` → `WindowPixel { id, rgb }`: one pixel, drawn (a
//!   test looks at colours with it).
//! - `Close { id }` → `Closed { id }`.
//! - `Copy { text }` → `Copied { ok }`: `text` on the desktop's clipboard.
//! - `Paste {}` → `Pasted { text }`: what is on it ("" when nothing is).
//!
//! Pushed, from the window's own thread:
//!
//! - `Key { id, name, text }` — a key, as the `tty` module's `Key`.
//! - `Resize { id, cols, rows }` — the grid is another size now.
//! - `Mouse { id, kind, button, row, col, mods, lines? }` — `kind` press,
//!   release, drag, move or wheel (`button` 0 left, 1 middle, 2 right; a
//!   wheel's 0 up, 1 down, `lines` how far); `row` / `col` the cell.
//! - `Focus { id, focused }` — the window got or lost the keys.
//! - `CloseRequested { id }` — its close button: the program decides.
//!
//! The program stays up while a window is open. The last window closed
//! ends the window thread, and a program cannot open windows again after —
//! the window system allows one such loop to a program.

mod grid;
mod keys;
mod raster;

use code_native::*;
use grid::{Grid, Span};
use raster::Face;
use std::collections::HashMap;
use std::num::NonZeroU32;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy};
use winit::keyboard::{Key as WKey, ModifiersState, NamedKey};
use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
use winit::window::{Window, WindowId};

const NAME: &str = "window";

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
/// Non-zero while the window thread runs: the host must not unmap code a
/// thread is in.
static THREADS: AtomicUsize = AtomicUsize::new(0);
static LOOP_ENDED: AtomicBool = AtomicBool::new(false);
static PROXY: Mutex<Option<EventLoopProxy<Cmd>>> = Mutex::new(None);

/// A window's screen, shared between the program's calls and the window
/// thread: what to draw, in which font, at which size.
struct Shared {
    grid: Grid,
    cursor: Option<(usize, usize)>,
    face: Face,
    font_size: f32,
    width: usize,
    height: usize,
    title: String,
    headless: bool,
}

type SharedRef = Arc<Mutex<Shared>>;

static WINDOWS: Mutex<Option<HashMap<u64, SharedRef>>> = Mutex::new(None);

fn with_windows<R>(f: impl FnOnce(&mut HashMap<u64, SharedRef>) -> R) -> R {
    let mut guard = WINDOWS.lock().unwrap_or_else(|e| e.into_inner());
    f(guard.get_or_insert_with(HashMap::new))
}

fn shared(id: u64) -> Option<SharedRef> {
    with_windows(|w| w.get(&id).cloned())
}

fn lock(s: &SharedRef) -> std::sync::MutexGuard<'_, Shared> {
    s.lock().unwrap_or_else(|e| e.into_inner())
}

const BACKGROUND: (u8, u8, u8) = (30, 30, 30);

// ── Values in and out ──────────────────────────────────────────────────

/// A value to hand back, built without the slot bookkeeping every time.
enum Val {
    Str(String),
    Num(f64),
    Bool(bool),
    Arr(Vec<Val>),
    Obj(Vec<(String, Val)>),
}

fn build(v: &Val) -> CodeValue {
    let mut out = CodeValue::zeroed();
    match v {
        Val::Str(s) => owned_str(&mut out, s),
        Val::Num(n) => number(&mut out, *n),
        Val::Bool(b) => boolean(&mut out, *b),
        Val::Arr(items) => {
            let mut buf = SlotBuffer::new(items.len());
            for (i, item) in items.iter().enumerate() {
                let mut x = build(item);
                copy(buf.slot_mut(i as i64), &x);
                release(&mut x);
            }
            array(&mut out, &mut buf);
            buf.release_all();
        }
        Val::Obj(fields) => {
            let keys: Vec<&str> = fields.iter().map(|(k, _)| k.as_str()).collect();
            let mut buf = SlotBuffer::new(keys.len());
            for (i, (_, x)) in fields.iter().enumerate() {
                let mut x = build(x);
                copy(buf.slot_mut(i as i64), &x);
                release(&mut x);
            }
            object_dyn(&mut out, &keys, &mut buf);
            buf.release_all();
        }
    }
    out
}

fn particle(class: &str, fields: Vec<(&str, Val)>) -> Val {
    let mut all = vec![("_class".to_string(), Val::Str(class.to_string()))];
    all.extend(fields.into_iter().map(|(k, v)| (k.to_string(), v)));
    Val::Obj(all)
}

fn answer(out: &mut CodeValue, v: Val) {
    let mut built = build(&v);
    copy(out, &built);
    release(&mut built);
}

fn push(v: Val) {
    let mut built = build(&v);
    emit_inbound(&built);
    release(&mut built);
}

fn num(n: usize) -> Val {
    Val::Num(n as f64)
}

fn rgb(v: &CodeValue) -> Option<(u8, u8, u8)> {
    if v.tag != CodeTag::Array || v.len != 3 {
        return None;
    }
    let parts: Vec<u8> = array_elems(v).filter_map(read_number).filter(|n| (0.0..=255.0).contains(n)).map(|n| n as u8).collect();
    (parts.len() == 3).then(|| (parts[0], parts[1], parts[2]))
}

fn span_of(v: &CodeValue) -> Span {
    if let Some(s) = read_str(v) {
        return Span { text: s.to_string(), style: "plain".to_string(), ..Default::default() };
    }
    Span {
        text: read_field_str(v, "text").unwrap_or("").to_string(),
        style: read_field_str(v, "style").unwrap_or("plain").to_string(),
        width: read_field_number(v, "width").filter(|w| *w >= 0.0).map(|w| w as usize),
        right: read_field_str(v, "align") == Some("right"),
        fg: find_field(v, "fg").and_then(rgb),
        bg: find_field(v, "bg").and_then(rgb),
        bold: read_field_bool(v, "bold"),
    }
}

fn spans_of(row: &CodeValue) -> Vec<Span> {
    if row.tag == CodeTag::Array {
        array_elems(row).map(span_of).collect()
    } else if read_str(row).is_some() {
        vec![span_of(row)]
    } else {
        Vec::new()
    }
}

fn place(n: Option<f64>) -> Option<usize> {
    n.filter(|n| *n >= 0.0 && n.is_finite()).map(|n| n as usize)
}

fn id_of(p: &CodeValue) -> Option<u64> {
    read_field_number(p, "id").filter(|n| *n >= 1.0).map(|n| n as u64)
}

/// Runs `f` on window `id`'s screen, or answers that there is none.
fn on(out: &mut CodeValue, p: &CodeValue, what: &str, f: impl FnOnce(&mut CodeValue, u64, &SharedRef)) {
    let Some(id) = id_of(p) else {
        return exception(out, NAME, &format!("{what} needs `id`, a window's number"));
    };
    match shared(id) {
        Some(s) => f(out, id, &s),
        None => exception(out, NAME, &format!("{what}: no window {id}")),
    }
}

// ── Handlers ───────────────────────────────────────────────────────────

fn open(out: &mut CodeValue, p: &CodeValue) {
    let size = |field: &str, default: usize| {
        read_field_number(p, field).filter(|n| *n >= 1.0 && *n <= 2000.0).map(|n| n as usize).unwrap_or(default)
    };
    let (cols, rows) = (size("cols", 100), size("rows", 30));
    let font_size = read_field_number(p, "font_size").filter(|n| *n >= 4.0 && *n <= 200.0).unwrap_or(15.0) as f32;
    let title = read_field_str(p, "title").unwrap_or("code").to_string();
    let headless = read_field_bool(p, "headless") == Some(true);
    let face = match Face::new(read_field_str(p, "font"), read_field_str(p, "bold_font"), font_size) {
        Ok(f) => f,
        Err(e) => return exception(out, NAME, &format!("Open: {e}")),
    };
    let (width, height) = (cols * face.cell_w, rows * face.cell_h);
    let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
    let screen = Arc::new(Mutex::new(Shared { grid: Grid::new(cols, rows), cursor: None, face, font_size, width, height, title, headless }));
    with_windows(|w| w.insert(id, screen.clone()));
    if !headless {
        if let Err(e) = open_window(id) {
            with_windows(|w| w.remove(&id));
            return exception(out, NAME, &format!("Open: {e}"));
        }
    }
    let s = lock(&screen);
    answer(
        out,
        particle(
            "WindowOpened",
            vec![("id", Val::Num(id as f64)), ("cols", num(s.grid.cols)), ("rows", num(s.grid.rows)), ("cell_width", num(s.face.cell_w)), ("cell_height", num(s.face.cell_h))],
        ),
    );
}

fn draw(out: &mut CodeValue, p: &CodeValue) {
    on(out, p, "Draw", |out, id, screen| {
        let headless = {
            let mut s = lock(screen);
            let mut grid = Grid::new(s.grid.cols, s.grid.rows);
            if let Some(rows) = find_field(p, "rows").filter(|r| r.tag == CodeTag::Array) {
                for (i, row) in array_elems(rows).enumerate() {
                    grid.put(i, 0, &spans_of(row));
                }
            }
            if let Some(overlays) = find_field(p, "overlays").filter(|r| r.tag == CodeTag::Array) {
                for o in array_elems(overlays) {
                    if let (Some(row), Some(col), Some(spans)) = (place(read_field_number(o, "row")), place(read_field_number(o, "col")), find_field(o, "spans")) {
                        grid.put(row, col, &spans_of(spans));
                    }
                }
            }
            s.grid = grid;
            s.cursor = match (place(read_field_number(p, "cursor_row")), place(read_field_number(p, "cursor_col"))) {
                (Some(r), Some(c)) => Some((r, c)),
                _ => None,
            };
            s.headless
        };
        if !headless {
            send(Cmd::Redraw(id));
        }
        answer(out, particle("Drawn", vec![("id", Val::Num(id as f64))]));
    });
}

fn title(out: &mut CodeValue, p: &CodeValue) {
    let text = read_field_str(p, "text").unwrap_or("").to_string();
    on(out, p, "Title", |out, id, screen| {
        let headless = {
            let mut s = lock(screen);
            s.title = text.clone();
            s.headless
        };
        if !headless {
            send(Cmd::Title(id, text));
        }
        answer(out, particle("Titled", vec![("id", Val::Num(id as f64))]));
    });
}

fn size(out: &mut CodeValue, p: &CodeValue) {
    on(out, p, "Size", |out, id, screen| {
        let s = lock(screen);
        answer(
            out,
            particle(
                "WindowSize",
                vec![
                    ("id", Val::Num(id as f64)),
                    ("cols", num(s.grid.cols)),
                    ("rows", num(s.grid.rows)),
                    ("cell_width", num(s.face.cell_w)),
                    ("cell_height", num(s.face.cell_h)),
                    ("width", num(s.width)),
                    ("height", num(s.height)),
                ],
            ),
        );
    });
}

fn text(out: &mut CodeValue, p: &CodeValue) {
    on(out, p, "Text", |out, id, screen| {
        let s = lock(screen);
        let rows = (0..s.grid.rows).map(|r| Val::Str(s.grid.text(r))).collect();
        answer(out, particle("WindowText", vec![("id", Val::Num(id as f64)), ("rows", Val::Arr(rows))]));
    });
}

fn pixel(out: &mut CodeValue, p: &CodeValue) {
    let (x, y) = (place(read_field_number(p, "x")).unwrap_or(0), place(read_field_number(p, "y")).unwrap_or(0));
    on(out, p, "Pixel", |out, id, screen| {
        let mut s = lock(screen);
        let (w, h) = (s.grid.cols * s.face.cell_w, s.grid.rows * s.face.cell_h);
        if x >= w || y >= h {
            return exception(out, NAME, &format!("Pixel: ({x}, {y}) is outside the {w} × {h} drawn"));
        }
        let mut buf = vec![0u32; w * h];
        let (grid, cursor) = (s.grid.clone(), s.cursor);
        s.face.draw(&grid, cursor, &mut buf, w, h, BACKGROUND);
        let (r, g, b) = raster::unpack(buf[y * w + x]);
        answer(out, particle("WindowPixel", vec![("id", Val::Num(id as f64)), ("rgb", Val::Arr(vec![Val::Num(r.into()), Val::Num(g.into()), Val::Num(b.into())]))]));
    });
}

fn close(out: &mut CodeValue, p: &CodeValue) {
    let Some(id) = id_of(p) else {
        return exception(out, NAME, "Close needs `id`, a window's number");
    };
    let Some(screen) = with_windows(|w| w.remove(&id)) else {
        return exception(out, NAME, &format!("Close: no window {id}"));
    };
    if !lock(&screen).headless {
        send(Cmd::Close(id));
    }
    answer(out, particle("Closed", vec![("id", Val::Num(id as f64))]));
}

/// The desktop clipboard, made once: on X11 it is a thread serving whoever
/// pastes what this program copied, so it has to outlive the call.
static CLIPBOARD: Mutex<Option<arboard::Clipboard>> = Mutex::new(None);

fn with_clipboard<R>(f: impl FnOnce(&mut arboard::Clipboard) -> Result<R, String>) -> Result<R, String> {
    let mut guard = CLIPBOARD.lock().unwrap_or_else(|e| e.into_inner());
    if guard.is_none() {
        *guard = Some(arboard::Clipboard::new().map_err(|e| format!("no clipboard: {e}"))?);
    }
    f(guard.as_mut().expect("made above"))
}

fn copy_text(out: &mut CodeValue, p: &CodeValue) {
    let text = read_field_str(p, "text").unwrap_or("").to_string();
    match with_clipboard(|c| c.set_text(text).map_err(|e| e.to_string())) {
        Ok(()) => answer(out, particle("Copied", vec![("ok", Val::Bool(true))])),
        Err(e) => exception(out, NAME, &format!("Copy: {e}")),
    }
}

fn paste_text(out: &mut CodeValue, _p: &CodeValue) {
    match with_clipboard(|c| Ok(c.get_text().unwrap_or_default())) {
        Ok(text) => answer(out, particle("Pasted", vec![("text", Val::Str(text))])),
        Err(e) => exception(out, NAME, &format!("Paste: {e}")),
    }
}

// ── The window thread ──────────────────────────────────────────────────

/// What the program's calls ask of the window thread.
enum Cmd {
    Open(u64, mpsc::Sender<Result<(), String>>),
    Redraw(u64),
    Title(u64, String),
    Close(u64),
}

fn send(cmd: Cmd) {
    if let Some(proxy) = PROXY.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        let _ = proxy.send_event(cmd);
    }
}

/// A window on screen, once the window thread made it.
fn open_window(id: u64) -> Result<(), String> {
    ensure_loop()?;
    let (tx, rx) = mpsc::channel();
    send(Cmd::Open(id, tx));
    rx.recv_timeout(Duration::from_secs(10)).map_err(|_| "the window did not open in time".to_string())?
}

/// The window thread, started once: winit's event loop, off the main
/// thread (the program's own thread is the main one).
fn ensure_loop() -> Result<(), String> {
    if PROXY.lock().unwrap_or_else(|e| e.into_inner()).is_some() {
        return Ok(());
    }
    if LOOP_ENDED.load(Ordering::SeqCst) {
        return Err("the window thread has ended (its last window closed); a program opens windows in one run".into());
    }
    let (tx, rx) = mpsc::channel::<Result<EventLoopProxy<Cmd>, String>>();
    THREADS.fetch_add(1, Ordering::SeqCst);
    std::thread::spawn(move || {
        let mut builder = EventLoop::<Cmd>::with_user_event();
        winit::platform::x11::EventLoopBuilderExtX11::with_any_thread(&mut builder, true);
        winit::platform::wayland::EventLoopBuilderExtWayland::with_any_thread(&mut builder, true);
        match builder.build() {
            Err(e) => {
                let _ = tx.send(Err(format!("no display to open a window on ({e})")));
            }
            Ok(event_loop) => {
                let _ = tx.send(Ok(event_loop.create_proxy()));
                let mut app = App::default();
                let _ = event_loop.run_app(&mut app);
            }
        }
        *PROXY.lock().unwrap_or_else(|e| e.into_inner()) = None;
        LOOP_ENDED.store(true, Ordering::SeqCst);
        THREADS.fetch_sub(1, Ordering::SeqCst);
    });
    let proxy = rx.recv_timeout(Duration::from_secs(10)).map_err(|_| "the window thread did not start".to_string())??;
    *PROXY.lock().unwrap_or_else(|e| e.into_inner()) = Some(proxy);
    Ok(())
}

struct Live {
    id: u64,
    window: Arc<Window>,
    surface: softbuffer::Surface<Arc<Window>, Arc<Window>>,
    pointer: (f64, f64),
    held: Option<u32>,
}

#[derive(Default)]
struct App {
    windows: HashMap<WindowId, Live>,
    mods: ModifiersState,
}

impl App {
    fn by_id(&mut self, id: u64) -> Option<&mut Live> {
        self.windows.values_mut().find(|l| l.id == id)
    }

    fn mods(&self) -> keys::Mods {
        keys::Mods { ctrl: self.mods.control_key(), shift: self.mods.shift_key(), alt: self.mods.alt_key() }
    }

    fn mods_text(&self) -> String {
        let m = self.mods();
        let mut parts = Vec::new();
        if m.ctrl {
            parts.push("ctrl");
        }
        if m.shift {
            parts.push("shift");
        }
        if m.alt {
            parts.push("alt");
        }
        parts.join("+")
    }

    fn create(&mut self, el: &ActiveEventLoop, id: u64) -> Result<(), String> {
        let screen = shared(id).ok_or("the window was closed before it opened")?;
        let (title, width, height) = {
            let s = lock(&screen);
            (s.title.clone(), s.width, s.height)
        };
        let attrs = Window::default_attributes().with_title(title).with_inner_size(PhysicalSize::new(width as u32, height as u32));
        let window = Arc::new(el.create_window(attrs).map_err(|e| format!("the window could not be made: {e}"))?);
        // On a scaled display the letters are drawn that much bigger.
        let scale = window.scale_factor() as f32;
        if (scale - 1.0).abs() > f32::EPSILON {
            let mut s = lock(&screen);
            let px = s.font_size * scale;
            s.face.resize(px);
            let (w, h) = (s.grid.cols * s.face.cell_w, s.grid.rows * s.face.cell_h);
            let _ = window.request_inner_size(PhysicalSize::new(w as u32, h as u32));
        }
        let context = softbuffer::Context::new(window.clone()).map_err(|e| format!("no drawing surface: {e}"))?;
        let surface = softbuffer::Surface::new(&context, window.clone()).map_err(|e| format!("no drawing surface: {e}"))?;
        self.windows.insert(window.id(), Live { id, window, surface, pointer: (0.0, 0.0), held: None });
        Ok(())
    }

    /// The grid fitted to `width` × `height` pixels; `Resize` pushed when
    /// that is another number of cells.
    fn fit(id: u64, width: usize, height: usize) {
        let Some(screen) = shared(id) else { return };
        let (cols, rows, changed) = {
            let mut s = lock(&screen);
            s.width = width;
            s.height = height;
            let cols = (width / s.face.cell_w).max(1);
            let rows = (height / s.face.cell_h).max(1);
            let changed = cols != s.grid.cols || rows != s.grid.rows;
            if changed {
                let mut grid = Grid::new(cols, rows);
                for (r, line) in s.grid.cells.iter().enumerate().take(rows) {
                    for (c, cell) in line.iter().enumerate().take(cols) {
                        grid.cells[r][c] = *cell;
                    }
                }
                s.grid = grid;
            }
            (cols, rows, changed)
        };
        if changed {
            push(particle("Resize", vec![("id", Val::Num(id as f64)), ("cols", num(cols)), ("rows", num(rows))]));
        }
    }

    fn paint(live: &mut Live) {
        let Some(screen) = shared(live.id) else { return };
        let size = live.window.inner_size();
        let (Some(w), Some(h)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height)) else { return };
        if live.surface.resize(w, h).is_err() {
            return;
        }
        let Ok(mut buffer) = live.surface.buffer_mut() else { return };
        {
            let mut s = lock(&screen);
            let (grid, cursor) = (s.grid.clone(), s.cursor);
            s.face.draw(&grid, cursor, &mut buffer, w.get() as usize, h.get() as usize, BACKGROUND);
        }
        let _ = buffer.present();
    }

    /// The cell under the pointer.
    fn cell_at(live: &Live) -> (usize, usize) {
        let Some(screen) = shared(live.id) else { return (0, 0) };
        let s = lock(&screen);
        let col = (live.pointer.0.max(0.0) as usize / s.face.cell_w).min(s.grid.cols.saturating_sub(1));
        let row = (live.pointer.1.max(0.0) as usize / s.face.cell_h).min(s.grid.rows.saturating_sub(1));
        (row, col)
    }

    fn mouse(&self, live: &Live, kind: &str, button: u32, lines: Option<usize>) {
        let (row, col) = Self::cell_at(live);
        let mut fields = vec![
            ("id", Val::Num(live.id as f64)),
            ("kind", Val::Str(kind.into())),
            ("button", Val::Num(button.into())),
            ("row", num(row)),
            ("col", num(col)),
            ("mods", Val::Str(self.mods_text())),
        ];
        if let Some(n) = lines {
            fields.push(("lines", num(n)));
        }
        push(particle("Mouse", fields));
    }
}

fn named(key: &NamedKey) -> Option<&'static str> {
    Some(match key {
        NamedKey::Tab => "tab",
        NamedKey::Enter => "enter",
        NamedKey::Escape => "escape",
        NamedKey::Backspace => "backspace",
        NamedKey::Delete => "delete",
        NamedKey::Insert => "insert",
        NamedKey::Home => "home",
        NamedKey::End => "end",
        NamedKey::PageUp => "pageup",
        NamedKey::PageDown => "pagedown",
        NamedKey::ArrowUp => "up",
        NamedKey::ArrowDown => "down",
        NamedKey::ArrowLeft => "left",
        NamedKey::ArrowRight => "right",
        NamedKey::F1 => "f1",
        NamedKey::F2 => "f2",
        NamedKey::F3 => "f3",
        NamedKey::F4 => "f4",
        NamedKey::F5 => "f5",
        NamedKey::F6 => "f6",
        NamedKey::F7 => "f7",
        NamedKey::F8 => "f8",
        NamedKey::F9 => "f9",
        NamedKey::F10 => "f10",
        NamedKey::F11 => "f11",
        NamedKey::F12 => "f12",
        _ => return None,
    })
}

impl ApplicationHandler<Cmd> for App {
    fn resumed(&mut self, _: &ActiveEventLoop) {}

    fn user_event(&mut self, el: &ActiveEventLoop, cmd: Cmd) {
        match cmd {
            Cmd::Open(id, reply) => {
                let _ = reply.send(self.create(el, id));
            }
            Cmd::Redraw(id) => {
                if let Some(live) = self.by_id(id) {
                    live.window.request_redraw();
                }
            }
            Cmd::Title(id, text) => {
                if let Some(live) = self.by_id(id) {
                    live.window.set_title(&text);
                }
            }
            Cmd::Close(id) => {
                self.windows.retain(|_, l| l.id != id);
                if self.windows.is_empty() {
                    el.exit();
                }
            }
        }
    }

    fn window_event(&mut self, _el: &ActiveEventLoop, wid: WindowId, event: WindowEvent) {
        let Some(id) = self.windows.get(&wid).map(|l| l.id) else { return };
        match event {
            WindowEvent::CloseRequested => push(particle("CloseRequested", vec![("id", Val::Num(id as f64))])),
            WindowEvent::Resized(size) => {
                Self::fit(id, size.width as usize, size.height as usize);
                if let Some(live) = self.windows.get(&wid) {
                    live.window.request_redraw();
                }
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                if let Some(screen) = shared(id) {
                    let mut s = lock(&screen);
                    let px = s.font_size * scale_factor as f32;
                    s.face.resize(px);
                }
                if let Some(live) = self.windows.get(&wid) {
                    let size = live.window.inner_size();
                    Self::fit(id, size.width as usize, size.height as usize);
                    live.window.request_redraw();
                }
            }
            WindowEvent::ModifiersChanged(m) => self.mods = m.state(),
            WindowEvent::Focused(focused) => push(particle("Focus", vec![("id", Val::Num(id as f64)), ("focused", Val::Bool(focused))])),
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state != ElementState::Pressed {
                    return;
                }
                let pressed = match &event.logical_key {
                    WKey::Named(NamedKey::Space) => Some(keys::Pressed::Char { base: ' ', shown: ' ' }),
                    WKey::Named(n) => named(n).map(keys::Pressed::Named),
                    WKey::Character(shown) => {
                        let base = match event.key_without_modifiers() {
                            WKey::Character(b) => b.chars().next(),
                            _ => None,
                        };
                        let shown = shown.chars().next();
                        match (base.or(shown), shown.or(base)) {
                            (Some(base), Some(shown)) => Some(keys::Pressed::Char { base, shown }),
                            _ => None,
                        }
                    }
                    _ => None,
                };
                if let Some(pressed) = pressed {
                    let (name, text) = keys::name(&pressed, self.mods());
                    push(particle("Key", vec![("id", Val::Num(id as f64)), ("name", Val::Str(name)), ("text", Val::Str(text))]));
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let held = {
                    let Some(live) = self.windows.get_mut(&wid) else { return };
                    let before = Self::cell_at(live);
                    live.pointer = (position.x, position.y);
                    let after = Self::cell_at(live);
                    (before != after).then_some(live.held)
                };
                if let (Some(held), Some(live)) = (held, self.windows.get(&wid)) {
                    match held {
                        Some(b) => self.mouse(live, "drag", b, None),
                        None => self.mouse(live, "move", 0, None),
                    }
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let b = match button {
                    MouseButton::Left => 0,
                    MouseButton::Middle => 1,
                    MouseButton::Right => 2,
                    _ => return,
                };
                let pressed = state == ElementState::Pressed;
                if let Some(live) = self.windows.get_mut(&wid) {
                    live.held = if pressed { Some(b) } else { None };
                }
                if let Some(live) = self.windows.get(&wid) {
                    self.mouse(live, if pressed { "press" } else { "release" }, b, None);
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y as f64,
                    MouseScrollDelta::PixelDelta(p) => {
                        let h = shared(id).map(|s| lock(&s).face.cell_h).unwrap_or(16) as f64;
                        p.y / h
                    }
                };
                if lines.abs() < 0.5 {
                    return;
                }
                if let Some(live) = self.windows.get(&wid) {
                    self.mouse(live, "wheel", if lines > 0.0 { 0 } else { 1 }, Some(lines.abs().round() as usize));
                }
            }
            WindowEvent::RedrawRequested => {
                if let Some(live) = self.windows.get_mut(&wid) {
                    Self::paint(live);
                }
            }
            _ => {}
        }
    }
}

// ── The module ─────────────────────────────────────────────────────────

/// Non-zero while the window thread runs.
#[no_mangle]
pub extern "C" fn code_module_serving() -> std::ffi::c_int {
    i32::from(THREADS.load(Ordering::SeqCst) > 0)
}

declare_inbound!();
declare_inbound_reply!(answered);

/// The program's answer to something pushed. Nothing waits on it.
fn answered(_particle: &CodeValue, _result: &CodeValue) {}

#[no_mangle]
pub extern "C" fn code_module_abi_version() -> u32 {
    CODE_ABI_VERSION
}

/// # Safety
///
/// Both pointers must be valid for the duration of the call and laid out
/// per `code_abi.h` — the host guarantees this on every dispatch.
#[no_mangle]
pub unsafe extern "C" fn code_module_dispatch(out: *mut CodeValue, particle: *const CodeValue) {
    let p = &*particle;
    guarded(&mut *out, NAME, |out| match read_field_str(p, "_class") {
        Some("Open") => open(out, p),
        Some("Draw") => draw(out, p),
        Some("Title") => title(out, p),
        Some("Size") => size(out, p),
        Some("Text") => text(out, p),
        Some("Pixel") => pixel(out, p),
        Some("Close") => close(out, p),
        Some("Copy") => copy_text(out, p),
        Some("Paste") => paste_text(out, p),
        _ => null(out),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(f: fn(&mut CodeValue, &CodeValue), p: Val) -> CodeValue {
        let mut out = CodeValue::zeroed();
        f(&mut out, &build(&p));
        out
    }

    /// Headless: opened, drawn, read back as text and pixels, closed — no
    /// display.
    #[test]
    fn headless_window() {
        if raster::system_font(false).is_none() {
            return;
        }
        let opened = call(open, particle("Open", vec![("cols", Val::Num(10.0)), ("rows", Val::Num(2.0)), ("headless", Val::Bool(true))]));
        let id = read_field_number(&opened, "id").expect("an id");
        assert_eq!(read_field_number(&opened, "cols"), Some(10.0));
        let cw = read_field_number(&opened, "cell_width").unwrap() as usize;
        let red = Val::Obj(vec![("text".into(), Val::Str("hi".into())), ("bg".into(), Val::Arr(vec![Val::Num(200.0), Val::Num(0.0), Val::Num(0.0)]))]);
        let drawn = call(
            draw,
            particle(
                "Draw",
                vec![
                    ("id", Val::Num(id)),
                    ("rows", Val::Arr(vec![Val::Arr(vec![red])])),
                    ("overlays", Val::Arr(vec![Val::Obj(vec![("row".into(), Val::Num(1.0)), ("col".into(), Val::Num(3.0)), ("spans".into(), Val::Arr(vec![Val::Str("xy".into())]))])])),
                ],
            ),
        );
        assert_eq!(read_field_str(&drawn, "_class"), Some("Drawn"));
        let shown = call(text, particle("Text", vec![("id", Val::Num(id))]));
        let rows: Vec<String> = array_elems(find_field(&shown, "rows").unwrap()).filter_map(read_str).map(str::to_string).collect();
        assert_eq!(rows, ["hi        ", "   xy     "]);
        let px = call(pixel, particle("Pixel", vec![("id", Val::Num(id)), ("x", Val::Num(1.0)), ("y", Val::Num(1.0))]));
        let rgb: Vec<f64> = array_elems(find_field(&px, "rgb").unwrap()).filter_map(read_number).collect();
        assert_eq!(rgb, [200.0, 0.0, 0.0]);
        let px = call(pixel, particle("Pixel", vec![("id", Val::Num(id)), ("x", Val::Num((cw * 9 + 1) as f64)), ("y", Val::Num(1.0))]));
        let rgb: Vec<f64> = array_elems(find_field(&px, "rgb").unwrap()).filter_map(read_number).collect();
        assert_eq!(rgb, [30.0, 30.0, 30.0]);
        let closed = call(close, particle("Close", vec![("id", Val::Num(id))]));
        assert_eq!(read_field_str(&closed, "_class"), Some("Closed"));
        let gone = call(text, particle("Text", vec![("id", Val::Num(id))]));
        assert_eq!(read_field_str(&gone, "_class"), Some("Exception"));
    }

    /// A real window, where there is a display (the tests run it under
    /// Xvfb): it opens at its size, is drawn, closes — and the thread ends.
    #[test]
    fn a_real_window() {
        if std::env::var("DISPLAY").is_err() && std::env::var("WAYLAND_DISPLAY").is_err() {
            return;
        }
        if raster::system_font(false).is_none() {
            return;
        }
        let opened = call(open, particle("Open", vec![("cols", Val::Num(20.0)), ("rows", Val::Num(5.0)), ("title", Val::Str("test".into()))]));
        assert_eq!(read_field_str(&opened, "_class"), Some("WindowOpened"), "{:?}", read_field_str(&opened, "message"));
        let id = read_field_number(&opened, "id").unwrap();
        let drawn = call(draw, particle("Draw", vec![("id", Val::Num(id)), ("rows", Val::Arr(vec![Val::Str("hello".into())]))]));
        assert_eq!(read_field_str(&drawn, "_class"), Some("Drawn"));
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(THREADS.load(Ordering::SeqCst), 1);
        call(close, particle("Close", vec![("id", Val::Num(id))]));
        for _ in 0..100 {
            if THREADS.load(Ordering::SeqCst) == 0 {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(THREADS.load(Ordering::SeqCst), 0, "the window thread ended with its last window");
        // The desktop clipboard: copied, then pasted back.
        let copied = call(copy_text, particle("Copy", vec![("text", Val::Str("from code".into()))]));
        assert_eq!(read_field_str(&copied, "_class"), Some("Copied"), "{:?}", read_field_str(&copied, "message"));
        let pasted = call(paste_text, particle("Paste", vec![]));
        assert_eq!(read_field_str(&pasted, "text"), Some("from code"));
    }
}
