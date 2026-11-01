//! The reader as a WebAssembly module for the online viewer (web/pdf-worker.js).
//!
//! Plain C ABI, no imports: the host copies bytes in with `pv_alloc`, calls a
//! function, and reads the answer from `pv_out_ptr` / `pv_out_len` (JSON, or
//! RGBA pixels after `pv_render`). One document is open at a time; each
//! page's text is kept once it is rendered, so search and the text layer
//! do not render it again.

use pdf::pdf::{find_in, PdfDocument};
use pdf::pdf_display::{PageOut, TextItem};
use pdf::pdf_font::FontFiles;
use pdf::pdf_picture;
use pdf::pdf_raster;
use std::collections::HashMap;

struct State {
    fonts: FontFiles,
    doc: Option<PdfDocument>,
    /// the text of each page rendered so far
    text: HashMap<i64, PageOut>,
    out: Vec<u8>,
    out_w: i64,
    out_h: i64,
}

static mut STATE: Option<State> = None;

#[allow(static_mut_refs)]
fn st() -> &'static mut State {
    unsafe {
        if STATE.is_none() {
            STATE = Some(State { fonts: FontFiles::new(), doc: None, text: HashMap::new(), out: vec![], out_w: 0, out_h: 0 });
        }
        STATE.as_mut().unwrap()
    }
}

fn take(ptr: *mut u8, len: usize) -> Vec<u8> {
    if ptr.is_null() || len == 0 {
        return vec![];
    }
    unsafe { Vec::from_raw_parts(ptr, len, len) }
}

fn text_of(ptr: *mut u8, len: usize) -> String {
    String::from_utf8_lossy(&take(ptr, len)).into_owned()
}

fn esc(s: &str, o: &mut String) {
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
}

fn num(x: f64, o: &mut String) {
    if x.is_finite() {
        o.push_str(&format!("{}", (x * 100.0).round() / 100.0));
    } else {
        o.push('0');
    }
}

fn items_json(items: &Vec<TextItem>, o: &mut String) {
    o.push('[');
    for (k, t) in items.iter().enumerate() {
        if k > 0 {
            o.push(',');
        }
        o.push('[');
        num(t.x, o);
        o.push(',');
        num(t.y, o);
        o.push(',');
        num(t.w, o);
        o.push(',');
        num(t.h, o);
        o.push(',');
        o.push_str(&t.dir.to_string());
        o.push(',');
        esc(&t.text, o);
        o.push(']');
    }
    o.push(']');
}

fn answer(s: String) -> usize {
    let st = st();
    st.out = s.into_bytes();
    st.out.len()
}

/// Keeps a rendered page's text (not its drawing, which can be large).
fn keep(i: i64, p: &PageOut) {
    let mut t = PageOut::new(p.width, p.height);
    t.text = p.text.clone();
    st().text.insert(i, t);
}

fn kept(i: i64) -> Option<&'static PageOut> {
    let st = st();
    if !st.text.contains_key(&i) {
        let p = st.doc.as_mut()?.render(i);
        keep(i, &p);
    }
    st.text.get(&i)
}

/// `len` bytes for the host to fill; the next call that takes them owns them.
#[no_mangle]
pub extern "C" fn pv_alloc(len: usize) -> *mut u8 {
    let mut v: Vec<u8> = Vec::with_capacity(len.max(1));
    let p = v.as_mut_ptr();
    std::mem::forget(v);
    p
}

#[no_mangle]
pub extern "C" fn pv_out_ptr() -> *const u8 {
    st().out.as_ptr()
}

#[no_mangle]
pub extern "C" fn pv_out_len() -> usize {
    st().out.len()
}

#[no_mangle]
pub extern "C" fn pv_out_w() -> i32 {
    st().out_w as i32
}

#[no_mangle]
pub extern "C" fn pv_out_h() -> i32 {
    st().out_h as i32
}

/// A substitute font (keys as FontFiles: sans, serif-bold, mono, symbol, ...).
#[no_mangle]
pub extern "C" fn pv_font(key: *mut u8, key_len: usize, data: *mut u8, len: usize) {
    let k = text_of(key, key_len);
    let d = take(data, len);
    st().fonts.add(&k, d);
}

