//! Picture files to RGBA for the preview: PNG, JPEG, BMP, GIF (the first
//! frame) and TIFF (the first page). The format is told from the bytes, not
//! the file name.

use crate::pdf_gif;
use crate::pdf_inflate;
use crate::pdf_jpeg;
use crate::pdf_png;
use crate::pdf_tiff;

pub struct PicImage {
    pub ok: bool,
    pub error: String,
    /// "png", "jpeg", "bmp", "gif", "tiff", or "" when unknown
    pub kind: String,
    pub w: i64,
    pub h: i64,
    /// w * h * 4 bytes, not premultiplied
    pub rgba: Vec<u8>,
}

impl PicImage {
    /// A picture file's bytes held as ints (hosts that keep bytes so).
    pub fn open_ints(data: &Vec<i64>) -> PicImage {
        let mut b: Vec<u8> = vec![];
        for x in data.iter() {
            b.push((*x & 255) as u8);
        }
        decode(&b)
    }

    /// An icon file's (.ico) entry nearest `size` pixels wide: the smallest
    /// at least that wide, else the largest.
    pub fn open_icon_ints(data: &Vec<i64>, size: i64) -> PicImage {
        let mut b: Vec<u8> = vec![];
        for x in data.iter() {
            b.push((*x & 255) as u8);
        }
        decode_ico(&b, size)
    }

    /// The picture as a PNG file at most `max_w` pixels wide (a larger one
    /// is scaled down by averaging), as ints.
    pub fn png_ints(&self, max_w: i64) -> Vec<i64> {
        self.png_ints_turned(max_w, 0)
    }

    /// The same, turned clockwise by `quarter` quarter turns; `max_w` is the
    /// width after turning.
    pub fn png_ints_turned(&self, max_w: i64, quarter: i64) -> Vec<i64> {
        let odd = (quarter % 2) != 0;
        let mut fit = max_w;
        if odd && self.h > 0 {
            // the source's height becomes the width
            fit = if self.h <= max_w { self.w } else { (self.w * max_w + self.h / 2) / self.h };
        }
        let s = rotate(&shrink(self, fit), quarter);
        let f = pdf_png::encode_rgba(s.w, s.h, &s.rgba);
        let mut out: Vec<i64> = vec![];
        for b in f.iter() {
            out.push(*b as i64);
        }
        out
    }
}

/// The picture turned clockwise by `quarter` quarter turns (0 to 3).
pub fn rotate(p: &PicImage, quarter: i64) -> PicImage {
    let q = ((quarter % 4) + 4) % 4;
    if !p.ok || q == 0 {
        return made(&p.kind, p.w, p.h, p.rgba.clone());
    }
    let w = if q == 2 { p.w } else { p.h };
    let h = if q == 2 { p.h } else { p.w };
    let mut out: Vec<u8> = vec![0; (w * h * 4) as usize];
    let mut y: i64 = 0;
    while y < h {
        let mut x: i64 = 0;
        while x < w {
            // the source pixel that lands at (x, y)
            let sx = if q == 1 { y } else if q == 2 { p.w - 1 - x } else { p.w - 1 - y };
            let sy = if q == 1 { p.h - 1 - x } else if q == 2 { p.h - 1 - y } else { x };
            let from = ((sy * p.w + sx) * 4) as usize;
            let to = ((y * w + x) * 4) as usize;
            out[to] = p.rgba[from];
            out[to + 1] = p.rgba[from + 1];
            out[to + 2] = p.rgba[from + 2];
            out[to + 3] = p.rgba[from + 3];
            x += 1;
        }
        y += 1;
    }
    made(&p.kind, w, h, out)
}

