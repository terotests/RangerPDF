//! The content stream interpreter (ISO 32000-1 §8, §9): graphics state,
//! paths, colours, clipping, text, images, form XObjects, Type 3 glyphs,
//! shadings and patterns, drawn into a `PageOut` display list.

use crate::pdf_color;
use crate::pdf_color::ColorSpace;
use crate::pdf_display;
use crate::pdf_display::Cmd;
use crate::pdf_display::PageOut;
use crate::pdf_display::TextItem;
use crate::pdf_doc::Doc;
use crate::pdf_font::Font;
use crate::pdf_font::FontFiles;
use crate::pdf_func;
use crate::pdf_glyphs::Glyphs;
use crate::pdf_image;
use crate::pdf_lexer::PdfLexer;
use crate::pdf_obj;
use crate::pdf_obj::Obj;
use crate::pdf_obj::K;
use crate::pdf_path;
use crate::pdf_path::Path;

#[derive(Clone, Debug)]
struct GState {
    ctm: Vec<f64>,
    fill_cs: ColorSpace,
    stroke_cs: ColorSpace,
    fill: Vec<f64>,
    stroke: Vec<f64>,
    fill_a: f64,
    stroke_a: f64,
    fill_pattern: Obj,
    stroke_pattern: Obj,
    fill_none: bool,
    stroke_none: bool,
    lw: f64,
    cap: i64,
    join: i64,
    miter: f64,
    dash: Vec<f64>,
    phase: f64,
    font: i64,
    fs: f64,
    tc: f64,
    tw: f64,
    th: f64,
    tl: f64,
    rise: f64,
    mode: i64,
    /// CLIP commands pushed while this state was current
    clips: i64,
    /// the clip's device bounding box
    clip_box: Vec<f64>,
}

/// What stays between pages: the glyph list, the substitute fonts and the
/// fonts loaded so far (by object number).
pub struct Renderer {
    pub glyphs: Glyphs,
    pub files: FontFiles,
    fonts: Vec<Font>,
    font_keys: Vec<i64>,
    /// the flattening tolerance in device units (points); smaller for zoom
    pub tol: f64,
    /// stop a page after this many operators
    pub max_ops: i64,
}

impl Renderer {
    pub fn new(files: FontFiles) -> Renderer {
        Renderer { glyphs: Glyphs::load(), files: files, fonts: vec![], font_keys: vec![], tol: 0.1, max_ops: 3000000 }
    }

    /// Forget the fonts (a new document).
    pub fn reset(&mut self) {
        self.fonts = vec![];
        self.font_keys = vec![];
    }
}

struct Run {
    gs: GState,
    stack: Vec<GState>,
    path: Path,
    /// set by W / W*: the path becomes a clip when it is painted
    pending_clip: i64,
    tm: Vec<f64>,
    tlm: Vec<f64>,
    /// glyph outlines of text render modes 4-7, clipped at ET
    text_clip: Path,
    ops: i64,
    depth: i64,
    out: PageOut,
    /// form XObjects being run (object numbers), against loops
    forms: Vec<i64>,
    fontless: bool,
    /// drawing annotation appearances: their text is not the page's text
    annot: bool,
}

fn initial_gs(base: &Vec<f64>, w: f64, h: f64) -> GState {
    let gray = pdf_color::device(pdf_color::CS_GRAY);
    GState {
        ctm: base.clone(),
        fill_cs: gray.clone(),
        stroke_cs: gray,
        fill: vec![0.0, 0.0, 0.0],
        stroke: vec![0.0, 0.0, 0.0],
        fill_a: 1.0,
        stroke_a: 1.0,
        fill_pattern: pdf_obj::null(),
        stroke_pattern: pdf_obj::null(),
        fill_none: false,
        stroke_none: false,
        lw: 1.0,
        cap: 0,
        join: 0,
        miter: 10.0,
        dash: vec![],
        phase: 0.0,
        font: -1,
        fs: 0.0,
        tc: 0.0,
        tw: 0.0,
        th: 1.0,
        tl: 0.0,
        rise: 0.0,
        mode: 0,
        clips: 0,
        clip_box: vec![0.0, 0.0, w, h],
    }
}

/// User space of page `p` -> device space (y down, the crop box at 0,0, rotated).
pub fn page_matrix(p: &crate::pdf_doc::Page) -> Vec<f64> {
    let x0 = p.crop[0];
    let y1 = p.crop[3];
    let cw = p.crop[2] - p.crop[0];
    let ch = p.crop[3] - p.crop[1];
    let flip: Vec<f64> = vec![1.0, 0.0, 0.0, -1.0, 0.0 - x0, y1];
    let rot: Vec<f64> = if p.rotate == 90 {
        vec![0.0, 1.0, -1.0, 0.0, ch, 0.0]
    } else if p.rotate == 180 {
        vec![-1.0, 0.0, 0.0, -1.0, cw, ch]
    } else if p.rotate == 270 {
        vec![0.0, -1.0, 1.0, 0.0, 0.0, cw]
    } else {
        pdf_path::identity()
    };
    pdf_path::mat_mul(&flip, &rot)
}

fn num_at(st: &Vec<Obj>, i: usize) -> f64 {
    if i < st.len() {
        return st[i].f();
    }
    0.0
}

fn nums(st: &Vec<Obj>) -> Vec<f64> {
    let mut v: Vec<f64> = vec![];
    for o in st.iter() {
        if o.k == K::Num {
            v.push(o.n);
        }
    }
    v
}

fn byte(x: f64) -> i64 {
    pdf_image::to_byte(x) as i64
}

fn inter_box(a: &Vec<f64>, b: &Vec<f64>) -> Vec<f64> {
    vec![pdf_path::fmax(a[0], b[0]), pdf_path::fmax(a[1], b[1]), pdf_path::fmin(a[2], b[2]), pdf_path::fmin(a[3], b[3])]
}

pub fn render_page(doc: &mut Doc, r: &mut Renderer, page: usize) -> PageOut {
    if page >= doc.pages.len() {
        return PageOut::new(0.0, 0.0);
    }
    let p = doc.pages[page].clone();
    let w = p.width();
    let h = p.height();
    let base = page_matrix(&p);
    let mut run = Run {
        gs: initial_gs(&base, w, h),
        stack: vec![],
        path: Path::new(),
        pending_clip: 0,
        tm: pdf_path::identity(),
        tlm: pdf_path::identity(),
        text_clip: Path::new(),
        ops: 0,
        depth: 0,
        out: PageOut::new(w, h),
        forms: vec![],
        fontless: false,
        annot: false,
    };
    let content = doc.page_content(page);
    let res = p.resources.clone();
    run_content(doc, r, &mut run, content, &res);
    while !run.stack.is_empty() {
        restore(&mut run);
    }
    let mut c0 = 0;
    while c0 < run.gs.clips {
        run.out.cmds.push(pdf_display::cmd(pdf_display::UNCLIP));
        c0 += 1;
    }
    run.gs.clips = 0;
    annotations(doc, r, &mut run, &p, &base, w, h);
    // close what the content left open
    while !run.stack.is_empty() {
        restore(&mut run);
    }
    let mut k = 0;
    while k < run.gs.clips {
        run.out.cmds.push(pdf_display::cmd(pdf_display::UNCLIP));
        k += 1;
    }
    run.out
}

