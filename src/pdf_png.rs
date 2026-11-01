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
