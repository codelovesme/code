//! Bytes from the terminal, as keys — a pure function, tested without one.
//!
//! A key is a `name` in VS Code's spelling (`ctrl+s`, `shift+tab`,
//! `ctrl+shift+e`, `alt+f`, `pagedown`, `f10`) and the `text` it types:
//! the character for a printable key pressed alone or with shift, `""` for
//! everything else. An editor inserts `text` and looks commands up by
//! `name`, so neither has to be worked out again on the other side.
//!
//! Three encodings arrive: the legacy one every terminal speaks (control
//! bytes, `ESC [ … ~`, `ESC O P`, `ESC x` for alt), and two that tell
//! apart what the legacy one folds together — `ctrl+1` from `1`,
//! `ctrl+shift+e` from `ctrl+e`: xterm's modifyOtherKeys (`ESC [ 27 ; m ;
//! code ~`) and kitty's (`ESC [ code ; m u`). `Start` asks for both; a
//! terminal that knows neither ignores the request and sends legacy.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Key {
    pub name: String,
    pub text: String,
}

const SHIFT: u32 = 1;
const ALT: u32 = 2;
const CTRL: u32 = 4;

fn with_mods(mods: u32, base: &str) -> String {
    let mut name = String::new();
    if mods & CTRL != 0 {
        name.push_str("ctrl+");
    }
    if mods & SHIFT != 0 {
        name.push_str("shift+");
    }
    if mods & ALT != 0 {
        name.push_str("alt+");
    }
    name.push_str(base);
    name
}

fn named(mods: u32, base: &str) -> Key {
    Key { name: with_mods(mods, base), text: String::new() }
}

/// A character pressed with `mods`. Shift on a letter becomes part of the
/// name (`shift+a`) and of the text (`A`); shift on anything else is
/// already in the character (`!`), so it is not said twice.
fn character(mods: u32, c: char) -> Key {
    if c == ' ' {
        let text = if mods & (CTRL | ALT) == 0 { " " } else { "" };
        return Key { name: with_mods(mods, "space"), text: text.to_string() };
    }
    let mut mods = mods;
    let mut base = c;
    if c.is_uppercase() {
        mods |= SHIFT;
        base = c.to_lowercase().next().unwrap_or(c);
    } else if mods & SHIFT != 0 && c.is_alphabetic() {
        // A shifted letter reported as its lowercase code (kitty does this).
    } else if !c.is_alphanumeric() {
        mods &= !SHIFT;
    }
    let text = if mods & (CTRL | ALT) == 0 {
        if mods & SHIFT != 0 {
            base.to_uppercase().collect()
        } else {
            base.to_string()
        }
    } else {
        String::new()
    };
    Key { name: with_mods(mods, &base.to_string()), text }
}

/// A key by its unicode code point, as the two modern encodings send it.
fn by_code(mods: u32, code: u32) -> Option<Key> {
    Some(match code {
        9 => named(mods, "tab"),
        13 => named(mods, "enter"),
        27 => named(mods, "escape"),
        127 | 8 => named(mods, "backspace"),
        c => character(mods, char::from_u32(c)?),
    })
}

/// The modifier parameter both CSI forms carry: `1 + bits`.
fn mods_of(param: Option<u32>) -> u32 {
    param.map(|m| m.saturating_sub(1)).unwrap_or(0) & (SHIFT | ALT | CTRL)
}

fn tilde(mods: u32, n: u32) -> Option<Key> {
    let base = match n {
        1 | 7 => "home",
        2 => "insert",
        3 => "delete",
        4 | 8 => "end",
        5 => "pageup",
        6 => "pagedown",
        11 => "f1",
        12 => "f2",
        13 => "f3",
        14 => "f4",
        15 => "f5",
        17 => "f6",
        18 => "f7",
        19 => "f8",
        20 => "f9",
        21 => "f10",
        23 => "f11",
        24 => "f12",
        _ => return None,
    };
    Some(named(mods, base))
}

fn letter_final(mods: u32, f: u8) -> Option<Key> {
    let base = match f {
        b'A' => "up",
        b'B' => "down",
        b'C' => "right",
        b'D' => "left",
        b'H' => "home",
        b'F' => "end",
        b'P' => "f1",
        b'Q' => "f2",
        b'R' => "f3",
        b'S' => "f4",
        b'Z' => return Some(named(mods | SHIFT, "tab")),
        _ => return None,
    };
    Some(named(mods, base))
}

/// One control byte or printable character, legacy encoding.
fn plain(bytes: &[u8]) -> (Option<Key>, usize) {
    let b = bytes[0];
    match b {
        b'\r' | b'\n' => (Some(named(0, "enter")), 1),
        b'\t' => (Some(named(0, "tab")), 1),
        0x7f => (Some(named(0, "backspace")), 1),
        0x08 => (Some(named(CTRL, "backspace")), 1),
        0x00 => (Some(named(CTRL, "space")), 1),
        1..=26 => (Some(named(CTRL, &((b'a' + b - 1) as char).to_string())), 1),
        0x1c => (Some(named(CTRL, "\\")), 1),
        0x1d => (Some(named(CTRL, "]")), 1),
        0x1e => (Some(named(CTRL, "6")), 1),
        0x1f => (Some(named(CTRL, "/")), 1),
        _ => {
            let len = utf8_len(b);
            if bytes.len() < len {
                return (None, bytes.len());
            }
            match std::str::from_utf8(&bytes[..len]).ok().and_then(|s| s.chars().next()) {
                Some(c) => (Some(character(0, c)), len),
                None => (None, 1),
            }
        }
    }
}

fn utf8_len(b: u8) -> usize {
    match b {
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf7 => 4,
        _ => 1,
    }
}

