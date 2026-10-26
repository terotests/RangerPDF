//! Type 1 fonts (Adobe Type 1 Font Format): the /FontFile of a PDF, or a
//! .pfb/.pfa file. The clear-text part gives the encoding and the font
//! matrix; the eexec part the subroutines and charstrings, whose outlines
//! the Type 1 charstring interpreter draws.

use crate::pdf_lexer::hexval;
use crate::pdf_lexer::is_ws;
use crate::pdf_obj;
use crate::pdf_func::Pc;
use crate::pdf_path::Path;

#[derive(Clone, Debug)]
pub struct Type1 {
    pub ok: bool,
    pub matrix: Vec<f64>,
    /// code -> glyph name ("" when unset)
    pub encoding: Vec<String>,
    pub standard_encoding: bool,
    names: Vec<String>,
    charstrings: Vec<Vec<u8>>,
    subrs: Vec<Vec<u8>>,
}

fn decrypt(data: &Vec<u8>, from: usize, to: usize, key: i64, skip: usize) -> Vec<u8> {
    let mut r = key;
    let mut out: Vec<u8> = vec![];
    let mut i = from;
    let mut n: usize = 0;
    while i < to && i < data.len() {
        let c = data[i] as i64;
        let p = (c ^ (r >> 8)) & 255;
        r = ((c + r) * 52845 + 22719) & 65535;
        if n >= skip {
            out.push(p as u8);
        }
        n += 1;
        i += 1;
    }
    out
}

fn find(d: &Vec<u8>, from: usize, word: &str) -> i64 {
    let w: Vec<char> = word.chars().collect();
    let mut i = from;
    while i + w.len() <= d.len() {
        let mut ok = true;
        let mut k: usize = 0;
        while k < w.len() {
            if (d[i + k] as u32) != (w[k] as u32) {
                ok = false;
                break;
            }
            k += 1;
        }
        if ok {
            return i as i64;
        }
        i += 1;
    }
    -1
}

/// A token of the clear text or the decrypted private part.
fn word_at(d: &Vec<u8>, pos: &mut Pc) -> String {
    while pos.i < d.len() && is_ws(d[pos.i]) {
        pos.i += 1;
    }
    let mut s = String::new();
    if pos.i < d.len() && (d[pos.i] == 47 || d[pos.i] == 91 || d[pos.i] == 93 || d[pos.i] == 123 || d[pos.i] == 125) {
        s.push(pdf_obj::ch(d[pos.i] as u32));
        pos.i += 1;
        if s != "/" {
            return s;
        }
    }
    while pos.i < d.len() && !is_ws(d[pos.i]) && d[pos.i] != 47 && d[pos.i] != 91 && d[pos.i] != 93 && d[pos.i] != 123 && d[pos.i] != 125 {
        s.push(pdf_obj::ch(d[pos.i] as u32));
        pos.i += 1;
    }
    s
}

fn no_slash(s: &str) -> String {
    let mut out = String::new();
    let mut first = true;
    for c in s.chars() {
        if !(first && c == '/') {
            out.push(c);
        }
        first = false;
    }
    out
}

fn num(s: &str) -> i64 {
    crate::pdf_cff::parse_int(s)
}

