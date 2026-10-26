//! TrueType and OpenType font files (the sfnt tables a PDF embeds as
//! /FontFile2, or /FontFile3 /OpenType): glyph outlines from glyf/loca,
//! character maps, advance widths and glyph names. An OpenType file with
//! CFF outlines hands its CFF table to cff.rs.

use crate::pdf_doc::be;
use crate::pdf_obj;
use crate::pdf_path::Path;

#[derive(Clone, Debug)]
pub struct TrueType {
    pub data: Vec<u8>,
    pub ok: bool,
    pub units_per_em: f64,
    pub num_glyphs: i64,
    loc_long: bool,
    pub glyf: i64,
    pub loca: i64,
    hmtx: i64,
    num_hmetrics: i64,
    /// per cmap subtable: platform, encoding, offset
    cmaps: Vec<i64>,
    /// glyph names from post format 2, by glyph id
    pub names: Vec<String>,
    /// the offset and length of a CFF table (OpenType), or -1
    pub cff_off: i64,
    pub cff_len: i64,
}

fn s16(v: i64) -> i64 {
    if v >= 32768 {
        return v - 65536;
    }
    v
}

impl TrueType {
    pub fn parse(data: Vec<u8>) -> TrueType {
        let mut t = TrueType {
            data: data,
            ok: false,
            units_per_em: 1000.0,
            num_glyphs: 0,
            loc_long: false,
            glyf: -1,
            loca: -1,
            hmtx: -1,
            num_hmetrics: 0,
            cmaps: vec![],
            names: vec![],
            cff_off: -1,
            cff_len: 0,
        };
        if t.data.len() < 12 {
            return t;
        }
        let n = t.u16(4);
        let mut head: i64 = -1;
        let mut maxp: i64 = -1;
        let mut hhea: i64 = -1;
        let mut cmap: i64 = -1;
        let mut post: i64 = -1;
        let mut loca_len: i64 = 0;
        let mut i: i64 = 0;
        while i < n {
            let rec = (12 + i * 16) as usize;
            if rec + 16 > t.data.len() {
                break;
            }
            let tag = pdf_obj::latin1(&vec![t.data[rec], t.data[rec + 1], t.data[rec + 2], t.data[rec + 3]]);
            let off = be(&t.data, rec + 8, 4);
            let len = be(&t.data, rec + 12, 4);
            if tag == "head" {
                head = off;
            } else if tag == "maxp" {
                maxp = off;
            } else if tag == "hhea" {
                hhea = off;
            } else if tag == "cmap" {
                cmap = off;
            } else if tag == "post" {
                post = off;
            } else if tag == "glyf" {
                t.glyf = off;
            } else if tag == "loca" {
                t.loca = off;
                loca_len = len;
            } else if tag == "hmtx" {
                t.hmtx = off;
            } else if tag == "CFF " {
                t.cff_off = off;
                t.cff_len = len;
            }
            i += 1;
        }
        if head >= 0 {
            let u = t.u16((head + 18) as usize);
            if u >= 16 && u <= 16384 {
                t.units_per_em = u as f64;
            }
            t.loc_long = t.u16((head + 50) as usize) == 1;
        }
        if maxp >= 0 {
            t.num_glyphs = t.u16((maxp + 4) as usize);
        }
        if t.num_glyphs == 0 && t.loca >= 0 {
            // a subset without maxp: the glyphs loca has room for
            let per = if t.loc_long { 4 } else { 2 };
            t.num_glyphs = loca_len / per - 1;
        }
        if hhea >= 0 {
            t.num_hmetrics = t.u16((hhea + 34) as usize);
        }
        if cmap >= 0 {
            let cn = t.u16((cmap + 2) as usize);
            let mut k: i64 = 0;
            while k < cn {
                let r = (cmap + 4 + k * 8) as usize;
                t.cmaps.push(t.u16(r));
                t.cmaps.push(t.u16(r + 2));
                t.cmaps.push(cmap + be(&t.data, r + 4, 4));
                k += 1;
            }
        }
        if post >= 0 {
            t.read_post(post as usize);
        }
        t.ok = (t.glyf >= 0 && t.loca >= 0) || t.cff_off >= 0;
        t
    }

