//! JPEG (ITU T.81) decoding: baseline and progressive Huffman-coded files
//! with 1 to 4 components, any sampling factors and restart intervals; the
//! DCTDecode filter of PDF and pictures in the preview. Arithmetic coding,
//! lossless and 12-bit JPEG are not read (`ok` is false).
//!
//! The samples come out as the file's components after the colour transform
//! the file asks for (YCbCr to RGB, YCCK to CMYK), one byte each; chroma is
//! upsampled by repeating samples, as pdf.js does.

pub struct Jpeg {
    pub ok: bool,
    pub error: String,
    pub w: i64,
    pub h: i64,
    /// components per pixel in `data`: 1, 3 or 4
    pub n: i64,
    /// w * h * n bytes
    pub data: Vec<u8>,
    /// the Adobe APP14 transform flag, -1 without the marker
    pub adobe: i64,
}

struct Comp {
    id: i64,
    hs: i64,
    vs: i64,
    tq: usize,
    /// blocks per line and per column, padded to whole MCUs
    bpl: i64,
    bpc: i64,
    /// the coefficients of each block, 64 per block, in natural order
    coef: Vec<i64>,
    pred: i64,
    td: usize,
    ta: usize,
}

struct Huff {
    /// the largest code of each length (index 1..16), -1 when there is none
    maxcode: Vec<i64>,
    /// the index in `vals` of the first code of each length, less that code
    delta: Vec<i64>,
    vals: Vec<i64>,
    set: bool,
}

/// The reader's state: the byte position, the bit buffer, and whether a
/// marker stopped the entropy-coded data.
struct Rd {
    pos: usize,
    buf: i64,
    nbits: i64,
    marker: bool,
}

struct Dec {
    comps: Vec<Comp>,
    dc: Vec<Huff>,
    ac: Vec<Huff>,
    qt: Vec<Vec<i64>>,
    restart: i64,
    progressive: bool,
    mcux: i64,
    mcuy: i64,
    hmax: i64,
    vmax: i64,
    eobrun: i64,
    frame: bool,
    w: i64,
    h: i64,
}

const ZIGZAG: [usize; 64] = [0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20, 13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59, 52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63];

fn empty_huff() -> Huff {
    Huff { maxcode: vec![-1; 18], delta: vec![0; 18], vals: vec![], set: false }
}

fn build_huff(counts: &Vec<i64>, vals: &Vec<i64>) -> Huff {
    let mut h = empty_huff();
    let mut code: i64 = 0;
    let mut k: i64 = 0;
    let mut len: usize = 1;
    while len <= 16 {
        let c = counts[len - 1];
        if c > 0 {
            h.delta[len] = k - code;
            code += c;
            k += c;
            h.maxcode[len] = code - 1;
        }
        code = code << 1;
        len += 1;
    }
    h.vals = vals.clone();
    h.set = true;
    h
}

fn u16_at(d: &Vec<u8>, p: usize) -> i64 {
    if p + 1 >= d.len() {
        return 0;
    }
    (d[p] as i64) * 256 + (d[p + 1] as i64)
}

fn fail(msg: &str) -> Jpeg {
    Jpeg { ok: false, error: msg.to_string(), w: 0, h: 0, n: 0, data: vec![], adobe: -1 }
}

/// Whether the bytes start like a JPEG file.
pub fn is_jpeg(d: &Vec<u8>) -> bool {
    d.len() > 3 && d[0] == 255 && d[1] == 216 && d[2] == 255
}

fn bit(d: &Vec<u8>, r: &mut Rd) -> i64 {
    if r.nbits == 0 {
        if r.marker || r.pos >= d.len() {
            // past the data: zeros, as decoders do with a truncated file
            return 0;
        }
        let b = d[r.pos] as i64;
        if b == 255 {
            let nx = if r.pos + 1 < d.len() { d[r.pos + 1] as i64 } else { 0 };
            if nx == 0 {
                r.pos += 2;
            } else if nx >= 0xD0 && nx <= 0xD7 {
                // a restart marker where none was due; stop here
                r.marker = true;
                return 0;
            } else {
                r.marker = true;
                return 0;
            }
        } else {
            r.pos += 1;
        }
        r.buf = b;
        r.nbits = 8;
    }
    r.nbits -= 1;
    (r.buf >> r.nbits) & 1
}

