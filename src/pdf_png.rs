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

/// `v` as 4 big-endian bytes. Helpers return new bytes: a Ranger buffer
/// (`Vec<u8>`) cannot grow through a `&mut` parameter.
fn be32(v: i64) -> Vec<u8> {
    vec![((v >> 24) & 255) as u8, ((v >> 16) & 255) as u8, ((v >> 8) & 255) as u8, (v & 255) as u8]
}

/// One PNG chunk: length, kind, data, CRC.
fn chunk(crc: &Crc, kind: &str, data: &Vec<u8>) -> Vec<u8> {
    let mut out: Vec<u8> = be32(data.len() as i64);
    for b in pdf_obj::bytes_of(kind).iter() {
        out.push(*b);
    }
    for b in data.iter() {
        out.push(*b);
    }
    let end = out.len();
    let c = crc.of(&out, 4, end);
    for b in c.iter() {
        out.push(*b);
    }
    out
}

/// `px` is w * h * 4 bytes, RGBA, rows top to bottom.
pub fn encode_rgba(w: i64, h: i64, px: &Vec<u8>) -> Vec<u8> {
    let crc = Crc::new();
    let mut out: Vec<u8> = vec![137, 80, 78, 71, 13, 10, 26, 10];
    let mut ihdr: Vec<u8> = be32(w);
    for b in be32(h).iter() {
        ihdr.push(*b);
    }
    ihdr.push(8); // bit depth
    ihdr.push(6); // RGBA
    ihdr.push(0);
    ihdr.push(0);
    ihdr.push(0);
    for b in chunk(&crc, "IHDR", &ihdr).iter() {
        out.push(*b);
    }
    // the scanlines with filter type 0, in stored deflate blocks
    let row = (w * 4) as usize;
    let total = (h as usize) * (row + 1);
    let mut z: Vec<u8> = vec![0x78, 0x01];
    let mut s1: i64 = 1;
    let mut s2: i64 = 0;
    let mut done: usize = 0;
    let mut y: usize = 0;
    let mut x: usize = 0; // position in the row, 0 = the filter byte
    while done < total {
        let mut n = total - done;
        if n > 65535 {
            n = 65535;
        }
        z.push(if done + n == total { 1 } else { 0 });
        z.push((n & 255) as u8);
        z.push(((n >> 8) & 255) as u8);
        let nn = 65535 - n;
        z.push((nn & 255) as u8);
        z.push(((nn >> 8) & 255) as u8);
        let mut k: usize = 0;
        while k < n {
            let b: u8 = if x == 0 { 0 } else { px[y * row + x - 1] };
            z.push(b);
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
    if total == 0 {
        z.push(1);
        z.push(0);
        z.push(0);
        z.push(255);
        z.push(255);
    }
    z.push(((s2 >> 8) & 255) as u8);
    z.push((s2 & 255) as u8);
    z.push(((s1 >> 8) & 255) as u8);
    z.push((s1 & 255) as u8);
    for b in chunk(&crc, "IDAT", &z).iter() {
        out.push(*b);
    }
    let none: Vec<u8> = vec![];
    for b in chunk(&crc, "IEND", &none).iter() {
        out.push(*b);
    }
    out
}
