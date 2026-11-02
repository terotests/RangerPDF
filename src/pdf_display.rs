//! A rendered page: drawing commands in device space (points, y down, the
//! crop box's corner at 0,0, /Rotate applied), the images they refer to,
//! and the text with its positions for search and selection.

use crate::pdf_image::Img;
use crate::pdf_path::Path;

pub const FILL: i64 = 0;
pub const STROKE: i64 = 1;
/// intersect the clip with `pts` (until the matching UNCLIP)
pub const CLIP: i64 = 2;
pub const UNCLIP: i64 = 3;
/// image `image` drawn into the parallelogram of `m` (the unit square, y up)
pub const IMAGE: i64 = 4;
/// text in a font the host draws: `text` at `m` (glyph space at size 1, y up)
pub const TEXT: i64 = 5;
/// glyph `image` of `PageOut::glyphs` (its outline at size 1, y up) filled
/// at `m`: only when the renderer keeps glyphs (`Renderer::keep_glyphs`,
/// for pdf_svg), else glyphs are FILL commands
pub const GLYPH: i64 = 6;

#[derive(Clone, Debug)]
pub struct Cmd {
    pub op: i64,
    pub pts: Vec<f64>,
    pub ends: Vec<i64>,
    pub closed: Vec<bool>,
    pub even_odd: bool,
    pub r: i64,
    pub g: i64,
    pub b: i64,
    pub a: f64,
    pub width: f64,
    pub cap: i64,
    pub join: i64,
    pub miter: f64,
    pub dash: Vec<f64>,
    pub phase: f64,
    pub image: i64,
    pub m: Vec<f64>,
    pub text: String,
    pub family: String,
    pub bold: bool,
    pub italic: bool,
}

pub fn cmd(op: i64) -> Cmd {
    Cmd {
        op: op,
        pts: vec![],
        ends: vec![],
        closed: vec![],
        even_odd: false,
        r: 0,
        g: 0,
        b: 0,
        a: 1.0,
        width: 1.0,
        cap: 0,
        join: 0,
        miter: 10.0,
        dash: vec![],
        phase: 0.0,
        image: -1,
        m: vec![],
        text: String::new(),
        family: String::new(),
        bold: false,
        italic: false,
    }
}

/// One shown character: its text and its box in device space.
#[derive(Clone, Debug)]
pub struct TextItem {
    pub text: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    /// the baseline's direction: 0 left to right, 90 top to bottom, ...
    pub dir: i64,
}

pub struct PageOut {
    pub width: f64,
    pub height: f64,
    pub cmds: Vec<Cmd>,
    pub images: Vec<Img>,
    pub text: Vec<TextItem>,
    pub warnings: Vec<String>,
    /// the outlines GLYPH commands use, each once, and their keys
    pub glyphs: Vec<Path>,
    pub glyph_keys: Vec<String>,
}

impl PageOut {
    pub fn new(w: f64, h: f64) -> PageOut {
        PageOut { width: w, height: h, cmds: vec![], images: vec![], text: vec![], warnings: vec![], glyphs: vec![], glyph_keys: vec![] }
    }

    pub fn warn(&mut self, s: &str) {
        let k = s.to_string();
        if !self.warnings.contains(&k) && self.warnings.len() < 30 {
            self.warnings.push(k);
        }
    }

    /// The page's text in reading order of the content, with a space where
    /// a gap is wider than a fraction of the font size and a line break where
    /// the baseline moves.
    pub fn plain_text(&self) -> String {
        let mut out = String::new();
        let mut i: usize = 0;
        let mut last_x = 0.0;
        let mut last_y = 0.0;
        let mut last_h = 0.0;
        while i < self.text.len() {
            let t = &self.text[i];
            if i > 0 {
                let dy = t.y - last_y;
                let ady = if dy < 0.0 { 0.0 - dy } else { dy };
                if ady > last_h * 0.5 {
                    out.push('\n');
                } else {
                    let gap = t.x - last_x;
                    if gap > t.h * 0.15 && !out.ends_with(' ') && t.text != " " {
                        out.push(' ');
                    }
                }
            }
            out.push_str(&t.text);
            last_x = t.x + t.w;
            last_y = t.y;
            last_h = if t.h > 0.0 { t.h } else { 1.0 };
            i += 1;
        }
        out
    }
}