fn bits(d: &Vec<u8>, r: &mut Rd, n: i64) -> i64 {
    let mut v: i64 = 0;
    let mut i: i64 = 0;
    while i < n {
        v = (v << 1) | bit(d, r);
        i += 1;
    }
    v
}

fn extend(v: i64, n: i64) -> i64 {
    if n == 0 {
        return 0;
    }
    if v < (1 << (n - 1)) {
        return v - (1 << n) + 1;
    }
    v
}

fn receive(d: &Vec<u8>, r: &mut Rd, n: i64) -> i64 {
    let v = bits(d, r, n);
    extend(v, n)
}

fn decode_huff(d: &Vec<u8>, r: &mut Rd, h: &Huff) -> i64 {
    let mut code: i64 = 0;
    let mut len: usize = 1;
    while len <= 16 {
        code = (code << 1) | bit(d, r);
        if h.maxcode[len] >= code && h.maxcode[len] >= 0 {
            let i = (code + h.delta[len]) as usize;
            if i < h.vals.len() {
                return h.vals[i];
            }
            return 0;
        }
        len += 1;
    }
    0
}

/// A scan's parameters.
struct Scan {
    comps: Vec<usize>,
    ss: i64,
    se: i64,
    ah: i64,
    al: i64,
}

fn block_baseline(d: &Vec<u8>, r: &mut Rd, dec: &mut Dec, ci: usize, off: usize) {
    let t = decode_huff(d, r, &dec.dc[dec.comps[ci].td]);
    let diff = if t == 0 { 0 } else { receive(d, r, t) };
    dec.comps[ci].pred += diff;
    dec.comps[ci].coef[off] = dec.comps[ci].pred;
    let mut k: usize = 1;
    while k < 64 {
        let rs = decode_huff(d, r, &dec.ac[dec.comps[ci].ta]);
        let s = rs & 15;
        let run = rs >> 4;
        if s == 0 {
            if run < 15 {
                break;
            }
            k += 16;
            continue;
        }
        k += run as usize;
        if k > 63 {
            break;
        }
        dec.comps[ci].coef[off + ZIGZAG[k]] = receive(d, r, s);
        k += 1;
    }
}

fn block_dc_first(d: &Vec<u8>, r: &mut Rd, dec: &mut Dec, ci: usize, off: usize, al: i64) {
    let t = decode_huff(d, r, &dec.dc[dec.comps[ci].td]);
    let diff = if t == 0 { 0 } else { receive(d, r, t) };
    dec.comps[ci].pred += diff;
    dec.comps[ci].coef[off] = dec.comps[ci].pred * (1 << al);
}

fn block_dc_refine(d: &Vec<u8>, r: &mut Rd, dec: &mut Dec, ci: usize, off: usize, al: i64) {
    if bit(d, r) == 1 {
        dec.comps[ci].coef[off] = dec.comps[ci].coef[off] | (1 << al);
    }
}

fn block_ac_first(d: &Vec<u8>, r: &mut Rd, dec: &mut Dec, ci: usize, off: usize, sc: &Scan) {
    if dec.eobrun > 0 {
        dec.eobrun -= 1;
        return;
    }
    let mut k = sc.ss;
    while k <= sc.se {
        let rs = decode_huff(d, r, &dec.ac[dec.comps[ci].ta]);
        let s = rs & 15;
        let run = rs >> 4;
        if s == 0 {
            if run < 15 {
                dec.eobrun = (1 << run) - 1;
                if run > 0 {
                    dec.eobrun += bits(d, r, run);
                }
                break;
            }
            k += 16;
            continue;
        }
        k += run;
        if k > 63 {
            break;
        }
        dec.comps[ci].coef[off + ZIGZAG[k as usize]] = receive(d, r, s) * (1 << sc.al);
        k += 1;
    }
}

