//! TIFF: the first page (IFD), in strips or tiles, uncompressed, PackBits,
//! LZW, Deflate or JPEG; bilevel, grey, palette, RGB, CMYK and YCbCr (JPEG)
//! with 1 to 16 bits per sample, an alpha channel, the horizontal predictor
//! and separate planes. CCITT fax compression is not read.

use crate::pdf_filters;
use crate::pdf_inflate;
use crate::pdf_jpeg;
use crate::pdf_picture;
use crate::pdf_picture::PicImage;

struct Tif {
    le: bool,
}

fn u16_at(d: &Vec<u8>, t: &Tif, p: usize) -> i64 {
    if p + 1 >= d.len() {
        return 0;
    }
    if t.le {
        return (d[p] as i64) + (d[p + 1] as i64) * 256;
    }
    (d[p] as i64) * 256 + (d[p + 1] as i64)
}

fn u32_at(d: &Vec<u8>, t: &Tif, p: usize) -> i64 {
    if p + 3 >= d.len() {
        return 0;
    }
    if t.le {
        return (d[p] as i64) + (d[p + 1] as i64) * 256 + (d[p + 2] as i64) * 65536 + (d[p + 3] as i64) * 16777216;
    }
    (d[p] as i64) * 16777216 + (d[p + 1] as i64) * 65536 + (d[p + 2] as i64) * 256 + (d[p + 3] as i64)
}

/// The values of a tag (BYTE, SHORT or LONG; others read as their first word).
fn values(d: &Vec<u8>, t: &Tif, entry: usize) -> Vec<i64> {
    let typ = u16_at(d, t, entry + 2);
    let count = u32_at(d, t, entry + 4);
    let size: i64 = if typ == 3 { 2 } else if typ == 4 || typ == 9 { 4 } else if typ == 1 || typ == 2 || typ == 6 || typ == 7 { 1 } else { 4 };
    let mut out: Vec<i64> = vec![];
    if count <= 0 || count > 100000000 {
        return out;
    }
    let at: usize = if size * count <= 4 { entry + 8 } else { u32_at(d, t, entry + 8) as usize };
    let mut i: i64 = 0;
    while i < count {
        let p = at + ((i * size) as usize);
        if p + (size as usize) > d.len() {
            break;
        }
        let v = if size == 1 { d[p] as i64 } else if size == 2 { u16_at(d, t, p) } else { u32_at(d, t, p) };
        out.push(v);
        i += 1;
    }
    out
}

fn first(v: &Vec<i64>, dflt: i64) -> i64 {
    if v.is_empty() {
        return dflt;
    }
    v[0]
}

fn packbits(src: &Vec<u8>, want: usize) -> Vec<u8> {
    let mut out: Vec<u8> = vec![];
    let mut p: usize = 0;
    while p < src.len() && out.len() < want {
        let n = src[p] as i64;
        p += 1;
        if n < 128 {
            let mut i: i64 = 0;
            while i <= n && p < src.len() {
                out.push(src[p]);
                p += 1;
                i += 1;
            }
        } else if n > 128 {
            if p < src.len() {
                let mut i: i64 = 0;
                while i < 257 - n {
                    out.push(src[p]);
                    i += 1;
                }
                p += 1;
            }
        }
    }
    out
}

fn slice(d: &Vec<u8>, from: usize, len: usize) -> Vec<u8> {
    let mut out: Vec<u8> = vec![];
    let mut i = from;
    while i < from + len && i < d.len() {
        out.push(d[i]);
        i += 1;
    }
    out
}

/// Sample `i` of a row at `o`, as it is (up to 16 bits, big-endian when
/// the file is; 16-bit samples were made big-endian by `decode`).
fn sample(row: &Vec<u8>, o: usize, i: i64, bits: i64) -> i64 {
    if bits == 8 {
        let p = o + (i as usize);
        return if p < row.len() { row[p] as i64 } else { 0 };
    }
    if bits == 16 {
        let p = o + (i as usize) * 2;
        return if p + 1 < row.len() { (row[p] as i64) * 256 + (row[p + 1] as i64) } else { 0 };
    }
    let bit = i * bits;
    let mut v: i64 = 0;
    let mut k: i64 = 0;
    while k < bits {
        let b = bit + k;
        let p = o + (b / 8) as usize;
        let byte = if p < row.len() { row[p] as i64 } else { 0 };
        v = v * 2 + ((byte >> (7 - (b % 8))) & 1);
        k += 1;
    }
    v
}