impl Type1 {
    pub fn parse(file: &Vec<u8>, len1: i64) -> Type1 {
        let mut t = Type1 { ok: false, matrix: vec![0.001, 0.0, 0.0, 0.001, 0.0, 0.0], encoding: vec![String::new(); 256], standard_encoding: false, names: vec![], charstrings: vec![], subrs: vec![] };
        // a .pfb: segments with 6-byte headers
        let mut d: Vec<u8> = vec![];
        let mut l1 = len1;
        if file.len() > 6 && file[0] == 128 && file[1] == 1 {
            let mut p: usize = 0;
            let mut first = true;
            while p + 6 <= file.len() && file[p] == 128 && (file[p + 1] == 1 || file[p + 1] == 2) {
                let len = (file[p + 2] as usize) | ((file[p + 3] as usize) << 8) | ((file[p + 4] as usize) << 16) | ((file[p + 5] as usize) << 24);
                let mut k = p + 6;
                while k < p + 6 + len && k < file.len() {
                    d.push(file[k]);
                    k += 1;
                }
                if first {
                    l1 = len as i64;
                    first = false;
                }
                p = p + 6 + len;
            }
        } else {
            d = file.clone();
        }
        let ee = find(&d, 0, "eexec");
        if ee < 0 {
            return t;
        }
        if l1 <= 0 || (l1 as usize) > d.len() {
            l1 = ee + 5;
        }
        t.read_clear(&d, (ee as usize) + 5);
        // the encrypted part starts after white space following eexec
        let mut s = (ee as usize) + 5;
        while s < d.len() && is_ws(d[s]) {
            s += 1;
        }
        if (l1 as usize) > s && (l1 as usize) < d.len() {
            s = l1 as usize;
            while s < d.len() && (d[s] == 13 || d[s] == 10) {
                s += 1;
            }
        }
        // hex (PFA) or binary?
        let mut hex = true;
        let mut k = s;
        while k < s + 4 && k < d.len() {
            if hexval(d[k]) < 0 {
                hex = false;
            }
            k += 1;
        }
        let mut enc: Vec<u8> = vec![];
        if hex {
            let mut hi: i64 = -1;
            let mut q = s;
            while q < d.len() {
                let v = hexval(d[q]);
                q += 1;
                if v < 0 {
                    if is_ws(d[q - 1]) {
                        continue;
                    }
                    break;
                }
                if hi < 0 {
                    hi = v;
                } else {
                    enc.push((hi * 16 + v) as u8);
                    hi = -1;
                }
            }
        } else {
            let mut q = s;
            while q < d.len() {
                enc.push(d[q]);
                q += 1;
            }
        }
        let n = enc.len();
        let priv_part = decrypt(&enc, 0, n, 55665, 4);
        t.read_private(&priv_part);
        t.ok = !t.charstrings.is_empty();
        t
    }

    fn read_clear(&mut self, d: &Vec<u8>, end: usize) {
        let mut pos = Pc { i: 0 };
        let mut last: Vec<String> = vec![];
        while pos.i < end {
            let w = word_at(d, &mut pos);
            if w.is_empty() {
                pos.i += 1;
                continue;
            }
            if w == "/FontMatrix" || w == "FontMatrix" {
                let mut vals: Vec<f64> = vec![];
                let mut guard = 0;
                while vals.len() < 6 && guard < 12 && pos.i < end {
                    let x = word_at(d, &mut pos);
                    guard += 1;
                    if x == "[" || x == "{" {
                        continue;
                    }
                    let v = parse_float(&x);
                    vals.push(v);
                }
                if vals.len() == 6 {
                    self.matrix = vals;
                }
            } else if w == "StandardEncoding" && last.len() >= 1 && last[last.len() - 1] == "Encoding" {
                self.standard_encoding = true;
            } else if w == "put" && last.len() >= 3 && last[last.len() - 3] == "dup" {
                let code = num(&last[last.len() - 2]);
                let name = no_slash(&last[last.len() - 1]);
                if code >= 0 && code < 256 {
                    self.encoding[code as usize] = name;
                }
            }
            if w != "/" {
                last.push(w);
            }
            if last.len() > 8 {
                last.remove(0);
            }
        }
        if self.standard_encoding {
            let se = crate::pdf_fontdata::standard_encoding();
            for item in se.split(';') {
                let parts: Vec<&str> = item.split(' ').collect();
                if parts.len() == 2 {
                    let c = num(parts[0]);
                    if c >= 0 && c < 256 && self.encoding[c as usize].is_empty() {
                        self.encoding[c as usize] = parts[1].to_string();
                    }
                }
            }
        }
    }