/// A nonzero coefficient's correction bit in an AC refinement scan.
fn refine_bit(d: &Vec<u8>, r: &mut Rd, dec: &mut Dec, ci: usize, at: usize, p1: i64) {
    let v = dec.comps[ci].coef[at];
    if bit(d, r) == 1 && (v & p1) == 0 {
        dec.comps[ci].coef[at] = if v >= 0 { v + p1 } else { v - p1 };
    }
}

fn block_ac_refine(d: &Vec<u8>, r: &mut Rd, dec: &mut Dec, ci: usize, off: usize, sc: &Scan) {
    let p1: i64 = 1 << sc.al;
    let m1: i64 = 0 - p1;
    let mut k = sc.ss;
    if dec.eobrun == 0 {
        while k <= sc.se {
            let rs = decode_huff(d, r, &dec.ac[dec.comps[ci].ta]);
            let s = rs & 15;
            let mut run = rs >> 4;
            let mut val: i64 = 0;
            if s == 0 {
                if run < 15 {
                    dec.eobrun = 1 << run;
                    if run > 0 {
                        dec.eobrun += bits(d, r, run);
                    }
                    break;
                }
            } else {
                val = if bit(d, r) == 1 { p1 } else { m1 };
            }
            // skip `run` zero coefficients, refining the nonzero ones passed
            while k <= sc.se {
                let at = off + ZIGZAG[k as usize];
                if dec.comps[ci].coef[at] != 0 {
                    refine_bit(d, r, dec, ci, at, p1);
                } else {
                    if run == 0 {
                        if val != 0 {
                            dec.comps[ci].coef[at] = val;
                        }
                        k += 1;
                        break;
                    }
                    run -= 1;
                }
                k += 1;
            }
        }
    }
    if dec.eobrun > 0 {
        while k <= sc.se {
            let at = off + ZIGZAG[k as usize];
            if dec.comps[ci].coef[at] != 0 {
                refine_bit(d, r, dec, ci, at, p1);
            }
            k += 1;
        }
        dec.eobrun -= 1;
    }
}

fn decode_block(d: &Vec<u8>, r: &mut Rd, dec: &mut Dec, ci: usize, row: i64, col: i64, sc: &Scan) {
    let c = &dec.comps[ci];
    if row >= c.bpc || col >= c.bpl {
        return;
    }
    let off = ((row * c.bpl + col) * 64) as usize;
    if !dec.progressive {
        block_baseline(d, r, dec, ci, off);
    } else if sc.ss == 0 {
        if sc.ah == 0 {
            block_dc_first(d, r, dec, ci, off, sc.al);
        } else {
            block_dc_refine(d, r, dec, ci, off, sc.al);
        }
    } else if sc.ah == 0 {
        block_ac_first(d, r, dec, ci, off, sc);
    } else {
        block_ac_refine(d, r, dec, ci, off, sc);
    }
}

