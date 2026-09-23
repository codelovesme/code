//! The `tty` native module — the terminal as a screen. Machine only.
//!
//! Handlers:
//!
//! - `Start { key?, resize? }` — takes the terminal over: the alternate
//!   screen, raw mode (so `ctrl+c` and `ctrl+q` are keys, not signals), the
//!   cursor hidden until a `Draw` places it. Answers `StartResult { cols,
//!   rows }`. From then on every key pressed arrives as `Key { name, text }`
//!   and every change of the window's size as `Resize { cols, rows }` —
//!   pushed in from this module's own thread. `key` and `resize` rename the
//!   two classes, the way `timer` lets a program name what it is sent.
//!   While started, the program stays up (`code_module_serving`).
//! - `Draw { rows?, overlays?, cursor_row?, cursor_col? }` — puts a whole
//!   screen up. `rows` is laid from the top at column 0, one entry per
//!   screen row; `overlays` then go on top, in order, each `{ row, col,
//!   spans }` — which is how a program puts panes side by side and a menu
//!   over them. A row (and an overlay's `spans`) is a list of spans `{
//!   text, style, width?, align? }`, or a bare string. `width` pads or cuts
//!   the span to exactly that many characters (`align = "right"` pads on the
//!   left), so a program can lay out columns without counting characters.
//!   Styles are names this module owns — see `screen.rs`. Only rows that
//!   changed since the last `Draw` are written. The cursor shows at
//!   `cursor_row` / `cursor_col` (zero-based) and is hidden when they are
//!   absent. Answers `DrawResult { rows }`, the number of rows written.
//! - `Size {}` — `SizeResult { cols, rows }`.
//! - `Stop {}` — gives the terminal back as it was found. Answers
//!   `StopResult { ok }`, and the program is free to end.
//!
//! The terminal is also given back when the process exits without `Stop`
//! (an `atexit`), and on `SIGTERM` / `SIGHUP` — a program that dies must not
//! leave a shell that echoes nothing.
//!
//! Keys are named the way VS Code names them (`ctrl+s`, `shift+tab`,
//! `ctrl+shift+e`, `alt+f`, `pagedown`, `f10`); `text` is what a printable
//! key types, `""` otherwise. See `keys.rs`.

mod keys;
mod screen;

use code_native::*;
use std::io::Write;
use std::mem::MaybeUninit;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::thread;

const NAME: &str = "tty";

/// True between `Start` and `Stop`: the reader thread runs, and the
/// program is held open.
static RUNNING: AtomicBool = AtomicBool::new(false);
/// Set by `SIGWINCH`, taken by the reader thread.
static RESIZED: AtomicBool = AtomicBool::new(false);
/// The terminal as `Start` found it. Written once before `HAVE_ORIGINAL`
/// is set, read by the restore path — including from a signal handler,
/// which is why it is not behind a lock.
static mut ORIGINAL: MaybeUninit<libc::termios> = MaybeUninit::uninit();
static HAVE_ORIGINAL: AtomicBool = AtomicBool::new(false);
static HOOKS_INSTALLED: AtomicBool = AtomicBool::new(false);

struct State {
    reader: Option<thread::JoinHandle<()>>,
    /// The last frame, row by row, and the size it was drawn at. `None`
    /// means the next `Draw` redraws everything.
    previous: Option<(Vec<String>, (u16, u16))>,
}

static STATE: Mutex<State> = Mutex::new(State { reader: None, previous: None });

fn state() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

const ENTER: &[u8] = b"\x1b[?1049h\x1b[?25l\x1b[>4;2m\x1b[>1u\x1b[6 q\x1b[2J";
const LEAVE: &[u8] = b"\x1b[<u\x1b[>4m\x1b[0 q\x1b[0m\x1b[?25h\x1b[?1049l";

fn write_raw(bytes: &[u8]) {
    let mut rest = bytes;
    while !rest.is_empty() {
        let n = unsafe { libc::write(1, rest.as_ptr().cast(), rest.len()) };
        if n <= 0 {
            return;
        }
        rest = &rest[n as usize..];
    }
}

/// Gives the terminal back. Only async-signal-safe calls (`write`,
/// `tcsetattr`), since a signal handler runs it too; safe to run twice.
fn restore() {
    if HAVE_ORIGINAL.swap(false, Ordering::SeqCst) {
        write_raw(LEAVE);
        unsafe {
            let original = std::ptr::addr_of!(ORIGINAL).read().assume_init();
            libc::tcsetattr(0, libc::TCSAFLUSH, &original);
        }
    }
}

extern "C" fn restore_at_exit() {
    RUNNING.store(false, Ordering::SeqCst);
    restore();
}

extern "C" fn on_signal(sig: libc::c_int) {
    if sig == libc::SIGWINCH {
        RESIZED.store(true, Ordering::SeqCst);
        return;
    }
    restore();
    unsafe { libc::_exit(128 + sig) };
}