    fn u16(&self, pos: usize) -> i64 {
        be(&self.data, pos, 2)
    }

    fn read_post(&mut self, off: usize) {
        if be(&self.data, off, 4) != 0x20000 {
            return;
        }
        let n = self.u16(off + 32);
        let mut idx: Vec<i64> = vec![];
        let mut i: i64 = 0;
        while i < n {
            idx.push(self.u16(off + 34 + (i as usize) * 2));
            i += 1;
        }
        // the Pascal strings after the index
        let mut extra: Vec<String> = vec![];
        let mut p = off + 34 + (n as usize) * 2;
        while p < self.data.len() {
            let len = self.data[p] as usize;
            p += 1;
            let mut s = String::new();
            let mut k: usize = 0;
            while k < len && p < self.data.len() {
                s.push(pdf_obj::ch(self.data[p] as u32));
                p += 1;
                k += 1;
            }
            extra.push(s);
        }
        for x in idx.iter() {
            if *x < 258 {
                self.names.push(mac_glyph(*x));
            } else if ((*x - 258) as usize) < extra.len() {
                self.names.push(extra[(*x - 258) as usize].clone());
            } else {
                self.names.push(String::new());
            }
        }
    }

    /// The glyph id of `code` in subtable (platform, encoding), or -1
    /// when there is no such subtable, 0 when the code is not in it.
    pub fn lookup(&self, platform: i64, encoding: i64, code: i64) -> i64 {
        let mut i: usize = 0;
        while i + 2 < self.cmaps.len() {
            if self.cmaps[i] == platform && self.cmaps[i + 1] == encoding {
                let g = self.lookup_at(self.cmaps[i + 2] as usize, code);
                return g;
            }
            i += 3;
        }
        -1
    }

    pub fn has_cmap(&self, platform: i64, encoding: i64) -> bool {
        let mut i: usize = 0;
        while i + 2 < self.cmaps.len() {
            if self.cmaps[i] == platform && self.cmaps[i + 1] == encoding {
                return true;
            }
            i += 3;
        }
        false
    }

    pub fn has_any_cmap(&self) -> bool {
        !self.cmaps.is_empty()
    }

    /// The first subtable's glyph for `code` (for symbolic fonts with odd cmaps).
    pub fn lookup_any(&self, code: i64) -> i64 {
        let mut i: usize = 0;
        while i + 2 < self.cmaps.len() {
            let g = self.lookup_at(self.cmaps[i + 2] as usize, code);
            if g > 0 {
                return g;
            }
            i += 3;
        }
        0
    }

    fn lookup_at(&self, off: usize, code: i64) -> i64 {
        let format = self.u16(off);
        if format == 0 {
            if code >= 0 && code < 256 && off + 6 + (code as usize) < self.data.len() {
                return self.data[off + 6 + (code as usize)] as i64;
            }
            return 0;
        }
        if format == 4 {
            let segx2 = self.u16(off + 6) as usize;
            let ends = off + 14;
            let starts = ends + segx2 + 2;
            let deltas = starts + segx2;
            let ranges = deltas + segx2;
            let mut s: usize = 0;
            while s < segx2 {
                let end = self.u16(ends + s);
                if code <= end {
                    let start = self.u16(starts + s);
                    if code < start {
                        return 0;
                    }
                    let delta = self.u16(deltas + s);
                    let ro = self.u16(ranges + s);
                    if ro == 0 {
                        return (code + delta) % 65536;
                    }
                    let gi = ranges + s + (ro as usize) + ((code - start) as usize) * 2;
                    let g = self.u16(gi);
                    if g == 0 {
                        return 0;
                    }
                    return (g + delta) % 65536;
                }
                s += 2;
            }
            return 0;
        }
        if format == 6 {
            let first = self.u16(off + 6);
            let count = self.u16(off + 8);
            if code >= first && code < first + count {
                return self.u16(off + 10 + ((code - first) as usize) * 2);
            }
            return 0;
        }
        if format == 12 {
            let n = be(&self.data, off + 12, 4);
            let mut i: i64 = 0;
            while i < n {
                let g = off + 16 + (i as usize) * 12;
                let a = be(&self.data, g, 4);
                let b = be(&self.data, g + 4, 4);
                if code >= a && code <= b {
                    return be(&self.data, g + 8, 4) + (code - a);
                }
                i += 1;
            }
            return 0;
        }
        0
    }

