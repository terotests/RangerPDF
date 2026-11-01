//! The viewer's entry point: a document opened from bytes, its pages
//! rendered to display lists and their text. Sans-IO: the host reads the
//! file, supplies the substitute fonts and draws the result.

use crate::pdf_display::PageOut;
use crate::pdf_display::TextItem;
use crate::pdf_doc::Doc;
use crate::pdf_font::FontFiles;
use crate::pdf_obj;
use crate::pdf_obj::Obj;
use crate::pdf_obj::K;
use crate::pdf_picture;
use crate::pdf_png;
use crate::pdf_raster;
use crate::pdf_render;
use crate::pdf_render::Renderer;

/// A bookmark: its title, depth (1 for the top level) and page (-1 when its
/// destination is not a page of this file).
pub struct OutlineItem {
    pub title: String,
    pub level: i64,
    pub page: i64,
}

pub struct PdfDocument {
    pub doc: Doc,
    pub renderer: Renderer,
}

impl PdfDocument {
    pub fn open(data: Vec<u8>, password: &str) -> PdfDocument {
        PdfDocument { doc: Doc::open(data, password), renderer: Renderer::new(FontFiles::new()) }
    }

    /// The same, with substitute fonts for fonts that are not embedded.
    pub fn open_with_fonts(data: Vec<u8>, password: &str, fonts: FontFiles) -> PdfDocument {
        PdfDocument { doc: Doc::open(data, password), renderer: Renderer::new(fonts) }
    }

    /// From a list of byte values (for hosts that hold bytes as ints).
    pub fn open_ints(data: &Vec<i64>, password: &str) -> PdfDocument {
        let mut b: Vec<u8> = vec![];
        for x in data.iter() {
            b.push((*x & 255) as u8);
        }
        PdfDocument::open(b, password)
    }

    pub fn open_ints_with_fonts(data: &Vec<i64>, password: &str, fonts: FontFiles) -> PdfDocument {
        let mut b: Vec<u8> = vec![];
        for x in data.iter() {
            b.push((*x & 255) as u8);
        }
        PdfDocument::open_with_fonts(b, password, fonts)
    }

    /// A document of `size` bytes that come later, in chunks of `chunk`
    /// bytes: the host feeds what needs() lists and calls restart() (for
    /// the structure) or the same page again until short() is false.
    pub fn open_sparse(size: i64, chunk: i64, password: &str, fonts: FontFiles) -> PdfDocument {
        let sz: usize = if size > 0 { size as usize } else { 0 };
        PdfDocument { doc: Doc::sparse(sz, chunk as usize, password), renderer: Renderer::new(fonts) }
    }

    /// The structure read again with the bytes fed so far.
    pub fn restart(&mut self, password: &str) {
        self.renderer.reset();
        self.doc.start(password);
    }

    /// The last step (open, restart, a page) wanted bytes that were not here.
    pub fn short(&self) -> bool {
        self.doc.short
    }

    pub fn complete(&self) -> bool {
        self.doc.complete()
    }

    /// The byte ranges the last step wanted, as start, end pairs; next to
    /// each other chunks are one range.
    pub fn needs(&self) -> Vec<i64> {
        let mut cs: Vec<i64> = self.doc.need.clone();
        cs.sort();
        let size = self.doc.lex.buf.len() as i64;
        let ch = self.doc.chunk as i64;
        let mut out: Vec<i64> = vec![];
        let mut i: usize = 0;
        while i < cs.len() {
            let from = cs[i] * ch;
            let mut last = cs[i];
            while i + 1 < cs.len() && cs[i + 1] == last + 1 {
                i += 1;
                last = cs[i];
            }
            let mut to = (last + 1) * ch;
            if to > size {
                to = size;
            }
            out.push(from);
            out.push(to);
            i += 1;
        }
        out
    }

    /// Bytes (as byte values) from offset `at`.
    pub fn feed_ints(&mut self, at: i64, bytes: &Vec<i64>) {
        let mut b: Vec<u8> = vec![];
        for x in bytes.iter() {
            b.push((*x & 255) as u8);
        }
        self.doc.feed(at as usize, &b);
    }

    pub fn error(&self) -> String {
        self.doc.error.clone()
    }

    pub fn page_count(&self) -> i64 {
        self.doc.pages.len() as i64
    }

    pub fn page_width(&self, i: i64) -> f64 {
        if i < 0 || (i as usize) >= self.doc.pages.len() {
            return 0.0;
        }
        self.doc.pages[i as usize].width()
    }

    pub fn page_height(&self, i: i64) -> f64 {
        if i < 0 || (i as usize) >= self.doc.pages.len() {
            return 0.0;
        }
        self.doc.pages[i as usize].height()
    }