    fn read_private(&mut self, d: &Vec<u8>) {
        let mut len_iv: i64 = 4;
        let li = find(d, 0, "/lenIV");
        if li >= 0 {
            let mut p = Pc { i: (li as usize) + 6 };
            let w = word_at(d, &mut p);
            len_iv = num(&w);
        }
        let sub = find(d, 0, "/Subrs");
        if sub >= 0 {
            let mut p = Pc { i: (sub as usize) + 6 };
            let cnt = num(&word_at(d, &mut p));
            let mut i: i64 = 0;
            while i < cnt && i < 65536 {
                self.subrs.push(vec![]);
                i += 1;
            }
            // `array`, then dup <index> <len> RD <bytes> NP
            let mut skip = 0;
            while skip < 4 {
                let save = p.i;
                let w = word_at(d, &mut p);
                if w == "dup" || w.is_empty() {
                    p.i = save;
                    break;
                }
                skip += 1;
            }
            loop {
                let save = p.i;
                let w = word_at(d, &mut p);
                if w != "dup" {
                    p.i = save;
                    break;
                }
                let idx = num(&word_at(d, &mut p));
                let len = num(&word_at(d, &mut p)) as usize;
                let _rd = word_at(d, &mut p);
                p.i += 1;
                let cs = decrypt(d, p.i, p.i + len, 4330, if len_iv >= 0 { len_iv as usize } else { 0 });
                let raw = if len_iv < 0 { slice_of(d, p.i, p.i + len) } else { cs };
                if idx >= 0 && (idx as usize) < self.subrs.len() {
                    self.subrs[idx as usize] = raw;
                }
                p.i += len;
                let _np = word_at(d, &mut p);
                if p.i >= d.len() {
                    break;
                }
            }
        }
        let cs = find(d, 0, "/CharStrings");
        if cs < 0 {
            return;
        }
        let mut p = Pc { i: (cs as usize) + 12 };
        let _count = word_at(d, &mut p);
        // `dict dup begin`, then /name <len> RD <bytes> ND
        while p.i < d.len() {
            let w = word_at(d, &mut p);
            if w == "end" || w.is_empty() {
                break;
            }
            if !w.starts_with('/') {
                continue;
            }
            // word_at gives `/name` as one word
            let name = if w == "/" { word_at(d, &mut p) } else { no_slash(&w) };
            let len = num(&word_at(d, &mut p)) as usize;
            let _rd = word_at(d, &mut p);
            p.i += 1;
            let raw = if len_iv < 0 { slice_of(d, p.i, p.i + len) } else { decrypt(d, p.i, p.i + len, 4330, len_iv as usize) };
            self.names.push(name);
            self.charstrings.push(raw);
            p.i += len;
        }
    }

    pub fn glyph_index(&self, name: &str) -> i64 {
        let mut i: usize = 0;
        while i < self.names.len() {
            if self.names[i] == name {
                return i as i64;
            }
            i += 1;
        }
        -1
    }

    pub fn has_glyph(&self, name: &str) -> bool {
        self.glyph_index(name) >= 0
    }

    /// The outline of glyph `name` in charstring units, and its advance in `width[0]`.
    pub fn glyph_path(&self, name: &str, width: &mut Vec<f64>) -> Path {
        let mut st = T1 { p: Path::new(), stack: vec![], ps: vec![], x: 0.0, y: 0.0, sbx: 0.0, open: false, done: false, depth: 0, flex: vec![], in_flex: false, width: 0.0 };
        let g = self.glyph_index(name);
        if g < 0 {
            return st.p;
        }
        let cs = self.charstrings[g as usize].clone();
        self.run(&mut st, &cs);
        if st.open {
            st.p.close();
        }
        width.push(st.width);
        st.p
    }

