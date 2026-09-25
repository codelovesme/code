//! The `pty` native module — a shell in a pseudo-terminal. Machine only.
//!
//! What an editor's integrated terminal is made of: a program (a shell, by
//! default) running on a pseudo-terminal, a terminal emulator keeping its
//! screen (`vt100`), and the screen handed out as rows of coloured spans —
//! exactly what the `tty` module's `Draw` takes.
//!
//! Handlers:
//!
//! - `Open { cols?, rows?, command?, args?, cwd?, env?, scrollback? }` →
//!   `TerminalOpened { id }`. `command` defaults to `$SHELL`, else
//!   `/bin/sh`; the size to 80 × 24; `scrollback` (rows kept above the
//!   screen) to 1000. `TERM` is `xterm-256color`.
//! - `Type { id, name, text }` → `Typed { id, bytes }`: a key as the `tty`
//!   module names it (`enter`, `ctrl+c`, `up`, `alt+b`, …) sent as the bytes
//!   a terminal sends for it — or, when the program asked for kitty's keys
//!   or xterm's modifyOtherKeys, whole (ctrl+tab told from tab) — see
//!   `keys.rs`.
//! - `Paste { id, text }` → `Pasted { id, bytes }`: text as a paste, in the
//!   brackets a program asked for (bracketed paste).
//! - `Mouse { id, kind, button?, row, col, mods? }` → `MouseSent { id, sent }`:
//!   `kind` press, release, drag, move or wheel, sent the way the program
//!   asked for mouse reports (`sent = false`: it asked for none of this).
//! - `Write { id, data }` → `Written { id, bytes }`: text sent as it is.
//! - `Resize { id, cols, rows }` → `Resized { id }`. The program is told.
//! - `Screen { id, scroll? }` → `TerminalScreen { id, lines, cursor_row,
//!   cursor_col, cursor_visible, alive, code, scrolled, title, alternate,
//!   mouse, keys }`: `scroll` rows up into the scrollback (`scrolled`, what
//!   there was room for); the title the program set; whether it is on the
//!   alternate screen; the mouse reports (`none`, `press`, `press_release`,
//!   `button_motion`, `any_motion`) and keys (`legacy`, `kitty`, `xterm`) it
//!   asked for. `lines` has one entry per row, each a
//!   list of spans `{ text, fg?, bg?, bold? }` (`fg` / `bg` as `[r, g, b]`,
//!   absent for the terminal's default). `code` is the exit code once the
//!   program has ended.
//! - `Close { id }` → `Closed { id }`: the program is hung up on.
//!
//! Pushed, from the terminal's own thread:
//!
//! - `TerminalOutput { id }` — the screen changed. At most one waits at a
//!   time: the next comes only after a `Screen` has read it, so a flood of
//!   output is one redraw, not thousands.
//! - `TerminalExited { id, code }` — the program ended.
//!
//! The program stays up while any terminal's thread is running.

mod keys;
mod render;

use code_native::*;
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write as _};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const NAME: &str = "pty";

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
/// Threads of this module still running — readers and hang-up watchers. A
/// host must not unmap code a thread is in, so this is what keeps it serving.
static THREADS: AtomicUsize = AtomicUsize::new(0);

/// What the program asks of the terminal beyond its screen, caught as the
/// emulator reads its output: its window title, the key forms it wants
/// (see `keys::Modes`), and the answers it waits for, to be written back.
#[derive(Default)]
struct Hooks {
    title: String,
    kitty: Vec<u32>,
    modify_other: u16,
    replies: Vec<u8>,
}

impl Hooks {
    fn modes(&self) -> keys::Modes {
        keys::Modes { kitty: self.kitty.last().copied().unwrap_or(0), modify_other: self.modify_other }
    }
}

impl vt100::Callbacks for Hooks {
    fn set_window_title(&mut self, _: &mut vt100::Screen, title: &[u8]) {
        self.title = String::from_utf8_lossy(title).into_owned();
    }

