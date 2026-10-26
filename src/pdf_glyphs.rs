//! Glyph names and the simple-font encodings: the Adobe Glyph List and the
//! encodings of ISO 32000-1 Annex D (fontdata.rs), read once into a `Glyphs`
//! a document's fonts share.

use std::collections::HashMap;

use crate::pdf_fontdata;

pub struct Glyphs {
    agl: HashMap<String, String>,
    pub standard: Vec<String>,
    pub win_ansi: Vec<String>,
    pub mac_roman: Vec<String>,
    pub pdf_doc: Vec<String>,
    pub symbol: Vec<String>,
    pub zapf: Vec<String>,
}

fn hexnum(s: &str) -> i64 {
    let mut v: i64 = 0;
    let mut any = false;
    for c in s.chars() {
        let h = crate::pdf_lexer::hexval(c as u8);
        if (c as u32) > 127 || h < 0 {
            return -1;
        }
        v = v * 16 + h;
        any = true;
    }
    if !any {
        return -1;
    }
    v
}

fn coded(s: &str) -> Vec<String> {
    // "code name;..." as 256 names
    let mut v: Vec<String> = vec![String::new(); 256];
    for item in s.split(';') {
        let parts: Vec<&str> = item.split(' ').collect();
        if parts.len() == 2 {
            let c = crate::pdf_cff::parse_int(parts[0]);
            if c >= 0 && c < 256 {
                v[c as usize] = parts[1].to_string();
            }
        }
    }
    v
}

fn spaced(s: &str) -> Vec<String> {
    let mut v: Vec<String> = vec![];
    for n in s.split(' ') {
        v.push(n.to_string());
    }
    while v.len() < 256 {
        v.push(String::new());
    }
    v
}

/// uniXXXX[XXXX...], uXXXX[XX]: the Unicode a name spells out, or "".
pub fn name_to_unicode(name: &str) -> String {
    let cs: Vec<char> = name.chars().collect();
    let n = cs.len();
    if n >= 7 && cs[0] == 'u' && cs[1] == 'n' && cs[2] == 'i' && (n - 3) % 4 == 0 {
        let mut out = String::new();
        let mut i: usize = 3;
        while i + 4 <= n {
            let mut part = String::new();
            let mut k = i;
            while k < i + 4 {
                part.push(cs[k]);
                k += 1;
            }
            let v = hexnum(&part);
            if v < 0 {
                return String::new();
            }
            out.push(crate::pdf_obj::ch(v as u32));
            i += 4;
        }
        return out;
    }
    if n >= 5 && n <= 7 && cs[0] == 'u' {
        let mut part = String::new();
        let mut k: usize = 1;
        while k < n {
            part.push(cs[k]);
            k += 1;
        }
        let v = hexnum(&part);
        if v >= 0 {
            let mut out = String::new();
            out.push(crate::pdf_obj::ch(v as u32));
            return out;
        }
    }
    String::new()
}

impl Glyphs {
    pub fn load() -> Glyphs {
        let mut agl: HashMap<String, String> = HashMap::new();
        let text = pdf_fontdata::agl();
        for item in text.split(';') {
            let parts: Vec<&str> = item.split(' ').collect();
            if parts.len() >= 2 {
                let mut u = String::new();
                let mut i: usize = 1;
                while i < parts.len() {
                    let v = hexnum(parts[i]);
                    if v >= 0 {
                        u.push(crate::pdf_obj::ch(v as u32));
                    }
                    i += 1;
                }
                agl.insert(parts[0].to_string(), u);
            }
        }
        Glyphs {
            agl: agl,
            standard: coded(&pdf_fontdata::standard_encoding()),
            win_ansi: spaced(&pdf_fontdata::win_ansi()),
            mac_roman: spaced(&pdf_fontdata::mac_roman()),
            pdf_doc: spaced(&pdf_fontdata::pdf_doc()),
            symbol: coded(&pdf_fontdata::enc_symbol()),
            zapf: coded(&pdf_fontdata::enc_zapfdingbats()),
        }
    }

    /// The text of glyph `name`: the glyph list, uniXXXX/uXXXX, a name with
    /// a suffix (a.sc, f_i), or "" when unknown.
    pub fn unicode(&self, name: &str) -> String {
        match self.agl.get(name) {
            Some(u) => {
                return u.clone();
            }
            None => {}
        }
        let u = name_to_unicode(name);
        if !u.is_empty() {
            return u;
        }
        // a.sc, T.alt: the part before the dot
        let dot: Vec<&str> = name.split('.').collect();
        if dot.len() > 1 && !dot[0].is_empty() {
            return self.unicode(dot[0]);
        }
        // f_f_i: the parts joined
        let parts: Vec<&str> = name.split('_').collect();
        if parts.len() > 1 {
            let mut s = String::new();
            for p in parts.iter() {
                let x = self.unicode(p);
                if x.is_empty() {
                    return String::new();
                }
                s.push_str(&x);
            }
            return s;
        }
        // gXX, cidXX, and the like carry no text
        String::new()
    }

    pub fn encoding(&self, name: &str) -> Vec<String> {
        if name == "WinAnsiEncoding" {
            return self.win_ansi.clone();
        }
        if name == "MacRomanEncoding" {
            return self.mac_roman.clone();
        }
        if name == "PDFDocEncoding" {
            return self.pdf_doc.clone();
        }
        if name == "Symbol" {
            return self.symbol.clone();
        }
        if name == "ZapfDingbats" {
            return self.zapf.clone();
        }
        self.standard.clone()
    }
}
