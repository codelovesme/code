//! Rows of styled spans, as what the terminal is sent — pure, tested
//! without one.
//!
//! A program cannot write an escape character (a `code` string has no
//! escape for it), and should not have to know one: it names a *style*
//! (`keyword`, `selected`, `status`) and this module owns what that looks
//! like. A name it does not know is `plain`.
//!
//! The screen is a grid of cells, exactly the terminal's size, so a row
//! that was drawn is a row fully overwritten — the previous frame never
//! shows through a shorter line.

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

fn sgr(name: &str) -> String {
    let ((fr, fg, fb), (br, bg, bb), bold) = style(name);
    format!(
        "\x1b[0;{}38;2;{fr};{fg};{fb};48;2;{br};{bg};{bb}m",
        if bold { "1;" } else { "" }
    )
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
/// without counting characters itself.
#[derive(Debug, Clone, Default)]
pub struct Span {
    pub text: String,
    pub style: String,
    pub width: Option<usize>,
    pub right: bool,
}

/// One cell of the screen: a character and the style it is drawn in.
type Cell = (char, String);

/// The screen as cells, before it is turned into bytes. Rows are laid at
/// column 0 first, then overlays on top, in order — a menu drawn last is
/// drawn over whatever is under it.
pub struct Grid {
    cols: usize,
    cells: Vec<Vec<Cell>>,
}

impl Grid {
    pub fn new(cols: usize, rows: usize) -> Self {
        Grid { cols, cells: vec![vec![(' ', "plain".to_string()); cols]; rows] }
    }

    /// Writes `spans` from (`row`, `col`) rightward, clipped at the edge.
    /// A span without a width writes its text; with one, exactly that many
    /// cells, padded with spaces in the span's style.
    pub fn put(&mut self, row: usize, col: usize, spans: &[Span]) {
        let cols = self.cols;
        let Some(line) = self.cells.get_mut(row) else { return };
        let mut at = col;
        for span in spans {
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
                    let pad = std::iter::repeat(' ').take(w - chars.len());
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
                line[at] = (c, span.style.clone());
                at += 1;
            }
        }
    }

    /// Each row as terminal output: runs of one style share one colour
    /// change, and every row ends by resetting it.
    pub fn rows(&self) -> Vec<String> {
        self.cells
            .iter()
            .map(|line| {
                let mut out = String::new();
                let mut current: Option<&str> = None;
                for (c, style_name) in line {
                    if current != Some(style_name.as_str()) {
                        out.push_str(&sgr(style_name));
                        current = Some(style_name);
                    }
                    out.push(*c);
                }
                out.push_str("\x1b[0m");
                out
            })
            .collect()
    }
}

/// The bytes that turn `previous` into `next`: only rows that differ are
/// rewritten. `None` for `previous` (the first frame, or after a resize)
/// clears the screen and writes every row.
pub fn frame(previous: Option<&[String]>, next: &[String]) -> String {
    let mut out = String::new();
    if previous.is_none() {
        out.push_str("\x1b[0m\x1b[2J");
    }
    for (i, row) in next.iter().enumerate() {
        let same = previous.and_then(|p| p.get(i)).is_some_and(|old| old == row);
        if !same {
            out.push_str(&format!("\x1b[{};1H", i + 1));
            out.push_str(row);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn visible(s: &str) -> String {
        let mut out = String::new();
        let mut in_esc = false;
        for c in s.chars() {
            if in_esc {
                if c.is_ascii_alphabetic() {
                    in_esc = false;
                }
            } else if c == '\x1b' {
                in_esc = true;
            } else {
                out.push(c);
            }
        }
        out
    }

    fn span(t: &str, s: &str) -> Span {
        Span { text: t.to_string(), style: s.to_string(), ..Span::default() }
    }

    fn one_row(spans: &[Span], cols: usize) -> String {
        let mut g = Grid::new(cols, 1);
        g.put(0, 0, spans);
        g.rows().remove(0)
    }

    #[test]
    fn a_row_is_exactly_the_width() {
        assert_eq!(visible(&one_row(&[span("ab", "plain")], 5)), "ab   ");
        assert_eq!(visible(&one_row(&[span("abc", "plain"), span("defg", "keyword")], 5)), "abcde");
        assert_eq!(visible(&one_row(&[span("∈é", "plain")], 3)), "∈é ");
        assert_eq!(visible(&one_row(&[], 2)), "  ");
    }

    #[test]
    fn a_width_pads_or_cuts_and_can_align_right() {
        let w = |t: &str, n: usize, right: bool| Span { width: Some(n), right, ..span(t, "plain") };
        assert_eq!(visible(&one_row(&[w("ab", 4, false), span("|", "plain")], 8)), "ab  |   ");
        assert_eq!(visible(&one_row(&[w("abcdef", 3, false), span("|", "plain")], 5)), "abc| ");
        assert_eq!(visible(&one_row(&[w("12", 4, true)], 4)), "  12");
        assert_eq!(visible(&one_row(&[w("abcdef", 3, true)], 3)), "def");
    }

    #[test]
    fn overlays_draw_over_what_is_under_them() {
        let mut g = Grid::new(6, 2);
        g.put(0, 0, &[span("aaaaaa", "plain")]);
        g.put(0, 2, &[span("XY", "menu")]);
        g.put(1, 4, &[span("long", "plain")]);
        let rows: Vec<String> = g.rows().iter().map(|r| visible(r)).collect();
        assert_eq!(rows, ["aaXYaa", "    lo"]);
        g.put(9, 0, &[span("off screen", "plain")]);
    }

    #[test]
    fn control_characters_never_reach_the_terminal() {
        assert_eq!(visible(&one_row(&[span("a\tb\x1b[2Jc\x07", "plain")], 8)), "a b[2Jc ");
    }

    #[test]
    fn styles_become_colours() {
        assert!(one_row(&[span("x", "keyword")], 1).contains("38;2;197;134;192"));
        assert!(one_row(&[span("x", "nonsense")], 1).contains("38;2;212;212;212"));
    }

    #[test]
    fn only_changed_rows_are_written() {
        let a = vec!["r0".to_string(), "r1".to_string()];
        let b = vec!["r0".to_string(), "R1".to_string()];
        let first = frame(None, &a);
        assert!(first.contains("\x1b[2J") && first.contains("r0") && first.contains("r1"));
        let next = frame(Some(&a), &b);
        assert!(!next.contains("r0") && next.contains("\x1b[2;1HR1"));
        assert!(frame(Some(&b), &b).is_empty());
    }
}
