//! Stream filters (ISO 32000-1 §7.4). The image codecs (DCT, JPX, JBIG2,
//! CCITT) are not decoded here: `codec` names the one left, and `data` is
//! its input.

use crate::pdf_inflate;
use crate::pdf_lexer::hexval;
use crate::pdf_lexer::is_ws;
use crate::pdf_obj::Obj;
use crate::pdf_obj::K;

pub struct Decoded {
    pub data: Vec<u8>,
    /// the image codec still to apply ("" when the data is decoded)
    pub codec: String,
    pub codec_parms: Obj,
    /// false when a filter was damaged or unknown
    pub ok: bool,
}

pub fn ascii_hex(src: &Vec<u8>) -> Vec<u8> {
    let mut out: Vec<u8> = vec![];
    let mut hi: i64 = -1;
    for c in src.iter() {
        if *c == 62 {
            break;
        }
        let v = hexval(*c);
        if v < 0 {
            continue;
        }
        if hi < 0 {
            hi = v;
        } else {
            out.push((hi * 16 + v) as u8);
            hi = -1;
        }
    }
    if hi >= 0 {
        out.push((hi * 16) as u8);
    }
    out
}

pub fn ascii85(src: &Vec<u8>) -> Vec<u8> {
    let mut out: Vec<u8> = vec![];
    let mut group: Vec<i64> = vec![];
    let mut i: usize = 0;
    // a leading "<~" is allowed
    if src.len() >= 2 && src[0] == 60 && src[1] == 126 {
        i = 2;
    }
    while i < src.len() {
        let c = src[i];
        i += 1;
        if c == 126 {
            break;
        }
        if is_ws(c) {
            continue;
        }
        if c == 122 && group.is_empty() {
            out.push(0);
            out.push(0);
            out.push(0);
            out.push(0);
            continue;
        }
        if c < 33 || c > 117 {
            continue;
        }
        group.push((c - 33) as i64);
        if group.len() == 5 {
            for b in bytes85(&group, 4).iter() {
                out.push(*b);
            }
            group = vec![];
        }
    }
    if group.len() > 1 {
        let n = group.len() - 1;
        while group.len() < 5 {
            group.push(84);
        }
        for b in bytes85(&group, n).iter() {
            out.push(*b);
        }
    }
    out
}

/// The first `n` bytes of a base-85 group. Helpers return new bytes: a
/// Ranger buffer (`Vec<u8>`) cannot grow through a `&mut` parameter.
fn bytes85(g: &Vec<i64>, n: usize) -> Vec<u8> {
    let mut out: Vec<u8> = vec![];
    // as a float: the value can pass 2^31, past JavaScript's 32-bit bit operators
    let mut v: f64 = 0.0;
    for d in g.iter() {
        v = v * 85.0 + (*d as f64);
    }
    let b0 = (v / 16777216.0).floor();
    let r0 = v - b0 * 16777216.0;
    let b1 = (r0 / 65536.0).floor();
    let r1 = r0 - b1 * 65536.0;
    let b2 = (r1 / 256.0).floor();
    let b3 = r1 - b2 * 256.0;
    let bs: Vec<f64> = vec![b0, b1, b2, b3];
    let mut k: usize = 0;
    while k < n {
        out.push(((bs[k] as i64) & 255) as u8);
        k += 1;
    }
    out
}

pub fn run_length(src: &Vec<u8>) -> Vec<u8> {
    let mut out: Vec<u8> = vec![];
    let mut i: usize = 0;
    while i < src.len() {
        let n = src[i] as usize;
        i += 1;
        if n == 128 {
            break;
        }
        if n < 128 {
            let mut k: usize = 0;
            while k <= n && i < src.len() {
                out.push(src[i]);
                i += 1;
                k += 1;
            }
        } else {
            if i >= src.len() {
                break;
            }
            let v = src[i];
            i += 1;
            let mut k: usize = 0;
            while k < 257 - n {
                out.push(v);
                k += 1;
            }
        }
    }
    out
}