/// The entropy-coded data of one scan from `start`; returns where it ends.
fn decode_scan(d: &Vec<u8>, start: usize, dec: &mut Dec, sc: &Scan) -> usize {
    let mut r = Rd { pos: start, buf: 0, nbits: 0, marker: false };
    for ci in sc.comps.iter() {
        dec.comps[*ci].pred = 0;
    }
    dec.eobrun = 0;
    let single = sc.comps.len() == 1;
    let total: i64 = if single {
        let c = &dec.comps[sc.comps[0]];
        own_blocks_x(dec, c.hs) * own_blocks_y(dec, c.vs)
    } else {
        dec.mcux * dec.mcuy
    };
    let mut n: i64 = 0;
    while n < total {
        if dec.restart > 0 && n > 0 && n % dec.restart == 0 {
            // the restart marker: realign, reset predictors and EOB runs
            r.nbits = 0;
            r.marker = false;
            while r.pos + 1 < d.len() && !(d[r.pos] == 255 && d[r.pos + 1] >= 0xD0 && d[r.pos + 1] <= 0xD7) {
                if d[r.pos] == 255 && d[r.pos + 1] != 0 && d[r.pos + 1] != 255 {
                    break;
                }
                r.pos += 1;
            }
            if r.pos + 1 < d.len() && d[r.pos] == 255 && d[r.pos + 1] >= 0xD0 && d[r.pos + 1] <= 0xD7 {
                r.pos += 2;
            }
            for ci in sc.comps.iter() {
                dec.comps[*ci].pred = 0;
            }
            dec.eobrun = 0;
        }
        if single {
            let ci = sc.comps[0];
            let cw = own_blocks_x(dec, dec.comps[ci].hs);
            let row = n / cw;
            let col = n % cw;
            decode_block(d, &mut r, dec, ci, row, col, sc);
        } else {
            let my = n / dec.mcux;
            let mx = n % dec.mcux;
            let mut k: usize = 0;
            while k < sc.comps.len() {
                let ci = sc.comps[k];
                let hs = dec.comps[ci].hs;
                let vs = dec.comps[ci].vs;
                let mut v: i64 = 0;
                while v < vs {
                    let mut h: i64 = 0;
                    while h < hs {
                        decode_block(d, &mut r, dec, ci, my * vs + v, mx * hs + h, sc);
                        h += 1;
                    }
                    v += 1;
                }
                k += 1;
            }
        }
        n += 1;
    }
    // to the next marker
    let mut p = r.pos;
    while p + 1 < d.len() {
        if d[p] == 255 && d[p + 1] != 0 && !(d[p + 1] >= 0xD0 && d[p + 1] <= 0xD7) {
            break;
        }
        p += 1;
    }
    p
}

fn dec_ceil(a: i64, b: i64) -> i64 {
    (a + b - 1) / b
}

/// The component's blocks per line and column in a non-interleaved scan:
/// those that cover its own samples, not the MCU padding.
fn own_blocks_x(dec: &Dec, hs: i64) -> i64 {
    dec_ceil(dec_ceil(dec.w * hs, dec.hmax), 8)
}

fn own_blocks_y(dec: &Dec, vs: i64) -> i64 {
    dec_ceil(dec_ceil(dec.h * vs, dec.vmax), 8)
}

/// The 8x8 inverse DCT of one block of dequantized coefficients, to samples
/// 0..255 in `out` at `o` with line length `stride`.
fn idct(blk: &Vec<f64>, cosv: &Vec<f64>, out: &mut Vec<u8>, o: usize, stride: usize) {
    let mut tmp: Vec<f64> = vec![0.0; 64];
    let mut y: usize = 0;
    while y < 8 {
        let mut x: usize = 0;
        while x < 8 {
            let mut s = 0.0;
            let mut u: usize = 0;
            while u < 8 {
                s += cosv[x * 8 + u] * blk[y * 8 + u];
                u += 1;
            }
            tmp[y * 8 + x] = s;
            x += 1;
        }
        y += 1;
    }
    let mut x: usize = 0;
    while x < 8 {
        let mut y2: usize = 0;
        while y2 < 8 {
            let mut s = 0.0;
            let mut v: usize = 0;
            while v < 8 {
                s += cosv[y2 * 8 + v] * tmp[v * 8 + x];
                v += 1;
            }
            out[o + y2 * stride + x] = clamp_byte(s / 4.0 + 128.0);
            y2 += 1;
        }
        x += 1;
    }
}

/// For each output position along one axis, the two samples around it and
/// the weight of the second.
struct Split {
    i0: Vec<usize>,
    i1: Vec<usize>,
    t: Vec<f64>,
}