    fn unhandled_csi(&mut self, _: &mut vt100::Screen, i1: Option<u8>, _i2: Option<u8>, params: &[&[u16]], c: char) {
        let first = params.first().and_then(|p| p.first()).copied();
        match (i1, c) {
            // kitty: push flags, pop n, set, query.
            (Some(b'>'), 'u') => self.kitty.push(u32::from(first.unwrap_or(0))),
            (Some(b'<'), 'u') => {
                for _ in 0..first.unwrap_or(1).max(1) {
                    self.kitty.pop();
                }
            }
            (Some(b'='), 'u') => {
                let flags = u32::from(first.unwrap_or(0));
                match self.kitty.last_mut() {
                    Some(top) => *top = flags,
                    None => self.kitty.push(flags),
                }
            }
            (Some(b'?'), 'u') => {
                let flags = self.kitty.last().copied().unwrap_or(0);
                self.replies.extend(format!("\x1b[?{flags}u").into_bytes());
            }
            // xterm: modifyOtherKeys (`ESC [ > 4 ; n m`, or `ESC [ > 4 m` off).
            (Some(b'>'), 'm') if first == Some(4) => {
                self.modify_other = params.get(1).and_then(|p| p.first()).copied().unwrap_or(0);
            }
            _ => {}
        }
    }
}

type Emulator = vt100::Parser<Hooks>;

struct Terminal {
    master: File,
    parser: Arc<Mutex<Emulator>>,
    pid: i32,
    alive: Arc<AtomicBool>,
    pending: Arc<AtomicBool>,
    code: Arc<Mutex<Option<i32>>>,
}

static TERMINALS: Mutex<Option<HashMap<u64, Terminal>>> = Mutex::new(None);

fn with_terminals<R>(f: impl FnOnce(&mut HashMap<u64, Terminal>) -> R) -> R {
    let mut guard = TERMINALS.lock().unwrap_or_else(|e| e.into_inner());
    f(guard.get_or_insert_with(HashMap::new))
}

/// A value to hand back, built without the slot bookkeeping every time.
enum Val {
    Str(String),
    Num(f64),
    Bool(bool),
    Null,
    Arr(Vec<Val>),
    Obj(Vec<(String, Val)>),
}