pub fn lzw(src: &Vec<u8>, early: i64) -> Vec<u8> {
    let mut out: Vec<u8> = vec![];
    // the table as (prefix code, last byte, length); codes < 256 are bytes
    let mut prefix: Vec<i64> = vec![];
    let mut last: Vec<u8> = vec![];
    let mut lens: Vec<i64> = vec![];
    let mut i: i64 = 0;
    while i < 258 {
        prefix.push(-1);
        last.push((i & 255) as u8);
        lens.push(1);
        i += 1;
    }
    let mut width: i64 = 9;
    let mut bit: i64 = 0;
    let mut nbits: i64 = 0;
    let mut pos: usize = 0;
    let mut prev: i64 = -1;
    loop {
        while nbits < width {
            if pos >= src.len() {
                return out;
            }
            bit = ((bit << 8) | (src[pos] as i64)) & 0xFFFFFF;
            pos += 1;
            nbits += 8;
        }
        let code = (bit >> (nbits - width)) & ((1 << width) - 1);
        nbits -= width;
        if code == 256 {
            prefix.truncate(258);
            last.truncate(258);
            lens.truncate(258);
            width = 9;
            prev = -1;
            continue;
        }
        if code == 257 {
            return out;
        }
        let size = prefix.len() as i64;
        if code > size {
            return out;
        }
        let start = out.len();
        if code < size {
            for b in emit(&prefix, &last, &lens, code).iter() {
                out.push(*b);
            }
            if prev >= 0 {
                let first = out[start];
                prefix.push(prev);
                last.push(first);
                lens.push(lens[prev as usize] + 1);
            }
        } else {
            // code == size: the previous string plus its own first byte
            if prev < 0 {
                return out;
            }
            for b in emit(&prefix, &last, &lens, prev).iter() {
                out.push(*b);
            }
            let first = out[start];
            out.push(first);
            prefix.push(prev);
            last.push(first);
            lens.push(lens[prev as usize] + 1);
        }
        prev = code;
        let n = (prefix.len() as i64) + early;
        if n >= 512 && width == 9 {
            width = 10;
        } else if n >= 1024 && width == 10 {
            width = 11;
        } else if n >= 2048 && width == 11 {
            width = 12;
        }
    }
}

/// The bytes of LZW code `code`.
fn emit(prefix: &Vec<i64>, last: &Vec<u8>, lens: &Vec<i64>, code: i64) -> Vec<u8> {
    let n = lens[code as usize] as usize;
    let base: usize = 0;
    let mut out: Vec<u8> = vec![0; n];
    let mut c = code;
    let mut j = n;
    while j > 0 && c >= 0 {
        j -= 1;
        out[base + j] = last[c as usize];
        c = prefix[c as usize];
    }
    out
}

fn parm(p: &Obj, key: &str, dflt: i64) -> i64 {
    if p.is_dict() && p.at(key).is_num() {
        return p.at(key).i();
    }
    dflt
}

