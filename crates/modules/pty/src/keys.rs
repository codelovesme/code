//! A key, as the `tty` module names it, as the bytes a terminal sends for
//! it — what a shell or a full-screen program in the terminal reads. Pure.
//!
//! A program can ask for more than the old bytes say — ctrl+tab told apart
//! from tab, ctrl+j from enter, ctrl+shift+e from ctrl+e: kitty's keyboard
//! protocol (`ESC [ > flags u`) or xterm's modifyOtherKeys (`ESC [ > 4 ; 2
//! m`). `Modes` is what it asked for; with either, a key the old bytes lose
//! is sent in that form (the `tty` module reads both).

/// What the program in the terminal asked keys to be sent as.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Modes {
    /// kitty's keyboard protocol flags (1: disambiguate), 0 when not asked.
    pub kitty: u32,
    /// xterm's modifyOtherKeys level, 0 when not asked.
    pub modify_other: u16,
}

const SHIFT: u32 = 1;
const ALT: u32 = 2;
const CTRL: u32 = 4;

/// `ctrl+shift+up` → (modifier bits, `up`).
fn split(name: &str) -> (u32, &str) {
    let mut mods = 0;
    let mut rest = name;
    loop {
        if let Some(r) = rest.strip_prefix("ctrl+") {
            mods |= CTRL;
            rest = r;
        } else if let Some(r) = rest.strip_prefix("shift+") {
            mods |= SHIFT;
            rest = r;
        } else if let Some(r) = rest.strip_prefix("alt+") {
            mods |= ALT;
            rest = r;
        } else {
            return (mods, rest);
        }
    }
}

/// The xterm modifier parameter: 1 + bits (shift 1, alt 2, ctrl 4).
fn param(mods: u32) -> u32 {
    1 + mods
}

/// The character a key is, for the extended forms: its code point.
fn code_of(base: &str) -> Option<u32> {
    match base {
        "tab" => Some(9),
        "enter" => Some(13),
        "escape" => Some(27),
        "backspace" => Some(127),
        "space" => Some(32),
        _ => {
            let mut chars = base.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) => Some(c.to_ascii_lowercase() as u32),
                _ => None,
            }
        }
    }
}

/// Whether the old bytes lose `mods` on `base`: a modified tab, enter,
/// backspace or escape; ctrl or alt with a character (alt alone is only
/// lost with shift); ctrl+shift with anything.
fn lost_in_legacy(mods: u32, base: &str) -> bool {
    if mods == 0 {
        return false;
    }
    match base {
        "tab" => mods != SHIFT,
        "enter" | "backspace" | "escape" | "space" => mods & (CTRL | SHIFT) != 0,
        _ => {
            let single = base.chars().count() == 1;
            single && (mods & CTRL != 0 || mods == ALT | SHIFT)
        }
    }
}

/// `name` and `text` (see `tty`'s `Key`) as bytes. `app_cursor` is the
/// terminal's application-cursor mode, in which the arrows say `ESC O`;
/// `modes` what the program asked keys to be sent as (see `Modes`).
/// Nothing for a key a terminal has no bytes for.
pub fn bytes(name: &str, text: &str, app_cursor: bool, modes: Modes) -> Vec<u8> {
    let (mods, base) = split(name);
    if (modes.kitty & 1 != 0 || modes.modify_other >= 2) && lost_in_legacy(mods, base) {
        if let Some(code) = code_of(base) {
            return if modes.kitty & 1 != 0 {
                format!("\x1b[{code};{}u", param(mods)).into_bytes()
            } else {
                format!("\x1b[27;{};{code}~", param(mods)).into_bytes()
            };
        }
    }
    // Typed text, when nothing but shift is held.
    if !text.is_empty() && mods & (CTRL | ALT) == 0 {
        return text.as_bytes().to_vec();
    }
    let letter_final = |f: char| -> Vec<u8> {
        if mods == 0 {
            if app_cursor { format!("\x1bO{f}") } else { format!("\x1b[{f}") }.into_bytes()
        } else {
            format!("\x1b[1;{}{f}", param(mods)).into_bytes()
        }
    };
    let tilde = |n: u32| -> Vec<u8> {
        if mods == 0 { format!("\x1b[{n}~") } else { format!("\x1b[{n};{}~", param(mods)) }.into_bytes()
    };
    let special: Option<Vec<u8>> = match base {
        "enter" => Some(b"\r".to_vec()),
        "tab" if mods & SHIFT != 0 => Some(b"\x1b[Z".to_vec()),
        "tab" => Some(b"\t".to_vec()),
        "backspace" if mods & CTRL != 0 => Some(b"\x08".to_vec()),
        "backspace" => Some(b"\x7f".to_vec()),
        "escape" => Some(b"\x1b".to_vec()),
        "space" if mods & CTRL != 0 => Some(b"\x00".to_vec()),
        "space" => Some(b" ".to_vec()),
        "up" => Some(letter_final('A')),
        "down" => Some(letter_final('B')),
        "right" => Some(letter_final('C')),
        "left" => Some(letter_final('D')),
        "home" => Some(letter_final('H')),
        "end" => Some(letter_final('F')),
        "insert" => Some(tilde(2)),
        "delete" => Some(tilde(3)),
        "pageup" => Some(tilde(5)),
        "pagedown" => Some(tilde(6)),
        "f1" | "f2" | "f3" | "f4" => {
            let f = match base { "f1" => 'P', "f2" => 'Q', "f3" => 'R', _ => 'S' };
            Some(if mods == 0 { format!("\x1bO{f}") } else { format!("\x1b[1;{}{f}", param(mods)) }.into_bytes())
        }
        "f5" => Some(tilde(15)),
        "f6" => Some(tilde(17)),
        "f7" => Some(tilde(18)),
        "f8" => Some(tilde(19)),
        "f9" => Some(tilde(20)),
        "f10" => Some(tilde(21)),
        "f11" => Some(tilde(23)),
        "f12" => Some(tilde(24)),
        _ => None,
    };
    // Alt on a named key is in its sequence already; on a character it is ESC first.
    if let Some(seq) = special {
        let named_with_params = seq.starts_with(b"\x1b[1;") || (seq.starts_with(b"\x1b[") && seq.contains(&b';'));
        if mods & ALT != 0 && !named_with_params {
            let mut out = vec![0x1b];
            out.extend(seq);
            return out;
        }
        return seq;
    }
    let mut chars = base.chars();
    let (Some(c), None) = (chars.next(), chars.next()) else { return Vec::new() };
    let mut out = Vec::new();
    if mods & ALT != 0 {
        out.push(0x1b);
    }
    if mods & CTRL != 0 {
        let b = match c {
            'a'..='z' => c as u8 - b'a' + 1,
            '@' | '2' => 0,
            '[' | '3' => 0x1b,
            '\\' | '4' => 0x1c,
            ']' | '5' => 0x1d,
            '^' | '6' => 0x1e,
            '_' | '/' | '7' => 0x1f,
            '?' | '8' => 0x7f,
            _ => return out_or_char(out, c, mods),
        };
        out.push(b);
        return out;
    }
    out_or_char(out, c, mods)
}