fn build(v: &Val) -> CodeValue {
    let mut out = CodeValue::zeroed();
    match v {
        Val::Str(s) => owned_str(&mut out, s),
        Val::Num(n) => number(&mut out, *n),
        Val::Bool(b) => boolean(&mut out, *b),
        Val::Null => null(&mut out),
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

fn rgb(c: Option<render::Rgb>) -> Option<Val> {
    c.map(|(r, g, b)| Val::Arr(vec![Val::Num(r as f64), Val::Num(g as f64), Val::Num(b as f64)]))
}

fn winsize(cols: u16, rows: u16) -> libc::winsize {
    libc::winsize { ws_row: rows, ws_col: cols, ws_xpixel: 0, ws_ypixel: 0 }
}

fn size_of(particle: &CodeValue, field: &str, default: u16) -> u16 {
    read_field_number(particle, field).filter(|n| *n >= 1.0 && *n <= 1000.0).map(|n| n as u16).unwrap_or(default)
}

fn open(out: &mut CodeValue, p: &CodeValue) {
    let cols = size_of(p, "cols", 80);
    let rows = size_of(p, "rows", 24);
    let command = read_field_str(p, "command")
        .map(str::to_string)
        .unwrap_or_else(|| std::env::var("SHELL").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "/bin/sh".into()));
    let args: Vec<String> = find_field(p, "args")
        .filter(|a| a.tag == CodeTag::Array)
        .map(|a| array_elems(a).filter_map(read_str).map(str::to_string).collect())
        .unwrap_or_default();

    let (mut master_fd, mut slave_fd) = (-1, -1);
    let ws = winsize(cols, rows);
    let opened = unsafe { libc::openpty(&mut master_fd, &mut slave_fd, std::ptr::null_mut(), std::ptr::null(), &ws) };
    if opened != 0 {
        return exception(out, NAME, "Open: no pseudo-terminal could be made");
    }
    // The program must not hold the master: it would never see its hang-up.
    unsafe { libc::fcntl(master_fd, libc::F_SETFD, libc::FD_CLOEXEC) };
    let master = unsafe { File::from_raw_fd(master_fd) };
    let slave = unsafe { File::from_raw_fd(slave_fd) };
    let stdio = |f: &File| f.try_clone().map(Stdio::from);
    let (Ok(i), Ok(o), Ok(e)) = (stdio(&slave), stdio(&slave), stdio(&slave)) else {
        return exception(out, NAME, "Open: the pseudo-terminal could not be shared");
    };
    let mut cmd = Command::new(&command);
    cmd.args(&args).stdin(i).stdout(o).stderr(e).env("TERM", "xterm-256color");
    if let Some(cwd) = read_field_str(p, "cwd").filter(|c| !c.is_empty()) {
        cmd.current_dir(cwd);
    }
    if let Some(env) = find_field(p, "env").filter(|e| e.tag == CodeTag::Object) {
        for (k, v) in object_entries(env) {
            if k != "_class" {
                if let Some(v) = read_str(v) {
                    cmd.env(k, v);
                }
            }
        }
    }
    // Its own session, with the pseudo-terminal as its controlling one — so
    // ctrl+c reaches it as a signal, and job control works.
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            libc::ioctl(0, libc::TIOCSCTTY, 0);
            Ok(())
        });
    }
    let child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return exception(out, NAME, &format!("Open: cannot start '{command}': {e}")),
    };
    drop(slave);
    let pid = child.id() as i32;
    // Waited on below with waitpid, by pid: the handle has done its job.
    std::mem::forget(child);

    let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
    let scrollback = read_field_number(p, "scrollback").filter(|n| *n >= 0.0 && *n <= 100_000.0).map(|n| n as usize).unwrap_or(1000);
    let parser = Arc::new(Mutex::new(vt100::Parser::new_with_callbacks(rows, cols, scrollback, Hooks::default())));
    let alive = Arc::new(AtomicBool::new(true));
    let pending = Arc::new(AtomicBool::new(false));
    let code = Arc::new(Mutex::new(None));
    let (Ok(mut reading), Ok(mut answering)) = (master.try_clone(), master.try_clone()) else {
        return exception(out, NAME, "Open: the pseudo-terminal could not be read");
    };
    {
        let (parser, alive, pending, code) = (parser.clone(), alive.clone(), pending.clone(), code.clone());
        THREADS.fetch_add(1, Ordering::SeqCst);
        thread::spawn(move || {
            let mut buf = [0u8; 16384];
            loop {
                match reading.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let replies = {
                            let mut emulator = parser.lock().unwrap_or_else(|e| e.into_inner());
                            emulator.process(&buf[..n]);
                            std::mem::take(&mut emulator.callbacks_mut().replies)
                        };
                        // What the program asked and waits on (kitty's `ESC [ ? u`).
                        if !replies.is_empty() {
                            let _ = answering.write_all(&replies);
                        }
                        if !pending.swap(true, Ordering::SeqCst) {
                            push(particle("TerminalOutput", vec![("id", Val::Num(id as f64))]));
                        }
                    }
                }
            }
            let mut status = 0;
            let exit = if unsafe { libc::waitpid(pid, &mut status, 0) } == pid {
                if libc::WIFEXITED(status) { libc::WEXITSTATUS(status) } else { 128 + libc::WTERMSIG(status) }
            } else {
                -1
            };
            *code.lock().unwrap_or_else(|e| e.into_inner()) = Some(exit);
            alive.store(false, Ordering::SeqCst);
            push(particle("TerminalExited", vec![("id", Val::Num(id as f64)), ("code", Val::Num(exit as f64))]));
            THREADS.fetch_sub(1, Ordering::SeqCst);
        });
    }
    with_terminals(|t| t.insert(id, Terminal { master, parser, pid, alive, pending, code }));
    answer(out, particle("TerminalOpened", vec![("id", Val::Num(id as f64))]));
}