    pub fn render(&mut self, i: i64) -> PageOut {
        if i < 0 {
            return PageOut::new(0.0, 0.0);
        }
        self.doc.begin();
        if !self.doc.realize(i as usize) {
            return PageOut::new(0.0, 0.0);
        }
        let out = pdf_render::render_page(&mut self.doc, &mut self.renderer, i as usize);
        if self.doc.short {
            // fonts read from a hole are read again next time
            self.renderer.reset();
        }
        out
    }

    /// Page i's own dictionary read (a sparse document reads it when the
    /// page is first shown); its size may differ from the one given before.
    /// false while its bytes are not here (short).
    pub fn realize(&mut self, i: i64) -> bool {
        if i < 0 {
            return true;
        }
        self.doc.begin();
        self.doc.realize(i as usize)
    }

    /// The page's text, as plain text.
    pub fn text(&mut self, i: i64) -> String {
        let p = self.render(i);
        p.plain_text()
    }

    pub fn title(&mut self) -> String {
        self.doc.info("Title")
    }

    /// Page `i` as a PNG file `width_px` pixels wide.
    pub fn page_png(&mut self, i: i64, width_px: f64) -> Vec<u8> {
        let p = self.render(i);
        let scale = if p.width > 0.0 { width_px / p.width } else { 1.0 };
        let r = pdf_raster::rasterize(&p, scale);
        pdf_png::encode_rgba(r.w, r.h, &r.px)
    }

    /// Page `i` turned clockwise by `quarter` quarter turns, as a PNG
    /// `width_px` wide after turning.
    pub fn page_png_turned(&mut self, i: i64, width_px: f64, quarter: i64) -> Vec<u8> {
        let q = ((quarter % 4) + 4) % 4;
        if q == 0 {
            return self.page_png(i, width_px);
        }
        let p = self.render(i);
        let across = if q == 2 { p.width } else { p.height };
        let scale = if across > 0.0 { width_px / across } else { 1.0 };
        let r = pdf_raster::rasterize(&p, scale);
        let t = pdf_picture::rotate(&pdf_picture::made("", r.w, r.h, r.px), q);
        pdf_png::encode_rgba(t.w, t.h, &t.rgba)
    }

    pub fn page_png_turned_ints(&mut self, i: i64, width_px: f64, quarter: i64) -> Vec<i64> {
        let f = self.page_png_turned(i, width_px, quarter);
        let mut out: Vec<i64> = vec![];
        for b in f.iter() {
            out.push(*b as i64);
        }
        out
    }

    /// The same as byte values (for hosts that hold bytes as ints).
    pub fn page_png_ints(&mut self, i: i64, width_px: f64) -> Vec<i64> {
        let f = self.page_png(i, width_px);
        let mut out: Vec<i64> = vec![];
        for b in f.iter() {
            out.push(*b as i64);
        }
        out
    }

    /// The boxes (in points, from the page's top left) of each match of
    /// `want` on page `i`, case-insensitive; a match is one box spanning
    /// its characters.
    pub fn find(&mut self, i: i64, want: &str) -> Vec<TextItem> {
        let p = self.render(i);
        find_in(&p, want)
    }

    /// The document's bookmarks, in order, at most 2000.
    pub fn outline(&mut self) -> Vec<OutlineItem> {
        self.doc.begin();
        let mut out: Vec<OutlineItem> = vec![];
        let root_ref = self.doc.trailer.at("Root");
        let root = self.doc.resolve(&root_ref);
        let ol = self.doc.get_key(&root, "Outlines");
        if !ol.is_dict() {
            return out;
        }
        let first = ol.at("First");
        let mut seen: Vec<i64> = vec![];
        self.outline_level(&root, &first, 1, &mut seen, &mut out);
        out
    }

    fn outline_level(&mut self, root: &Obj, first: &Obj, level: i64, seen: &mut Vec<i64>, out: &mut Vec<OutlineItem>) {
        let mut cur = first.clone();
        while cur.k == K::Ref && out.len() < 2000 && level < 20 {
            let n = cur.i();
            if seen.contains(&n) {
                return;
            }
            seen.push(n);
            let it = self.doc.resolve(&cur);
            if !it.is_dict() {
                return;
            }
            let t = self.doc.get_key(&it, "Title");
            let mut dest = self.doc.get_key(&it, "Dest");
            if dest.is_null() {
                let a = self.doc.get_key(&it, "A");
                if a.is_dict() && a.at("S").is_name("GoTo") {
                    dest = self.doc.get_key(&a, "D");
                }
            }
            let page = self.dest_page(root, &dest, 0);
            out.push(OutlineItem { title: t.text().trim().to_string(), level: level, page: page });
            let kid = it.at("First");
            if kid.k == K::Ref {
                self.outline_level(root, &kid, level + 1, seen, out);
            }
            cur = it.at("Next");
        }
    }

