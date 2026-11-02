//! RGBA pixels as a PNG file, for hosts whose image cache decodes PNG. The
//! image data is stored (deflate without compression): the file is made to be
//! decoded at once, not kept. Checksums are kept in 16-bit halves, so that
//! every target's integers hold them.

use crate::pdf_obj;

pub struct Crc {
    hi: Vec<i64>,
    lo: Vec<i64>,
}

impl Crc {
    pub fn new() -> Crc {
        let mut hi: Vec<i64> = vec![];
        let mut lo: Vec<i64> = vec![];
        let mut n: i64 = 0;
        while n < 256 {
            let mut h: i64 = 0;
            let mut l: i64 = n;
            let mut k = 0;
            while k < 8 {
                let odd = (l & 1) == 1;
                l = (l >> 1) | ((h & 1) << 15);
                h = h >> 1;
                if odd {
                    h = h ^ 0xEDB8;
                    l = l ^ 0x8320;
                }
                k += 1;
            }
            hi.push(h);
            lo.push(l);
            n += 1;
        }
        Crc { hi: hi, lo: lo }
    }

    /// The CRC-32 of `d[from..to]`, as 4 big-endian bytes.
    pub fn of(&self, d: &Vec<u8>, from: usize, to: usize) -> Vec<u8> {
        let mut h: i64 = 0xFFFF;
        let mut l: i64 = 0xFFFF;
        let mut i = from;
        while i < to {
            let idx = ((l ^ (d[i] as i64)) & 255) as usize;
            let nl = (l >> 8) | ((h & 255) << 8);
            let nh = h >> 8;
            h = nh ^ self.hi[idx];
            l = nl ^ self.lo[idx];
            i += 1;
        }
        h = h ^ 0xFFFF;
        l = l ^ 0xFFFF;
        vec![(h >> 8) as u8, (h & 255) as u8, (l >> 8) as u8, (l & 255) as u8]
    }
}

/// `v` as 4 big-endian bytes into `out` at `at`.
fn put32(out: &mut Vec<u8>, at: usize, v: i64) {
    out[at] = ((v >> 24) & 255) as u8;
    out[at + 1] = ((v >> 16) & 255) as u8;
    out[at + 2] = ((v >> 8) & 255) as u8;
    out[at + 3] = (v & 255) as u8;
}

/// A chunk's kind (4 ASCII letters) into `out` at `at`.
fn put_kind(out: &mut Vec<u8>, at: usize, kind: &str) {
    let k = pdf_obj::bytes_of(kind);
    let mut i: usize = 0;
    while i < 4 {
        out[at + i] = k[i];
        i += 1;
    }
}

/// The CRC of the chunk that starts at `at` with `n` data bytes, after it.
fn put_crc(out: &mut Vec<u8>, crc: &Crc, at: usize, n: usize) {
    let c = crc.of(out, at + 4, at + 8 + n);
    let mut i: usize = 0;
    while i < 4 {
        out[at + 8 + n + i] = c[i];
        i += 1;
    }
}

/// `px` is w * h * 4 bytes, RGBA, rows top to bottom. The file is written
/// into one buffer of its final size: a Ranger buffer that grows is copied
/// on every push on most targets, which made a page's PNG take minutes.
pub fn encode_rgba(w: i64, h: i64, px: &Vec<u8>) -> Vec<u8> {
    let crc = Crc::new();
    // the scanlines with filter type 0, in stored deflate blocks
    let row = (w * 4) as usize;
    let total = (h as usize) * (row + 1);
    let mut blocks: usize = (total + 65534) / 65535;
    if blocks == 0 {
        blocks = 1;
    }
    let zlen = 2 + blocks * 5 + total + 4;
    let ihdr_at: usize = 8;
    let idat_at: usize = ihdr_at + 12 + 13;
    let iend_at: usize = idat_at + 12 + zlen;
    let size = iend_at + 12;
    let mut out: Vec<u8> = vec![0; size];
    let sig: Vec<u8> = vec![137, 80, 78, 71, 13, 10, 26, 10];
    let mut i: usize = 0;
    while i < 8 {
        out[i] = sig[i];
        i += 1;
    }
    put32(&mut out, ihdr_at, 13);
    put_kind(&mut out, ihdr_at + 4, "IHDR");
    put32(&mut out, ihdr_at + 8, w);
    put32(&mut out, ihdr_at + 12, h);
    out[ihdr_at + 16] = 8; // bit depth
    out[ihdr_at + 17] = 6; // RGBA
    // compression, filter, interlace stay 0
    put_crc(&mut out, &crc, ihdr_at, 13);
    put32(&mut out, idat_at, zlen as i64);
    put_kind(&mut out, idat_at + 4, "IDAT");
    let mut p: usize = idat_at + 8;
    out[p] = 0x78;
    out[p + 1] = 0x01;
    p += 2;
    let mut s1: i64 = 1;
    let mut s2: i64 = 0;
    let mut done: usize = 0;
    let mut y: usize = 0;
    let mut x: usize = 0; // position in the row, 0 = the filter byte
    let mut first = true;
    while first || done < total {
        first = false;
        let mut n = total - done;
        if n > 65535 {
            n = 65535;
        }
        out[p] = if done + n == total { 1 } else { 0 };
        out[p + 1] = (n & 255) as u8;
        out[p + 2] = ((n >> 8) & 255) as u8;
        let nn = 65535 - n;
        out[p + 3] = (nn & 255) as u8;
        out[p + 4] = ((nn >> 8) & 255) as u8;
        p += 5;
        let mut k: usize = 0;
        while k < n {
            let b: u8 = if x == 0 { 0 } else { px[y * row + x - 1] };
            out[p] = b;
            p += 1;
            s1 = (s1 + (b as i64)) % 65521;
            s2 = (s2 + s1) % 65521;
            x += 1;
            if x > row {
                x = 0;
                y += 1;
            }
            k += 1;
        }
        done += n;
    }
    out[p] = ((s2 >> 8) & 255) as u8;
    out[p + 1] = (s2 & 255) as u8;
    out[p + 2] = ((s1 >> 8) & 255) as u8;
    out[p + 3] = (s1 & 255) as u8;
    put_crc(&mut out, &crc, idat_at, zlen);
    put32(&mut out, iend_at, 0);
    put_kind(&mut out, iend_at + 4, "IEND");
    put_crc(&mut out, &crc, iend_at, 0);
    out
}