/// The normal appearances of the page's annotations that are not hidden.
fn annotations(doc: &mut Doc, r: &mut Renderer, run: &mut Run, p: &crate::pdf_doc::Page, base: &Vec<f64>, w: f64, h: f64) {
    let annots = doc.get_key(&p.dict, "Annots");
    if annots.k != K::Arr {
        return;
    }
    run.annot = true;
    let mut i: usize = 0;
    while i < annots.len() && i < 2000 {
        let a = doc.resolve(&annots.item(i));
        i += 1;
        if !a.is_dict() {
            continue;
        }
        let flags = doc.get_key(&a, "F").i();
        if (flags & 2) != 0 || (flags & 32) != 0 {
            continue;
        }
        let ap = doc.get_key(&a, "AP");
        if !ap.is_dict() {
            continue;
        }
        let mut nref = ap.at("N");
        let mut n = doc.resolve(&nref);
        if n.is_dict() && n.k != K::Stream {
            let st = doc.get_key(&a, "AS");
            if st.k != K::Name || !n.has(&st.name) {
                continue;
            }
            nref = n.at(&st.name);
            n = doc.resolve(&nref);
        }
        if n.k != K::Stream {
            continue;
        }
        let rect = doc.get_key(&a, "Rect").nums();
        let bb = doc.get_key(&n, "BBox").nums();
        if rect.len() != 4 || bb.len() != 4 {
            continue;
        }
        // the BBox through the form's Matrix, fitted to the Rect
        let mut fm = doc.get_key(&n, "Matrix").nums();
        if fm.len() != 6 {
            fm = pdf_path::identity();
        }
        let xs: Vec<f64> = vec![bb[0], bb[2], bb[0], bb[2]];
        let ys: Vec<f64> = vec![bb[1], bb[1], bb[3], bb[3]];
        let mut x0 = 0.0;
        let mut y0 = 0.0;
        let mut x1 = 0.0;
        let mut y1 = 0.0;
        let mut k: usize = 0;
        while k < 4 {
            let tx = xs[k] * fm[0] + ys[k] * fm[2] + fm[4];
            let ty = xs[k] * fm[1] + ys[k] * fm[3] + fm[5];
            if k == 0 || tx < x0 {
                x0 = tx;
            }
            if k == 0 || ty < y0 {
                y0 = ty;
            }
            if k == 0 || tx > x1 {
                x1 = tx;
            }
            if k == 0 || ty > y1 {
                y1 = ty;
            }
            k += 1;
        }
        let rx0 = pdf_path::fmin(rect[0], rect[2]);
        let ry0 = pdf_path::fmin(rect[1], rect[3]);
        let rx1 = pdf_path::fmax(rect[0], rect[2]);
        let ry1 = pdf_path::fmax(rect[1], rect[3]);
        let sx = if x1 - x0 > 0.0 { (rx1 - rx0) / (x1 - x0) } else { 1.0 };
        let sy = if y1 - y0 > 0.0 { (ry1 - ry0) / (y1 - y0) } else { 1.0 };
        let fit: Vec<f64> = vec![sx, 0.0, 0.0, sy, rx0 - x0 * sx, ry0 - y0 * sy];
        run.gs = initial_gs(base, w, h);
        let res = p.resources.clone();
        draw_form(doc, r, run, &nref, &n, &res, &fit);
    }
}

fn restore(run: &mut Run) {
    let mut k = 0;
    while k < run.gs.clips {
        run.out.cmds.push(pdf_display::cmd(pdf_display::UNCLIP));
        k += 1;
    }
    if !run.stack.is_empty() {
        let n = run.stack.len() - 1;
        run.gs = run.stack[n].clone();
        run.stack.truncate(n);
    }
}

fn run_content(doc: &mut Doc, r: &mut Renderer, run: &mut Run, content: Vec<u8>, res: &Obj) {
    run.depth += 1;
    if run.depth > 12 {
        run.depth -= 1;
        return;
    }
    let mut lx = PdfLexer::new(content);
    let mut st: Vec<Obj> = vec![];
    let start_stack = run.stack.len();
    loop {
        let t = lx.token();
        if t.k == K::Eof {
            break;
        }
        if t.k != K::Cmd || t.name == "[" || t.name == "<<" {
            let o = lx.object_from(t, 0);
            st.push(o);
            if st.len() > 4000 {
                st = vec![];
            }
            continue;
        }
        run.ops += 1;
        if run.ops > r.max_ops {
            run.out.warn("too many operators: page cut short");
            break;
        }
        let op = t.name.clone();
        if op == "BI" {
            inline_image(doc, r, run, &mut lx, res);
            st = vec![];
            continue;
        }
        exec(doc, r, run, &op, &st, res);
        st = vec![];
    }
    // a content stream must not leave its q's open to its caller
    while run.stack.len() > start_stack {
        restore(run);
    }
    run.depth -= 1;
}

