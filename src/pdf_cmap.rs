//! CMaps (Adobe Technical Note 5014, ISO 32000-1 §9.7.5, §9.10.3): the
//! codespace ranges that split a string into codes, code -> CID mappings of
//! an embedded CMap, and code -> Unicode of a ToUnicode CMap.

use std::collections::HashMap;

use crate::pdf_lexer::PdfLexer;
use crate::pdf_obj::Obj;
use crate::pdf_obj::K;


pub struct CMap {
    /// per range: byte length, low, high
    pub space_len: Vec<i64>,
    pub space_lo: Vec<i64>,
    pub space_hi: Vec<i64>,
    /// code -> CID ranges: lo, hi, first cid (and single codes as ranges of one)
    pub cid_lo: Vec<i64>,
    pub cid_hi: Vec<i64>,
    pub cid_first: Vec<i64>,
    /// code -> text
    pub uni: HashMap<i64, String>,
    pub identity: bool,
    pub vertical: bool,
    pub name: String,
    pub use_cmap: String,
}

fn code_of(s: &Vec<u8>) -> i64 {
    let mut v: i64 = 0;
    for b in s.iter() {
        v = v * 256 + (*b as i64);
    }
    v
}

/// UTF-16BE bytes as text.
pub fn utf16(s: &Vec<u8>) -> String {
    let mut out = String::new();
    let mut i: usize = 0;
    while i + 1 < s.len() {
        let mut u = ((s[i] as u32) << 8) | (s[i + 1] as u32);
        i += 2;
        if u >= 0xD800 && u < 0xDC00 && i + 1 < s.len() {
            let lo = ((s[i] as u32) << 8) | (s[i + 1] as u32);
            i += 2;
            u = 0x10000 + ((u - 0xD800) << 10) + (lo - 0xDC00);
        }
        out.push(crate::pdf_obj::ch(u));
    }
    if s.len() == 1 {
        out.push(crate::pdf_obj::ch(s[0] as u32));
    }
    out
}

/// The text `s` (UTF-16BE) with its last code unit increased by `n`.
fn plus(s: &Vec<u8>, n: i64) -> Vec<u8> {
    let mut v = s.clone();
    if v.is_empty() {
        return v;
    }
    let mut carry = n;
    let mut i = v.len();
    while i > 0 && carry != 0 {
        i -= 1;
        let x = (v[i] as i64) + carry;
        v[i] = (x & 255) as u8;
        carry = x >> 8;
    }
    v
}

impl CMap {
    pub fn new() -> CMap {
        CMap {
            space_len: vec![],
            space_lo: vec![],
            space_hi: vec![],
            cid_lo: vec![],
            cid_hi: vec![],
            cid_first: vec![],
            uni: HashMap::new(),
            identity: false,
            vertical: false,
            name: String::new(),
            use_cmap: String::new(),
        }
    }

    /// Identity-H or Identity-V: two-byte codes that are their CIDs.
    pub fn identity(vertical: bool) -> CMap {
        let mut c = CMap::new();
        c.identity = true;
        c.vertical = vertical;
        c.space_len.push(2);
        c.space_lo.push(0);
        c.space_hi.push(65535);
        c
    }

    pub fn single_byte() -> CMap {
        let mut c = CMap::new();
        c.space_len.push(1);
        c.space_lo.push(0);
        c.space_hi.push(255);
        c
    }