    /// The page a destination (an array, a name or a string) points to.
    fn dest_page(&mut self, root: &Obj, dest: &Obj, depth: i64) -> i64 {
        if depth > 4 {
            return -1;
        }
        let d = self.doc.resolve(dest);
        if d.k == K::Arr {
            if d.len() == 0 {
                return -1;
            }
            let p = d.item(0);
            if p.k == K::Ref {
                let n = p.i();
                let mut i: usize = 0;
                while i < self.doc.pages.len() {
                    if self.doc.pages[i].num == n {
                        return i as i64;
                    }
                    i += 1;
                }
                return -1;
            }
            if p.k == K::Num {
                return p.i();
            }
            return -1;
        }
        if d.is_dict() && d.has("D") {
            let dd = d.at("D");
            return self.dest_page(root, &dd, depth + 1);
        }
        let key = if d.k == K::Name { d.name.clone() } else if d.k == K::Str { pdf_obj::latin1(&d.s) } else { String::new() };
        if key.is_empty() {
            return -1;
        }
        // the catalog's /Dests, then the /Dests name tree
        let dests = self.doc.get_key(root, "Dests");
        if dests.is_dict() && dests.has(&key) {
            let v = dests.at(&key);
            return self.dest_page(root, &v, depth + 1);
        }
        let names = self.doc.get_key(root, "Names");
        if names.is_dict() {
            let tree = self.doc.get_key(&names, "Dests");
            let v = self.name_lookup(&tree, &key, 0);
            if !v.is_null() {
                return self.dest_page(root, &v, depth + 1);
            }
        }
        -1
    }

    fn name_lookup(&mut self, node_ref: &Obj, key: &str, depth: i64) -> Obj {
        let node = self.doc.resolve(node_ref);
        if !node.is_dict() || depth > 32 {
            return pdf_obj::null();
        }
        let names = self.doc.get_key(&node, "Names");
        if names.k == K::Arr {
            let mut i: usize = 0;
            while i + 1 < names.len() {
                let k = self.doc.resolve(&names.item(i));
                if pdf_obj::latin1(&k.s) == key {
                    return names.item(i + 1);
                }
                i += 2;
            }
        }
        let kids = self.doc.get_key(&node, "Kids");
        if kids.k == K::Arr {
            let mut i: usize = 0;
            while i < kids.len() {
                let v = self.name_lookup(&kids.item(i), key, depth + 1);
                if !v.is_null() {
                    return v;
                }
                i += 1;
            }
        }
        pdf_obj::null()
    }
}

/// `want` in a rendered page's text (PdfDocument::find without the render),
/// for hosts that keep pages' text.
pub fn find_in(p: &PageOut, want: &str) -> Vec<TextItem> {
    let mut out: Vec<TextItem> = vec![];
    let w: Vec<char> = want.to_lowercase().chars().collect();
    if w.is_empty() {
        return out;
    }
    // the characters with the item each came from (-1 for a gap)
    let mut cs: Vec<char> = vec![];
    let mut from: Vec<i64> = vec![];
    let mut k: usize = 0;
    while k < p.text.len() {
        let t = &p.text[k];
        if k > 0 {
            let prev = &p.text[k - 1];
            let dy = t.y - prev.y;
            let ady = if dy < 0.0 { 0.0 - dy } else { dy };
            let gap = t.x - (prev.x + prev.w);
            if (ady > prev.h * 0.5 || gap > t.h * 0.15) && t.text != " " {
                cs.push(' ');
                from.push(-1);
            }
        }
        for c in t.text.to_lowercase().chars() {
            cs.push(c);
            from.push(k as i64);
        }
        k += 1;
    }
    let mut at: usize = 0;
    while at + w.len() <= cs.len() {
        let mut j: usize = 0;
        while j < w.len() && cs[at + j] == w[j] {
            j += 1;
        }
        if j == w.len() {
            let mut x0 = 0.0;
            let mut y0 = 0.0;
            let mut x1 = 0.0;
            let mut y1 = 0.0;
            let mut any = false;
            let mut q = at;
            while q < at + w.len() {
                if from[q] >= 0 {
                    let t = &p.text[from[q] as usize];
                    if !any || t.x < x0 {
                        x0 = t.x;
                    }
                    if !any || t.y < y0 {
                        y0 = t.y;
                    }
                    if !any || t.x + t.w > x1 {
                        x1 = t.x + t.w;
                    }
                    if !any || t.y + t.h > y1 {
                        y1 = t.y + t.h;
                    }
                    any = true;
                }
                q += 1;
            }
            if any {
                out.push(TextItem { text: want.to_string(), x: x0, y: y0, w: x1 - x0, h: y1 - y0, dir: 0 });
            }
            at += w.len();
        } else {
            at += 1;
        }
    }
    out
}