fn axis(size: i64, s: i64, smax: i64, lim: i64) -> Split {
    let mut sp = Split { i0: vec![], i1: vec![], t: vec![] };
    let f = (s as f64) / (smax as f64);
    let mut i: i64 = 0;
    while i < size {
        if s == smax {
            sp.i0.push(i as usize);
            sp.i1.push(i as usize);
            sp.t.push(0.0);
        } else {
            let mut pos = ((i as f64) + 0.5) * f - 0.5;
            if pos < 0.0 {
                pos = 0.0;
            }
            let mut a = pos.floor() as i64;
            if a > lim {
                a = lim;
            }
            let b = if a + 1 > lim { lim } else { a + 1 };
            sp.i0.push(a as usize);
            sp.i1.push(b as usize);
            sp.t.push(pos - (a as f64));
        }
        i += 1;
    }
    sp
}

fn clamp_byte(v: f64) -> u8 {
    if v <= 0.0 {
        return 0;
    }
    if v >= 254.5 {
        return 255;
    }
    (v + 0.5) as u8
}

/// Decodes a JPEG file. `transform` is -1 to follow the file (the Adobe
/// marker, else YCbCr for 3 components), 0 or 1 to override it (PDF's
/// /ColorTransform).
pub fn decode(d: &Vec<u8>, transform: i64) -> Jpeg {
    if !is_jpeg(d) {
        return fail("not a JPEG file");
    }
    let mut dec = Dec { comps: vec![], dc: vec![], ac: vec![], qt: vec![], restart: 0, progressive: false, mcux: 0, mcuy: 0, hmax: 1, vmax: 1, eobrun: 0, frame: false, w: 0, h: 0 };
    let mut i: usize = 0;
    while i < 4 {
        dec.dc.push(empty_huff());
        dec.ac.push(empty_huff());
        dec.qt.push(vec![1; 64]);
        i += 1;
    }
    let mut adobe: i64 = -1;
    let mut jfif = false;
    let mut p: usize = 2;
    while p + 3 < d.len() {
        if d[p] != 255 {
            p += 1;
            continue;
        }
        let m = d[p + 1] as i64;
        if m == 255 {
            p += 1;
            continue;
        }
        p += 2;
        if m == 0xD9 {
            break;
        }
        if m >= 0xD0 && m <= 0xD7 || m == 0x01 {
            continue;
        }
        let len = u16_at(d, p) as usize;
        let seg = p + 2;
        let end = p + len;
        if len < 2 || end > d.len() {
            if dec.frame {
                break;
            }
            return fail("truncated segment");
        }
        if m == 0xDB {
            let mut q = seg;
            while q < end {
                let pq = (d[q] >> 4) as i64;
                let tq = (d[q] & 15) as usize;
                q += 1;
                let mut tab: Vec<i64> = vec![0; 64];
                let mut k: usize = 0;
                while k < 64 && tq < 4 {
                    let v = if pq == 0 { d[q] as i64 } else { u16_at(d, q) };
                    tab[ZIGZAG[k]] = v;
                    q += if pq == 0 { 1 } else { 2 };
                    k += 1;
                }
                if tq < 4 {
                    dec.qt[tq] = tab;
                } else {
                    break;
                }
            }
        } else if m == 0xC4 {
            let mut q = seg;
            while q + 17 <= end {
                let tc = (d[q] >> 4) as i64;
                let th = (d[q] & 15) as usize;
                let mut counts: Vec<i64> = vec![];
                let mut total: usize = 0;
                let mut k: usize = 0;
                while k < 16 {
                    let c = d[q + 1 + k] as i64;
                    counts.push(c);
                    total += c as usize;
                    k += 1;
                }
                q += 17;
                let mut vals: Vec<i64> = vec![];
                let mut j: usize = 0;
                while j < total && q < end {
                    vals.push(d[q] as i64);
                    q += 1;
                    j += 1;
                }
                if th < 4 {
                    if tc == 0 {
                        dec.dc[th] = build_huff(&counts, &vals);
                    } else {
                        dec.ac[th] = build_huff(&counts, &vals);
                    }
                }
            }
        } else if m == 0xDD {
            dec.restart = u16_at(d, seg);
        } else if m == 0xC0 || m == 0xC1 || m == 0xC2 {
            dec.progressive = m == 0xC2;
            if d[seg] != 8 {
                return fail("only 8-bit JPEG is read");
            }
            let h = u16_at(d, seg + 1);
            let w = u16_at(d, seg + 3);
            let nc = d[seg + 5] as usize;
            if w <= 0 || h <= 0 || nc == 0 || nc > 4 || w * h > 40000000 {
                return fail("bad frame size");
            }
            dec.w = w;
            dec.h = h;
            let mut k: usize = 0;
            while k < nc {
                let b = seg + 6 + k * 3;
                let mut hs = (d[b + 1] >> 4) as i64;
                let mut vs = (d[b + 1] & 15) as i64;
                if hs < 1 || hs > 4 {
                    hs = 1;
                }
                if vs < 1 || vs > 4 {
                    vs = 1;
                }
                dec.comps.push(Comp { id: d[b] as i64, hs: hs, vs: vs, tq: (d[b + 2] & 3) as usize, bpl: 0, bpc: 0, coef: vec![], pred: 0, td: 0, ta: 0 });
                if hs > dec.hmax {
                    dec.hmax = hs;
                }
                if vs > dec.vmax {
                    dec.vmax = vs;
                }
                k += 1;
            }
            dec.mcux = dec_ceil(w, 8 * dec.hmax);
            dec.mcuy = dec_ceil(h, 8 * dec.vmax);
            let mut c: usize = 0;
            while c < dec.comps.len() {
                let bpl = dec.mcux * dec.comps[c].hs;
                let bpc = dec.mcuy * dec.comps[c].vs;
                dec.comps[c].bpl = bpl;
                dec.comps[c].bpc = bpc;
                dec.comps[c].coef = vec![0; (bpl * bpc * 64) as usize];
                c += 1;
            }
            dec.frame = true;
        } else if m >= 0xC3 && m <= 0xCF && m != 0xC4 && m != 0xC8 && m != 0xCC {
            return fail("unsupported JPEG process (arithmetic, lossless or hierarchical)");
        } else if m == 0xEE {
            if len >= 12 && d[seg] == 65 && d[seg + 1] == 100 && d[seg + 2] == 111 && d[seg + 3] == 98 && d[seg + 4] == 101 {
                adobe = d[seg + 11] as i64;
            }
        } else if m == 0xE0 {
            if len >= 7 && d[seg] == 74 && d[seg + 1] == 70 && d[seg + 2] == 73 && d[seg + 3] == 70 {
                jfif = true;
            }
        } else if m == 0xDA {
            if !dec.frame {
                return fail("scan before the frame header");
            }
            let ns = d[seg] as usize;
            let mut sc = Scan { comps: vec![], ss: 0, se: 63, ah: 0, al: 0 };
            let mut k: usize = 0;
            while k < ns {
                let id = d[seg + 1 + k * 2] as i64;
                let tt = d[seg + 2 + k * 2];
                let mut c: usize = 0;
                while c < dec.comps.len() {
                    if dec.comps[c].id == id {
                        dec.comps[c].td = ((tt >> 4) & 3) as usize;
                        dec.comps[c].ta = (tt & 3) as usize;
                        sc.comps.push(c);
                    }
                    c += 1;
                }
                k += 1;
            }
            let b = seg + 1 + ns * 2;
            sc.ss = d[b] as i64;
            sc.se = d[b + 1] as i64;
            sc.ah = (d[b + 2] >> 4) as i64;
            sc.al = (d[b + 2] & 15) as i64;
            if sc.se > 63 {
                sc.se = 63;
            }
            if sc.comps.is_empty() {
                return fail("scan names no component");
            }
            p = decode_scan(d, end, &mut dec, &sc);
            continue;
        }
        p = end;
    }
    if !dec.frame {
        return fail("no frame header");
    }
    output(&dec, adobe, jfif, transform)
}