fn id_of(p: &CodeValue) -> Option<u64> {
    read_field_number(p, "id").filter(|n| *n >= 1.0).map(|n| n as u64)
}

/// Runs `f` on terminal `id`, or answers that there is none.
fn on(out: &mut CodeValue, p: &CodeValue, what: &str, f: impl FnOnce(&mut CodeValue, u64, &mut Terminal)) {
    let Some(id) = id_of(p) else {
        return exception(out, NAME, &format!("{what} needs `id`, a terminal's number"));
    };
    with_terminals(|t| match t.get_mut(&id) {
        Some(term) => f(out, id, term),
        None => exception(out, NAME, &format!("{what}: no terminal {id}")),
    })
}

fn send(out: &mut CodeValue, id: u64, term: &mut Terminal, bytes: &[u8], class: &str) {
    if !bytes.is_empty() && term.master.write_all(bytes).is_err() {
        return exception(out, NAME, &format!("terminal {id} has ended"));
    }
    answer(out, particle(class, vec![("id", Val::Num(id as f64)), ("bytes", Val::Num(bytes.len() as f64))]));
}

fn type_key(out: &mut CodeValue, p: &CodeValue) {
    let name = read_field_str(p, "name").unwrap_or("").to_string();
    let text = read_field_str(p, "text").unwrap_or("").to_string();
    on(out, p, "Type", |out, id, term| {
        let (app_cursor, modes) = {
            let emulator = term.parser.lock().unwrap_or_else(|e| e.into_inner());
            (emulator.screen().application_cursor(), emulator.callbacks().modes())
        };
        let bytes = keys::bytes(&name, &text, app_cursor, modes);
        send(out, id, term, &bytes, "Typed");
    });
}

fn write(out: &mut CodeValue, p: &CodeValue) {
    let data = read_field_str(p, "data").unwrap_or("").as_bytes().to_vec();
    on(out, p, "Write", |out, id, term| send(out, id, term, &data, "Written"));
}

fn resize(out: &mut CodeValue, p: &CodeValue) {
    let cols = size_of(p, "cols", 80);
    let rows = size_of(p, "rows", 24);
    on(out, p, "Resize", |out, id, term| {
        let ws = winsize(cols, rows);
        unsafe { libc::ioctl(term.master.as_raw_fd(), libc::TIOCSWINSZ, &ws) };
        term.parser.lock().unwrap_or_else(|e| e.into_inner()).screen_mut().set_size(rows, cols);
        answer(out, particle("Resized", vec![("id", Val::Num(id as f64))]));
    });
}

