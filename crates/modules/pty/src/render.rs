//! A terminal's screen as rows of spans — runs of cells that look the same.
//! Pure: a `vt100::Screen` in, plain data out.
//!
//! A colour is `[r, g, b]`, or nothing where the terminal said "default", so
//! the program drawing it can use its own (the `tty` module's `fg` / `bg` on
//! a span take exactly this). Inverse video swaps the two, which needs the
//! defaults to be real colours: `DEFAULT_FG` / `DEFAULT_BG`, the `tty`
//! module's plain style.

pub type Rgb = (u8, u8, u8);

pub const DEFAULT_FG: Rgb = (212, 212, 212);
pub const DEFAULT_BG: Rgb = (30, 30, 30);

/// The 16 standard colours, VS Code's terminal palette.
const ANSI: [Rgb; 16] = [
    (0, 0, 0),
    (205, 49, 49),
    (13, 188, 121),
    (229, 229, 16),
    (36, 114, 200),
    (188, 63, 188),
    (17, 168, 205),
    (229, 229, 229),
    (102, 102, 102),
    (241, 76, 76),
    (35, 209, 139),
    (245, 245, 67),
    (59, 142, 234),
    (214, 112, 214),
    (41, 184, 219),
    (229, 229, 229),
];

/// One of the 256 indexed colours: the 16, the 6×6×6 cube, the 24 greys.
pub fn indexed(i: u8) -> Rgb {
    match i {
        0..=15 => ANSI[i as usize],
        16..=231 => {
            let n = i - 16;
            let level = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            (level(n / 36), level((n / 6) % 6), level(n % 6))
        }
        _ => {
            let g = 8 + (i - 232) * 10;
            (g, g, g)
        }
    }
}

fn colour(c: vt100::Color) -> Option<Rgb> {
    match c {
        vt100::Color::Default => None,
        vt100::Color::Idx(i) => Some(indexed(i)),
        vt100::Color::Rgb(r, g, b) => Some((r, g, b)),
    }
}

/// A run of cells that look the same.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub fg: Option<Rgb>,
    pub bg: Option<Rgb>,
    pub bold: bool,
}

/// Every row of `screen`, as spans. An empty cell is a space; the second
/// half of a wide character is skipped, since the character already said it.
pub fn rows(screen: &vt100::Screen) -> Vec<Vec<Span>> {
    let (height, width) = screen.size();
    (0..height)
        .map(|r| {
            let mut spans: Vec<Span> = Vec::new();
            for c in 0..width {
                let Some(cell) = screen.cell(r, c) else { continue };
                if cell.is_wide_continuation() {
                    continue;
                }
                let mut fg = colour(cell.fgcolor());
                let mut bg = colour(cell.bgcolor());
                if cell.bold() {
                    // Bold on one of the first eight colours means its bright twin.
                    if let vt100::Color::Idx(i @ 0..=7) = cell.fgcolor() {
                        fg = Some(indexed(i + 8));
                    }
                }
                if cell.inverse() {
                    let (f, b) = (fg.unwrap_or(DEFAULT_FG), bg.unwrap_or(DEFAULT_BG));
                    fg = Some(b);
                    bg = Some(f);
                }
                let text = if cell.has_contents() { cell.contents() } else { " " };
                match spans.last_mut() {
                    Some(last) if last.fg == fg && last.bg == bg && last.bold == cell.bold() => last.text.push_str(text),
                    _ => spans.push(Span { text: text.to_string(), fg, bg, bold: cell.bold() }),
                }
            }
            spans
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(bytes: &[u8]) -> vt100::Parser {
        let mut p = vt100::Parser::new(3, 10, 0);
        p.process(bytes);
        p
    }

    #[test]
    fn plain_text_is_one_span_a_row() {
        let p = screen(b"hello");
        let rows = rows(p.screen());
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0], vec![Span { text: "hello     ".into(), fg: None, bg: None, bold: false }]);
    }

    #[test]
    fn colours_split_the_row() {
        let p = screen(b"\x1b[31mred\x1b[0m ok\r\n\x1b[1;32mB\x1b[38;2;1;2;3mT\x1b[0m\r\n\x1b[7mI");
        let rows = rows(p.screen());
        assert_eq!(rows[0][0], Span { text: "red".into(), fg: Some((205, 49, 49)), bg: None, bold: false });
        assert_eq!(rows[0][1].text, " ok    ");
        assert_eq!(rows[0][1].fg, None);
        assert_eq!(rows[1][0], Span { text: "B".into(), fg: Some((35, 209, 139)), bg: None, bold: true });
        assert_eq!(rows[1][1].fg, Some((1, 2, 3)));
        assert_eq!(rows[2][0], Span { text: "I".into(), fg: Some(DEFAULT_BG), bg: Some(DEFAULT_FG), bold: false });
    }

    #[test]
    fn the_palette() {
        assert_eq!(indexed(1), (205, 49, 49));
        assert_eq!(indexed(16), (0, 0, 0));
        assert_eq!(indexed(231), (255, 255, 255));
        assert_eq!(indexed(232), (8, 8, 8));
    }
}