/// The picture scaled down to `max_w` pixels wide by averaging the source
/// pixels each target pixel covers, alpha-weighted; as it is when narrower.
pub fn shrink(p: &PicImage, max_w: i64) -> PicImage {
    if !p.ok || max_w <= 0 || p.w <= max_w {
        return made(&p.kind, p.w, p.h, p.rgba.clone());
    }
    let w = max_w;
    let mut h = (p.h * max_w + p.w / 2) / p.w;
    if h < 1 {
        h = 1;
    }
    let mut out: Vec<u8> = vec![0; (w * h * 4) as usize];
    let mut y: i64 = 0;
    while y < h {
        let y0 = y * p.h / h;
        let mut y1 = (y + 1) * p.h / h;
        if y1 <= y0 {
            y1 = y0 + 1;
        }
        let mut x: i64 = 0;
        while x < w {
            let x0 = x * p.w / w;
            let mut x1 = (x + 1) * p.w / w;
            if x1 <= x0 {
                x1 = x0 + 1;
            }
            let mut r: i64 = 0;
            let mut g: i64 = 0;
            let mut b: i64 = 0;
            let mut a: i64 = 0;
            let mut n: i64 = 0;
            let mut sy = y0;
            while sy < y1 {
                let mut q = ((sy * p.w + x0) * 4) as usize;
                let mut sx = x0;
                while sx < x1 {
                    let al = p.rgba[q + 3] as i64;
                    r += (p.rgba[q] as i64) * al;
                    g += (p.rgba[q + 1] as i64) * al;
                    b += (p.rgba[q + 2] as i64) * al;
                    a += al;
                    n += 1;
                    q += 4;
                    sx += 1;
                }
                sy += 1;
            }
            let o = ((y * w + x) * 4) as usize;
            if a > 0 {
                out[o] = (r / a) as u8;
                out[o + 1] = (g / a) as u8;
                out[o + 2] = (b / a) as u8;
            }
            out[o + 3] = (a / n) as u8;
            x += 1;
        }
        y += 1;
    }
    made(&p.kind, w, h, out)
}

pub fn failed(kind: &str, msg: &str) -> PicImage {
    PicImage { ok: false, error: msg.to_string(), kind: kind.to_string(), w: 0, h: 0, rgba: vec![] }
}

pub fn made(kind: &str, w: i64, h: i64, rgba: Vec<u8>) -> PicImage {
    PicImage { ok: true, error: String::new(), kind: kind.to_string(), w: w, h: h, rgba: rgba }
}

/// The largest picture read, in pixels (as large as a 100-megapixel photo).
pub const MAX_PIXELS: i64 = 100000000;

/// The format of the bytes: "png", "jpeg", "bmp", "gif", "tiff" or "".
pub fn sniff(d: &Vec<u8>) -> String {
    if d.len() >= 8 && d[0] == 137 && d[1] == 80 && d[2] == 78 && d[3] == 71 {
        return "png".to_string();
    }
    if pdf_jpeg::is_jpeg(d) {
        return "jpeg".to_string();
    }
    if d.len() >= 26 && d[0] == 66 && d[1] == 77 {
        return "bmp".to_string();
    }
    if d.len() >= 6 && d[0] == 71 && d[1] == 73 && d[2] == 70 && d[3] == 56 {
        return "gif".to_string();
    }
    if d.len() >= 8 && ((d[0] == 73 && d[1] == 73 && d[2] == 42 && d[3] == 0) || (d[0] == 77 && d[1] == 77 && d[2] == 0 && d[3] == 42)) {
        return "tiff".to_string();
    }
    String::new()
}

/// Whether a file with this extension is a picture the preview shows.
pub fn is_picture_ext(ext: &str) -> bool {
    let e = ext.to_lowercase();
    e == "png" || e == "jpg" || e == "jpeg" || e == "jpe" || e == "jfif" || e == "bmp" || e == "dib" || e == "gif" || e == "tif" || e == "tiff"
}