fn out_or_char(mut out: Vec<u8>, c: char, mods: u32) -> Vec<u8> {
    let c = if mods & SHIFT != 0 { c.to_ascii_uppercase() } else { c };
    let mut buf = [0u8; 4];
    out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::{bytes, Modes};

    fn bytes_(name: &str, text: &str, app_cursor: bool) -> Vec<u8> {
        bytes(name, text, app_cursor, Modes::default())
    }

    const KITTY: Modes = Modes { kitty: 1, modify_other: 0 };
    const XTERM: Modes = Modes { kitty: 0, modify_other: 2 };

    #[test]
    fn asked_for_more_keys_come_whole() {
        assert_eq!(bytes("ctrl+tab", "", false, KITTY), b"\x1b[9;5u");
        assert_eq!(bytes("ctrl+shift+tab", "", false, KITTY), b"\x1b[9;6u");
        assert_eq!(bytes("ctrl+j", "", false, KITTY), b"\x1b[106;5u");
        assert_eq!(bytes("ctrl+shift+e", "", false, KITTY), b"\x1b[101;6u");
        assert_eq!(bytes("shift+enter", "", false, KITTY), b"\x1b[13;2u");
        assert_eq!(bytes("ctrl+tab", "", false, XTERM), b"\x1b[27;5;9~");
        // What the old bytes say well stays as it was.
        assert_eq!(bytes("a", "a", false, KITTY), b"a");
        assert_eq!(bytes("shift+tab", "", false, KITTY), b"\x1b[Z");
        assert_eq!(bytes("up", "", false, KITTY), b"\x1b[A");
        assert_eq!(bytes("alt+b", "", false, KITTY), b"\x1bb");
        assert_eq!(bytes("enter", "", false, KITTY), b"\r");
        // Not asked for: the old bytes, even where they lose something.
        assert_eq!(bytes("ctrl+tab", "", false, Modes::default()), b"\t");
    }

    #[test]
    fn typed_text_is_itself() {
        assert_eq!(bytes_("a", "a", false), b"a");
        assert_eq!(bytes_("shift+a", "A", false), b"A");
        assert_eq!(bytes_("∈", "∈", false), "∈".as_bytes());
        assert_eq!(bytes_("space", " ", false), b" ");
    }

    #[test]
    fn control_keys() {
        assert_eq!(bytes_("ctrl+c", "", false), b"\x03");
        assert_eq!(bytes_("ctrl+d", "", false), b"\x04");
        assert_eq!(bytes_("ctrl+shift+e", "", false), b"\x05");
        assert_eq!(bytes_("enter", "", false), b"\r");
        assert_eq!(bytes_("backspace", "", false), b"\x7f");
        assert_eq!(bytes_("tab", "", false), b"\t");
        assert_eq!(bytes_("shift+tab", "", false), b"\x1b[Z");
        assert_eq!(bytes_("escape", "", false), b"\x1b");
    }

    #[test]
    fn arrows_and_their_modes() {
        assert_eq!(bytes_("up", "", false), b"\x1b[A");
        assert_eq!(bytes_("up", "", true), b"\x1bOA");
        assert_eq!(bytes_("ctrl+left", "", false), b"\x1b[1;5D");
        assert_eq!(bytes_("shift+end", "", true), b"\x1b[1;2F");
        assert_eq!(bytes_("delete", "", false), b"\x1b[3~");
        assert_eq!(bytes_("ctrl+pagedown", "", false), b"\x1b[6;5~");
        assert_eq!(bytes_("f1", "", false), b"\x1bOP");
        assert_eq!(bytes_("f12", "", false), b"\x1b[24~");
    }

    #[test]
    fn alt_is_escape_first() {
        assert_eq!(bytes_("alt+b", "", false), b"\x1bb");
        assert_eq!(bytes_("alt+backspace", "", false), b"\x1b\x7f");
        assert_eq!(bytes_("ctrl+alt+x", "", false), b"\x1b\x18");
        assert_eq!(bytes_("alt+left", "", false), b"\x1b[1;3D");
    }

    #[test]
    fn unknown_keys_send_nothing() {
        assert!(bytes_("ctrl+wat", "", false).is_empty());
    }
}