    pub fn glyph_by_name(&self, name: &str) -> i64 {
        let mut i: usize = 0;
        while i < self.names.len() {
            if self.names[i] == name {
                return i as i64;
            }
            i += 1;
        }
        -1
    }

    /// The advance width of glyph `gid` in font units.
    pub fn advance(&self, gid: i64) -> f64 {
        if self.hmtx < 0 || self.num_hmetrics <= 0 {
            return 0.0;
        }
        let mut g = gid;
        if g >= self.num_hmetrics {
            g = self.num_hmetrics - 1;
        }
        self.u16((self.hmtx + g * 4) as usize) as f64
    }

    fn glyph_range(&self, gid: i64) -> Vec<i64> {
        if gid < 0 || (self.num_glyphs > 0 && gid >= self.num_glyphs + 1) {
            return vec![0, 0];
        }
        let mut a: i64 = 0;
        let mut b: i64 = 0;
        if self.loc_long {
            a = be(&self.data, (self.loca + gid * 4) as usize, 4);
            b = be(&self.data, (self.loca + gid * 4 + 4) as usize, 4);
        } else {
            a = self.u16((self.loca + gid * 2) as usize) * 2;
            b = self.u16((self.loca + gid * 2 + 2) as usize) * 2;
        }
        vec![self.glyf + a, self.glyf + b]
    }

