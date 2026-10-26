//! Image XObjects and inline images (ISO 32000-1 §8.9): samples of any bit
//! depth in any colour space to RGBA, with the Decode array, stencil masks,
//! colour-key masks, /Mask images and soft masks. JPEG (DCTDecode) data is
//! decoded by `pdf_jpeg`; when that fails it is passed on for the host to
//! decode, with the mask as alpha.

use crate::pdf_color;
use crate::pdf_color::ColorSpace;
use crate::pdf_doc::Doc;
use crate::pdf_jpeg;
use crate::pdf_obj;
use crate::pdf_obj::Obj;
use crate::pdf_obj::K;

#[derive(Clone, Debug)]
pub struct Img {
    pub w: i64,
    pub h: i64,
    /// w * h * 4 bytes, or empty when `jpeg` holds the picture
    pub rgba: Vec<u8>,
    /// the DCTDecode data, for the host to decode
    pub jpeg: Vec<u8>,
    /// alpha for a JPEG with a mask: w * h bytes, or empty
    pub alpha: Vec<u8>,
    /// the codec that could not be decoded ("JPXDecode", ...), or ""
    pub unsupported: String,
    pub ok: bool,
}

pub fn empty() -> Img {
    Img { w: 0, h: 0, rgba: vec![], jpeg: vec![], alpha: vec![], unsupported: String::new(), ok: false }
}

/// The unpacked samples: one value 0..(2^bpc - 1) per component.
fn unpack(data: &Vec<u8>, w: i64, h: i64, ncomp: i64, bpc: i64) -> Vec<i64> {
    let total = (w * h * ncomp) as usize;
    let mut out: Vec<i64> = vec![];
    if bpc == 8 {
        let mut i: usize = 0;
        while i < total {
            out.push(if i < data.len() { data[i] as i64 } else { 0 });
            i += 1;
        }
        return out;
    }
    let row_bits = w * ncomp * bpc;
    let row_bytes = ((row_bits + 7) / 8) as usize;
    let mut y: i64 = 0;
    while y < h {
        let base = (y as usize) * row_bytes;
        let mut bit: usize = 0;
        let mut k: i64 = 0;
        while k < w * ncomp {
            let mut v: i64 = 0;
            if bpc == 16 {
                let p = base + bit / 8;
                let hi = if p < data.len() { data[p] as i64 } else { 0 };
                let lo = if p + 1 < data.len() { data[p + 1] as i64 } else { 0 };
                v = hi * 256 + lo;
                bit += 16;
            } else {
                let mut b: i64 = 0;
                while b < bpc {
                    let p = base + bit / 8;
                    let byte = if p < data.len() { data[p] as i64 } else { 0 };
                    let s = 7 - (bit % 8) as i64;
                    v = v * 2 + ((byte >> s) & 1);
                    bit += 1;
                    b += 1;
                }
            }
            out.push(v);
            k += 1;
        }
        y += 1;
    }
    out
}

fn pow2(n: i64) -> f64 {
    let mut p = 1.0;
    let mut i = 0;
    while i < n {
        p = p * 2.0;
        i += 1;
    }
    p
}

/// The colours of one-component samples, as a table by sample value.
fn table(cs: &ColorSpace, bpc: i64, decode: &Vec<f64>) -> Vec<u8> {
    let maxv = pow2(bpc) - 1.0;
    let n = (maxv as i64) + 1;
    let mut t: Vec<u8> = vec![];
    let mut v: i64 = 0;
    while v < n {
        let mut x = (v as f64) / maxv;
        if decode.len() >= 2 {
            x = decode[0] + x * (decode[1] - decode[0]);
        }
        if cs.kind == pdf_color::CS_INDEXED && decode.len() < 2 {
            x = v as f64;
        } else if cs.kind == pdf_color::CS_INDEXED {
            x = decode[0] + ((v as f64) / maxv) * (decode[1] - decode[0]);
        }
        let c: Vec<f64> = vec![x];
        let rgb = cs.to_rgb(&c);
        t.push(to_byte(rgb[0]));
        t.push(to_byte(rgb[1]));
        t.push(to_byte(rgb[2]));
        v += 1;
    }
    t
}

pub fn to_byte(x: f64) -> u8 {
    let v = (x * 255.0 + 0.5).floor();
    if v < 0.0 {
        return 0;
    }
    if v > 255.0 {
        return 255;
    }
    v as u8
}