/// `ESC [ params final` starting at `bytes[0] == ESC`. Answers the key (if
/// it is one) and how many bytes it took.
fn csi(bytes: &[u8]) -> (Option<Key>, usize) {
    let mut i = 2;
    while i < bytes.len() && !(0x40..=0x7e).contains(&bytes[i]) {
        i += 1;
    }
    if i >= bytes.len() {
        // Cut off mid-sequence: nothing to say, and nothing left to read.
        return (None, bytes.len());
    }
    let fin = bytes[i];
    let body = std::str::from_utf8(&bytes[2..i]).unwrap_or("");
    let taken = i + 1;
    // A private marker (`<`, `>`, `?`) is a report or a mouse event, not a key.
    if body.starts_with(['<', '>', '?', '=']) {
        return (None, taken);
    }
    let params: Vec<Option<u32>> = body
        .split(';')
        .map(|p| p.split(':').next().and_then(|n| n.parse().ok()))
        .collect();
    let p = |i: usize| params.get(i).copied().flatten();
    let key = match fin {
        b'~' if p(0) == Some(27) => p(2).and_then(|code| by_code(mods_of(p(1)), code)),
        b'~' if p(0) == Some(200) || p(0) == Some(201) => None,
        b'~' => p(0).and_then(|n| tilde(mods_of(p(1)), n)),
        b'u' => p(0).and_then(|code| by_code(mods_of(p(1)), code)),
        f => letter_final(mods_of(p(1)), f),
    };
    (key, taken)
}

/// Every key in `bytes`, in order. A lone `ESC` at the end is the escape
/// key; the reader hands over whatever one `read` returned, and a terminal
/// writes a whole sequence at once, so a sequence split across two reads
/// is rare enough to cost at most one lost key.
pub fn decode(bytes: &[u8]) -> Vec<Key> {
    let mut keys = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let rest = &bytes[i..];
        let (key, taken) = if rest[0] == 0x1b {
            match rest.get(1) {
                None => (Some(named(0, "escape")), 1),
                Some(b'[') => csi(rest),
                Some(b'O') if rest.len() >= 3 => (letter_final(0, rest[2]), 3),
                Some(0x1b) => (Some(named(0, "escape")), 1),
                Some(_) => {
                    let (inner, n) = plain(&rest[1..]);
                    let key = inner.map(|k| {
                        let name = if let Some(rest) = k.name.strip_prefix("ctrl+") {
                            format!("ctrl+alt+{rest}")
                        } else if let Some(rest) = k.name.strip_prefix("shift+") {
                            format!("shift+alt+{rest}")
                        } else {
                            format!("alt+{}", k.name)
                        };
                        Key { name, text: String::new() }
                    });
                    (key, 1 + n)
                }
            }
        } else {
            plain(rest)
        };
        if let Some(k) = key {
            keys.push(k);
        }
        i += taken.max(1);
    }
    keys
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(bytes: &[u8]) -> Vec<String> {
        decode(bytes).into_iter().map(|k| k.name).collect()
    }

    #[test]
    fn printable_keys_type_themselves() {
        let keys = decode(b"aZ!");
        assert_eq!(keys[0], Key { name: "a".into(), text: "a".into() });
        assert_eq!(keys[1], Key { name: "shift+z".into(), text: "Z".into() });
        assert_eq!(keys[2], Key { name: "!".into(), text: "!".into() });
        assert_eq!(decode(" ".as_bytes())[0], Key { name: "space".into(), text: " ".into() });
        assert_eq!(decode("é∈".as_bytes())[1].text, "∈");
    }

    #[test]
    fn control_bytes_are_ctrl_keys() {
        assert_eq!(names(b"\x13\x11\x02\r\t\x7f"), ["ctrl+s", "ctrl+q", "ctrl+b", "enter", "tab", "backspace"]);
        assert!(decode(b"\x13")[0].text.is_empty());
    }

    #[test]
    fn escape_sequences() {
        assert_eq!(names(b"\x1b[A\x1b[B\x1b[C\x1b[D"), ["up", "down", "right", "left"]);
        assert_eq!(names(b"\x1b[H\x1b[F\x1b[3~\x1b[5~\x1b[6~"), ["home", "end", "delete", "pageup", "pagedown"]);
        assert_eq!(names(b"\x1bOP\x1b[21~\x1b[Z"), ["f1", "f10", "shift+tab"]);
        assert_eq!(names(b"\x1b[1;5C\x1b[1;2A\x1b[1;5H"), ["ctrl+right", "shift+up", "ctrl+home"]);
        assert_eq!(names(b"\x1b"), ["escape"]);
    }

    #[test]
    fn alt_is_escape_first() {
        assert_eq!(names(b"\x1bf\x1bv"), ["alt+f", "alt+v"]);
        assert_eq!(names(b"\x1b\x13"), ["ctrl+alt+s"]);
    }

    #[test]
    fn modern_encodings_tell_ctrl_digits_apart() {
        // modifyOtherKeys: ctrl+1, ctrl+shift+E
        assert_eq!(names(b"\x1b[27;5;49~\x1b[27;6;69~"), ["ctrl+1", "ctrl+shift+e"]);
        // kitty: ctrl+1, ctrl+shift+e, escape
        assert_eq!(names(b"\x1b[49;5u\x1b[101;6u\x1b[27u"), ["ctrl+1", "ctrl+shift+e", "escape"]);
        // kitty: a shifted letter reported by its lowercase code
        assert_eq!(decode(b"\x1b[97;2u")[0], Key { name: "shift+a".into(), text: "A".into() });
    }

    #[test]
    fn reports_and_pastes_are_not_keys() {
        assert!(decode(b"\x1b[?1u\x1b[200~\x1b[201~").is_empty());
    }
}