    /// Glyph `gid`'s outline in font units (y up).
    pub fn glyph_path(&self, gid: i64) -> Path {
        let mut p = Path::new();
        if self.glyf < 0 {
            return p;
        }
        let m: Vec<f64> = vec![1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
        self.add_glyph(&mut p, gid, &m, 0);
        p
    }

    fn add_glyph(&self, p: &mut Path, gid: i64, m: &Vec<f64>, depth: i64) {
        if depth > 8 {
            return;
        }
        let r = self.glyph_range(gid);
        let start = r[0] as usize;
        let end = r[1] as usize;
        if end <= start || end > self.data.len() || start + 10 > self.data.len() {
            return;
        }
        let nc = s16(self.u16(start));
        if nc >= 0 {
            self.simple_glyph(p, start, nc, m);
        } else {
            self.composite_glyph(p, start + 10, m, depth);
        }
    }

    fn simple_glyph(&self, p: &mut Path, start: usize, nc: i64, m: &Vec<f64>) {
        let d = &self.data;
        let mut ends: Vec<i64> = vec![];
        let mut pos = start + 10;
        let mut i: i64 = 0;
        while i < nc {
            ends.push(self.u16(pos));
            pos += 2;
            i += 1;
        }
        if ends.is_empty() {
            return;
        }
        let npts = (ends[ends.len() - 1] + 1) as usize;
        let ilen = self.u16(pos) as usize;
        pos += 2 + ilen;
        let mut flags: Vec<i64> = vec![];
        while flags.len() < npts && pos < d.len() {
            let f = d[pos] as i64;
            pos += 1;
            flags.push(f);
            if (f & 8) != 0 && pos < d.len() {
                let rep = d[pos] as i64;
                pos += 1;
                let mut k = 0;
                while k < rep && flags.len() < npts {
                    flags.push(f);
                    k += 1;
                }
            }
        }
        if flags.len() < npts {
            return;
        }
        let mut xs: Vec<f64> = vec![];
        let mut v: i64 = 0;
        for f in flags.iter() {
            if (f & 2) != 0 {
                if pos >= d.len() {
                    return;
                }
                let dx = d[pos] as i64;
                pos += 1;
                v += if (f & 16) != 0 { dx } else { 0 - dx };
            } else if (f & 16) == 0 {
                v += s16(self.u16(pos));
                pos += 2;
            }
            xs.push(v as f64);
        }
        let mut ys: Vec<f64> = vec![];
        v = 0;
        for f in flags.iter() {
            if (f & 4) != 0 {
                if pos >= d.len() {
                    return;
                }
                let dy = d[pos] as i64;
                pos += 1;
                v += if (f & 32) != 0 { dy } else { 0 - dy };
            } else if (f & 32) == 0 {
                v += s16(self.u16(pos));
                pos += 2;
            }
            ys.push(v as f64);
        }
        let mut s: usize = 0;
        for e in ends.iter() {
            let last = *e as usize;
            if last < s || last >= npts {
                break;
            }
            self.contour(p, &xs, &ys, &flags, s, last, m);
            s = last + 1;
        }
    }

    fn contour(&self, p: &mut Path, xs: &Vec<f64>, ys: &Vec<f64>, flags: &Vec<i64>, s: usize, e: usize, m: &Vec<f64>) {
        let n = e - s + 1;
        if n < 2 {
            return;
        }
        // start at an on-curve point, or at the middle of two off-curve ones
        let mut first: usize = n;
        let mut i: usize = 0;
        while i < n {
            if (flags[s + i] & 1) != 0 {
                first = i;
                break;
            }
            i += 1;
        }
        let mut sx: f64 = 0.0;
        let mut sy: f64 = 0.0;
        let mut start_i: usize = 0;
        if first == n {
            sx = (xs[s] + xs[s + n - 1]) / 2.0;
            sy = (ys[s] + ys[s + n - 1]) / 2.0;
        } else {
            sx = xs[s + first];
            sy = ys[s + first];
            start_i = first + 1;
        }
        let x0 = crate::pdf_path::tx(m, sx, sy);
        let y0 = crate::pdf_path::ty(m, sx, sy);
        p.move_to(x0, y0);
        let mut has_q = false;
        let mut qx: f64 = 0.0;
        let mut qy: f64 = 0.0;
        let total = if first == n { n } else { n };
        let mut k: usize = 0;
        while k < total {
            let idx = s + ((start_i + k) % n);
            let x = xs[idx];
            let y = ys[idx];
            let on = (flags[idx] & 1) != 0;
            if on {
                if has_q {
                    p.quad_to(crate::pdf_path::tx(m, qx, qy), crate::pdf_path::ty(m, qx, qy), crate::pdf_path::tx(m, x, y), crate::pdf_path::ty(m, x, y));
                    has_q = false;
                } else {
                    p.line_to(crate::pdf_path::tx(m, x, y), crate::pdf_path::ty(m, x, y));
                }
            } else {
                if has_q {
                    let mx = (qx + x) / 2.0;
                    let my = (qy + y) / 2.0;
                    p.quad_to(crate::pdf_path::tx(m, qx, qy), crate::pdf_path::ty(m, qx, qy), crate::pdf_path::tx(m, mx, my), crate::pdf_path::ty(m, mx, my));
                }
                qx = x;
                qy = y;
                has_q = true;
            }
            k += 1;
        }
        if has_q {
            p.quad_to(crate::pdf_path::tx(m, qx, qy), crate::pdf_path::ty(m, qx, qy), x0, y0);
        }
        p.close();
    }

    fn composite_glyph(&self, p: &mut Path, start: usize, m: &Vec<f64>, depth: i64) {
        let mut pos = start;
        let mut guard = 0;
        loop {
            guard += 1;
            if pos + 4 > self.data.len() || guard > 64 {
                return;
            }
            let flags = self.u16(pos);
            let gid = self.u16(pos + 2);
            pos += 4;
            let mut dx: f64 = 0.0;
            let mut dy: f64 = 0.0;
            if (flags & 1) != 0 {
                dx = s16(self.u16(pos)) as f64;
                dy = s16(self.u16(pos + 2)) as f64;
                pos += 4;
            } else {
                let a = self.data[pos] as i64;
                let b = self.data[pos + 1] as i64;
                dx = (if a >= 128 { a - 256 } else { a }) as f64;
                dy = (if b >= 128 { b - 256 } else { b }) as f64;
                pos += 2;
            }
            let mut a = 1.0;
            let mut b = 0.0;
            let mut c = 0.0;
            let mut d = 1.0;
            if (flags & 8) != 0 {
                a = (s16(self.u16(pos)) as f64) / 16384.0;
                d = a;
                pos += 2;
            } else if (flags & 64) != 0 {
                a = (s16(self.u16(pos)) as f64) / 16384.0;
                d = (s16(self.u16(pos + 2)) as f64) / 16384.0;
                pos += 4;
            } else if (flags & 128) != 0 {
                a = (s16(self.u16(pos)) as f64) / 16384.0;
                b = (s16(self.u16(pos + 2)) as f64) / 16384.0;
                c = (s16(self.u16(pos + 4)) as f64) / 16384.0;
                d = (s16(self.u16(pos + 6)) as f64) / 16384.0;
                pos += 8;
            }
            // point matching (ARGS_ARE_XY_VALUES off) is rare and not done
            let local: Vec<f64> = vec![a, b, c, d, dx, dy];
            let mm = crate::pdf_path::mat_mul(&local, m);
            self.add_glyph(p, gid, &mm, depth + 1);
            if (flags & 32) == 0 {
                return;
            }
        }
    }
}

/// The 258 standard Macintosh glyph names (post format 2 indexes < 258).
fn mac_glyph(i: i64) -> String {
    let all = ".notdef .null nonmarkingreturn space exclam quotedbl numbersign dollar percent ampersand quotesingle parenleft parenright asterisk plus comma hyphen period slash zero one two three four five six seven eight nine colon semicolon less equal greater question at A B C D E F G H I J K L M N O P Q R S T U V W X Y Z bracketleft backslash bracketright asciicircum underscore grave a b c d e f g h i j k l m n o p q r s t u v w x y z braceleft bar braceright asciitilde Adieresis Aring Ccedilla Eacute Ntilde Odieresis Udieresis aacute agrave acircumflex adieresis atilde aring ccedilla eacute egrave ecircumflex edieresis iacute igrave icircumflex idieresis ntilde oacute ograve ocircumflex odieresis otilde uacute ugrave ucircumflex udieresis dagger degree cent sterling section bullet paragraph germandbls registered copyright trademark acute dieresis notequal AE Oslash infinity plusminus lessequal greaterequal yen mu partialdiff summation product pi integral ordfeminine ordmasculine Omega ae oslash questiondown exclamdown logicalnot radical florin approxequal Delta guillemotleft guillemotright ellipsis nonbreakingspace Agrave Atilde Otilde OE oe endash emdash quotedblleft quotedblright quoteleft quoteright divide lozenge ydieresis Ydieresis fraction currency guilsinglleft guilsinglright fi fl daggerdbl periodcentered quotesinglbase quotedblbase perthousand Acircumflex Ecircumflex Aacute Edieresis Egrave Iacute Icircumflex Idieresis Igrave Oacute Ocircumflex apple Ograve Uacute Ucircumflex Ugrave dotlessi circumflex tilde macron breve dotaccent ring cedilla hungarumlaut ogonek caron Lslash lslash Scaron scaron Zcaron zcaron brokenbar Eth eth Yacute yacute Thorn thorn minus multiply onesuperior twosuperior threesuperior onehalf onequarter threequarters franc Gbreve gbreve Idotaccent Scedilla scedilla Cacute cacute Ccaron ccaron dcroat";
    let names: Vec<&str> = all.split(' ').collect();
    if i >= 0 && (i as usize) < names.len() {
        return names[i as usize].to_string();
    }
    String::new()
}