fn exec(doc: &mut Doc, r: &mut Renderer, run: &mut Run, op: &str, st: &Vec<Obj>, res: &Obj) {
    let n = st.len();
    if op == "q" {
        let mut saved = run.gs.clone();
        saved.clips = run.gs.clips;
        run.stack.push(saved);
        run.gs.clips = 0;
        return;
    }
    if op == "Q" {
        if !run.stack.is_empty() {
            restore(run);
        }
        return;
    }
    if op == "cm" {
        if n >= 6 {
            let m: Vec<f64> = vec![num_at(st, 0), num_at(st, 1), num_at(st, 2), num_at(st, 3), num_at(st, 4), num_at(st, 5)];
            run.gs.ctm = pdf_path::mat_mul(&m, &run.gs.ctm);
        }
        return;
    }
    // path construction (in user space, transformed when painted)
    if op == "m" {
        if n >= 2 {
            let x = num_at(st, n - 2);
            let y = num_at(st, n - 1);
            run.path.move_to(x, y);
        }
        return;
    }
    if op == "l" {
        if n >= 2 {
            run.path.line_to(num_at(st, n - 2), num_at(st, n - 1));
        }
        return;
    }
    if op == "c" {
        if n >= 6 {
            run.path.curve_to(num_at(st, 0), num_at(st, 1), num_at(st, 2), num_at(st, 3), num_at(st, 4), num_at(st, 5));
        }
        return;
    }
    if op == "v" {
        if n >= 4 {
            let cx = run.path.cx;
            let cy = run.path.cy;
            run.path.curve_to(cx, cy, num_at(st, 0), num_at(st, 1), num_at(st, 2), num_at(st, 3));
        }
        return;
    }
    if op == "y" {
        if n >= 4 {
            let x = num_at(st, 2);
            let y = num_at(st, 3);
            run.path.curve_to(num_at(st, 0), num_at(st, 1), x, y, x, y);
        }
        return;
    }
    if op == "h" {
        run.path.close();
        return;
    }
    if op == "re" {
        if n >= 4 {
            run.path.rect(num_at(st, 0), num_at(st, 1), num_at(st, 2), num_at(st, 3));
        }
        return;
    }
    // painting
    if op == "S" || op == "s" || op == "f" || op == "F" || op == "f*" || op == "B" || op == "B*" || op == "b" || op == "b*" || op == "n" {
        if op == "s" || op == "b" || op == "b*" {
            run.path.close();
        }
        let eo = op == "f*" || op == "B*" || op == "b*";
        let fill = op == "f" || op == "F" || op == "f*" || op == "B" || op == "B*" || op == "b" || op == "b*";
        let stroke = op == "S" || op == "s" || op == "B" || op == "B*" || op == "b" || op == "b*";
        let p = run.path.clone();
        if fill {
            fill_path(doc, r, run, &p, eo, res);
        }
        if stroke {
            stroke_path(doc, r, run, &p, res);
        }
        if run.pending_clip != 0 {
            let eoc = run.pending_clip == 2;
            clip_path(run, &p, eoc);
            run.pending_clip = 0;
        }
        run.path = Path::new();
        return;
    }
    if op == "W" {
        run.pending_clip = 1;
        return;
    }
    if op == "W*" {
        run.pending_clip = 2;
        return;
    }
    // graphics state
    if op == "w" {
        run.gs.lw = num_at(st, 0);
        return;
    }
    if op == "J" {
        run.gs.cap = num_at(st, 0) as i64;
        return;
    }
    if op == "j" {
        run.gs.join = num_at(st, 0) as i64;
        return;
    }
    if op == "M" {
        run.gs.miter = num_at(st, 0);
        return;
    }
    if op == "d" {
        if n >= 2 {
            run.gs.dash = st[0].nums();
            run.gs.phase = st[1].f();
        }
        return;
    }
    if op == "gs" {
        if n >= 1 && st[0].k == K::Name {
            ext_gstate(doc, r, run, &st[0].name, res);
        }
        return;
    }
    if op == "ri" || op == "i" {
        return;
    }
    // colour
    if op == "g" || op == "G" || op == "rg" || op == "RG" || op == "k" || op == "K" {
        let kind = if op == "g" || op == "G" { pdf_color::CS_GRAY } else if op == "rg" || op == "RG" { pdf_color::CS_RGB } else { pdf_color::CS_CMYK };
        let cs = pdf_color::device(kind);
        let comps = nums(st);
        let rgb = cs.to_rgb(&comps);
        if op == "g" || op == "rg" || op == "k" {
            run.gs.fill_cs = cs;
            run.gs.fill = rgb;
            run.gs.fill_pattern = pdf_obj::null();
            run.gs.fill_none = false;
        } else {
            run.gs.stroke_cs = cs;
            run.gs.stroke = rgb;
            run.gs.stroke_pattern = pdf_obj::null();
            run.gs.stroke_none = false;
        }
        return;
    }
    if op == "cs" || op == "CS" {
        if n >= 1 {
            let cs = pdf_color::read(doc, &st[0], res, 0);
            let init = cs.initial();
            let rgb = cs.to_rgb(&init);
            if op == "cs" {
                run.gs.fill_none = cs.none;
                run.gs.fill_cs = cs;
                run.gs.fill = rgb;
                run.gs.fill_pattern = pdf_obj::null();
            } else {
                run.gs.stroke_none = cs.none;
                run.gs.stroke_cs = cs;
                run.gs.stroke = rgb;
                run.gs.stroke_pattern = pdf_obj::null();
            }
        }
        return;
    }
    if op == "sc" || op == "scn" || op == "SC" || op == "SCN" {
        let fill = op == "sc" || op == "scn";
        let comps = nums(st);
        let mut pat = pdf_obj::null();
        if n >= 1 && st[n - 1].k == K::Name {
            pat = st[n - 1].clone();
        }
        let cs = if fill { run.gs.fill_cs.clone() } else { run.gs.stroke_cs.clone() };
        if cs.kind == pdf_color::CS_PATTERN {
            let rgb = if !cs.base.is_empty() && !comps.is_empty() { cs.to_rgb(&comps) } else { vec![0.5, 0.5, 0.5] };
            if fill {
                run.gs.fill_pattern = pat;
                run.gs.fill = rgb;
            } else {
                run.gs.stroke_pattern = pat;
                run.gs.stroke = rgb;
            }
            return;
        }
        let rgb = cs.to_rgb(&comps);
        if fill {
            run.gs.fill = rgb;
        } else {
            run.gs.stroke = rgb;
        }
        return;
    }
    // text
    if op == "BT" {
        run.tm = pdf_path::identity();
        run.tlm = pdf_path::identity();
        run.text_clip = Path::new();
        return;
    }
    if op == "ET" {
        if run.gs.mode >= 4 {
            let tc = run.text_clip.clone();
            // the glyphs are in device space already
            let id = pdf_path::identity();
            let rings = pdf_path::flatten(&tc, &id, r.tol);
            push_clip(run, rings, false);
        }
        run.text_clip = Path::new();
        return;
    }
    if op == "Tc" {
        run.gs.tc = num_at(st, 0);
        return;
    }
    if op == "Tw" {
        run.gs.tw = num_at(st, 0);
        return;
    }
    if op == "Tz" {
        run.gs.th = num_at(st, 0) / 100.0;
        return;
    }
    if op == "TL" {
        run.gs.tl = num_at(st, 0);
        return;
    }
    if op == "Ts" {
        run.gs.rise = num_at(st, 0);
        return;
    }
    if op == "Tr" {
        run.gs.mode = num_at(st, 0) as i64;
        return;
    }
    if op == "Tf" {
        if n >= 2 {
            let fname = st[0].clone();
            run.gs.fs = st[1].f();
            run.gs.font = font_index(doc, r, run, &fname, res);
        }
        return;
    }
    if op == "Td" || op == "TD" {
        if n >= 2 {
            let x = num_at(st, 0);
            let y = num_at(st, 1);
            if op == "TD" {
                run.gs.tl = 0.0 - y;
            }
            let t: Vec<f64> = vec![1.0, 0.0, 0.0, 1.0, x, y];
            run.tlm = pdf_path::mat_mul(&t, &run.tlm);
            run.tm = run.tlm.clone();
        }
        return;
    }
    if op == "Tm" {
        if n >= 6 {
            run.tlm = vec![num_at(st, 0), num_at(st, 1), num_at(st, 2), num_at(st, 3), num_at(st, 4), num_at(st, 5)];
            run.tm = run.tlm.clone();
        }
        return;
    }
    if op == "T*" {
        next_line(run);
        return;
    }
    if op == "Tj" {
        if n >= 1 {
            let s = st[n - 1].s.clone();
            show(doc, r, run, &s, res);
        }
        return;
    }
    if op == "'" {
        next_line(run);
        if n >= 1 {
            let s = st[n - 1].s.clone();
            show(doc, r, run, &s, res);
        }
        return;
    }
    if op == "\"" {
        if n >= 3 {
            run.gs.tw = st[0].f();
            run.gs.tc = st[1].f();
            next_line(run);
            let s = st[2].s.clone();
            show(doc, r, run, &s, res);
        }
        return;
    }
    if op == "TJ" {
        if n >= 1 && st[n - 1].k == K::Arr {
            let a = st[n - 1].clone();
            for x in a.items.iter() {
                if x.k == K::Str {
                    let s = x.s.clone();
                    show(doc, r, run, &s, res);
                } else if x.k == K::Num {
                    let adj = 0.0 - x.n / 1000.0 * run.gs.fs * run.gs.th;
                    let vertical = run.gs.font >= 0 && r.fonts[run.gs.font as usize].vertical;
                    let t: Vec<f64> = if vertical { vec![1.0, 0.0, 0.0, 1.0, 0.0, 0.0 - x.n / 1000.0 * run.gs.fs] } else { vec![1.0, 0.0, 0.0, 1.0, adj, 0.0] };
                    run.tm = pdf_path::mat_mul(&t, &run.tm);
                }
            }
        }
        return;
    }
    // XObjects, shadings, Type 3 metrics
    if op == "Do" {
        if n >= 1 && st[0].k == K::Name {
            do_xobject(doc, r, run, &st[0].name, res);
        }
        return;
    }
    if op == "sh" {
        if n >= 1 && st[0].k == K::Name {
            let shd = doc.get_key(res, "Shading");
            let sh = doc.get_key(&shd, &st[0].name);
            let ctm = run.gs.ctm.clone();
            let bx = run.gs.clip_box.clone();
            shade(doc, run, &sh, &ctm, &bx, res);
        }
        return;
    }
    // d0 d1 BMC BDC EMC MP DP BX EX: nothing to draw
}