fn output(dec: &Dec, adobe: i64, jfif: bool, transform: i64) -> Jpeg {
    let w = dec.w;
    let h = dec.h;
    let nc = dec.comps.len();
    // the cosine table: c(u) cos((2x + 1) u pi / 16)
    let mut cosv: Vec<f64> = vec![];
    let mut x: usize = 0;
    while x < 8 {
        let mut u: usize = 0;
        while u < 8 {
            let cu = if u == 0 { 1.0 / (2.0 as f64).sqrt() } else { 1.0 };
            cosv.push(cu * ((((2 * x + 1) * u) as f64) * 3.141592653589793 / 16.0).cos());
            u += 1;
        }
        x += 1;
    }
    // each component's samples at its own resolution
    let mut planes: Vec<Vec<u8>> = vec![];
    let mut blk: Vec<f64> = vec![0.0; 64];
    let mut c: usize = 0;
    while c < nc {
        let cp = &dec.comps[c];
        let stride = (cp.bpl * 8) as usize;
        let mut plane: Vec<u8> = vec![0; stride * ((cp.bpc * 8) as usize)];
        let q = &dec.qt[cp.tq];
        let mut row: i64 = 0;
        while row < cp.bpc {
            let mut col: i64 = 0;
            while col < cp.bpl {
                let off = ((row * cp.bpl + col) * 64) as usize;
                let mut k: usize = 0;
                while k < 64 {
                    blk[k] = (cp.coef[off + k] * q[k]) as f64;
                    k += 1;
                }
                idct(&blk, &cosv, &mut plane, (row as usize) * 8 * stride + (col as usize) * 8, stride);
                col += 1;
            }
            row += 1;
        }
        planes.push(plane);
        c += 1;
    }
    let mut ct: i64 = transform;
    if ct < 0 {
        if nc == 3 {
            ct = if adobe == 0 && !jfif { 0 } else { 1 };
            // RGB files that name their components 'R', 'G', 'B'
            if adobe < 0 && !jfif && dec.comps[0].id == 82 && dec.comps[1].id == 71 && dec.comps[2].id == 66 {
                ct = 0;
            }
        } else if nc == 4 {
            ct = if adobe == 2 { 1 } else { 0 };
        } else {
            ct = 0;
        }
    }
    let n = if nc == 2 { 1 } else { nc };
    let mut data: Vec<u8> = vec![0; (w * h) as usize * n];
    // subsampled components are interpolated between sample centres, as
    // libjpeg's "fancy" upsampling and browsers do
    let mut x0: Vec<Vec<usize>> = vec![];
    let mut x1: Vec<Vec<usize>> = vec![];
    let mut xw: Vec<Vec<f64>> = vec![];
    let mut c2: usize = 0;
    while c2 < nc {
        let lim = dec_ceil(w * dec.comps[c2].hs, dec.hmax) - 1;
        let sp = axis(w, dec.comps[c2].hs, dec.hmax, lim);
        x0.push(sp.i0);
        x1.push(sp.i1);
        xw.push(sp.t);
        c2 += 1;
    }
    let mut ys: Vec<Split> = vec![];
    let mut c4: usize = 0;
    while c4 < nc {
        let lim = dec_ceil(h * dec.comps[c4].vs, dec.vmax) - 1;
        ys.push(axis(h, dec.comps[c4].vs, dec.vmax, lim));
        c4 += 1;
    }
    let mut y: i64 = 0;
    let mut o: usize = 0;
    let mut px: Vec<f64> = vec![0.0; 4];
    while y < h {
        let mut r0: Vec<usize> = vec![];
        let mut r1: Vec<usize> = vec![];
        let mut wy: Vec<f64> = vec![];
        let mut c3: usize = 0;
        while c3 < nc {
            let stride = (dec.comps[c3].bpl * 8) as usize;
            r0.push(ys[c3].i0[y as usize] * stride);
            r1.push(ys[c3].i1[y as usize] * stride);
            wy.push(ys[c3].t[y as usize]);
            c3 += 1;
        }
        let mut xx: usize = 0;
        while xx < (w as usize) {
            let mut k: usize = 0;
            while k < n {
                let pl = &planes[k];
                let a0 = x0[k][xx];
                let a1 = x1[k][xx];
                let tx = xw[k][xx];
                let ty = wy[k];
                if tx == 0.0 && ty == 0.0 {
                    px[k] = pl[r0[k] + a0] as f64;
                } else {
                    let top = (pl[r0[k] + a0] as f64) * (1.0 - tx) + (pl[r0[k] + a1] as f64) * tx;
                    let bot = (pl[r1[k] + a0] as f64) * (1.0 - tx) + (pl[r1[k] + a1] as f64) * tx;
                    px[k] = top * (1.0 - ty) + bot * ty;
                }
                k += 1;
            }
            if ct == 1 && (nc == 3 || nc == 4) {
                let yy = px[0];
                let cb = px[1] - 128.0;
                let cr = px[2] - 128.0;
                let r = clamp_byte(yy + 1.402 * cr);
                let g = clamp_byte(yy - 0.344136 * cb - 0.714136 * cr);
                let b = clamp_byte(yy + 1.772 * cb);
                if nc == 3 {
                    data[o] = r;
                    data[o + 1] = g;
                    data[o + 2] = b;
                } else {
                    // YCCK: the inverted RGB is CMY
                    data[o] = 255 - r;
                    data[o + 1] = 255 - g;
                    data[o + 2] = 255 - b;
                    data[o + 3] = clamp_byte(px[3]);
                }
            } else {
                let mut k2: usize = 0;
                while k2 < n {
                    data[o + k2] = clamp_byte(px[k2]);
                    k2 += 1;
                }
            }
            o += n;
            xx += 1;
        }
        y += 1;
    }
    Jpeg { ok: true, error: String::new(), w: w, h: h, n: n as i64, data: data, adobe: adobe }
}