pub fn decode(d: &Vec<u8>) -> PicImage {
    let t = Tif { le: d[0] == 73 };
    let ifd = u32_at(d, &t, 4) as usize;
    let n = u16_at(d, &t, ifd) as usize;
    let mut w: i64 = 0;
    let mut h: i64 = 0;
    let mut bps: Vec<i64> = vec![];
    let mut comp: i64 = 1;
    let mut photo: i64 = -1;
    let mut offs: Vec<i64> = vec![];
    let mut counts: Vec<i64> = vec![];
    let mut spp: i64 = 1;
    let mut rps: i64 = 0;
    let mut planar: i64 = 1;
    let mut predictor: i64 = 1;
    let mut cmap: Vec<i64> = vec![];
    let mut extra: Vec<i64> = vec![];
    let mut tw: i64 = 0;
    let mut th: i64 = 0;
    let mut tables: Vec<u8> = vec![];
    let mut fill_order: i64 = 1;
    let mut i: usize = 0;
    while i < n {
        let e = ifd + 2 + i * 12;
        let tag = u16_at(d, &t, e);
        if tag == 256 {
            w = first(&values(d, &t, e), 0);
        } else if tag == 257 {
            h = first(&values(d, &t, e), 0);
        } else if tag == 258 {
            bps = values(d, &t, e);
        } else if tag == 259 {
            comp = first(&values(d, &t, e), 1);
        } else if tag == 262 {
            photo = first(&values(d, &t, e), -1);
        } else if tag == 266 {
            fill_order = first(&values(d, &t, e), 1);
        } else if tag == 273 || tag == 324 {
            offs = values(d, &t, e);
        } else if tag == 277 {
            spp = first(&values(d, &t, e), 1);
        } else if tag == 278 {
            rps = first(&values(d, &t, e), 0);
        } else if tag == 279 || tag == 325 {
            counts = values(d, &t, e);
        } else if tag == 284 {
            planar = first(&values(d, &t, e), 1);
        } else if tag == 317 {
            predictor = first(&values(d, &t, e), 1);
        } else if tag == 320 {
            cmap = values(d, &t, e);
        } else if tag == 322 {
            tw = first(&values(d, &t, e), 0);
        } else if tag == 323 {
            th = first(&values(d, &t, e), 0);
        } else if tag == 338 {
            extra = values(d, &t, e);
        } else if tag == 347 {
            let v = values(d, &t, e);
            for b in v.iter() {
                tables.push(*b as u8);
            }
        }
        i += 1;
    }
    if w <= 0 || h <= 0 || w * h > pdf_picture::MAX_PIXELS {
        return pdf_picture::failed("tiff", "bad image size");
    }
    if comp == 2 || comp == 3 || comp == 4 {
        return pdf_picture::failed("tiff", "CCITT fax compression is not read");
    }
    let bits = first(&bps, 1);
    if spp < 1 || spp > 5 || !(bits == 1 || bits == 2 || bits == 4 || bits == 8 || bits == 16) {
        return pdf_picture::failed("tiff", "unsupported samples");
    }
    if photo < 0 {
        photo = if spp >= 3 { 2 } else { 1 };
    }
    if comp == 6 || comp == 7 {
        return tiff_jpeg(d, &offs, &counts, &tables, w, h, tw, th, rps, if photo == 2 { 0 } else { -1 });
    }
    let tiled = tw > 0 && th > 0;
    let rows_per = if tiled { th } else if rps <= 0 || rps > h { h } else { rps };
    let seg_w = if tiled { tw } else { w };
    let planes = if planar == 2 { spp } else { 1 };
    let per_pixel = if planar == 2 { 1 } else { spp };
    let seg_row = ((seg_w * per_pixel * bits + 7) / 8) as usize;
    // the whole image, the planes one after the other, rows of `row_bytes`
    // (one flat buffer: nested vectors are values on some targets)
    let row_bytes = ((w * per_pixel * bits + 7) / 8) as usize;
    let plane_size = row_bytes * (h as usize);
    let mut img: Vec<u8> = vec![0; plane_size * (planes as usize)];
    let across = if tiled { (w + tw - 1) / tw } else { 1 };
    let down = (h + rows_per - 1) / rows_per;
    let per_plane = across * down;
    let mut s: i64 = 0;
    while (s as usize) < offs.len() && s < per_plane * planes {
        let off = offs[s as usize] as usize;
        let len = if (s as usize) < counts.len() { counts[s as usize] as usize } else { d.len() - off.min(d.len()) };
        let raw = slice(d, off, len);
        let want = seg_row * (rows_per as usize);
        let mut seg = if comp == 5 {
            pdf_filters::lzw(&raw, 1)
        } else if comp == 8 || comp == 32946 {
            pdf_inflate::inflate(&raw).data
        } else if comp == 32773 {
            packbits(&raw, want)
        } else {
            raw
        };
        if fill_order == 2 {
            let mut k: usize = 0;
            while k < seg.len() {
                let mut b = seg[k] as i64;
                let mut r: i64 = 0;
                let mut j = 0;
                while j < 8 {
                    r = r * 2 + (b & 1);
                    b = b / 2;
                    j += 1;
                }
                seg[k] = r as u8;
                k += 1;
            }
        }
        let plane = (s / per_plane) as usize;
        let ix = s % per_plane;
        let sx = (ix % across) * seg_w;
        let sy = (ix / across) * rows_per;
        let mut r: i64 = 0;
        while r < rows_per && sy + r < h {
            let ro = (r as usize) * seg_row;
            // the predictor works on the segment's own rows
            if predictor == 2 {
                let step = per_pixel as usize;
                if bits == 8 {
                    let mut k = step;
                    while k < seg_row && ro + k < seg.len() {
                        seg[ro + k] = ((seg[ro + k] as i64 + seg[ro + k - step] as i64) & 255) as u8;
                        k += 1;
                    }
                } else if bits == 16 {
                    let mut k = step * 2;
                    while k + 1 < seg_row && ro + k + 1 < seg.len() {
                        let a = if t.le { (seg[ro + k] as i64) + (seg[ro + k + 1] as i64) * 256 } else { (seg[ro + k] as i64) * 256 + (seg[ro + k + 1] as i64) };
                        let pb = ro + k - step * 2;
                        let b = if t.le { (seg[pb] as i64) + (seg[pb + 1] as i64) * 256 } else { (seg[pb] as i64) * 256 + (seg[pb + 1] as i64) };
                        let v = (a + b) & 65535;
                        if t.le {
                            seg[ro + k] = (v & 255) as u8;
                            seg[ro + k + 1] = (v >> 8) as u8;
                        } else {
                            seg[ro + k] = (v >> 8) as u8;
                            seg[ro + k + 1] = (v & 255) as u8;
                        }
                        k += 2;
                    }
                }
            }
            // copy the row's part that is inside the image
            let dst = plane * plane_size + ((sy + r) as usize) * row_bytes;
            let bit_from = sx * per_pixel * bits;
            let nbits = (if sx + seg_w > w { w - sx } else { seg_w }) * per_pixel * bits;
            if bit_from % 8 == 0 {
                let from = (bit_from / 8) as usize;
                let nb = ((nbits + 7) / 8) as usize;
                let mut k: usize = 0;
                while k < nb && ro + k < seg.len() && dst + from + k < img.len() {
                    img[dst + from + k] = seg[ro + k];
                    k += 1;
                }
            }
            r += 1;
        }
        s += 1;
    }
    // 16-bit samples to big-endian, as `sample` reads them
    if bits == 16 && t.le {
        let mut k: usize = 0;
        while k + 1 < img.len() {
            let a = img[k];
            img[k] = img[k + 1];
            img[k + 1] = a;
            k += 2;
        }
    }
    let colours: i64 = if photo == 2 { 3 } else if photo == 5 { 4 } else { 1 };
    let has_alpha = spp > colours && !extra.is_empty() && (extra[0] == 1 || extra[0] == 2);
    let premultiplied = has_alpha && extra[0] == 1;
    let maxv = (1 << bits) - 1;
    let mut rgba: Vec<u8> = vec![0; (w * h * 4) as usize];
    let mut cv: Vec<i64> = vec![0; 5];
    let mut y: i64 = 0;
    while y < h {
        let ro = (y as usize) * row_bytes;
        let mut x: i64 = 0;
        while x < w {
            let mut k: i64 = 0;
            while k < spp {
                cv[k as usize] = if planar == 2 { sample(&img, (k as usize) * plane_size + ro, x, bits) } else { sample(&img, ro, x * spp + k, bits) };
                k += 1;
            }
            let at = ((y * w + x) * 4) as usize;
            let mut r: i64 = 0;
            let mut g: i64 = 0;
            let mut b: i64 = 0;
            if photo == 3 {
                let nc = (cmap.len() / 3) as i64;
                let ix = cv[0];
                if ix < nc {
                    r = cmap[ix as usize] / 257;
                    g = cmap[(nc + ix) as usize] / 257;
                    b = cmap[(2 * nc + ix) as usize] / 257;
                }
            } else if photo == 2 {
                r = cv[0] * 255 / maxv;
                g = cv[1] * 255 / maxv;
                b = cv[2] * 255 / maxv;
            } else if photo == 5 {
                let c = cv[0] * 255 / maxv;
                let m = cv[1] * 255 / maxv;
                let ye = cv[2] * 255 / maxv;
                let kk = cv[3] * 255 / maxv;
                r = (255 - c) * (255 - kk) / 255;
                g = (255 - m) * (255 - kk) / 255;
                b = (255 - ye) * (255 - kk) / 255;
            } else {
                let mut v = cv[0] * 255 / maxv;
                if photo == 0 {
                    v = 255 - v;
                }
                r = v;
                g = v;
                b = v;
            }
            let mut a: i64 = 255;
            if has_alpha {
                a = cv[colours as usize] * 255 / maxv;
                if premultiplied && a > 0 && a < 255 {
                    r = (r * 255 / a).min(255);
                    g = (g * 255 / a).min(255);
                    b = (b * 255 / a).min(255);
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
    pdf_picture::made("tiff", w, h, rgba)
}

/// JPEG-compressed strips or tiles: each is a JPEG stream, with the shared
/// tables of JPEGTables put in front of it.
fn tiff_jpeg(d: &Vec<u8>, offs: &Vec<i64>, counts: &Vec<i64>, tables: &Vec<u8>, w: i64, h: i64, tw: i64, th: i64, rps: i64, transform: i64) -> PicImage {
    let tiled = tw > 0 && th > 0;
    let rows_per = if tiled { th } else if rps <= 0 || rps > h { h } else { rps };
    let seg_w = if tiled { tw } else { w };
    let across = if tiled { (w + tw - 1) / tw } else { 1 };
    let mut rgba: Vec<u8> = vec![0; (w * h * 4) as usize];
    let mut s: usize = 0;
    while s < offs.len() && s < counts.len() {
        let seg = slice(d, offs[s] as usize, counts[s] as usize);
        let mut file: Vec<u8> = vec![];
        if tables.len() > 4 && seg.len() > 2 {
            // SOI, the tables without their SOI and EOI, then the strip after its SOI
            file.push(255);
            file.push(216);
            let mut k: usize = 2;
            while k + 2 < tables.len() {
                file.push(tables[k]);
                k += 1;
            }
            let mut j: usize = 2;
            while j < seg.len() {
                file.push(seg[j]);
                j += 1;
            }
        } else {
            file = seg;
        }
        let jp = pdf_jpeg::decode(&file, transform);
        if !jp.ok {
            return pdf_picture::failed("tiff", &jp.error);
        }
        let px = pdf_jpeg::to_rgba(&jp);
        let sx = ((s as i64) % across) * seg_w;
        let sy = ((s as i64) / across) * rows_per;
        let mut y: i64 = 0;
        while y < jp.h && sy + y < h {
            let mut x: i64 = 0;
            while x < jp.w && sx + x < w {
                let from = ((y * jp.w + x) * 4) as usize;
                let to = (((sy + y) * w + sx + x) * 4) as usize;
                rgba[to] = px[from];
                rgba[to + 1] = px[from + 1];
                rgba[to + 2] = px[from + 2];
                rgba[to + 3] = 255;
                x += 1;
            }
            y += 1;
        }
        s += 1;
    }
    pdf_picture::made("tiff", w, h, rgba)
}