/// The image in stream (or inline image) `s`; `fill` is the fill colour for a stencil mask.
pub fn decode(doc: &mut Doc, s: &Obj, res: &Obj, fill: &Vec<f64>, fill_alpha: f64) -> Img {
    let mut img = empty();
    let mut w = doc.get_key(s, "Width").i();
    let mut h = doc.get_key(s, "Height").i();
    if w <= 0 || h <= 0 || w * h > 40000000 {
        return img;
    }
    img.w = w;
    img.h = h;
    let is_mask = doc.get_key(s, "ImageMask").k == K::Bool && doc.get_key(s, "ImageMask").n == 1.0;
    let mut bpc = doc.get_key(s, "BitsPerComponent").i();
    if is_mask || bpc <= 0 {
        bpc = if is_mask { 1 } else { 8 };
    }
    let mut dec = doc.stream_decoded(s);
    let decode_arr = doc.get_key(s, "Decode").nums();
    if is_mask {
        if !dec.codec.is_empty() && dec.codec != "DCTDecode" {
            img.unsupported = dec.codec.clone();
            return img;
        }
        let bits = unpack(&dec.data, w, h, 1, 1);
        let paint_on = if decode_arr.len() >= 2 && decode_arr[0] == 1.0 { 1 } else { 0 };
        let mut rgba: Vec<u8> = vec![];
        let r = to_byte(fill[0]);
        let g = to_byte(fill[1]);
        let b = to_byte(fill[2]);
        let a = to_byte(fill_alpha);
        for v in bits.iter() {
            rgba.push(r);
            rgba.push(g);
            rgba.push(b);
            rgba.push(if *v == paint_on { a } else { 0 });
        }
        img.rgba = rgba;
        img.ok = true;
        return img;
    }
    let mut cso = doc.get_key(s, "ColorSpace");
    if cso.is_null() {
        cso = doc.get_key(s, "CS");
    }
    let mut cs = if cso.is_null() { pdf_color::device(pdf_color::CS_GRAY) } else { pdf_color::read(doc, &cso, res, 0) };
    if dec.codec == "DCTDecode" {
        let ctv = doc.get_key(&dec.codec_parms, "ColorTransform");
        let ct: i64 = if ctv.k == K::Num { ctv.i() } else { -1 };
        let j = pdf_jpeg::decode(&dec.data, ct);
        if j.ok && j.w > 0 && j.h > 0 {
            // the JPEG's own size and component count win over the dictionary's
            if (cs.n as i64) != j.n {
                cs = pdf_color::device(if j.n == 1 { pdf_color::CS_GRAY } else if j.n == 4 { pdf_color::CS_CMYK } else { pdf_color::CS_RGB });
            }
            img.w = j.w;
            img.h = j.h;
            dec.data = j.data;
            dec.codec = String::new();
            bpc = 8;
        }
    }
    w = img.w;
    h = img.h;
    if dec.codec == "DCTDecode" {
        img.jpeg = dec.data.clone();
        img.ok = true;
        let alpha = mask_alpha(doc, s, res, w, h, &vec![], 0, 0);
        img.alpha = alpha;
        return img;
    }
    if !dec.codec.is_empty() {
        img.unsupported = dec.codec.clone();
        return img;
    }
    let n: i64 = if cs.n == 0 { 1 } else { cs.n as i64 };
    let samples = unpack(&dec.data, w, h, n, bpc);
    let total = (w * h) as usize;
    let mut rgba: Vec<u8> = vec![];
    if n == 1 && bpc <= 8 {
        let t = table(&cs, bpc, &decode_arr);
        let mut i: usize = 0;
        while i < total {
            let v = (samples[i] * 3) as usize;
            rgba.push(t[v]);
            rgba.push(t[v + 1]);
            rgba.push(t[v + 2]);
            rgba.push(255);
            i += 1;
        }
    } else {
        let maxv = pow2(bpc) - 1.0;
        let plain = decode_arr.is_empty() && bpc == 8 && (cs.kind == pdf_color::CS_RGB || cs.kind == pdf_color::CS_CMYK);
        let mut comps: Vec<f64> = vec![0.0; n as usize];
        let mut i: usize = 0;
        while i < total {
            let base = i * (n as usize);
            if plain && cs.kind == pdf_color::CS_RGB {
                rgba.push(samples[base] as u8);
                rgba.push(samples[base + 1] as u8);
                rgba.push(samples[base + 2] as u8);
                rgba.push(255);
                i += 1;
                continue;
            }
            let mut k: usize = 0;
            while k < (n as usize) {
                let mut x = (samples[base + k] as f64) / maxv;
                if decode_arr.len() >= 2 * k + 2 {
                    x = decode_arr[2 * k] + x * (decode_arr[2 * k + 1] - decode_arr[2 * k]);
                }
                comps[k] = x;
                k += 1;
            }
            let rgb = cs.to_rgb(&comps);
            rgba.push(to_byte(rgb[0]));
            rgba.push(to_byte(rgb[1]));
            rgba.push(to_byte(rgb[2]));
            rgba.push(255);
            i += 1;
        }
    }
    let alpha = mask_alpha(doc, s, res, w, h, &samples, n, bpc);
    if !alpha.is_empty() {
        let mut i: usize = 0;
        while i < total {
            rgba[i * 4 + 3] = alpha[i];
            i += 1;
        }
    }
    img.rgba = rgba;
    img.ok = true;
    img
}