/// The picture as RGBA: CMYK as Adobe applications write it (inverted when
/// the file has the Adobe marker).
pub fn to_rgba(j: &Jpeg) -> Vec<u8> {
    let total = (j.w * j.h) as usize;
    let mut out: Vec<u8> = vec![0; total * 4];
    let n = j.n as usize;
    let mut i: usize = 0;
    while i < total {
        let b = i * n;
        if n == 1 {
            out[i * 4] = j.data[b];
            out[i * 4 + 1] = j.data[b];
            out[i * 4 + 2] = j.data[b];
        } else if n == 3 {
            out[i * 4] = j.data[b];
            out[i * 4 + 1] = j.data[b + 1];
            out[i * 4 + 2] = j.data[b + 2];
        } else {
            let mut cc = j.data[b] as i64;
            let mut mm = j.data[b + 1] as i64;
            let mut yy = j.data[b + 2] as i64;
            let mut kk = j.data[b + 3] as i64;
            if j.adobe >= 0 {
                cc = 255 - cc;
                mm = 255 - mm;
                yy = 255 - yy;
                kk = 255 - kk;
            }
            out[i * 4] = ((255 - cc) * (255 - kk) / 255) as u8;
            out[i * 4 + 1] = ((255 - mm) * (255 - kk) / 255) as u8;
            out[i * 4 + 2] = ((255 - yy) * (255 - kk) / 255) as u8;
        }
        out[i * 4 + 3] = 255;
        i += 1;
    }
    out
}