    fn run(&self, st: &mut T1, cs: &Vec<u8>) {
        st.depth += 1;
        if st.depth > 10 {
            st.depth -= 1;
            return;
        }
        let mut i: usize = 0;
        while i < cs.len() && !st.done {
            let v = cs[i] as i64;
            i += 1;
            if v >= 32 {
                if v <= 246 {
                    st.stack.push((v - 139) as f64);
                } else if v <= 250 {
                    let w = if i < cs.len() { cs[i] as i64 } else { 0 };
                    st.stack.push(((v - 247) * 256 + w + 108) as f64);
                    i += 1;
                } else if v <= 254 {
                    let w = if i < cs.len() { cs[i] as i64 } else { 0 };
                    st.stack.push((0 - (v - 251) * 256 - w - 108) as f64);
                    i += 1;
                } else {
                    let x = crate::pdf_doc::be(cs, i, 4) as f64;
                    st.stack.push(if x >= 2147483648.0 { x - 4294967296.0 } else { x });
                    i += 4;
                }
                continue;
            }
            let s = st.stack.clone();
            let n = s.len();
            if v == 12 {
                let e = if i < cs.len() { cs[i] as i64 } else { 0 };
                i += 1;
                self.escape(st, e, &s);
                continue;
            }
            st.stack = vec![];
            if v == 13 && n >= 2 {
                // hsbw
                st.sbx = s[0];
                st.x = s[0];
                st.y = 0.0;
                st.width = s[1];
            } else if v == 9 {
                if st.open {
                    st.p.close();
                    st.open = false;
                }
            } else if v == 5 && n >= 2 {
                self.line(st, s[0], s[1]);
            } else if v == 6 && n >= 1 {
                self.line(st, s[0], 0.0);
            } else if v == 7 && n >= 1 {
                self.line(st, 0.0, s[0]);
            } else if v == 8 && n >= 6 {
                self.curve(st, s[0], s[1], s[2], s[3], s[4], s[5]);
            } else if v == 30 && n >= 4 {
                self.curve(st, 0.0, s[0], s[1], s[2], s[3], 0.0);
            } else if v == 31 && n >= 4 {
                self.curve(st, s[0], 0.0, s[1], s[2], 0.0, s[3]);
            } else if v == 21 && n >= 2 {
                self.moveto(st, s[0], s[1]);
            } else if v == 22 && n >= 1 {
                self.moveto(st, s[0], 0.0);
            } else if v == 4 && n >= 1 {
                self.moveto(st, 0.0, s[0]);
            } else if v == 10 && n >= 1 {
                let k = s[n - 1] as i64;
                let mut rest = s.clone();
                rest.truncate(n - 1);
                st.stack = rest;
                if k >= 0 && (k as usize) < self.subrs.len() {
                    let sub = self.subrs[k as usize].clone();
                    self.run(st, &sub);
                }
            } else if v == 11 {
                st.stack = s;
                st.depth -= 1;
                return;
            } else if v == 14 {
                if st.open {
                    st.p.close();
                    st.open = false;
                }
                st.done = true;
            } else {
                // hstem vstem and the rest: hints
            }
        }
        st.depth -= 1;
    }

    fn escape(&self, st: &mut T1, e: i64, s: &Vec<f64>) {
        let n = s.len();
        st.stack = vec![];
        if e == 7 && n >= 4 {
            // sbw
            st.sbx = s[0];
            st.x = s[0];
            st.y = s[1];
            st.width = s[2];
        } else if e == 12 && n >= 2 {
            // div
            let mut r = s.clone();
            let v = if s[n - 1] != 0.0 { s[n - 2] / s[n - 1] } else { 0.0 };
            r.truncate(n - 2);
            r.push(v);
            st.stack = r;
        } else if e == 6 && n >= 5 {
            // seac
            self.seac(st, s[0], s[1], s[2], s[3] as i64, s[4] as i64);
        } else if e == 16 && n >= 2 {
            // callothersubr: flex (0, 1, 2) and hint replacement (3)
            let other = s[n - 1] as i64;
            let cnt = s[n - 2] as usize;
            let mut args: Vec<f64> = vec![];
            let mut k = n - 2 - (if cnt <= n - 2 { cnt } else { n - 2 });
            while k < n - 2 {
                args.push(s[k]);
                k += 1;
            }
            let mut rest = s.clone();
            rest.truncate(n - 2 - args.len());
            st.stack = rest;
            if other == 1 {
                st.in_flex = true;
                st.flex = vec![];
            } else if other == 0 {
                st.in_flex = false;
                // the 7 points: reference point and two curves
                if st.flex.len() >= 14 {
                    let f = st.flex.clone();
                    st.p.curve_to(f[2], f[3], f[4], f[5], f[6], f[7]);
                    st.p.curve_to(f[8], f[9], f[10], f[11], f[12], f[13]);
                    st.x = f[12];
                    st.y = f[13];
                }
                // the final point goes back for setcurrentpoint
                st.ps = vec![st.y, st.x];
            } else if other == 3 {
                st.ps = vec![3.0];
            } else {
                // unknown: the arguments come back in reverse for `pop`
                let mut r = args.clone();
                r.reverse();
                st.ps = r;
            }
        } else if e == 17 {
            // pop: from the PostScript stack
            let mut r = s.clone();
            if !st.ps.is_empty() {
                let v = st.ps[0];
                st.ps.remove(0);
                r.push(v);
            }
            st.stack = r;
        } else if e == 33 && n >= 2 {
            // setcurrentpoint
            st.x = s[0];
            st.y = s[1];
        }
    }