fn screen(out: &mut CodeValue, p: &CodeValue) {
    let scroll = read_field_number(p, "scroll").filter(|n| *n >= 0.0).map(|n| n as usize).unwrap_or(0);
    on(out, p, "Screen", |out, id, term| {
        term.pending.store(false, Ordering::SeqCst);
        let mut parser = term.parser.lock().unwrap_or_else(|e| e.into_inner());
        // `scroll` rows up into the scrollback (clamped to what there is),
        // read, and back to the live screen.
        parser.screen_mut().set_scrollback(scroll);
        let scrolled = parser.screen().scrollback();
        let title = parser.callbacks().title.clone();
        let modes = parser.callbacks().modes();
        let s = parser.screen();
        let lines = render::rows(s)
            .into_iter()
            .map(|row| {
                Val::Arr(
                    row.into_iter()
                        .map(|span| {
                            let mut f = vec![("text".to_string(), Val::Str(span.text))];
                            if let Some(c) = rgb(span.fg) {
                                f.push(("fg".into(), c));
                            }
                            if let Some(c) = rgb(span.bg) {
                                f.push(("bg".into(), c));
                            }
                            if span.bold {
                                f.push(("bold".into(), Val::Bool(true)));
                            }
                            Val::Obj(f)
                        })
                        .collect(),
                )
            })
            .collect();
        let (row, col) = s.cursor_position();
        let code = *term.code.lock().unwrap_or_else(|e| e.into_inner());
        answer(
            out,
            particle(
                "TerminalScreen",
                vec![
                    ("id", Val::Num(id as f64)),
                    ("lines", Val::Arr(lines)),
                    ("cursor_row", Val::Num(row as f64)),
                    ("cursor_col", Val::Num(col as f64)),
                    ("cursor_visible", Val::Bool(!s.hide_cursor() && scrolled == 0)),
                    ("scrolled", Val::Num(scrolled as f64)),
                    ("title", Val::Str(title)),
                    ("alternate", Val::Bool(s.alternate_screen())),
                    ("mouse", Val::Str(mouse_mode(s.mouse_protocol_mode()).into())),
                    ("keys", Val::Str(if modes.kitty & 1 != 0 { "kitty" } else if modes.modify_other >= 2 { "xterm" } else { "legacy" }.into())),
                    ("alive", Val::Bool(term.alive.load(Ordering::SeqCst))),
                    ("code", code.map(|c| Val::Num(c as f64)).unwrap_or(Val::Null)),
                ],
            ),
        );
        drop(parser);
        term.parser.lock().unwrap_or_else(|e| e.into_inner()).screen_mut().set_scrollback(0);
    });
}

fn mouse_mode(m: vt100::MouseProtocolMode) -> &'static str {
    match m {
        vt100::MouseProtocolMode::None => "none",
        vt100::MouseProtocolMode::Press => "press",
        vt100::MouseProtocolMode::PressRelease => "press_release",
        vt100::MouseProtocolMode::ButtonMotion => "button_motion",
        vt100::MouseProtocolMode::AnyMotion => "any_motion",
    }
}

/// `Paste { id, text }`: `text` written as a paste — between the brackets
/// a program that asked for bracketed paste reads it by, so an editor does
/// not take a pasted newline for Enter.
fn paste(out: &mut CodeValue, p: &CodeValue) {
    let text = read_field_str(p, "text").unwrap_or("").to_string();
    on(out, p, "Paste", |out, id, term| {
        let bracketed = term.parser.lock().unwrap_or_else(|e| e.into_inner()).screen().bracketed_paste();
        let mut data = Vec::new();
        if bracketed {
            data.extend_from_slice(b"\x1b[200~");
        }
        data.extend_from_slice(text.as_bytes());
        if bracketed {
            data.extend_from_slice(b"\x1b[201~");
        }
        send(out, id, term, &data, "Pasted");
    });
}