pub fn decode(d: &Vec<u8>) -> PicImage {
    let kind = sniff(d);
    if kind == "png" {
        return decode_png(d);
    }
    if kind == "jpeg" {
        let j = pdf_jpeg::decode(d, -1);
        if !j.ok {
            return failed("jpeg", &j.error);
        }
        return made("jpeg", j.w, j.h, pdf_jpeg::to_rgba(&j));
    }
    if kind == "bmp" {
        return decode_bmp(d);
    }
    if kind == "gif" {
        return pdf_gif::decode(d);
    }
    if kind == "tiff" {
        return pdf_tiff::decode(d);
    }
    failed("", "not a picture format the preview reads")
}

fn be32(d: &Vec<u8>, p: usize) -> i64 {
    if p + 3 >= d.len() {
        return 0;
    }
    (d[p] as i64) * 16777216 + (d[p + 1] as i64) * 65536 + (d[p + 2] as i64) * 256 + (d[p + 3] as i64)
}

fn le16(d: &Vec<u8>, p: usize) -> i64 {
    if p + 1 >= d.len() {
        return 0;
    }
    (d[p] as i64) + (d[p + 1] as i64) * 256
}

fn le32(d: &Vec<u8>, p: usize) -> i64 {
    if p + 3 >= d.len() {
        return 0;
    }
    (d[p] as i64) + (d[p + 1] as i64) * 256 + (d[p + 2] as i64) * 65536 + (d[p + 3] as i64) * 16777216
}

/// A signed 32-bit little-endian value.
fn le32s(d: &Vec<u8>, p: usize) -> i64 {
    let v = le32(d, p);
    if v >= 2147483648 {
        return v - 4294967296;
    }
    v
}

fn paeth(a: i64, b: i64, c: i64) -> i64 {
    let p = a + b - c;
    let pa = if p > a { p - a } else { a - p };
    let pb = if p > b { p - b } else { b - p };
    let pc = if p > c { p - c } else { c - p };
    if pa <= pb && pa <= pc {
        return a;
    }
    if pb <= pc {
        return b;
    }
    c
}

/// Undoes the PNG row filters of a `w` x `h` (sub)image whose rows start at
/// `from` in `raw`; returns the rows without their filter bytes.
fn unfilter(raw: &Vec<u8>, from: usize, w: i64, h: i64, bits_pp: i64) -> Vec<u8> {
    let row = ((w * bits_pp + 7) / 8) as usize;
    let bpp = (if bits_pp < 8 { 1 } else { bits_pp / 8 }) as usize;
    let mut out: Vec<u8> = vec![0; row * (h as usize)];
    let mut y: usize = 0;
    let mut p = from;
    while y < (h as usize) {
        let ft = if p < raw.len() { raw[p] as i64 } else { 0 };
        p += 1;
        let o = y * row;
        let mut x: usize = 0;
        while x < row {
            let v = if p + x < raw.len() { raw[p + x] as i64 } else { 0 };
            let a = if x >= bpp { out[o + x - bpp] as i64 } else { 0 };
            let b = if y > 0 { out[o - row + x] as i64 } else { 0 };
            let c = if y > 0 && x >= bpp { out[o - row + x - bpp] as i64 } else { 0 };
            let r = if ft == 1 {
                v + a
            } else if ft == 2 {
                v + b
            } else if ft == 3 {
                v + (a + b) / 2
            } else if ft == 4 {
                v + paeth(a, b, c)
            } else {
                v
            };
            out[o + x] = (r & 255) as u8;
            x += 1;
        }
        p += row;
        y += 1;
    }
    out
}

/// Sample `i` of a row of `bits`-bit samples, scaled to 0..255 (16-bit
/// samples keep their high byte).
fn sample8(row: &Vec<u8>, o: usize, i: i64, bits: i64) -> i64 {
    if bits == 8 {
        return row[o + (i as usize)] as i64;
    }
    if bits == 16 {
        return row[o + (i as usize) * 2] as i64;
    }
    let v = raw_sample(row, o, i, bits);
    if bits == 1 {
        return v * 255;
    }
    if bits == 2 {
        return v * 85;
    }
    v * 17
}