/// The PNG and TIFF predictors of Flate and LZW (§7.4.4.4).
pub fn predict(src: Vec<u8>, p: &Obj) -> Vec<u8> {
    let pred = parm(p, "Predictor", 1);
    if pred <= 1 {
        return src;
    }
    let colors = parm(p, "Colors", 1);
    let bpc = parm(p, "BitsPerComponent", 8);
    let cols = parm(p, "Columns", 1);
    let bpp_bits = colors * bpc;
    let mut bpp = (bpp_bits + 7) / 8;
    if bpp < 1 {
        bpp = 1;
    }
    let row = ((cols * bpp_bits + 7) / 8) as usize;
    let mut out: Vec<u8> = vec![];
    if row == 0 {
        return out;
    }
    if pred == 2 {
        if bpc != 8 {
            return src;
        }
        let mut i: usize = 0;
        while i < src.len() {
            let mut k: usize = 0;
            while k < row && i + k < src.len() {
                let mut v = src[i + k] as i64;
                if k >= (bpp as usize) {
                    v = (v + (out[out.len() - (bpp as usize)] as i64)) & 255;
                }
                out.push(v as u8);
                k += 1;
            }
            i += row;
        }
        return out;
    }
    let mut prev: Vec<u8> = vec![0; row];
    let mut cur: Vec<u8> = vec![0; row];
    let mut i: usize = 0;
    let b = bpp as usize;
    while i < src.len() {
        let ft = src[i];
        i += 1;
        let mut k: usize = 0;
        while k < row {
            let mut x: i64 = 0;
            if i + k < src.len() {
                x = src[i + k] as i64;
            }
            let mut a: i64 = 0;
            let mut c: i64 = 0;
            if k >= b {
                a = cur[k - b] as i64;
                c = prev[k - b] as i64;
            }
            let up = prev[k] as i64;
            let v = if ft == 1 {
                x + a
            } else if ft == 2 {
                x + up
            } else if ft == 3 {
                x + (a + up) / 2
            } else if ft == 4 {
                let pp = a + up - c;
                let mut pa = pp - a;
                if pa < 0 {
                    pa = 0 - pa;
                }
                let mut pb = pp - up;
                if pb < 0 {
                    pb = 0 - pb;
                }
                let mut pc = pp - c;
                if pc < 0 {
                    pc = 0 - pc;
                }
                if pa <= pb && pa <= pc {
                    x + a
                } else if pb <= pc {
                    x + up
                } else {
                    x + c
                }
            } else {
                x
            };
            cur[k] = (v & 255) as u8;
            k += 1;
        }
        for v in cur.iter() {
            out.push(*v);
        }
        let mut j: usize = 0;
        while j < row {
            prev[j] = cur[j];
            j += 1;
        }
        i += row;
    }
    out
}

fn filter_name(o: &Obj) -> String {
    let n = o.name.clone();
    if n == "Fl" {
        return "FlateDecode".to_string();
    }
    if n == "LZW" {
        return "LZWDecode".to_string();
    }
    if n == "AHx" {
        return "ASCIIHexDecode".to_string();
    }
    if n == "A85" {
        return "ASCII85Decode".to_string();
    }
    if n == "RL" {
        return "RunLengthDecode".to_string();
    }
    if n == "DCT" {
        return "DCTDecode".to_string();
    }
    if n == "CCF" {
        return "CCITTFaxDecode".to_string();
    }
    n
}

/// Applies the filters named in `filter` (a name or an array of names)
/// with their `parms` (a dictionary, or an array of them).
pub fn decode(raw: &Vec<u8>, filter: &Obj, parms: &Obj) -> Decoded {
    let mut names: Vec<Obj> = vec![];
    let mut ps: Vec<Obj> = vec![];
    if filter.k == K::Name {
        names.push(filter.clone());
        ps.push(parms.clone());
    } else if filter.k == K::Arr {
        let mut i: usize = 0;
        while i < filter.items.len() {
            names.push(filter.items[i].clone());
            if parms.k == K::Arr {
                ps.push(parms.item(i));
            } else if i == 0 {
                ps.push(parms.clone());
            } else {
                ps.push(crate::pdf_obj::null());
            }
            i += 1;
        }
    }
    let mut data = raw.clone();
    let mut ok = true;
    let mut i: usize = 0;
    while i < names.len() {
        let n = filter_name(&names[i]);
        let p = ps[i].clone();
        if n == "FlateDecode" {
            let r = pdf_inflate::inflate(&data);
            if !r.ok {
                ok = false;
            }
            data = predict(r.data, &p);
        } else if n == "LZWDecode" {
            let early = parm(&p, "EarlyChange", 1);
            let d = lzw(&data, early);
            data = predict(d, &p);
        } else if n == "ASCIIHexDecode" {
            data = ascii_hex(&data);
        } else if n == "ASCII85Decode" {
            data = ascii85(&data);
        } else if n == "RunLengthDecode" {
            data = run_length(&data);
        } else if n == "Crypt" {
            // the identity crypt filter
        } else {
            return Decoded { data: data, codec: n, codec_parms: p, ok: ok };
        }
        i += 1;
    }
    Decoded { data: data, codec: String::new(), codec_parms: crate::pdf_obj::null(), ok: ok }
}