/// `Mouse { id, kind, button?, row, col, mods? }` — `kind` press, release,
/// move or wheel (`button` 0 left, 1 middle, 2 right; for a wheel 0 up, 1
/// down), `row` / `col` from 0 — sent to the program the way it asked for
/// mouse reports; `Mouse { sent = false }` when it asked for none of this.
fn mouse(out: &mut CodeValue, p: &CodeValue) {
    let kind = read_field_str(p, "kind").unwrap_or("press").to_string();
    let button = read_field_number(p, "button").unwrap_or(0.0).max(0.0) as u32;
    let row = read_field_number(p, "row").unwrap_or(0.0).max(0.0) as u32;
    let col = read_field_number(p, "col").unwrap_or(0.0).max(0.0) as u32;
    let mods = read_field_str(p, "mods").unwrap_or("").to_string();
    on(out, p, "Mouse", |out, id, term| {
        let (mode, encoding) = {
            let emulator = term.parser.lock().unwrap_or_else(|e| e.into_inner());
            (emulator.screen().mouse_protocol_mode(), emulator.screen().mouse_protocol_encoding())
        };
        use vt100::MouseProtocolMode as M;
        let wanted = match kind.as_str() {
            "press" | "wheel" => mode != M::None,
            "release" => matches!(mode, M::PressRelease | M::ButtonMotion | M::AnyMotion),
            "drag" => matches!(mode, M::ButtonMotion | M::AnyMotion),
            "move" => mode == M::AnyMotion,
            _ => false,
        };
        if !wanted {
            return answer(out, particle("MouseSent", vec![("id", Val::Num(id as f64)), ("sent", Val::Bool(false))]));
        }
        let mut code = match kind.as_str() {
            "wheel" => 64 + button.min(1),
            "drag" | "move" => 32 + button.min(2),
            _ => button.min(2),
        };
        if mods.contains("shift") {
            code += 4;
        }
        if mods.contains("alt") {
            code += 8;
        }
        if mods.contains("ctrl") {
            code += 16;
        }
        let bytes = if encoding == vt100::MouseProtocolEncoding::Sgr {
            let end = if kind == "release" { 'm' } else { 'M' };
            format!("\x1b[<{code};{};{}{end}", col + 1, row + 1).into_bytes()
        } else {
            // The old form: one byte each, 32 on; a release is button 3.
            let code = if kind == "release" { 3 + (code & !3) } else { code };
            let at = |n: u32| (n + 1 + 32).min(255) as u8;
            vec![0x1b, b'[', b'M', (code + 32).min(255) as u8, at(col), at(row)]
        };
        let _ = term.master.write_all(&bytes);
        answer(out, particle("MouseSent", vec![("id", Val::Num(id as f64)), ("sent", Val::Bool(true))]));
    });
}

fn close(out: &mut CodeValue, p: &CodeValue) {
    let Some(id) = id_of(p) else {
        return exception(out, NAME, "Close needs `id`, a terminal's number");
    };
    let Some(term) = with_terminals(|t| t.remove(&id)) else {
        return exception(out, NAME, &format!("Close: no terminal {id}"));
    };
    // Hung up on, the way closing a terminal window does; one that will not
    // go is killed a moment later. Its reader ends when it has.
    unsafe { libc::kill(-term.pid, libc::SIGHUP) };
    unsafe { libc::kill(term.pid, libc::SIGHUP) };
    let (pid, alive) = (term.pid, term.alive.clone());
    THREADS.fetch_add(1, Ordering::SeqCst);
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(500));
        if alive.load(Ordering::SeqCst) {
            unsafe { libc::kill(pid, libc::SIGKILL) };
        }
        THREADS.fetch_sub(1, Ordering::SeqCst);
    });
    drop(term);
    answer(out, particle("Closed", vec![("id", Val::Num(id as f64))]));
}