/// Opens a document; the answer is JSON {pages, title, error, sizes:[[w,h]..]}
/// with page sizes in points after /Rotate. Returns the page count.
#[no_mangle]
pub extern "C" fn pv_open(data: *mut u8, len: usize, pw: *mut u8, pw_len: usize) -> i32 {
    let bytes = take(data, len);
    let password = text_of(pw, pw_len);
    let st = st();
    st.text.clear();
    let fonts = st.fonts.clone();
    let mut d = PdfDocument::open_with_fonts(bytes, &password, fonts);
    let n = d.page_count();
    let mut o = String::from("{\"pages\":");
    o.push_str(&n.to_string());
    o.push_str(",\"error\":");
    esc(&d.error(), &mut o);
    o.push_str(",\"title\":");
    let t = if n > 0 { d.title() } else { String::new() };
    esc(&t, &mut o);
    o.push_str(",\"sizes\":[");
    let mut i = 0;
    while i < n {
        if i > 0 {
            o.push(',');
        }
        o.push('[');
        num(d.page_width(i), &mut o);
        o.push(',');
        num(d.page_height(i), &mut o);
        o.push(']');
        i += 1;
    }
    o.push_str("]}");
    st.doc = Some(d);
    answer(o);
    n as i32
}

/// Page `i` as RGBA at `scale` pixels per point, turned `quarter` quarter
/// turns clockwise: the pixels in the answer, their size in pv_out_w/h.
#[no_mangle]
pub extern "C" fn pv_render(page: i32, scale: f64, quarter: i32) -> usize {
    let i = page as i64;
    let p = match st().doc.as_mut() {
        Some(d) if i >= 0 && i < d.page_count() => d.render(i),
        _ => return 0,
    };
    keep(i, &p);
    let r = pdf_raster::rasterize(&p, scale);
    let t = pdf_picture::rotate(&pdf_picture::made("", r.w, r.h, r.px), quarter as i64);
    let st = st();
    st.out_w = t.w;
    st.out_h = t.h;
    st.out = t.rgba;
    st.out.len()
}

/// Page `i`'s characters with their boxes in points (top-left origin):
/// JSON [[x,y,w,h,dir,"text"],..].
#[no_mangle]
pub extern "C" fn pv_text(page: i32) -> usize {
    let i = page as i64;
    let mut o = String::new();
    match kept(i) {
        Some(p) => items_json(&p.text, &mut o),
        None => o.push_str("[]"),
    }
    answer(o)
}

/// Page `i`'s plain text.
#[no_mangle]
pub extern "C" fn pv_plain(page: i32) -> usize {
    let i = page as i64;
    let s = match kept(i) {
        Some(p) => p.plain_text(),
        None => String::new(),
    };
    answer(s)
}

/// The matches of the query (case-insensitive) on page `i`: boxes as pv_text.
#[no_mangle]
pub extern "C" fn pv_find(page: i32, q: *mut u8, q_len: usize) -> usize {
    let i = page as i64;
    let want = text_of(q, q_len);
    let mut o = String::new();
    match kept(i) {
        Some(p) => items_json(&find_in(p, &want), &mut o),
        None => o.push_str("[]"),
    }
    answer(o)
}

/// Bookmarks: JSON [[level, page, "title"],..].
#[no_mangle]
pub extern "C" fn pv_outline() -> usize {
    let mut o = String::from("[");
    if let Some(d) = st().doc.as_mut() {
        for (k, it) in d.outline().iter().enumerate() {
            if k > 0 {
                o.push(',');
            }
            o.push('[');
            o.push_str(&it.level.to_string());
            o.push(',');
            o.push_str(&it.page.to_string());
            o.push(',');
            esc(&it.title, &mut o);
            o.push(']');
        }
    }
    o.push(']');
    answer(o)
}

/// Forgets the open document's kept pages (memory), not the document.
#[no_mangle]
pub extern "C" fn pv_forget() {
    st().text.clear();
}