/// Sample `i` of a row of 1-, 2- or 4-bit samples as it is.
fn raw_sample(row: &Vec<u8>, o: usize, i: i64, bits: i64) -> i64 {
    let bit = i * bits;
    let b = row[o + (bit / 8) as usize] as i64;
    let shift = 8 - bits - (bit % 8);
    (b >> shift) & ((1 << bits) - 1)
}

fn sample16(row: &Vec<u8>, o: usize, i: i64) -> i64 {
    (row[o + (i as usize) * 2] as i64) * 256 + (row[o + (i as usize) * 2 + 1] as i64)
}

pub fn decode_png(d: &Vec<u8>) -> PicImage {
    let mut p: usize = 8;
    let mut w: i64 = 0;
    let mut h: i64 = 0;
    let mut depth: i64 = 0;
    let mut ctype: i64 = 0;
    let mut interlace: i64 = 0;
    let mut pal: Vec<u8> = vec![];
    let mut trns: Vec<u8> = vec![];
    let mut idat: Vec<u8> = vec![];
    while p + 8 <= d.len() {
        let len = be32(d, p) as usize;
        let t = p + 4;
        let start = p + 8;
        if start + len > d.len() {
            break;
        }
        let name = format!("{}{}{}{}", d[t] as char, d[t + 1] as char, d[t + 2] as char, d[t + 3] as char);
        if name == "IHDR" {
            w = be32(d, start);
            h = be32(d, start + 4);
            depth = d[start + 8] as i64;
            ctype = d[start + 9] as i64;
            interlace = d[start + 12] as i64;
        } else if name == "PLTE" {
            let mut i: usize = 0;
            while i < len {
                pal.push(d[start + i]);
                i += 1;
            }
        } else if name == "tRNS" {
            let mut i: usize = 0;
            while i < len {
                trns.push(d[start + i]);
                i += 1;
            }
        } else if name == "IDAT" {
            let mut i: usize = 0;
            while i < len {
                idat.push(d[start + i]);
                i += 1;
            }
        } else if name == "IEND" {
            break;
        }
        p = start + len + 4;
    }
    if w <= 0 || h <= 0 || w * h > MAX_PIXELS {
        return failed("png", "bad image size");
    }
    let chans: i64 = if ctype == 0 {
        1
    } else if ctype == 2 {
        3
    } else if ctype == 3 {
        1
    } else if ctype == 4 {
        2
    } else if ctype == 6 {
        4
    } else {
        0
    };
    if chans == 0 || !(depth == 1 || depth == 2 || depth == 4 || depth == 8 || depth == 16) {
        return failed("png", "unsupported colour type or bit depth");
    }
    let raw = pdf_inflate::inflate(&idat).data;
    let bits_pp = chans * depth;
    let mut rgba: Vec<u8> = vec![0; (w * h * 4) as usize];
    // the passes of Adam7, or one pass for the whole image
    let xs: Vec<i64> = if interlace == 1 { vec![0, 4, 0, 2, 0, 1, 0] } else { vec![0] };
    let ys: Vec<i64> = if interlace == 1 { vec![0, 0, 4, 0, 2, 0, 1] } else { vec![0] };
    let dx: Vec<i64> = if interlace == 1 { vec![8, 8, 4, 4, 2, 2, 1] } else { vec![1] };
    let dy: Vec<i64> = if interlace == 1 { vec![8, 8, 8, 4, 4, 2, 2] } else { vec![1] };
    let mut from: usize = 0;
    let mut pass: usize = 0;
    while pass < xs.len() {
        let pw = (w - xs[pass] + dx[pass] - 1) / dx[pass];
        let ph = (h - ys[pass] + dy[pass] - 1) / dy[pass];
        if pw <= 0 || ph <= 0 {
            pass += 1;
            continue;
        }
        let rows = unfilter(&raw, from, pw, ph, bits_pp);
        let row = ((pw * bits_pp + 7) / 8) as usize;
        from += (row + 1) * (ph as usize);
        let mut y: i64 = 0;
        while y < ph {
            let o = (y as usize) * row;
            let mut x: i64 = 0;
            while x < pw {
                let at = (((ys[pass] + y * dy[pass]) * w + xs[pass] + x * dx[pass]) * 4) as usize;
                let mut r: i64 = 0;
                let mut g: i64 = 0;
                let mut b: i64 = 0;
                let mut a: i64 = 255;
                if ctype == 3 {
                    let ix = if depth == 8 { rows[o + (x as usize)] as i64 } else { raw_sample(&rows, o, x, depth) };
                    let pi = (ix * 3) as usize;
                    if pi + 2 < pal.len() {
                        r = pal[pi] as i64;
                        g = pal[pi + 1] as i64;
                        b = pal[pi + 2] as i64;
                    }
                    if (ix as usize) < trns.len() {
                        a = trns[ix as usize] as i64;
                    }
                } else if ctype == 0 || ctype == 4 {
                    let v = sample8(&rows, o, x * chans, depth);
                    r = v;
                    g = v;
                    b = v;
                    if ctype == 4 {
                        a = sample8(&rows, o, x * chans + 1, depth);
                    } else if trns.len() >= 2 {
                        let key = (trns[0] as i64) * 256 + (trns[1] as i64);
                        let raw_v = if depth == 16 { sample16(&rows, o, x) } else if depth == 8 { rows[o + (x as usize)] as i64 } else { raw_sample(&rows, o, x, depth) };
                        if raw_v == key {
                            a = 0;
                        }
                    }
                } else {
                    r = sample8(&rows, o, x * chans, depth);
                    g = sample8(&rows, o, x * chans + 1, depth);
                    b = sample8(&rows, o, x * chans + 2, depth);
                    if ctype == 6 {
                        a = sample8(&rows, o, x * chans + 3, depth);
                    } else if trns.len() >= 6 {
                        let kr = (trns[0] as i64) * 256 + (trns[1] as i64);
                        let kg = (trns[2] as i64) * 256 + (trns[3] as i64);
                        let kb = (trns[4] as i64) * 256 + (trns[5] as i64);
                        let same = if depth == 16 {
                            sample16(&rows, o, x * 3) == kr && sample16(&rows, o, x * 3 + 1) == kg && sample16(&rows, o, x * 3 + 2) == kb
                        } else {
                            r == kr && g == kg && b == kb
                        };
                        if same {
                            a = 0;
                        }
                    }
                }
                rgba[at] = r as u8;
                rgba[at + 1] = g as u8;
                rgba[at + 2] = b as u8;
                rgba[at + 3] = a as u8;
                x += 1;
            }
            y += 1;
        }
        pass += 1;
    }
    made("png", w, h, rgba)
}