fn install_hooks() {
    if HOOKS_INSTALLED.swap(true, Ordering::SeqCst) {
        return;
    }
    unsafe {
        libc::atexit(restore_at_exit);
        for sig in [libc::SIGWINCH, libc::SIGTERM, libc::SIGHUP] {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t;
            action.sa_flags = libc::SA_RESTART;
            libc::sigemptyset(&mut action.sa_mask);
            libc::sigaction(sig, &action, std::ptr::null_mut());
        }
    }
}

fn size() -> Option<(u16, u16)> {
    for fd in [1, 0] {
        let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
        if unsafe { libc::ioctl(fd, libc::TIOCGWINSZ, &mut ws) } == 0 && ws.ws_col > 0 && ws.ws_row > 0 {
            return Some((ws.ws_col, ws.ws_row));
        }
    }
    None
}

fn is_terminal() -> bool {
    unsafe { libc::isatty(0) == 1 && libc::isatty(1) == 1 }
}

fn push(class: &str, fields: &[(&str, Field)]) {
    let mut keys: Vec<&str> = vec!["_class"];
    keys.extend(fields.iter().map(|(k, _)| *k));
    let mut buf = SlotBuffer::new(keys.len());
    owned_str(buf.slot_mut(0), class);
    for (i, (_, v)) in fields.iter().enumerate() {
        let slot = buf.slot_mut(i as i64 + 1);
        match v {
            Field::Str(s) => owned_str(slot, s),
            Field::Num(n) => number(slot, *n),
        }
    }
    let mut particle = CodeValue::zeroed();
    object_dyn(&mut particle, &keys, &mut buf);
    buf.release_all();
    emit_inbound(&particle);
    release(&mut particle);
}

enum Field {
    Str(String),
    Num(f64),
}

/// The reader thread: bytes in, keys out, until `Stop`. `read` waits at
/// most a tenth of a second (`VTIME`), which is how often it looks at the
/// resize flag and at whether it has been told to go.
fn read_keys(key_class: String, resize_class: String) {
    let mut buf = [0u8; 4096];
    while RUNNING.load(Ordering::SeqCst) {
        if RESIZED.swap(false, Ordering::SeqCst) {
            if let Some((cols, rows)) = size() {
                state().previous = None;
                push(&resize_class, &[("cols", Field::Num(cols as f64)), ("rows", Field::Num(rows as f64))]);
            }
        }
        let n = unsafe { libc::read(0, buf.as_mut_ptr().cast(), buf.len()) };
        if n <= 0 {
            continue;
        }
        for key in keys::decode(&buf[..n as usize]) {
            push(&key_class, &[("name", Field::Str(key.name)), ("text", Field::Str(key.text))]);
        }
    }
}

fn start(out: &mut CodeValue, particle: &CodeValue) {
    if RUNNING.load(Ordering::SeqCst) {
        return exception(out, NAME, "Start: the terminal is already started");
    }
    if !is_terminal() {
        return exception(out, NAME, "Start: stdin and stdout must both be a terminal");
    }
    let key_class = read_field_str(particle, "key").unwrap_or("Key").to_string();
    let resize_class = read_field_str(particle, "resize").unwrap_or("Resize").to_string();
    let Some((cols, rows)) = size() else {
        return exception(out, NAME, "Start: the terminal did not say its size");
    };
    unsafe {
        let mut original = MaybeUninit::<libc::termios>::uninit();
        if libc::tcgetattr(0, original.as_mut_ptr()) != 0 {
            return exception(out, NAME, "Start: cannot read the terminal's settings");
        }
        let original = original.assume_init();
        std::ptr::addr_of_mut!(ORIGINAL).write(MaybeUninit::new(original));
        let mut raw = original;
        raw.c_iflag &= !(libc::BRKINT | libc::ICRNL | libc::INPCK | libc::ISTRIP | libc::IXON);
        raw.c_oflag &= !libc::OPOST;
        raw.c_cflag |= libc::CS8;
        raw.c_lflag &= !(libc::ECHO | libc::ICANON | libc::IEXTEN | libc::ISIG);
        raw.c_cc[libc::VMIN] = 0;
        raw.c_cc[libc::VTIME] = 1;
        install_hooks();
        HAVE_ORIGINAL.store(true, Ordering::SeqCst);
        if libc::tcsetattr(0, libc::TCSAFLUSH, &raw) != 0 {
            restore();
            return exception(out, NAME, "Start: cannot put the terminal in raw mode");
        }
    }
    write_raw(ENTER);
    RUNNING.store(true, Ordering::SeqCst);
    {
        let mut st = state();
        st.previous = None;
        st.reader = Some(thread::spawn(move || read_keys(key_class, resize_class)));
    }
    size_result(out, c"StartResult", cols, rows);
}