fn next_line(run: &mut Run) {
    let t: Vec<f64> = vec![1.0, 0.0, 0.0, 1.0, 0.0, 0.0 - run.gs.tl];
    run.tlm = pdf_path::mat_mul(&t, &run.tlm);
    run.tm = run.tlm.clone();
}

fn color_cmd(op: i64, rgb: &Vec<f64>, a: f64) -> Cmd {
    let mut c = pdf_display::cmd(op);
    c.r = byte(rgb[0]);
    c.g = byte(rgb[1]);
    c.b = byte(rgb[2]);
    c.a = a;
    c
}

fn fill_path(doc: &mut Doc, r: &mut Renderer, run: &mut Run, p: &Path, eo: bool, res: &Obj) {
    if run.gs.fill_none {
        return;
    }
    let rings = pdf_path::flatten(p, &run.gs.ctm, r.tol);
    if rings.ends.is_empty() {
        return;
    }
    if !run.gs.fill_pattern.is_null() {
        let pat = run.gs.fill_pattern.clone();
        push_clip(run, rings, eo);
        paint_pattern(doc, r, run, &pat, res, true);
        pop_clip(run);
        return;
    }
    let mut c = color_cmd(pdf_display::FILL, &run.gs.fill, run.gs.fill_a);
    c.pts = rings.pts;
    c.ends = rings.ends;
    c.closed = rings.closed;
    c.even_odd = eo;
    run.out.cmds.push(c);
}

fn stroke_path(doc: &mut Doc, r: &mut Renderer, run: &mut Run, p: &Path, res: &Obj) {
    if run.gs.stroke_none {
        return;
    }
    let rings = pdf_path::flatten(p, &run.gs.ctm, r.tol);
    if rings.ends.is_empty() {
        return;
    }
    let sc = pdf_path::mat_scale(&run.gs.ctm);
    let mut w = run.gs.lw * sc;
    // a zero-width line is the thinnest line the device can draw
    if w < 0.25 {
        w = 0.25;
    }
    let mut c = color_cmd(pdf_display::STROKE, &run.gs.stroke, run.gs.stroke_a);
    if !run.gs.stroke_pattern.is_null() {
        // a stroke pattern: drawn with its average colour
        let pat = run.gs.stroke_pattern.clone();
        let rgb = pattern_color(doc, &pat, res);
        c = color_cmd(pdf_display::STROKE, &rgb, run.gs.stroke_a);
    }
    c.pts = rings.pts;
    c.ends = rings.ends;
    c.closed = rings.closed;
    c.width = w;
    c.cap = run.gs.cap;
    c.join = run.gs.join;
    c.miter = run.gs.miter;
    let mut dash: Vec<f64> = vec![];
    let mut all_zero = true;
    for d in run.gs.dash.iter() {
        dash.push(*d * sc);
        if *d > 0.0 {
            all_zero = false;
        }
    }
    if !all_zero {
        c.dash = dash;
        c.phase = run.gs.phase * sc;
    }
    run.out.cmds.push(c);
}

