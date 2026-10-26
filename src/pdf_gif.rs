//! GIF (87a and 89a): the first frame on the logical screen, with its
//! transparent colour as alpha 0.

use crate::pdf_picture;
use crate::pdf_picture::PicImage;

fn le16(d: &Vec<u8>, p: usize) -> i64 {
    if p + 1 >= d.len() {
        return 0;
    }
    (d[p] as i64) + (d[p + 1] as i64) * 256
}

/// The sub-blocks from `p` joined; `end` is set to the byte after them.
struct Blocks {
    data: Vec<u8>,
    end: usize,
}

fn blocks(d: &Vec<u8>, from: usize) -> Blocks {
    let mut out = Blocks { data: vec![], end: from };
    let mut p = from;
    while p < d.len() {
        let n = d[p] as usize;
        p += 1;
        if n == 0 {
            break;
        }
        let mut i: usize = 0;
        while i < n && p + i < d.len() {
            out.data.push(d[p + i]);
            i += 1;
        }
        p += n;
    }
    out.end = p;
    out
}

/// GIF's LZW: codes are read least significant bit first and grow to 12 bits.
fn lzw(src: &Vec<u8>, min_size: i64, max_out: usize) -> Vec<u8> {
    let mut out: Vec<u8> = vec![];
    if min_size < 1 || min_size > 11 {
        return out;
    }
    let clear: i64 = 1 << min_size;
    let eoi = clear + 1;
    let mut size = min_size + 1;
    let mut next = clear + 2;
    // each code's prefix code and last byte; its first byte for KwKwK
    let mut prefix: Vec<i64> = vec![-1; 4096];
    let mut last: Vec<u8> = vec![0; 4096];
    let mut first: Vec<u8> = vec![0; 4096];
    let mut i: i64 = 0;
    while i < clear {
        last[i as usize] = i as u8;
        first[i as usize] = i as u8;
        i += 1;
    }
    let mut old: i64 = -1;
    let mut bitpos: usize = 0;
    let total_bits = src.len() * 8;
    let mut stack: Vec<u8> = vec![];
    while bitpos + (size as usize) <= total_bits && out.len() < max_out {
        let mut code: i64 = 0;
        let mut k: i64 = 0;
        while k < size {
            let b = bitpos + (k as usize);
            if ((src[b / 8] >> (b % 8)) & 1) == 1 {
                code = code | (1 << k);
            }
            k += 1;
        }
        bitpos += size as usize;
        if code == clear {
            size = min_size + 1;
            next = clear + 2;
            old = -1;
            continue;
        }
        if code == eoi {
            break;
        }
        if old < 0 {
            if code >= clear {
                break;
            }
            out.push(last[code as usize]);
            old = code;
            continue;
        }
        let known = code < next;
        let mut c = if known { code } else { old };
        stack = vec![];
        if !known {
            if code != next {
                break;
            }
            stack.push(first[old as usize]);
        }
        while c >= 0 {
            stack.push(last[c as usize]);
            c = prefix[c as usize];
        }
        let fb = stack[stack.len() - 1];
        let mut s = stack.len();
        while s > 0 {
            out.push(stack[s - 1]);
            s -= 1;
        }
        if next < 4096 {
            prefix[next as usize] = old;
            last[next as usize] = fb;
            first[next as usize] = first[old as usize];
            next += 1;
            if next == (1 << size) && size < 12 {
                size += 1;
            }
        }
        old = code;
    }
    out
}

pub fn decode(d: &Vec<u8>) -> PicImage {
    let sw = le16(d, 6);
    let sh = le16(d, 8);
    let flags = d[10] as i64;
    let mut p: usize = 13;
    let mut gct: Vec<u8> = vec![];
    if (flags & 128) != 0 {
        let n = 3 * (1 << ((flags & 7) + 1)) as usize;
        let mut i: usize = 0;
        while i < n && p + i < d.len() {
            gct.push(d[p + i]);
            i += 1;
        }
        p += n;
    }
    let mut transparent: i64 = -1;
    while p < d.len() {
        let b = d[p];
        if b == 0x21 {
            let label = if p + 1 < d.len() { d[p + 1] } else { 0 };
            if label == 0xF9 && p + 6 < d.len() {
                // graphic control: the transparent colour of the next frame
                if (d[p + 3] & 1) != 0 {
                    transparent = d[p + 6] as i64;
                }
            }
            p = blocks(d, p + 2).end;
        } else if b == 0x2C {
            let fx = le16(d, p + 1);
            let fy = le16(d, p + 3);
            let fw = le16(d, p + 5);
            let fh = le16(d, p + 7);
            let ff = if p + 9 < d.len() { d[p + 9] as i64 } else { 0 };
            p += 10;
            let mut ct = gct.clone();
            if (ff & 128) != 0 {
                let n = 3 * (1 << ((ff & 7) + 1)) as usize;
                ct = vec![];
                let mut i: usize = 0;
                while i < n && p + i < d.len() {
                    ct.push(d[p + i]);
                    i += 1;
                }
                p += n;
            }
            let interlaced = (ff & 64) != 0;
            let min_size = if p < d.len() { d[p] as i64 } else { 0 };
            let bl = blocks(d, p + 1);
            // a screen size of 0 means the frame's
            let w = if sw > 0 { sw } else { fx + fw };
            let h = if sh > 0 { sh } else { fy + fh };
            if w <= 0 || h <= 0 || w * h > pdf_picture::MAX_PIXELS || fw * fh > pdf_picture::MAX_PIXELS {
                return pdf_picture::failed("gif", "bad image size");
            }
            let px = lzw(&bl.data, min_size, (fw * fh) as usize);
            let mut rgba: Vec<u8> = vec![0; (w * h * 4) as usize];
            // the rows of each interlace pass, in the order they are stored
            let mut order: Vec<i64> = vec![];
            if interlaced {
                let starts: Vec<i64> = vec![0, 4, 2, 1];
                let steps: Vec<i64> = vec![8, 8, 4, 2];
                let mut k: usize = 0;
                while k < 4 {
                    let mut r = starts[k];
                    while r < fh {
                        order.push(r);
                        r += steps[k];
                    }
                    k += 1;
                }
            } else {
                let mut r: i64 = 0;
                while r < fh {
                    order.push(r);
                    r += 1;
                }
            }
            let mut i: usize = 0;
            let mut row: usize = 0;
            while row < order.len() {
                let y = fy + order[row];
                let mut x: i64 = 0;
                while x < fw {
                    if i < px.len() {
                        let c = px[i] as i64;
                        let sx = fx + x;
                        if c != transparent && sx < w && y < h {
                            let at = ((y * w + sx) * 4) as usize;
                            let q = (c * 3) as usize;
                            if q + 2 < ct.len() {
                                rgba[at] = ct[q];
                                rgba[at + 1] = ct[q + 1];
                                rgba[at + 2] = ct[q + 2];
                            }
                            rgba[at + 3] = 255;
                        }
                    }
                    i += 1;
                    x += 1;
                }
                row += 1;
            }
            return pdf_picture::made("gif", w, h, rgba);
        } else if b == 0x3B {
            break;
        } else {
            p += 1;
        }
    }
    pdf_picture::failed("gif", "no image in the file")
}