/// The shift and width of a BMP channel mask, for scaling it to 0..255.
struct Mask {
    shift: i64,
    bits: i64,
    mask: i64,
}

fn mask_of(m: i64) -> Mask {
    let mut out = Mask { shift: 0, bits: 0, mask: m };
    if m == 0 {
        return out;
    }
    let mut v = m;
    while (v & 1) == 0 {
        v = v / 2;
        out.shift += 1;
    }
    while (v & 1) == 1 {
        v = v / 2;
        out.bits += 1;
    }
    out
}

fn masked(px: i64, m: &Mask) -> i64 {
    if m.bits == 0 {
        return 0;
    }
    let mut v = px & m.mask;
    let mut s: i64 = 0;
    while s < m.shift {
        v = v / 2;
        s += 1;
    }
    let maxv = (1 << m.bits) - 1;
    (v * 255 + maxv / 2) / maxv
}

pub fn decode_bmp(d: &Vec<u8>) -> PicImage {
    let data_at = le32(d, 10) as usize;
    let hs = le32(d, 14);
    let mut w: i64 = 0;
    let mut hh: i64 = 0;
    let mut bpp: i64 = 0;
    let mut comp: i64 = 0;
    let mut ncol: i64 = 0;
    let mut pal_at: usize = 14 + (hs as usize);
    let mut pal_size: usize = 4;
    if hs == 12 {
        w = le16(d, 18);
        hh = le16(d, 20);
        bpp = le16(d, 24);
        pal_size = 3;
    } else if hs >= 40 {
        w = le32s(d, 18);
        hh = le32s(d, 22);
        bpp = le16(d, 28);
        comp = le32(d, 30);
        ncol = le32(d, 46);
    } else {
        return failed("bmp", "unknown BMP header");
    }
    let top_down = hh < 0;
    let h = if top_down { 0 - hh } else { hh };
    if w <= 0 || h <= 0 || w * h > MAX_PIXELS {
        return failed("bmp", "bad image size");
    }
    // the channel masks of BI_BITFIELDS / BI_ALPHABITFIELDS, after a
    // 40-byte header or inside a larger one
    let mut rm = mask_of(0x7C00);
    let mut gm = mask_of(0x03E0);
    let mut bm = mask_of(0x001F);
    let mut am = mask_of(0);
    if bpp == 32 {
        rm = mask_of(0xFF0000);
        gm = mask_of(0xFF00);
        bm = mask_of(0xFF);
    }
    let mut bitfields = false;
    if comp == 3 || comp == 6 {
        bitfields = true;
        let mp: usize = 54;
        rm = mask_of(le32(d, mp));
        gm = mask_of(le32(d, mp + 4));
        bm = mask_of(le32(d, mp + 8));
        if comp == 6 || hs >= 56 {
            am = mask_of(le32(d, mp + 12));
        }
        if hs == 40 {
            pal_at += if comp == 6 { 16 } else { 12 };
        }
    }
    let mut pal: Vec<i64> = vec![];
    if bpp <= 8 {
        let n = if ncol > 0 && ncol <= 256 { ncol } else { 1 << bpp };
        let mut i: i64 = 0;
        while i < n {
            let q = pal_at + (i as usize) * pal_size;
            if q + 2 < d.len() {
                pal.push(d[q + 2] as i64);
                pal.push(d[q + 1] as i64);
                pal.push(d[q] as i64);
            } else {
                pal.push(0);
                pal.push(0);
                pal.push(0);
            }
            i += 1;
        }
    }
    let total = (w * h) as usize;
    let mut rgba: Vec<u8> = vec![0; total * 4];
    // palette indexes, for the RLE forms and the paletted bit depths
    let mut idx: Vec<i64> = vec![];
    if comp == 1 || comp == 2 {
        idx = bmp_rle(d, data_at, w, h, comp == 2);
    } else if bpp <= 8 {
        idx = vec![0; total];
        let stride = (((w * bpp + 31) / 32) * 4) as usize;
        let mut y: i64 = 0;
        while y < h {
            let o = data_at + (y as usize) * stride;
            let mut x: i64 = 0;
            while x < w {
                let bit = x * bpp;
                let q = o + (bit / 8) as usize;
                let b = if q < d.len() { d[q] as i64 } else { 0 };
                let v = if bpp == 8 { b } else { (b >> (8 - bpp - (bit % 8))) & ((1 << bpp) - 1) };
                idx[(y * w + x) as usize] = v;
                x += 1;
            }
            y += 1;
        }
    }
    let stride = (((w * bpp + 31) / 32) * 4) as usize;
    let mut any_alpha = false;
    let mut y: i64 = 0;
    while y < h {
        // rows are stored bottom-up unless the height is negative
        let oy = if top_down { y } else { h - 1 - y };
        let mut x: i64 = 0;
        while x < w {
            let at = ((oy * w + x) * 4) as usize;
            if !idx.is_empty() {
                let pi = (idx[(y * w + x) as usize] * 3) as usize;
                if pi + 2 < pal.len() {
                    rgba[at] = pal[pi] as u8;
                    rgba[at + 1] = pal[pi + 1] as u8;
                    rgba[at + 2] = pal[pi + 2] as u8;
                }
                rgba[at + 3] = 255;
            } else if bpp == 24 {
                let q = data_at + (y as usize) * stride + (x as usize) * 3;
                if q + 2 < d.len() {
                    rgba[at] = d[q + 2];
                    rgba[at + 1] = d[q + 1];
                    rgba[at + 2] = d[q];
                }
                rgba[at + 3] = 255;
            } else if bpp == 16 || bpp == 32 {
                let q = data_at + (y as usize) * stride + (x as usize) * ((bpp / 8) as usize);
                let px = if bpp == 16 { le16(d, q) } else { le32(d, q) };
                rgba[at] = masked(px, &rm) as u8;
                rgba[at + 1] = masked(px, &gm) as u8;
                rgba[at + 2] = masked(px, &bm) as u8;
                if am.bits > 0 {
                    let a = masked(px, &am);
                    rgba[at + 3] = a as u8;
                    if a != 0 {
                        any_alpha = true;
                    }
                } else if bpp == 32 && !bitfields {
                    // BI_RGB's fourth byte is alpha only when some pixel uses it
                    let a = if q + 3 < d.len() { d[q + 3] } else { 0 };
                    rgba[at + 3] = a;
                    if a != 0 {
                        any_alpha = true;
                    }
                } else {
                    rgba[at + 3] = 255;
                }
            } else {
                return failed("bmp", "unsupported bit depth");
            }
            x += 1;
        }
        y += 1;
    }
    if (bpp == 32 || am.bits > 0) && !any_alpha {
        let mut i: usize = 0;
        while i < total {
            rgba[i * 4 + 3] = 255;
            i += 1;
        }
    }
    made("bmp", w, h, rgba)
}

