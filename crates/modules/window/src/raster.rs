//! The grid as pixels: a monospace font, a cell the size of one of its
//! characters, each cell's background filled and its character drawn over
//! it in its colour. On the CPU, into a `0RGB` buffer — what `softbuffer`
//! shows, and what a test can look at without a window.

use crate::grid::{Grid, Look};
use fontdue::{Font, FontSettings, Metrics};
use std::collections::HashMap;

/// A font at one size, and what it has drawn so far.
pub struct Face {
    regular: Font,
    bold: Option<Font>,
    pub px: f32,
    pub cell_w: usize,
    pub cell_h: usize,
    baseline: f32,
    cache: HashMap<(char, bool), (Metrics, Vec<u8>)>,
}

/// The monospace font the system names, by `fc-match` — `monospace`, or
/// `monospace:bold`. `None` when there is no fontconfig or no answer.
pub fn system_font(bold: bool) -> Option<String> {
    let pattern = if bold { "monospace:bold" } else { "monospace" };
    let out = std::process::Command::new("fc-match").args([pattern, "-f", "%{file}"]).output().ok()?;
    let path = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (!path.is_empty() && std::path::Path::new(&path).exists()).then_some(path)
}

fn load(path: &str) -> Result<Font, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("cannot read the font '{path}': {e}"))?;
    Font::from_bytes(bytes, FontSettings::default()).map_err(|e| format!("'{path}' is not a font: {e}"))
}

impl Face {
    /// `font` (a path, else the system's monospace), `bold` likewise (else
    /// drawn thicker), at `px` pixels.
    pub fn new(font: Option<&str>, bold: Option<&str>, px: f32) -> Result<Face, String> {
        let regular_path = match font {
            Some(p) => p.to_string(),
            None => system_font(false).ok_or("no font: give Open a `font` path (fc-match found no monospace font)")?,
        };
        let regular = load(&regular_path)?;
        let bold = match bold {
            Some(p) => Some(load(p)?),
            None if font.is_none() => system_font(true).filter(|p| *p != regular_path).and_then(|p| load(&p).ok()),
            None => None,
        };
        let mut face = Face { regular, bold, px, cell_w: 1, cell_h: 1, baseline: 0.0, cache: HashMap::new() };
        face.measure();
        Ok(face)
    }

    /// The cell: as wide as `M` advances, as tall as a line.
    fn measure(&mut self) {
        let m = self.regular.metrics('M', self.px);
        self.cell_w = (m.advance_width.ceil() as usize).max(1);
        match self.regular.horizontal_line_metrics(self.px) {
            Some(line) => {
                self.cell_h = ((line.ascent - line.descent + line.line_gap).ceil() as usize).max(1);
                self.baseline = line.ascent.ceil();
            }
            None => {
                self.cell_h = (self.px * 1.25).ceil() as usize;
                self.baseline = self.px;
            }
        }
    }

    /// The same font at another size (a display's scale changed, or the
    /// program asked).
    pub fn resize(&mut self, px: f32) {
        self.px = px;
        self.cache.clear();
        self.measure();
    }

    fn glyph(&mut self, c: char, bold: bool) -> &(Metrics, Vec<u8>) {
        let use_bold = bold && self.bold.is_some();
        let (font, px) = (if use_bold { self.bold.as_ref().unwrap() } else { &self.regular }, self.px);
        self.cache.entry((c, use_bold)).or_insert_with(|| font.rasterize(c, px))
    }

