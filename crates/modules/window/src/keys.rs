//! A key the window system reports, named the way the `tty` module names
//! keys — so a program reads a window exactly as it reads a terminal. Pure.
//!
//! A key is a `name` in VS Code's spelling (`ctrl+s`, `shift+tab`,
//! `ctrl+shift+e`, `alt+f`, `pagedown`, `f10`) and the `text` it types: the
//! character for a printable key pressed alone or with shift, `""` for
//! everything else. Unlike a terminal, a window loses nothing: ctrl+tab is
//! not tab, ctrl+j is not enter, ctrl+1 is not 1.

const SHIFT: u32 = 1;
const ALT: u32 = 2;
const CTRL: u32 = 4;

/// The modifiers held.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Mods {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

impl Mods {
    fn bits(self) -> u32 {
        (if self.shift { SHIFT } else { 0 }) | (if self.alt { ALT } else { 0 }) | (if self.ctrl { CTRL } else { 0 })
    }
}

/// What was pressed: a named key (`tab`, `up`, `f5`, … in the `tty`
/// module's spelling), or a character key — `base` the character with no
/// modifier, `shown` the one the layout gives with them (`!` for shift+1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pressed {
    Named(&'static str),
    Char { base: char, shown: char },
}

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

/// `(name, text)` for a key pressed with `mods`.
pub fn name(pressed: &Pressed, mods: Mods) -> (String, String) {
    let bits = mods.bits();
    match pressed {
        Pressed::Named(n) => (with_mods(bits, n), String::new()),
        Pressed::Char { base, shown } => {
            if *base == ' ' {
                let text = if bits & (CTRL | ALT) == 0 { " " } else { "" };
                return (with_mods(bits, "space"), text.to_string());
            }
            let letter = base.is_alphabetic();
            let lower = base.to_lowercase().next().unwrap_or(*base);
            // Shift on a letter is in the name (`shift+a`) and the text
            // (`A`); on anything else it is already in the character (`!`)
            // — unless ctrl or alt is held too, when the key itself is named.
            let (mut bits, key) = if letter {
                (bits, lower)
            } else if bits & (CTRL | ALT) == 0 {
                (bits & !SHIFT, *shown)
            } else {
                (bits, *base)
            };
            if base.is_uppercase() {
                bits |= SHIFT;
            }
            let text = if bits & (CTRL | ALT) != 0 {
                String::new()
            } else if letter && bits & SHIFT != 0 {
                lower.to_uppercase().collect()
            } else {
                key.to_string()
            };
            (with_mods(bits, &key.to_string()), text)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(ctrl: bool, shift: bool, alt: bool) -> Mods {
        Mods { ctrl, shift, alt }
    }
    fn ch(base: char, shown: char) -> Pressed {
        Pressed::Char { base, shown }
    }
    fn n(p: Pressed, mods: Mods) -> (String, String) {
        name(&p, mods)
    }

    #[test]
    fn as_the_tty_module_names_them() {
        assert_eq!(n(ch('a', 'a'), Mods::default()), ("a".into(), "a".into()));
        assert_eq!(n(ch('a', 'A'), m(false, true, false)), ("shift+a".into(), "A".into()));
        assert_eq!(n(ch('1', '!'), m(false, true, false)), ("!".into(), "!".into()));
        assert_eq!(n(ch('s', 's'), m(true, false, false)), ("ctrl+s".into(), "".into()));
        assert_eq!(n(ch('e', 'E'), m(true, true, false)), ("ctrl+shift+e".into(), "".into()));
        assert_eq!(n(ch('f', 'f'), m(false, false, true)), ("alt+f".into(), "".into()));
        assert_eq!(n(ch(' ', ' '), Mods::default()), ("space".into(), " ".into()));
        assert_eq!(n(ch(' ', ' '), m(true, false, false)), ("ctrl+space".into(), "".into()));
        assert_eq!(n(ch('∈', '∈'), Mods::default()), ("∈".into(), "∈".into()));
    }

    #[test]
    fn what_a_terminal_loses_a_window_keeps() {
        assert_eq!(n(Pressed::Named("tab"), m(true, false, false)).0, "ctrl+tab");
        assert_eq!(n(Pressed::Named("tab"), m(false, true, false)).0, "shift+tab");
        assert_eq!(n(ch('j', 'j'), m(true, false, false)).0, "ctrl+j");
        assert_eq!(n(ch('1', '1'), m(true, false, false)).0, "ctrl+1");
        assert_eq!(n(Pressed::Named("enter"), m(false, true, false)).0, "shift+enter");
        assert_eq!(n(Pressed::Named("pagedown"), m(true, false, false)).0, "ctrl+pagedown");
    }
}