/// An ICO file's entry nearest `size` pixels wide. An entry is a PNG file or
/// a BMP without its file header, its height doubled to cover the AND mask
/// that follows the pixels (1 = transparent; 32-bit entries carry alpha).
pub fn decode_ico(d: &Vec<u8>, size: i64) -> PicImage {
    if d.len() < 22 || le16(d, 0) != 0 || le16(d, 2) != 1 {
        return failed("ico", "not an icon file");
    }
    let n = le16(d, 4);
    let mut best: i64 = -1;
    let mut best_w: i64 = 0;
    let mut i: i64 = 0;
    while i < n && 6 + (i as usize) * 16 + 16 <= d.len() {
        let e = 6 + (i as usize) * 16;
        let w = if d[e] == 0 { 256 } else { d[e] as i64 };
        let better = if best < 0 {
            true
        } else if w >= size {
            best_w < size || w < best_w
        } else {
            best_w < size && w > best_w
        };
        if better {
            best = i;
            best_w = w;
        }
        i += 1;
    }
    if best < 0 {
        return failed("ico", "no icon entries");
    }
    let e = 6 + (best as usize) * 16;
    let len = le32(d, e + 8) as usize;
    let off = le32(d, e + 12) as usize;
    if off + len > d.len() || len < 40 {
        return failed("ico", "icon entry out of the file");
    }
    let mut sub: Vec<u8> = vec![];
    let mut k: usize = 0;
    while k < len {
        sub.push(d[off + k]);
        k += 1;
    }
    if sniff(&sub) == "png" {
        return decode_png(&sub);
    }
    let hs = le32(&sub, 0) as usize;
    let w = le32s(&sub, 4);
    let h = le32s(&sub, 8) / 2;
    let bpp = le16(&sub, 14);
    let comp = le32(&sub, 16);
    let ncol = le32(&sub, 32);
    let mut pal: usize = 0;
    if bpp <= 8 {
        let colors = if ncol > 0 && ncol <= 256 { ncol } else { 1 << bpp };
        pal = (4 * colors) as usize;
    }
    if comp == 3 && hs == 40 {
        pal += 12;
    }
    let data_at = 14 + hs + pal;
    // the entry as a BMP file: a file header, the height halved
    let mut bmp: Vec<u8> = vec![66, 77];
    let total = (14 + len) as i64;
    let mut j: i64 = 0;
    while j < 4 {
        bmp.push(((total >> (8 * j)) & 255) as u8);
        j += 1;
    }
    bmp.push(0);
    bmp.push(0);
    bmp.push(0);
    bmp.push(0);
    let at = data_at as i64;
    j = 0;
    while j < 4 {
        bmp.push(((at >> (8 * j)) & 255) as u8);
        j += 1;
    }
    k = 0;
    while k < len {
        if k >= 8 && k < 12 {
            bmp.push(((h >> (8 * ((k as i64) - 8))) & 255) as u8);
        } else {
            bmp.push(sub[k]);
        }
        k += 1;
    }
    let p = decode_bmp(&bmp);
    if !p.ok || bpp >= 32 {
        return made("ico", p.w, p.h, p.rgba);
    }
    let mut rgba = p.rgba;
    let xor_stride = (((w * bpp + 31) / 32) * 4) as usize;
    let and_at = data_at - 14 + xor_stride * (h as usize);
    let and_stride = (((w + 31) / 32) * 4) as usize;
    let mut y: i64 = 0;
    while y < h {
        // rows bottom-up, as the pixels
        let row = and_at + ((h - 1 - y) as usize) * and_stride;
        let mut x: i64 = 0;
        while x < w {
            let q = row + (x / 8) as usize;
            if q < sub.len() && (((sub[q] as i64) >> (7 - (x % 8))) & 1) == 1 {
                rgba[((y * w + x) * 4 + 3) as usize] = 0;
            }
            x += 1;
        }
        y += 1;
    }
    made("ico", w, h, rgba)
}