fn push_clip(run: &mut Run, rings: pdf_path::Rings, eo: bool) {
    let bb = rings.bbox();
    let mut c = pdf_display::cmd(pdf_display::CLIP);
    c.even_odd = eo;
    if rings.ends.is_empty() {
        // an empty clip hides everything
        c.pts = vec![0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        c.ends = vec![3];
        c.closed = vec![true];
        run.gs.clip_box = vec![0.0, 0.0, 0.0, 0.0];
    } else {
        c.pts = rings.pts;
        c.ends = rings.ends;
        c.closed = rings.closed;
        run.gs.clip_box = inter_box(&run.gs.clip_box, &bb);
    }
    run.out.cmds.push(c);
    run.gs.clips += 1;
}

fn pop_clip(run: &mut Run) {
    run.out.cmds.push(pdf_display::cmd(pdf_display::UNCLIP));
    run.gs.clips -= 1;
}

fn clip_path(run: &mut Run, p: &Path, eo: bool) {
    let rings = pdf_path::flatten(p, &run.gs.ctm, 0.2);
    push_clip(run, rings, eo);
}

fn ext_gstate(doc: &mut Doc, r: &mut Renderer, run: &mut Run, name: &str, res: &Obj) {
    let eg = doc.get_key(res, "ExtGState");
    let g = doc.get_key(&eg, name);
    if !g.is_dict() {
        return;
    }
    let mut i: usize = 0;
    while i < g.keys.len() {
        let k = g.keys[i].clone();
        let v = doc.resolve(&g.items[i]);
        if k == "LW" {
            run.gs.lw = v.f();
        } else if k == "LC" {
            run.gs.cap = v.i();
        } else if k == "LJ" {
            run.gs.join = v.i();
        } else if k == "ML" {
            run.gs.miter = v.f();
        } else if k == "D" && v.k == K::Arr && v.items.len() >= 2 {
            let d0 = doc.resolve(&v.items[0]);
            run.gs.dash = d0.nums();
            run.gs.phase = doc.resolve(&v.items[1]).f();
        } else if k == "CA" {
            run.gs.stroke_a = v.f();
        } else if k == "ca" {
            run.gs.fill_a = v.f();
        } else if k == "Font" && v.k == K::Arr && v.items.len() >= 2 {
            let fref = v.items[0].clone();
            run.gs.fs = doc.resolve(&v.items[1]).f();
            run.gs.font = load_font(doc, r, &fref);
        }
        i += 1;
    }
}

fn font_index(doc: &mut Doc, r: &mut Renderer, run: &mut Run, name: &Obj, res: &Obj) -> i64 {
    let fonts = doc.get_key(res, "Font");
    let fref = fonts.at(&name.name);
    if fref.is_null() {
        if !run.fontless {
            run.out.warn("a font that is not in the resources");
            run.fontless = true;
        }
        return -1;
    }
    load_font(doc, r, &fref)
}

fn load_font(doc: &mut Doc, r: &mut Renderer, fref: &Obj) -> i64 {
    let key = if fref.k == K::Ref { fref.i() } else { -1 };
    if key >= 0 {
        let mut i: usize = 0;
        while i < r.font_keys.len() {
            if r.font_keys[i] == key {
                return i as i64;
            }
            i += 1;
        }
    }
    let fd = doc.resolve(fref);
    if !fd.is_dict() {
        return -1;
    }
    let f = Font::load(doc, &r.glyphs, &r.files, &fd);
    r.fonts.push(f);
    r.font_keys.push(key);
    (r.fonts.len() - 1) as i64
}

fn show(doc: &mut Doc, r: &mut Renderer, run: &mut Run, s: &Vec<u8>, res: &Obj) {
    if run.gs.font < 0 {
        return;
    }
    let fi = run.gs.font as usize;
    let codes = r.fonts[fi].codes(&r.glyphs, s);
    let fs = run.gs.fs;
    let th = run.gs.th;
    let vertical = r.fonts[fi].vertical;
    let type3 = r.fonts[fi].subtype == "Type3";
    let mode = run.gs.mode;
    for c in codes.iter() {
        // the glyph's matrix: text space at size 1 -> device
        let tsm: Vec<f64> = vec![fs * th, 0.0, 0.0, fs, 0.0, run.gs.rise];
        let trm0 = pdf_path::mat_mul(&tsm, &run.tm);
        let trm = pdf_path::mat_mul(&trm0, &run.gs.ctm);
        let mut adv = c.width * fs + run.gs.tc;
        if c.space {
            adv += run.gs.tw;
        }
        // the character's box for the text layer
        let x0 = pdf_path::tx(&trm, 0.0, 0.0);
        let y0 = pdf_path::ty(&trm, 0.0, 0.0);
        let x1 = pdf_path::tx(&trm, c.width, 0.0);
        let y1 = pdf_path::ty(&trm, c.width, 0.0);
        let hgt = pdf_path::mat_scale(&trm);
        if !c.text.is_empty() {
            let mut dir: i64 = 0;
            if pdf_path::fabs(x1 - x0) >= pdf_path::fabs(y1 - y0) {
                if x1 < x0 {
                    dir = 180;
                }
            } else if y1 > y0 {
                dir = 90;
            } else {
                dir = 270;
            }
            let wd = if dir == 0 || dir == 180 { pdf_path::fabs(x1 - x0) } else { pdf_path::fabs(y1 - y0) };
            if !run.annot {
                run.out.text.push(TextItem { text: c.text.clone(), x: pdf_path::fmin(x0, x1), y: y0 - hgt, w: wd, h: hgt, dir: dir });
            }
        }
        if mode != 3 && mode != 7 {
            if type3 {
                type3_glyph(doc, r, run, fi, c.code, &trm0, res);
            } else if r.fonts[fi].has_outlines() {
                let g = r.fonts[fi].outline(c.code);
                if !g.is_empty() {
                    glyph(r, run, &g, &trm, mode);
                }
            } else if c.code != 32 || !c.text.is_empty() && c.text != " " {
                let f = &r.fonts[fi];
                let mut cmd = color_cmd(pdf_display::TEXT, &run.gs.fill, run.gs.fill_a);
                cmd.text = c.text.clone();
                cmd.family = f.family.clone();
                cmd.bold = f.bold;
                cmd.italic = f.italic;
                cmd.m = trm.clone();
                cmd.width = c.width;
                if !cmd.text.is_empty() && cmd.text != " " && !run.gs.fill_none {
                    run.out.cmds.push(cmd);
                }
            }
        }
        let t: Vec<f64> = if vertical { vec![1.0, 0.0, 0.0, 1.0, 0.0, 0.0 - adv] } else { vec![1.0, 0.0, 0.0, 1.0, adv * th, 0.0] };
        run.tm = pdf_path::mat_mul(&t, &run.tm);
    }
}

fn glyph(r: &mut Renderer, run: &mut Run, g: &Path, trm: &Vec<f64>, mode: i64) {
    let fill = mode == 0 || mode == 2 || mode == 4 || mode == 6;
    let stroke = mode == 1 || mode == 2 || mode == 5 || mode == 6;
    if fill && !run.gs.fill_none {
        let rings = pdf_path::flatten(g, trm, r.tol);
        if !rings.ends.is_empty() {
            let mut c = color_cmd(pdf_display::FILL, &run.gs.fill, run.gs.fill_a);
            c.pts = rings.pts;
            c.ends = rings.ends;
            c.closed = rings.closed;
            run.out.cmds.push(c);
        }
    }
    if stroke && !run.gs.stroke_none {
        let rings = pdf_path::flatten(g, trm, r.tol);
        let mut c = color_cmd(pdf_display::STROKE, &run.gs.stroke, run.gs.stroke_a);
        c.pts = rings.pts;
        c.ends = rings.ends;
        c.closed = rings.closed;
        let w = run.gs.lw * pdf_path::mat_scale(&run.gs.ctm);
        c.width = if w < 0.25 { 0.25 } else { w };
        run.out.cmds.push(c);
    }
    if mode >= 4 {
        run.text_clip.append(g, trm);
    }
}

fn type3_glyph(doc: &mut Doc, r: &mut Renderer, run: &mut Run, fi: usize, code: i64, trm0: &Vec<f64>, res: &Obj) {
    if code < 0 || code > 255 {
        return;
    }
    let name = r.fonts[fi].enc[code as usize].clone();
    let procs = r.fonts[fi].procs.clone();
    let proc_ref = procs.at(&name);
    let pnum = if proc_ref.k == K::Ref { proc_ref.i() } else { -1 };
    if pnum >= 0 && run.forms.contains(&pnum) {
        return;
    }
    let proc = doc.resolve(&proc_ref);
    if proc.k != K::Stream {
        return;
    }
    let data = doc.stream_data(&proc);
    let fm = r.fonts[fi].matrix.clone();
    let mut fres = r.fonts[fi].resources.clone();
    if !fres.is_dict() {
        fres = res.clone();
    }
    let saved = run.gs.clone();
    run.stack.push(saved);
    run.gs.clips = 0;
    // glyph space -> text space (font matrix), then the text rendering matrix
    let m = pdf_path::mat_mul(&fm, trm0);
    run.gs.ctm = pdf_path::mat_mul(&m, &run.gs.ctm);
    let save_tm = run.tm.clone();
    let save_tlm = run.tlm.clone();
    let save_path = run.path.clone();
    run.path = Path::new();
    if pnum >= 0 {
        run.forms.push(pnum);
    }
    run_content(doc, r, run, data, &fres);
    if pnum >= 0 {
        let nf = run.forms.len() - 1;
        run.forms.truncate(nf);
    }
    restore(run);
    run.tm = save_tm;
    run.tlm = save_tlm;
    run.path = save_path;
}

fn do_xobject(doc: &mut Doc, r: &mut Renderer, run: &mut Run, name: &str, res: &Obj) {
    let xs = doc.get_key(res, "XObject");
    let xref = xs.at(name);
    let x = doc.resolve(&xref);
    if x.k != K::Stream {
        return;
    }
    let st = doc.get_key(&x, "Subtype").name.clone();
    if st == "Image" {
        draw_image(doc, run, &x, res);
        return;
    }
    if st != "Form" {
        return;
    }
    let none: Vec<f64> = vec![];
    draw_form(doc, r, run, &xref, &x, res, &none);
}

/// Form XObject `x` (`xref` its reference); `pre` maps form space after its
/// /Matrix (an annotation's appearance), empty for none.
fn draw_form(doc: &mut Doc, r: &mut Renderer, run: &mut Run, xref: &Obj, x: &Obj, res: &Obj, pre: &Vec<f64>) {
    let num = if xref.k == K::Ref { xref.i() } else { -1 };
    if num >= 0 && run.forms.contains(&num) {
        return;
    }
    let data = doc.stream_data(x);
    let mut fres = doc.get_key(x, "Resources");
    if !fres.is_dict() {
        fres = res.clone();
    }
    let saved = run.gs.clone();
    run.stack.push(saved);
    run.gs.clips = 0;
    if pre.len() == 6 {
        run.gs.ctm = pdf_path::mat_mul(pre, &run.gs.ctm);
    }
    let fm = doc.get_key(x, "Matrix").nums();
    if fm.len() == 6 {
        run.gs.ctm = pdf_path::mat_mul(&fm, &run.gs.ctm);
    }
    let bb = doc.get_key(x, "BBox").nums();
    if bb.len() == 4 {
        let mut p = Path::new();
        p.rect(bb[0], bb[1], bb[2] - bb[0], bb[3] - bb[1]);
        clip_path(run, &p, false);
    }
    // the form's own group alpha is not composited separately
    if num >= 0 {
        run.forms.push(num);
    }
    let save_path = run.path.clone();
    run.path = Path::new();
    run_content(doc, r, run, data, &fres);
    run.path = save_path;
    if num >= 0 {
        let nf = run.forms.len() - 1;
        run.forms.truncate(nf);
    }
    restore(run);
}

fn draw_image(doc: &mut Doc, run: &mut Run, x: &Obj, res: &Obj) {
    let fill = run.gs.fill.clone();
    let img = pdf_image::decode(doc, x, res, &fill, run.gs.fill_a);
    if !img.ok {
        if !img.unsupported.is_empty() {
            run.out.warn(&format!("image codec {} not decoded", img.unsupported));
        }
        return;
    }
    let idx = run.out.images.len() as i64;
    run.out.images.push(img);
    let mut c = pdf_display::cmd(pdf_display::IMAGE);
    c.image = idx;
    c.m = run.gs.ctm.clone();
    c.a = run.gs.fill_a;
    run.out.cmds.push(c);
}

fn expand_inline_key(k: &str) -> String {
    if k == "W" {
        return "Width".to_string();
    }
    if k == "H" {
        return "Height".to_string();
    }
    if k == "BPC" {
        return "BitsPerComponent".to_string();
    }
    if k == "CS" {
        return "ColorSpace".to_string();
    }
    if k == "D" {
        return "Decode".to_string();
    }
    if k == "DP" {
        return "DecodeParms".to_string();
    }
    if k == "F" {
        return "Filter".to_string();
    }
    if k == "IM" {
        return "ImageMask".to_string();
    }
    if k == "I" {
        return "Interpolate".to_string();
    }
    k.to_string()
}

fn expand_inline_value(v: &Obj) -> Obj {
    if v.k == K::Name {
        let n = v.name.clone();
        let full = if n == "G" {
            "DeviceGray"
        } else if n == "RGB" {
            "DeviceRGB"
        } else if n == "CMYK" {
            "DeviceCMYK"
        } else if n == "I" {
            "Indexed"
        } else if n == "AHx" {
            "ASCIIHexDecode"
        } else if n == "A85" {
            "ASCII85Decode"
        } else if n == "LZW" {
            "LZWDecode"
        } else if n == "Fl" {
            "FlateDecode"
        } else if n == "RL" {
            "RunLengthDecode"
        } else if n == "CCF" {
            "CCITTFaxDecode"
        } else if n == "DCT" {
            "DCTDecode"
        } else {
            ""
        };
        if !full.is_empty() {
            return pdf_obj::name(full);
        }
        return v.clone();
    }
    if v.k == K::Arr {
        let mut a = pdf_obj::arr();
        for x in v.items.iter() {
            a.push(expand_inline_value(x));
        }
        return a;
    }
    v.clone()
}

fn inline_image(doc: &mut Doc, r: &mut Renderer, run: &mut Run, lx: &mut PdfLexer, res: &Obj) {
    let _ = r;
    let mut d = pdf_obj::dict();
    loop {
        let k = lx.token();
        if k.k == K::Eof {
            return;
        }
        if k.is_cmd("ID") {
            break;
        }
        if k.k == K::Name {
            let v = lx.object(0);
            d.set(&expand_inline_key(&k.name), expand_inline_value(&v));
        }
    }
    // one white-space byte after ID
    lx.pos += 1;
    let start = lx.pos;
    let buf_len = lx.buf.len();
    // without a filter the length is known; otherwise look for EI between white space
    let w = d.at("Width").i();
    let h = d.at("Height").i();
    let mut bpc = d.at("BitsPerComponent").i();
    let im = d.at("ImageMask").k == K::Bool && d.at("ImageMask").n == 1.0;
    if bpc <= 0 || im {
        bpc = if im { 1 } else { 8 };
    }
    let mut end = buf_len;
    let filt = d.at("Filter");
    let mut known = false;
    if filt.is_null() && w > 0 && h > 0 {
        let csn = d.at("ColorSpace");
        let mut nc: i64 = 1;
        if csn.is_name("DeviceRGB") || csn.is_name("CalRGB") {
            nc = 3;
        } else if csn.is_name("DeviceCMYK") {
            nc = 4;
        } else if csn.k == K::Name && !csn.is_name("DeviceGray") && !csn.is_name("Indexed") {
            let cs = pdf_color::read(doc, &csn, res, 0);
            nc = cs.n as i64;
        }
        if im {
            nc = 1;
        }
        let len = ((w * nc * bpc + 7) / 8 * h) as usize;
        if start + len <= buf_len {
            end = start + len;
            known = true;
        }
    }
    if !known {
        let mut p = start;
        while p + 2 <= buf_len {
            if lx.buf[p] == 69 && lx.buf[p + 1] == 73 && p > start && crate::pdf_lexer::is_ws(lx.buf[p - 1]) && (p + 2 == buf_len || crate::pdf_lexer::is_ws(lx.buf[p + 2])) {
                end = p - 1;
                break;
            }
            p += 1;
        }
    }
    let mut data: Vec<u8> = vec![];
    let mut i = start;
    while i < end {
        data.push(lx.buf[i]);
        i += 1;
    }
    lx.pos = end;
    // to after EI
    loop {
        let t = lx.token();
        if t.k == K::Eof || t.is_cmd("EI") {
            break;
        }
    }
    let mut s = d.clone();
    s.k = K::Stream;
    s.s = data;
    draw_image(doc, run, &s, res);
}

// shadings and patterns

fn pattern_color(doc: &mut Doc, pat: &Obj, res: &Obj) -> Vec<f64> {
    let pd = doc.get_key(res, "Pattern");
    let p = doc.get_key(&pd, &pat.name);
    if !p.is_dict() {
        return vec![0.5, 0.5, 0.5];
    }
    if doc.get_key(&p, "PatternType").i() == 2 {
        let sh = doc.get_key(&p, "Shading");
        return shading_color_at(doc, &sh, res, 0.5);
    }
    vec![0.5, 0.5, 0.5]
}

fn shading_color_at(doc: &mut Doc, sh: &Obj, res: &Obj, t: f64) -> Vec<f64> {
    let cso = doc.get_key(sh, "ColorSpace");
    let cs = pdf_color::read(doc, &cso, res, 0);
    let f = pdf_func::read(doc, &sh.at("Function"), 0);
    if f.ok {
        let input: Vec<f64> = vec![t];
        let c = f.eval(&input);
        return cs.to_rgb(&c);
    }
    let bg = doc.get_key(sh, "Background").nums();
    if !bg.is_empty() {
        return cs.to_rgb(&bg);
    }
    vec![0.5, 0.5, 0.5]
}

fn paint_pattern(doc: &mut Doc, r: &mut Renderer, run: &mut Run, pat: &Obj, res: &Obj, fill: bool) {
    let _ = fill;
    let pd = doc.get_key(res, "Pattern");
    let pref = pd.at(&pat.name);
    let p = doc.resolve(&pref);
    if !p.is_dict() {
        return;
    }
    let pm = doc.get_key(&p, "Matrix").nums();
    let pmat = if pm.len() == 6 { pm } else { pdf_path::identity() };
    // a pattern's space is the default space of the page (or form), not the current CTM
    let base = run_base(run);
    let m = pdf_path::mat_mul(&pmat, &base);
    let bx = run.gs.clip_box.clone();
    let ptype = doc.get_key(&p, "PatternType").i();
    if ptype == 2 {
        let sh = doc.get_key(&p, "Shading");
        shade(doc, run, &sh, &m, &bx, res);
        return;
    }
    if ptype == 1 && p.k == K::Stream {
        tile(doc, r, run, &p, &pref, &m, &bx, res);
    }
}

/// The matrix of the page (the bottom of the state stack).
fn run_base(run: &Run) -> Vec<f64> {
    if !run.stack.is_empty() {
        let g0 = run.stack[0].clone();
        return g0.ctm;
    }
    run.gs.ctm.clone()
}

fn tile(doc: &mut Doc, r: &mut Renderer, run: &mut Run, p: &Obj, pref: &Obj, m: &Vec<f64>, bx: &Vec<f64>, res: &Obj) {
    let num = if pref.k == K::Ref { pref.i() } else { -1 };
    if num >= 0 && run.forms.contains(&num) {
        return;
    }
    let bb = doc.get_key(p, "BBox").nums();
    let xs = doc.get_key(p, "XStep").f();
    let ys = doc.get_key(p, "YStep").f();
    if bb.len() < 4 || xs == 0.0 || ys == 0.0 {
        return;
    }
    // the tiles that meet the box: from the box's corners in pattern space
    let inv = pdf_path::invert(m);
    let cx: Vec<f64> = vec![bx[0], bx[2], bx[2], bx[0]];
    let cy: Vec<f64> = vec![bx[1], bx[1], bx[3], bx[3]];
    let mut u0 = 1.0e30;
    let mut u1 = -1.0e30;
    let mut v0 = 1.0e30;
    let mut v1 = -1.0e30;
    let mut i: usize = 0;
    while i < 4 {
        let u = pdf_path::tx(&inv, cx[i], cy[i]);
        let v = pdf_path::ty(&inv, cx[i], cy[i]);
        u0 = pdf_path::fmin(u0, u);
        u1 = pdf_path::fmax(u1, u);
        v0 = pdf_path::fmin(v0, v);
        v1 = pdf_path::fmax(v1, v);
        i += 1;
    }
    let axs = pdf_path::fabs(xs);
    let ays = pdf_path::fabs(ys);
    let i0 = ((u0 - bb[2]) / axs).floor() as i64;
    let i1 = ((u1 - bb[0]) / axs).ceil() as i64;
    let j0 = ((v0 - bb[3]) / ays).floor() as i64;
    let j1 = ((v1 - bb[1]) / ays).ceil() as i64;
    let count = (i1 - i0 + 1) * (j1 - j0 + 1);
    let paint_type = doc.get_key(p, "PaintType").i();
    if count > 4000 || count <= 0 {
        // too many tiles: the box in an average colour
        let mut c = color_cmd(pdf_display::FILL, &vec![0.75, 0.75, 0.75], run.gs.fill_a);
        if paint_type == 2 {
            c = color_cmd(pdf_display::FILL, &run.gs.fill, run.gs.fill_a);
        }
        c.pts = vec![bx[0], bx[1], bx[2], bx[1], bx[2], bx[3], bx[0], bx[3]];
        c.ends = vec![4];
        c.closed = vec![true];
        run.out.cmds.push(c);
        return;
    }
    let data = doc.stream_data(p);
    let mut pres = doc.get_key(p, "Resources");
    if !pres.is_dict() {
        pres = res.clone();
    }
    if num >= 0 {
        run.forms.push(num);
    }
    let save_path = run.path.clone();
    let fill_rgb = run.gs.fill.clone();
    let mut j = j0;
    while j <= j1 {
        let mut ii = i0;
        while ii <= i1 {
            let saved = run.gs.clone();
            run.stack.push(saved);
            run.gs.clips = 0;
            let t: Vec<f64> = vec![1.0, 0.0, 0.0, 1.0, (ii as f64) * xs, (j as f64) * ys];
            run.gs.ctm = pdf_path::mat_mul(&t, m);
            run.gs.fill_pattern = pdf_obj::null();
            run.gs.stroke_pattern = pdf_obj::null();
            if paint_type == 2 {
                // uncoloured: the pattern draws in the colour that was set
                run.gs.fill = fill_rgb.clone();
            }
            let mut cp = Path::new();
            cp.rect(bb[0], bb[1], bb[2] - bb[0], bb[3] - bb[1]);
            clip_path(run, &cp, false);
            run.path = Path::new();
            run_content(doc, r, run, data.clone(), &pres);
            restore(run);
            ii += 1;
        }
        j += 1;
    }
    run.path = save_path;
    if num >= 0 {
        let nf = run.forms.len() - 1;
        run.forms.truncate(nf);
    }
}

fn shade(doc: &mut Doc, run: &mut Run, sh0: &Obj, m: &Vec<f64>, bx: &Vec<f64>, res: &Obj) {
    let sh = doc.resolve(sh0);
    if !sh.is_dict() {
        return;
    }
    let st = doc.get_key(&sh, "ShadingType").i();
    let cso = doc.get_key(&sh, "ColorSpace");
    let cs = pdf_color::read(doc, &cso, res, 0);
    let fo = sh.at("Function");
    let f = pdf_func::read(doc, &fo, 0);
    let coords = doc.get_key(&sh, "Coords").nums();
    let mut dom = doc.get_key(&sh, "Domain").nums();
    if dom.len() < 2 {
        dom = vec![0.0, 1.0];
    }
    let ext = doc.get_key(&sh, "Extend");
    let e0 = ext.k == K::Arr && ext.item(0).n == 1.0;
    let e1 = ext.k == K::Arr && ext.item(1).n == 1.0;
    let a = run.gs.fill_a;
    if (st == 2 || st == 3) && f.ok && coords.len() >= 4 {
        let steps: i64 = 96;
        if st == 2 {
            // axial: bands across the axis, wide enough for the box
            let x0 = pdf_path::tx(m, coords[0], coords[1]);
            let y0 = pdf_path::ty(m, coords[0], coords[1]);
            let x1 = pdf_path::tx(m, coords[2], coords[3]);
            let y1 = pdf_path::ty(m, coords[2], coords[3]);
            let dx = x1 - x0;
            let dy = y1 - y0;
            let len = (dx * dx + dy * dy).sqrt();
            if len <= 0.0 {
                return;
            }
            let ux = dx / len;
            let uy = dy / len;
            let diag = ((bx[2] - bx[0]) * (bx[2] - bx[0]) + (bx[3] - bx[1]) * (bx[3] - bx[1])).sqrt() + len + 10.0;
            // the box's extent along the axis, in t
            let mut tmin = 0.0;
            let mut tmax = 1.0;
            let cxs: Vec<f64> = vec![bx[0], bx[2], bx[2], bx[0]];
            let cys: Vec<f64> = vec![bx[1], bx[1], bx[3], bx[3]];
            let mut lo = 1.0e30;
            let mut hi = -1.0e30;
            let mut i: usize = 0;
            while i < 4 {
                let t = ((cxs[i] - x0) * ux + (cys[i] - y0) * uy) / len;
                lo = pdf_path::fmin(lo, t);
                hi = pdf_path::fmax(hi, t);
                i += 1;
            }
            if e0 {
                tmin = pdf_path::fmin(0.0, lo);
            }
            if e1 {
                tmax = pdf_path::fmax(1.0, hi);
            }
            let mut k: i64 = -1;
            while k <= steps {
                let mut ta = (k as f64) / (steps as f64);
                let mut tb = ((k + 1) as f64) / (steps as f64);
                if k == -1 {
                    if !e0 || tmin >= 0.0 {
                        k += 1;
                        continue;
                    }
                    ta = tmin;
                    tb = 0.0;
                }
                if k == steps {
                    if !e1 || tmax <= 1.0 {
                        break;
                    }
                    ta = 1.0;
                    tb = tmax;
                }
                let tm = if k == -1 { 0.0 } else if k == steps { 1.0 } else { (ta + tb) / 2.0 };
                let tv = dom[0] + tm * (dom[1] - dom[0]);
                let input: Vec<f64> = vec![tv];
                let rgb = cs.to_rgb(&f.eval(&input));
                // a little overlap hides seams
                let pa = ta * len - 0.3;
                let pb = tb * len + 0.3;
                let ax = x0 + ux * pa;
                let ay = y0 + uy * pa;
                let bxp = x0 + ux * pb;
                let byp = y0 + uy * pb;
                let nx = 0.0 - uy * diag;
                let ny = ux * diag;
                let mut c = color_cmd(pdf_display::FILL, &rgb, a);
                c.pts = vec![ax + nx, ay + ny, bxp + nx, byp + ny, bxp - nx, byp - ny, ax - nx, ay - ny];
                c.ends = vec![4];
                c.closed = vec![true];
                run.out.cmds.push(c);
                k += 1;
            }
            return;
        }
        if coords.len() >= 6 {
            // radial: circles from the outermost in
            let sc = pdf_path::mat_scale(m);
            let mut k: i64 = steps;
            if e1 {
                let input: Vec<f64> = vec![dom[1]];
                let rgb = cs.to_rgb(&f.eval(&input));
                let diag = ((bx[2] - bx[0]) * (bx[2] - bx[0]) + (bx[3] - bx[1]) * (bx[3] - bx[1])).sqrt();
                let cx = pdf_path::tx(m, coords[3], coords[4]);
                let cy = pdf_path::ty(m, coords[3], coords[4]);
                let rr = coords[5] * sc + diag * 2.0;
                circle(run, cx, cy, rr, &rgb, a);
            }
            while k >= 0 {
                let t = (k as f64) / (steps as f64);
                let cx0 = coords[0] + t * (coords[3] - coords[0]);
                let cy0 = coords[1] + t * (coords[4] - coords[1]);
                let rr = (coords[2] + t * (coords[5] - coords[2])) * sc;
                let tv = dom[0] + t * (dom[1] - dom[0]);
                let input: Vec<f64> = vec![tv];
                let rgb = cs.to_rgb(&f.eval(&input));
                if rr > 0.0 {
                    circle(run, pdf_path::tx(m, cx0, cy0), pdf_path::ty(m, cx0, cy0), rr, &rgb, a);
                }
                k -= 1;
            }
            return;
        }
    }
    // function-based and mesh shadings: one colour over the box
    let rgb = shading_color_at(doc, &sh, res, 0.5);
    let mut c = color_cmd(pdf_display::FILL, &rgb, a);
    c.pts = vec![bx[0], bx[1], bx[2], bx[1], bx[2], bx[3], bx[0], bx[3]];
    c.ends = vec![4];
    c.closed = vec![true];
    run.out.cmds.push(c);
    run.out.warn(&format!("shading type {} drawn in one colour", st));
}

fn circle(run: &mut Run, cx: f64, cy: f64, rr: f64, rgb: &Vec<f64>, a: f64) {
    let mut c = color_cmd(pdf_display::FILL, rgb, a);
    let n = 48;
    let mut i = 0;
    while i < n {
        let ang = (i as f64) * 6.283185307179586 / (n as f64);
        c.pts.push(cx + rr * ang.cos());
        c.pts.push(cy + rr * ang.sin());
        i += 1;
    }
    c.ends = vec![n as i64];
    c.closed = vec![true];
    run.out.cmds.push(c);
}