    fn line(&self, st: &mut T1, dx: f64, dy: f64) {
        st.x += dx;
        st.y += dy;
        if !st.open {
            st.p.move_to(st.x - dx, st.y - dy);
            st.open = true;
        }
        st.p.line_to(st.x, st.y);
    }

    fn curve(&self, st: &mut T1, a: f64, b: f64, c: f64, d: f64, e: f64, f: f64) {
        if !st.open {
            st.p.move_to(st.x, st.y);
            st.open = true;
        }
        let x1 = st.x + a;
        let y1 = st.y + b;
        let x2 = x1 + c;
        let y2 = y1 + d;
        st.x = x2 + e;
        st.y = y2 + f;
        st.p.curve_to(x1, y1, x2, y2, st.x, st.y);
    }

    fn moveto(&self, st: &mut T1, dx: f64, dy: f64) {
        st.x += dx;
        st.y += dy;
        if st.in_flex {
            st.flex.push(st.x);
            st.flex.push(st.y);
            return;
        }
        if st.open {
            st.p.close();
        }
        st.p.move_to(st.x, st.y);
        st.open = true;
    }

    fn seac(&self, st: &mut T1, asb: f64, adx: f64, ady: f64, bchar: i64, achar: i64) {
        let se = crate::pdf_fontdata::standard_encoding();
        let mut bname = String::new();
        let mut aname = String::new();
        for item in se.split(';') {
            let parts: Vec<&str> = item.split(' ').collect();
            if parts.len() == 2 {
                let c = num(parts[0]);
                if c == bchar {
                    bname = parts[1].to_string();
                }
                if c == achar {
                    aname = parts[1].to_string();
                }
            }
        }
        if st.open {
            st.p.close();
            st.open = false;
        }
        let mut w: Vec<f64> = vec![];
        let bp = self.glyph_path(&bname, &mut w);
        let id: Vec<f64> = vec![1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
        st.p.append(&bp, &id);
        let mut w2: Vec<f64> = vec![];
        let ap = self.glyph_path(&aname, &mut w2);
        // the accent's origin moves by adx - asb (Type 1 Font Format §6.4)
        let m: Vec<f64> = vec![1.0, 0.0, 0.0, 1.0, adx - asb, ady];
        st.p.append(&ap, &m);
        st.done = true;
    }
}

struct T1 {
    p: Path,
    stack: Vec<f64>,
    ps: Vec<f64>,
    x: f64,
    y: f64,
    sbx: f64,
    open: bool,
    done: bool,
    depth: i64,
    flex: Vec<f64>,
    in_flex: bool,
    width: f64,
}

fn slice_of(d: &Vec<u8>, a: usize, b: usize) -> Vec<u8> {
    let mut v: Vec<u8> = vec![];
    let mut i = a;
    while i < b && i < d.len() {
        v.push(d[i]);
        i += 1;
    }
    v
}

pub fn parse_float(s: &str) -> f64 {
    let mut neg = false;
    let mut ip: f64 = 0.0;
    let mut fp: f64 = 0.0;
    let mut sc: f64 = 1.0;
    let mut dot = false;
    for c in s.chars() {
        if c == '-' {
            neg = true;
        } else if c == '.' {
            dot = true;
        } else if c >= '0' && c <= '9' {
            let v = ((c as u32) - 48) as f64;
            if dot {
                sc = sc / 10.0;
                fp += v * sc;
            } else {
                ip = ip * 10.0 + v;
            }
        }
    }
    let v = ip + fp;
    if neg {
        return 0.0 - v;
    }
    v
}
