//! Rows of styled spans laid into a grid of cells — pure, tested without a
//! window. The same spans, style names and rules as the `tty` module's
//! screen (copied from it, so a screen drawn for a terminal looks the same
//! in a window): a program names a style (`keyword`, `selected`, `status`)
//! or gives its own `fg` / `bg` / `bold`; a span may have a `width` it is
//! padded or cut to, left- or right-aligned. Rows go at column 0 first,
//! then overlays on top, in order.

/// (foreground, background, bold) for a style name. Colours are 24-bit,
/// VS Code's dark theme.
fn style(name: &str) -> ((u8, u8, u8), (u8, u8, u8), bool) {
    const EDITOR: (u8, u8, u8) = (30, 30, 30);
    const SIDEBAR: (u8, u8, u8) = (37, 37, 38);
    const BAR: (u8, u8, u8) = (60, 60, 60);
    match name {
        "menu" => ((204, 204, 204), BAR, false),
        "menu_active" => ((255, 255, 255), (80, 80, 80), false),
        "menu_item" => ((204, 204, 204), SIDEBAR, false),
        "menu_item_active" => ((255, 255, 255), (4, 57, 94), false),
        "menu_key" => ((140, 140, 140), SIDEBAR, false),
        "menu_key_active" => ((200, 200, 200), (4, 57, 94), false),
        "title" => ((150, 150, 150), BAR, false),
        "pane_title" => ((150, 150, 150), SIDEBAR, false),
        "pane_title_focus" => ((255, 255, 255), SIDEBAR, true),
        "sidebar" => ((204, 204, 204), SIDEBAR, false),
        "selected" => ((255, 255, 255), (55, 55, 61), false),
        "selected_focus" => ((255, 255, 255), (4, 57, 94), false),
        "folder" => ((204, 204, 204), SIDEBAR, true),
        "dim" => ((110, 110, 110), EDITOR, false),
        "line_number" => ((110, 118, 129), EDITOR, false),
        "line_number_active" => ((198, 198, 198), EDITOR, false),
        "status" => ((255, 255, 255), (0, 122, 204), false),
        "status_warn" => ((255, 255, 255), (204, 102, 0), false),
        "prompt" => ((255, 255, 255), (37, 37, 38), false),
        "border" => ((68, 68, 68), EDITOR, false),
        "comment" => ((106, 153, 85), EDITOR, false),
        "string" => ((206, 145, 120), EDITOR, false),
        "number" => ((181, 206, 168), EDITOR, false),
        "keyword" => ((197, 134, 192), EDITOR, false),
        "class" => ((78, 201, 176), EDITOR, false),
        "property" => ((220, 220, 170), EDITOR, false),
        "variable" => ((156, 220, 254), EDITOR, false),
        "operator" => ((212, 212, 212), EDITOR, false),
        _ => ((212, 212, 212), EDITOR, false),
    }
}

pub type Rgb = (u8, u8, u8);

/// What a cell looks like: its colours and weight, worked out when it is
/// written — from its style name, and whatever colours the span gave on top.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Look {
    pub fg: Rgb,
    pub bg: Rgb,
    pub bold: bool,
}

impl Look {
    fn of(span: &Span) -> Look {
        let (fg, bg, bold) = style(&span.style);
        Look { fg: span.fg.unwrap_or(fg), bg: span.bg.unwrap_or(bg), bold: span.bold.unwrap_or(bold) }
    }
}

/// What a program may put on the screen: a tab is a space (width is
/// counted in characters, and a tab is not one), a control character is
/// dropped — it would move the cursor or change the terminal's state.
fn printable(c: char) -> Option<char> {
    match c {
        '\t' => Some(' '),
        c if c.is_control() => None,
        c => Some(c),
    }
}

/// A span as a program writes it: text in a style, and optionally a
/// `width` it is padded or cut to — left-aligned, or right-aligned when
/// `right`. The width is what lets a program lay text out in columns
/// without counting characters itself. `fg`, `bg` and `bold`, when given,
/// win over the style's — which is how a terminal's own colours get shown.
#[derive(Debug, Clone, Default)]
pub struct Span {
    pub text: String,
    pub style: String,
    pub width: Option<usize>,
    pub right: bool,
    pub fg: Option<Rgb>,
    pub bg: Option<Rgb>,
    pub bold: Option<bool>,
}

/// One cell: a character and how it looks.
pub type Cell = (char, Look);

/// The screen as cells, `cols` × `rows`.
#[derive(Clone)]
pub struct Grid {
    pub cols: usize,
    pub rows: usize,
    pub cells: Vec<Vec<Cell>>,
}

/// How a blank cell looks: `plain`.
pub fn blank() -> Look {
    Look::of(&Span { style: "plain".to_string(), ..Span::default() })
}

impl Grid {
    pub fn new(cols: usize, rows: usize) -> Self {
        Grid { cols, rows, cells: vec![vec![(' ', blank()); cols]; rows] }
    }

    /// Writes `spans` from (`row`, `col`) rightward, clipped at the edge.
    /// A span without a width writes its text; with one, exactly that many
    /// cells, padded with spaces in the span's style.
    pub fn put(&mut self, row: usize, col: usize, spans: &[Span]) {
        let cols = self.cols;
        let Some(line) = self.cells.get_mut(row) else { return };
        let mut at = col;
        for span in spans {
            let look = Look::of(span);
            let chars: Vec<char> = span.text.chars().filter_map(printable).collect();
            let run: Vec<char> = match span.width {
                None => chars,
                Some(w) if chars.len() >= w => {
                    if span.right {
                        chars[chars.len() - w..].to_vec()
                    } else {
                        chars[..w].to_vec()
                    }
                }
                Some(w) => {
                    let pad = std::iter::repeat_n(' ', w - chars.len());
                    if span.right {
                        pad.chain(chars).collect()
                    } else {
                        chars.into_iter().chain(pad).collect()
                    }
                }
            };
            for c in run {
                if at >= cols {
                    return;
                }
                line[at] = (c, look);
                at += 1;
            }
        }
    }

    /// Row `r` as plain text (for tests, and a program that asks).
    pub fn text(&self, r: usize) -> String {
        self.cells.get(r).map(|line| line.iter().map(|(c, _)| *c).collect()).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(t: &str, s: &str) -> Span {
        Span { text: t.to_string(), style: s.to_string(), ..Span::default() }
    }

    #[test]
    fn rows_widths_and_overlays() {
        let mut g = Grid::new(8, 2);
        g.put(0, 0, &[Span { width: Some(4), ..span("ab", "plain") }, span("|", "plain")]);
        g.put(1, 0, &[span("aaaaaaaa", "plain")]);
        g.put(1, 2, &[span("XY", "menu")]);
        g.put(1, 7, &[span("long", "plain")]);
        assert_eq!(g.text(0), "ab  |   ");
        assert_eq!(g.text(1), "aaXYaaal");
        g.put(9, 0, &[span("off screen", "plain")]);
    }

    #[test]
    fn styles_and_own_colours() {
        let mut g = Grid::new(3, 1);
        g.put(0, 0, &[span("k", "keyword"), Span { fg: Some((205, 49, 49)), ..span("r", "plain") }, span("\x07", "plain")]);
        assert_eq!(g.cells[0][0].1.fg, (197, 134, 192));
        assert_eq!(g.cells[0][1].1.fg, (205, 49, 49));
        assert_eq!(g.text(0), "kr ");
    }
}