    /// Draws `grid` into `buf` (`width` × `height` pixels, `0RGB`), from
    /// the top left; what is outside the grid is `bg`. `cursor` is drawn as
    /// a block with its colours swapped.
    pub fn draw(&mut self, grid: &Grid, cursor: Option<(usize, usize)>, buf: &mut [u32], width: usize, height: usize, bg: (u8, u8, u8)) {
        let fill = pack(bg);
        for p in buf.iter_mut() {
            *p = fill;
        }
        let (cw, ch) = (self.cell_w, self.cell_h);
        for (r, line) in grid.cells.iter().enumerate() {
            for (c, (character, look)) in line.iter().enumerate() {
                let mut look = *look;
                if cursor == Some((r, c)) {
                    std::mem::swap(&mut look.fg, &mut look.bg);
                }
                self.cell(buf, width, height, c * cw, r * ch, *character, look);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn cell(&mut self, buf: &mut [u32], width: usize, height: usize, x0: usize, y0: usize, c: char, look: Look) {
        let (cw, ch, baseline) = (self.cell_w, self.cell_h, self.baseline);
        let bgp = pack(look.bg);
        for y in y0..(y0 + ch).min(height) {
            let row = &mut buf[y * width..y * width + width];
            for p in row.iter_mut().take((x0 + cw).min(width)).skip(x0) {
                *p = bgp;
            }
        }
        if c == ' ' {
            return;
        }
        // Thicker, when there is no bold font: drawn again a pixel over.
        let fake_bold = look.bold && self.bold.is_none();
        let (metrics, bitmap) = self.glyph(c, look.bold).clone();
        let top = y0 as i64 + baseline as i64 - metrics.height as i64 - metrics.ymin as i64;
        let left = x0 as i64 + metrics.xmin as i64;
        for pass in 0..if fake_bold { 2 } else { 1 } {
            for gy in 0..metrics.height {
                let y = top + gy as i64;
                if y < 0 || y >= height as i64 || y >= (y0 + ch) as i64 {
                    continue;
                }
                for gx in 0..metrics.width {
                    let x = left + gx as i64 + pass;
                    if x < 0 || x >= width as i64 {
                        continue;
                    }
                    let a = bitmap[gy * metrics.width + gx];
                    if a == 0 {
                        continue;
                    }
                    let at = y as usize * width + x as usize;
                    buf[at] = blend(buf[at], look.fg, a);
                }
            }
        }
    }
}

pub fn pack((r, g, b): (u8, u8, u8)) -> u32 {
    (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)
}

pub fn unpack(p: u32) -> (u8, u8, u8) {
    ((p >> 16) as u8, (p >> 8) as u8, p as u8)
}

fn blend(under: u32, (r, g, b): (u8, u8, u8), a: u8) -> u32 {
    let (ur, ug, ub) = unpack(under);
    let mix = |top: u8, bottom: u8| ((u32::from(top) * u32::from(a) + u32::from(bottom) * (255 - u32::from(a))) / 255) as u8;
    pack((mix(r, ur), mix(g, ug), mix(b, ub)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::Span;

    #[test]
    fn a_letter_in_its_colour_on_its_background() {
        let Some(path) = system_font(false) else { return };
        let mut face = Face::new(Some(&path), None, 16.0).expect("the font");
        assert!(face.cell_w >= 6 && face.cell_h >= 12, "{}x{}", face.cell_w, face.cell_h);
        let mut grid = Grid::new(2, 1);
        grid.put(0, 0, &[Span { text: "W".into(), style: "plain".into(), fg: Some((255, 0, 0)), bg: Some((0, 0, 255)), ..Span::default() }]);
        let (w, h) = (face.cell_w * 2, face.cell_h);
        let mut buf = vec![0; w * h];
        face.draw(&grid, None, &mut buf, w, h, (30, 30, 30));
        let first: Vec<(u8, u8, u8)> = buf[..].chunks(w).flat_map(|row| row[..face.cell_w].to_vec()).map(unpack).collect();
        assert!(first.contains(&(0, 0, 255)), "the background is blue");
        assert!(first.iter().any(|&(r, _, b)| r > 200 && b < 60), "the letter is red");
        // The second cell is the plain background, nothing drawn in it.
        assert_eq!(unpack(buf[face.cell_w + face.cell_w / 2]), (30, 30, 30));
    }

    #[test]
    fn the_cursor_swaps_the_colours() {
        let Some(path) = system_font(false) else { return };
        let mut face = Face::new(Some(&path), None, 16.0).unwrap();
        let grid = Grid::new(1, 1);
        let (w, h) = (face.cell_w, face.cell_h);
        let mut buf = vec![0; w * h];
        face.draw(&grid, Some((0, 0)), &mut buf, w, h, (30, 30, 30));
        assert_eq!(unpack(buf[0]), (212, 212, 212));
    }
}