/// The alpha of the image from /SMask, /Mask (an image or a colour key), or empty.
fn mask_alpha(doc: &mut Doc, s: &Obj, res: &Obj, w: i64, h: i64, samples: &Vec<i64>, n: i64, bpc: i64) -> Vec<u8> {
    let sm = doc.get_key(s, "SMask");
    if sm.k == K::Stream {
        let mw = doc.get_key(&sm, "Width").i();
        let mh = doc.get_key(&sm, "Height").i();
        let mut mb = doc.get_key(&sm, "BitsPerComponent").i();
        if mb <= 0 {
            mb = 8;
        }
        let d = doc.stream_decoded(&sm);
        if mw > 0 && mh > 0 && d.codec.is_empty() {
            let vals = unpack(&d.data, mw, mh, 1, mb);
            let maxv = pow2(mb) - 1.0;
            let dec = doc.get_key(&sm, "Decode").nums();
            let mut out: Vec<u8> = vec![];
            let mut y: i64 = 0;
            while y < h {
                let sy = y * mh / h;
                let mut x: i64 = 0;
                while x < w {
                    let sx = x * mw / w;
                    let mut v = (vals[(sy * mw + sx) as usize] as f64) / maxv;
                    if dec.len() >= 2 {
                        v = dec[0] + v * (dec[1] - dec[0]);
                    }
                    out.push(to_byte(v));
                    x += 1;
                }
                y += 1;
            }
            return out;
        }
    }
    let mk = doc.get_key(s, "Mask");
    if mk.k == K::Stream {
        let mw = doc.get_key(&mk, "Width").i();
        let mh = doc.get_key(&mk, "Height").i();
        let d = doc.stream_decoded(&mk);
        if mw > 0 && mh > 0 && d.codec.is_empty() {
            let bits = unpack(&d.data, mw, mh, 1, 1);
            let dec = doc.get_key(&mk, "Decode").nums();
            let paint_on = if dec.len() >= 2 && dec[0] == 1.0 { 1 } else { 0 };
            let mut out: Vec<u8> = vec![];
            let mut y: i64 = 0;
            while y < h {
                let sy = y * mh / h;
                let mut x: i64 = 0;
                while x < w {
                    let sx = x * mw / w;
                    out.push(if bits[(sy * mw + sx) as usize] == paint_on { 255 } else { 0 });
                    x += 1;
                }
                y += 1;
            }
            return out;
        }
    }
    if mk.k == K::Arr && n > 0 && !samples.is_empty() {
        let r = mk.nums();
        if r.len() >= (2 * n) as usize {
            let total = (w * h) as usize;
            let mut out: Vec<u8> = vec![];
            let mut i: usize = 0;
            while i < total {
                let mut inside = true;
                let mut k: usize = 0;
                while k < (n as usize) {
                    let v = samples[i * (n as usize) + k] as f64;
                    if v < r[2 * k] || v > r[2 * k + 1] {
                        inside = false;
                    }
                    k += 1;
                }
                out.push(if inside { 0 } else { 255 });
                i += 1;
            }
            let _ = bpc;
            return out;
        }
    }
    let _ = pdf_obj::null();
    let _ = res;
    vec![]
}