// ---- a compressed PNG: each row's filter chosen by the smallest sum of
// differences (libpng's heuristic), then deflate with the fixed Huffman
// codes and back-references found by a hash of 3 bytes. Much smaller than
// encode_rgba for drawn pages and pictures, a few times slower; for a file
// that is sent or kept (pdf_svg's images).

const LEN_BASE: [i64; 29] = [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
const LEN_EXTRA: [i64; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const DIST_BASE: [i64; 30] = [1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577];
const DIST_EXTRA: [i64; 30] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];

/// Bits written least significant first into a buffer of its final size.
struct PngBits {
    out: Vec<u8>,
    at: usize,
    acc: i64,
    n: i64,
}

impl PngBits {
    fn put(&mut self, v: i64, bits: i64) {
        self.acc = self.acc | (v << self.n);
        self.n += bits;
        while self.n >= 8 {
            self.out[self.at] = (self.acc & 255) as u8;
            self.at += 1;
            self.acc = self.acc >> 8;
            self.n -= 8;
        }
    }

    /// a Huffman code: its bits most significant first
    fn code(&mut self, c: i64, bits: i64) {
        let mut r: i64 = 0;
        let mut i: i64 = 0;
        while i < bits {
            r = (r << 1) | ((c >> i) & 1);
            i += 1;
        }
        self.put(r, bits);
    }

    fn lit(&mut self, v: i64) {
        if v < 144 {
            self.code(0x30 + v, 8);
        } else if v < 256 {
            self.code(0x190 + v - 144, 9);
        } else if v < 280 {
            self.code(v - 256, 7);
        } else {
            self.code(0xC0 + v - 280, 8);
        }
    }

    fn flush(&mut self) {
        if self.n > 0 {
            self.out[self.at] = (self.acc & 255) as u8;
            self.at += 1;
            self.acc = 0;
            self.n = 0;
        }
    }
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

/// The scanlines, each with the filter that makes its bytes smallest
fn filtered(w: i64, h: i64, px: &Vec<u8>) -> Vec<u8> {
    let row = (w * 4) as usize;
    let mut out: Vec<u8> = vec![0; (h as usize) * (row + 1)];
    let mut y: usize = 0;
    while y < h as usize {
        let mut best: i64 = 0;
        let mut best_sum: i64 = -1;
        let mut f: i64 = 0;
        while f < 5 {
            let mut sum: i64 = 0;
            let mut x: usize = 0;
            while x < row {
                let v = filter_byte(px, row, y, x, f);
                sum += if v < 128 { v } else { 256 - v };
                x += 1;
            }
            if best_sum < 0 || sum < best_sum {
                best_sum = sum;
                best = f;
            }
            f += 1;
        }
        let at = y * (row + 1);
        out[at] = best as u8;
        let mut x: usize = 0;
        while x < row {
            out[at + 1 + x] = filter_byte(px, row, y, x, best) as u8;
            x += 1;
        }
        y += 1;
    }
    out
}

fn filter_byte(px: &Vec<u8>, row: usize, y: usize, x: usize, f: i64) -> i64 {
    let cur = px[y * row + x] as i64;
    let a = if x >= 4 { px[y * row + x - 4] as i64 } else { 0 };
    let b = if y > 0 { px[(y - 1) * row + x] as i64 } else { 0 };
    let c = if x >= 4 && y > 0 { px[(y - 1) * row + x - 4] as i64 } else { 0 };
    let p = if f == 0 { 0 } else if f == 1 { a } else if f == 2 { b } else if f == 3 { (a + b) / 2 } else { paeth(a, b, c) };
    (cur - p + 256) & 255
}

/// `d` as a zlib stream: fixed Huffman codes, matches up to 258 bytes back
/// as far as 32 KB, found through a hash of 3 bytes (at most 24 tried)
pub fn zlib_fixed(d: &Vec<u8>) -> Vec<u8> {
    let n = d.len();
    let mut bits = PngBits { out: vec![0; n + n / 8 + 64], at: 0, acc: 0, n: 0 };
    bits.put(0x78, 8);
    bits.put(0x01, 8);
    bits.put(1, 1); // the last block
    bits.put(1, 2); // fixed codes
    let hsize: usize = 1 << 15;
    let mut head: Vec<i64> = vec![-1; hsize];
    let mut prev: Vec<i64> = vec![-1; 32768];
    let mut i: usize = 0;
    while i < n {
        let mut best_len: usize = 0;
        let mut best_dist: usize = 0;
        if i + 2 < n {
            let hsh = (((d[i] as usize) << 10) ^ ((d[i + 1] as usize) << 5) ^ (d[i + 2] as usize)) & (hsize - 1);
            let mut cand = head[hsh];
            let mut tries = 0;
            while cand >= 0 && tries < 24 {
                let c = cand as usize;
                if i - c > 32768 {
                    break;
                }
                let mut l: usize = 0;
                while l < 258 && i + l < n && d[c + l] == d[i + l] {
                    l += 1;
                }
                if l > best_len {
                    best_len = l;
                    best_dist = i - c;
                    if l == 258 {
                        break;
                    }
                }
                cand = prev[c & 32767];
                tries += 1;
            }
            prev[i & 32767] = head[hsh];
            head[hsh] = i as i64;
        }
        if best_len >= 3 {
            let mut k: usize = 0;
            while k < 28 && LEN_BASE[k + 1] as usize <= best_len {
                k += 1;
            }
            bits.lit(257 + k as i64);
            bits.put((best_len as i64) - LEN_BASE[k], LEN_EXTRA[k]);
            let mut dk: usize = 0;
            while dk < 29 && DIST_BASE[dk + 1] as usize <= best_dist {
                dk += 1;
            }
            bits.code(dk as i64, 5);
            bits.put((best_dist as i64) - DIST_BASE[dk], DIST_EXTRA[dk]);
            // the bytes inside the match go into the hash too
            let mut j: usize = 1;
            while j < best_len {
                let q = i + j;
                if q + 2 < n {
                    let hq = (((d[q] as usize) << 10) ^ ((d[q + 1] as usize) << 5) ^ (d[q + 2] as usize)) & (hsize - 1);
                    prev[q & 32767] = head[hq];
                    head[hq] = q as i64;
                }
                j += 1;
            }
            i += best_len;
        } else {
            bits.lit(d[i] as i64);
            i += 1;
        }
    }
    bits.lit(256);
    bits.flush();
    // Adler-32
    let mut s1: i64 = 1;
    let mut s2: i64 = 0;
    let mut k: usize = 0;
    while k < n {
        s1 = (s1 + (d[k] as i64)) % 65521;
        s2 = (s2 + s1) % 65521;
        k += 1;
    }
    bits.put((s2 >> 8) & 255, 8);
    bits.put(s2 & 255, 8);
    bits.put((s1 >> 8) & 255, 8);
    bits.put(s1 & 255, 8);
    let mut out: Vec<u8> = vec![0; bits.at];
    let mut m: usize = 0;
    while m < bits.at {
        out[m] = bits.out[m];
        m += 1;
    }
    out
}

/// `px` (w * h * 4 bytes, RGBA) as a compressed PNG
pub fn encode_rgba_small(w: i64, h: i64, px: &Vec<u8>) -> Vec<u8> {
    let crc = Crc::new();
    let z = zlib_fixed(&filtered(w, h, px));
    let zlen = z.len();
    let ihdr_at: usize = 8;
    let idat_at: usize = ihdr_at + 12 + 13;
    let iend_at: usize = idat_at + 12 + zlen;
    let mut out: Vec<u8> = vec![0; iend_at + 12];
    let sig: Vec<u8> = vec![137, 80, 78, 71, 13, 10, 26, 10];
    let mut i: usize = 0;
    while i < 8 {
        out[i] = sig[i];
        i += 1;
    }
    put32(&mut out, ihdr_at, 13);
    put_kind(&mut out, ihdr_at + 4, "IHDR");
    put32(&mut out, ihdr_at + 8, w);
    put32(&mut out, ihdr_at + 12, h);
    out[ihdr_at + 16] = 8;
    out[ihdr_at + 17] = 6;
    put_crc(&mut out, &crc, ihdr_at, 13);
    put32(&mut out, idat_at, zlen as i64);
    put_kind(&mut out, idat_at + 4, "IDAT");
    let mut k: usize = 0;
    while k < zlen {
        out[idat_at + 8 + k] = z[k];
        k += 1;
    }
    put_crc(&mut out, &crc, idat_at, zlen);
    put32(&mut out, iend_at, 0);
    put_kind(&mut out, iend_at + 4, "IEND");
    put_crc(&mut out, &crc, iend_at, 0);
    out
}