    pub fn parse(data: Vec<u8>) -> CMap {
        let mut c = CMap::new();
        let mut lx = PdfLexer::new(data);
        let mut prev: Vec<Obj> = vec![];
        loop {
            let t = lx.token();
            if t.k == K::Eof {
                break;
            }
            if t.is_cmd("begincodespacerange") {
                loop {
                    let a = lx.token();
                    if a.k != K::Str {
                        break;
                    }
                    let b = lx.token();
                    c.space_len.push(a.s.len() as i64);
                    c.space_lo.push(code_of(&a.s));
                    c.space_hi.push(code_of(&b.s));
                }
            } else if t.is_cmd("begincidrange") {
                loop {
                    let a = lx.token();
                    if a.k != K::Str {
                        break;
                    }
                    let b = lx.token();
                    let f = lx.token();
                    c.cid_lo.push(code_of(&a.s));
                    c.cid_hi.push(code_of(&b.s));
                    c.cid_first.push(f.i());
                }
            } else if t.is_cmd("begincidchar") {
                loop {
                    let a = lx.token();
                    if a.k != K::Str {
                        break;
                    }
                    let f = lx.token();
                    let code = code_of(&a.s);
                    c.cid_lo.push(code);
                    c.cid_hi.push(code);
                    c.cid_first.push(f.i());
                }
            } else if t.is_cmd("beginbfchar") {
                loop {
                    let a = lx.token();
                    if a.k != K::Str {
                        break;
                    }
                    let b = lx.token();
                    let code = code_of(&a.s);
                    if b.k == K::Str {
                        c.uni.insert(code, utf16(&b.s));
                    } else if b.k == K::Name {
                        c.uni.insert(code, crate::pdf_glyphs::name_to_unicode(&b.name));
                    }
                }
            } else if t.is_cmd("beginbfrange") {
                loop {
                    let a = lx.token();
                    if a.k != K::Str {
                        break;
                    }
                    let b = lx.token();
                    let lo = code_of(&a.s);
                    let hi = code_of(&b.s);
                    let d = lx.object(0);
                    if hi - lo > 65536 || hi < lo {
                        continue;
                    }
                    if d.k == K::Str {
                        let mut code = lo;
                        while code <= hi {
                            c.uni.insert(code, utf16(&plus(&d.s, code - lo)));
                            code += 1;
                        }
                    } else if d.k == K::Arr {
                        let mut code = lo;
                        let mut i: usize = 0;
                        while code <= hi && i < d.items.len() {
                            let x = d.items[i].clone();
                            if x.k == K::Str {
                                c.uni.insert(code, utf16(&x.s));
                            }
                            code += 1;
                            i += 1;
                        }
                    }
                }
            } else if t.is_cmd("usecmap") {
                if !prev.is_empty() && prev[prev.len() - 1].k == K::Name {
                    c.use_cmap = prev[prev.len() - 1].name.clone();
                }
            } else if t.is_cmd("def") && prev.len() >= 2 {
                let key = prev[prev.len() - 2].clone();
                let val = prev[prev.len() - 1].clone();
                if key.is_name("CMapName") && val.k == K::Name {
                    c.name = val.name.clone();
                }
                if key.is_name("WMode") && val.i() == 1 {
                    c.vertical = true;
                }
            }
            prev.push(t);
            if prev.len() > 4 {
                prev.remove(0);
            }
        }
        if c.space_len.is_empty() {
            // a ToUnicode CMap without a codespace: the font's codes decide
            if c.use_cmap.starts_with("Identity") {
                c.identity = true;
            }
        }
        c
    }

    /// The next code in `s` from `pos`: its value in `out[0]` and length in `out[1]`.
    pub fn next_code(&self, s: &Vec<u8>, pos: usize, out: &mut Vec<i64>) {
        let mut v: i64 = 0;
        let mut len: usize = 0;
        while len < 4 && pos + len < s.len() {
            v = v * 256 + (s[pos + len] as i64);
            len += 1;
            let mut i: usize = 0;
            while i < self.space_len.len() {
                if self.space_len[i] == (len as i64) && v >= self.space_lo[i] && v <= self.space_hi[i] {
                    out.push(v);
                    out.push(len as i64);
                    return;
                }
                i += 1;
            }
        }
        // not in any range: one byte, or the shortest range's length
        let mut n: usize = 1;
        if !self.space_len.is_empty() {
            n = self.space_len[0] as usize;
            for l in self.space_len.iter() {
                if (*l as usize) < n {
                    n = *l as usize;
                }
            }
        }
        let mut w: i64 = 0;
        let mut k: usize = 0;
        while k < n && pos + k < s.len() {
            w = w * 256 + (s[pos + k] as i64);
            k += 1;
        }
        if k == 0 {
            k = 1;
        }
        out.push(w);
        out.push(k as i64);
    }

    pub fn cid(&self, code: i64) -> i64 {
        if self.identity {
            return code;
        }
        let mut i = self.cid_lo.len();
        // later entries win
        while i > 0 {
            i -= 1;
            if code >= self.cid_lo[i] && code <= self.cid_hi[i] {
                return self.cid_first[i] + (code - self.cid_lo[i]);
            }
        }
        if self.cid_lo.is_empty() {
            return code;
        }
        0
    }
}