fn stop(out: &mut CodeValue) {
    let was = RUNNING.swap(false, Ordering::SeqCst);
    let reader = state().reader.take();
    if let Some(r) = reader {
        let _ = r.join();
    }
    restore();
    state().previous = None;
    let mut buf = SlotBuffer::new(2);
    borrowed_str(buf.slot_mut(0), c"StopResult");
    boolean(buf.slot_mut(1), was);
    object(out, &[c"_class", c"ok"], &mut buf);
    buf.release_all();
}

fn size_result(out: &mut CodeValue, class: &'static std::ffi::CStr, cols: u16, rows: u16) {
    let mut buf = SlotBuffer::new(3);
    borrowed_str(buf.slot_mut(0), class);
    number(buf.slot_mut(1), cols as f64);
    number(buf.slot_mut(2), rows as f64);
    object(out, &[c"_class", c"cols", c"rows"], &mut buf);
    buf.release_all();
}

fn span_of(v: &CodeValue) -> screen::Span {
    if let Some(s) = read_str(v) {
        return screen::Span { text: s.to_string(), style: "plain".to_string(), ..Default::default() };
    }
    screen::Span {
        text: read_field_str(v, "text").unwrap_or("").to_string(),
        style: read_field_str(v, "style").unwrap_or("plain").to_string(),
        width: read_field_number(v, "width").filter(|w| *w >= 0.0).map(|w| w as usize),
        right: read_field_str(v, "align") == Some("right"),
    }
}

/// A row as spans: a list of them, or a bare string.
fn spans_of(row: &CodeValue) -> Vec<screen::Span> {
    if row.tag == CodeTag::Array {
        array_elems(row).map(span_of).collect()
    } else if read_str(row).is_some() {
        vec![span_of(row)]
    } else {
        Vec::new()
    }
}

fn place(v: Option<f64>) -> Option<usize> {
    v.filter(|n| *n >= 0.0).map(|n| n as usize)
}

fn draw(out: &mut CodeValue, particle: &CodeValue) {
    if !RUNNING.load(Ordering::SeqCst) {
        return exception(out, NAME, "Draw: the terminal is not started — send Start first");
    }
    let (cols, height) = size().unwrap_or((80, 24));
    let mut grid = screen::Grid::new(cols as usize, height as usize);
    if let Some(rows) = find_field(particle, "rows").filter(|r| r.tag == CodeTag::Array) {
        for (i, row) in array_elems(rows).enumerate() {
            grid.put(i, 0, &spans_of(row));
        }
    }
    if let Some(overlays) = find_field(particle, "overlays").filter(|r| r.tag == CodeTag::Array) {
        for o in array_elems(overlays) {
            if let (Some(row), Some(col), Some(spans)) =
                (place(read_field_number(o, "row")), place(read_field_number(o, "col")), find_field(o, "spans"))
            {
                grid.put(row, col, &spans_of(spans));
            }
        }
    }
    let next = grid.rows();
    let mut st = state();
    let previous = st.previous.as_ref().filter(|(_, at)| *at == (cols, height)).map(|(rows, _)| rows.as_slice());
    let written = next.iter().enumerate().filter(|(i, r)| previous.and_then(|p| p.get(*i)) != Some(*r)).count();
    let mut bytes = screen::frame(previous, &next);
    match (place(read_field_number(particle, "cursor_row")), place(read_field_number(particle, "cursor_col"))) {
        (Some(r), Some(c)) => bytes.push_str(&format!("\x1b[{};{}H\x1b[?25h", r + 1, c + 1)),
        _ => bytes.push_str("\x1b[?25l"),
    }
    st.previous = Some((next, (cols, height)));
    drop(st);
    let mut stdout = std::io::stdout().lock();
    let _ = stdout.write_all(bytes.as_bytes());
    let _ = stdout.flush();
    let mut buf = SlotBuffer::new(2);
    borrowed_str(buf.slot_mut(0), c"DrawResult");
    number(buf.slot_mut(1), written as f64);
    object(out, &[c"_class", c"rows"], &mut buf);
    buf.release_all();
}

/// Non-zero while started: the reader thread is alive and the program is
/// waiting on keys, so it must not end at its last statement.
#[no_mangle]
pub extern "C" fn code_module_serving() -> std::ffi::c_int {
    i32::from(RUNNING.load(Ordering::SeqCst))
}

declare_inbound!();
declare_inbound_reply!(answered);

/// The program's answer to a key. Nothing waits on it.
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
    let particle = &*particle;
    guarded(&mut *out, NAME, |out| match read_field_str(particle, "_class") {
        Some("Start") => start(out, particle),
        Some("Stop") => stop(out),
        Some("Draw") => draw(out, particle),
        Some("Size") => match size() {
            Some((cols, rows)) => size_result(out, c"SizeResult", cols, rows),
            None => exception(out, NAME, "Size: stdout is not a terminal"),
        },
        _ => null(out),
    })
}