/// Non-zero while a thread of this module runs: a terminal is open, or one
/// just closed is still being hung up on.
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
        Some("Type") => type_key(out, p),
        Some("Write") => write(out, p),
        Some("Resize") => resize(out, p),
        Some("Screen") => screen(out, p),
        Some("Close") => close(out, p),
        Some("Paste") => paste(out, p),
        Some("Mouse") => mouse(out, p),
        _ => null(out),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real pseudo-terminal: a program that prints in colour and ends.
    #[test]
    fn a_program_on_a_pseudo_terminal() {
        let mut open_out = CodeValue::zeroed();
        let req = build(&particle(
            "Open",
            vec![
                ("cols", Val::Num(20.0)),
                ("rows", Val::Num(3.0)),
                ("command", Val::Str("/bin/sh".into())),
                ("args", Val::Arr(vec![Val::Str("-c".into()), Val::Str("printf '\\033[31mred\\033[0m ok'; sleep 0.2".into())])),
            ],
        ));
        open(&mut open_out, &req);
        let id = read_field_number(&open_out, "id").expect("an id") as u64;
        let mut text = String::new();
        let mut fg = None;
        for _ in 0..100 {
            thread::sleep(Duration::from_millis(20));
            with_terminals(|t| {
                let term = t.get(&id).unwrap();
                let rows = render::rows(term.parser.lock().unwrap().screen());
                text = rows[0].iter().map(|s| s.text.as_str()).collect();
                fg = rows[0][0].fg;
            });
            if text.starts_with("red ok") {
                break;
            }
        }
        assert!(text.starts_with("red ok"), "{text:?}");
        assert_eq!(fg, Some((205, 49, 49)));
        for _ in 0..100 {
            thread::sleep(Duration::from_millis(20));
            if with_terminals(|t| !t.get(&id).unwrap().alive.load(Ordering::SeqCst)) {
                break;
            }
        }
        assert_eq!(with_terminals(|t| *t.get(&id).unwrap().code.lock().unwrap()), Some(0));
    }

    /// Opens `script` on a `cols` × `rows` terminal; its id.
    fn run(script: &str, cols: f64, rows: f64) -> u64 {
        let mut opened = CodeValue::zeroed();
        let req = build(&particle(
            "Open",
            vec![
                ("cols", Val::Num(cols)),
                ("rows", Val::Num(rows)),
                ("command", Val::Str("/bin/sh".into())),
                ("args", Val::Arr(vec![Val::Str("-c".into()), Val::Str(script.into())])),
            ],
        ));
        open(&mut opened, &req);
        read_field_number(&opened, "id").expect("an id") as u64
    }

    /// Waits (up to 4 s) for `check` on the terminal to hold.
    fn wait(id: u64, check: impl Fn(&Terminal) -> bool) -> bool {
        for _ in 0..200 {
            if with_terminals(|t| check(t.get(&id).unwrap())) {
                return true;
            }
            thread::sleep(Duration::from_millis(20));
        }
        false
    }

    fn text(term: &Terminal) -> String {
        term.parser.lock().unwrap().screen().contents()
    }

    /// A program that asks for kitty's keys gets ctrl+tab whole.
    #[test]
    fn keys_as_the_program_asked() {
        let id = run("stty raw -echo; printf '\\033[>1u'; head -c 6 | od -An -c; sleep 1", 40.0, 4.0);
        assert!(wait(id, |t| t.parser.lock().unwrap().callbacks().modes().kitty == 1), "the program's ask was seen");
        let mut typed = CodeValue::zeroed();
        type_key(&mut typed, &build(&particle("Type", vec![("id", Val::Num(id as f64)), ("name", Val::Str("ctrl+tab".into())), ("text", Val::Str(String::new()))])));
        assert_eq!(read_field_number(&typed, "bytes"), Some(6.0));
        assert!(wait(id, |t| text(t).contains("[   9   ;   5   u")), "it read ESC [ 9 ; 5 u");
    }

    /// The title a program sets; the scrollback it scrolled off.
    #[test]
    fn title_and_scrollback() {
        let id = run("printf '\\033]2;my title\\007'; seq 1 40; sleep 1", 20.0, 5.0);
        assert!(wait(id, |t| text(t).contains("40")));
        assert!(wait(id, |t| t.parser.lock().unwrap().callbacks().title == "my title"));
        let mut shown = CodeValue::zeroed();
        screen(&mut shown, &build(&particle("Screen", vec![("id", Val::Num(id as f64)), ("scroll", Val::Num(10.0))])));
        assert_eq!(read_field_number(&shown, "scrolled"), Some(10.0));
        assert_eq!(read_field_str(&shown, "title"), Some("my title"));
        let first = find_field(&shown, "lines").and_then(|l| array_elems(l).next().map(|row| array_elems(row).filter_map(|s| find_field(s, "text").and_then(read_str)).collect::<String>()));
        assert_eq!(first.as_deref().map(str::trim), Some("27"));
        // And the live screen again after.
        assert!(wait(id, |t| t.parser.lock().unwrap().screen().scrollback() == 0));
    }
}