/// RLE8 / RLE4 data to palette indexes, rows in file order (bottom-up).
fn bmp_rle(d: &Vec<u8>, from: usize, w: i64, h: i64, four: bool) -> Vec<i64> {
    let mut out: Vec<i64> = vec![0; (w * h) as usize];
    let mut x: i64 = 0;
    let mut y: i64 = 0;
    let mut p = from;
    while p + 1 < d.len() && y < h {
        let n = d[p] as i64;
        let v = d[p + 1] as i64;
        p += 2;
        if n > 0 {
            let mut i: i64 = 0;
            while i < n {
                let c = if four { if i % 2 == 0 { v >> 4 } else { v & 15 } } else { v };
                if x < w {
                    out[(y * w + x) as usize] = c;
                }
                x += 1;
                i += 1;
            }
        } else if v == 0 {
            x = 0;
            y += 1;
        } else if v == 1 {
            break;
        } else if v == 2 {
            if p + 1 < d.len() {
                x += d[p] as i64;
                y += d[p + 1] as i64;
            }
            p += 2;
        } else {
            // v literal indexes, padded to a 16-bit boundary
            let bytes = if four { (v + 1) / 2 } else { v };
            let mut i: i64 = 0;
            while i < v {
                let b = if p + ((if four { i / 2 } else { i }) as usize) < d.len() { d[p + ((if four { i / 2 } else { i }) as usize)] as i64 } else { 0 };
                let c = if four { if i % 2 == 0 { b >> 4 } else { b & 15 } } else { b };
                if x < w {
                    out[(y * w + x) as usize] = c;
                }
                x += 1;
                i += 1;
            }
            p += (bytes + (bytes % 2)) as usize;
        }
    }
    out
}
