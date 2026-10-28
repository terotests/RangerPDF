//! PDF fonts (ISO 32000-1 §9.5–§9.10): simple fonts (Type1, TrueType,
//! Type3) with their encodings and widths, composite fonts (Type0 over
//! CIDFontType0/2) with CMaps, and the text of each code (ToUnicode, glyph
//! names). Outlines come from the embedded font program; a font that is not
//! embedded takes the substitute the host registered for its family
//! (`FontFiles`), as viewers do for the 14 standard fonts.

use std::collections::HashMap;

use crate::pdf_cff::Cff;
use crate::pdf_cmap::CMap;
use crate::pdf_doc::Doc;
use crate::pdf_glyphs::Glyphs;
use crate::pdf_obj;
use crate::pdf_obj::Obj;
use crate::pdf_obj::K;
use crate::pdf_path::Path;
use crate::pdf_truetype::TrueType;
use crate::pdf_type1::Type1;

/// Substitute font programs from the host: key "sans", "sans-bold",
/// "sans-italic", "sans-bolditalic", the same for "serif" and "mono", then
/// "symbol" and "dingbats". Each is a TrueType or Type 1 (pfb) file.
#[derive(Clone)]
pub struct FontFiles {
    pub keys: Vec<String>,
    pub data: Vec<Vec<u8>>,
}

impl FontFiles {
    pub fn new() -> FontFiles {
        FontFiles { keys: vec![], data: vec![] }
    }

    pub fn add(&mut self, key: &str, data: Vec<u8>) {
        self.keys.push(key.to_string());
        self.data.push(data);
    }

    pub fn add_ints(&mut self, key: &str, data: &Vec<i64>) {
        let mut b: Vec<u8> = vec![];
        for x in data.iter() {
            b.push((*x & 255) as u8);
        }
        self.add(key, b);
    }

    pub fn find(&self, key: &str) -> i64 {
        let mut i: usize = 0;
        while i < self.keys.len() {
            if self.keys[i] == key {
                return i as i64;
            }
            i += 1;
        }
        -1
    }
}

/// One code of a shown string.
#[derive(Clone, Debug)]
pub struct Code {
    pub code: i64,
    pub len: i64,
    /// the advance in glyph space / 1000 (text space units at size 1)
    pub width: f64,
    pub text: String,
    /// a single-byte code 32: word spacing applies
    pub space: bool,
}

pub const PROG_NONE: i64 = 0;
pub const PROG_TT: i64 = 1;
pub const PROG_CFF: i64 = 2;
pub const PROG_T1: i64 = 3;

pub struct Font {
    pub subtype: String,
    pub base: String,
    pub composite: bool,
    pub cmap: CMap,
    pub to_unicode: CMap,
    pub has_to_unicode: bool,
    pub vertical: bool,
    first_char: i64,
    widths: Vec<f64>,
    has_widths: bool,
    missing_width: f64,
    cid_w: HashMap<i64, f64>,
    dw: f64,
    /// code -> glyph name for simple fonts
    pub enc: Vec<String>,
    /// codes the /Differences named
    differences: Vec<bool>,
    pub symbolic: bool,
    pub flags: i64,
    pub bold: bool,
    pub italic: bool,
    /// "sans", "serif", "mono", "symbol" or "dingbats"
    pub family: String,
    /// the standard font whose metrics apply, or ""
    pub std_name: String,
    std_w: HashMap<String, f64>,
    pub prog: i64,
    pub embedded: bool,
    tt: TrueType,
    cff: Cff,
    t1: Type1,
    /// glyph space -> text space
    pub matrix: Vec<f64>,
    cid_to_gid: Vec<i64>,
    cid_to_gid_identity: bool,
    cache: HashMap<i64, Path>,
    /// a simple TrueType font's glyph per code
    simple_gid: Vec<i64>,
    // Type 3
    pub procs: Obj,
    pub resources: Obj,
}

fn strip_subset(name: &str) -> String {
    let cs: Vec<char> = name.chars().collect();
    if cs.len() > 7 && cs[6] == '+' {
        let mut ok = true;
        let mut i: usize = 0;
        while i < 6 {
            if cs[i] < 'A' || cs[i] > 'Z' {
                ok = false;
            }
            i += 1;
        }
        if ok {
            let mut s = String::new();
            let mut k: usize = 7;
            while k < cs.len() {
                s.push(cs[k]);
                k += 1;
            }
            return s;
        }
    }
    name.to_string()
}

fn has(s: &str, part: &str) -> bool {
    s.contains(part)
}

fn lower(s: &str) -> String {
    let mut o = String::new();
    for c in s.chars() {
        if c >= 'A' && c <= 'Z' {
            o.push(crate::pdf_obj::ch((c as u32) + 32));
        } else {
            o.push(c);
        }
    }
    o
}

/// The standard font a name stands for (Arial is Helvetica, ...), or "".
pub fn standard_name(base: &str, bold: bool, italic: bool, family: &str) -> String {
    let l = lower(base);
    let mut fam = String::new();
    if has(&l, "zapfdingbats") || has(&l, "dingbats") {
        return "ZapfDingbats".to_string();
    }
    if l == "symbol" || has(&l, "symbolmt") || l.starts_with("symbol,") || l.starts_with("symbol-") {
        return "Symbol".to_string();
    }
    if has(&l, "courier") {
        fam = "Courier".to_string();
    } else if has(&l, "helvetica") || has(&l, "arial") {
        fam = "Helvetica".to_string();
    } else if has(&l, "times") {
        fam = "Times".to_string();
    } else if family == "mono" {
        fam = "Courier".to_string();
    } else if family == "serif" {
        fam = "Times".to_string();
    } else if family == "sans" {
        fam = "Helvetica".to_string();
    }
    if fam.is_empty() {
        return String::new();
    }
    if fam == "Times" {
        if bold && italic {
            return "Times-BoldItalic".to_string();
        }
        if bold {
            return "Times-Bold".to_string();
        }
        if italic {
            return "Times-Italic".to_string();
        }
        return "Times-Roman".to_string();
    }
    if bold && italic {
        return format!("{}-BoldOblique", fam);
    }
    if bold {
        return format!("{}-Bold", fam);
    }
    if italic {
        return format!("{}-Oblique", fam);
    }
    fam
}

impl Font {
    fn empty() -> Font {
        Font {
            subtype: String::new(),
            base: String::new(),
            composite: false,
            cmap: CMap::single_byte(),
            to_unicode: CMap::new(),
            has_to_unicode: false,
            vertical: false,
            first_char: 0,
            widths: vec![],
            has_widths: false,
            missing_width: 0.0,
            cid_w: HashMap::new(),
            dw: 1000.0,
            enc: vec![String::new(); 256],
            differences: vec![false; 256],
            symbolic: false,
            flags: 0,
            bold: false,
            italic: false,
            family: "sans".to_string(),
            std_name: String::new(),
            std_w: HashMap::new(),
            prog: PROG_NONE,
            embedded: false,
            tt: TrueType::parse(vec![]),
            cff: Cff::parse(vec![]),
            t1: Type1::parse(&vec![], 0),
            matrix: vec![0.001, 0.0, 0.0, 0.001, 0.0, 0.0],
            cid_to_gid: vec![],
            cid_to_gid_identity: true,
            cache: HashMap::new(),
            simple_gid: vec![],
            procs: pdf_obj::null(),
            resources: pdf_obj::null(),
        }
    }

    pub fn load(doc: &mut Doc, g: &Glyphs, files: &FontFiles, fd: &Obj) -> Font {
        let mut f = Font::empty();
        f.subtype = doc.get_key(fd, "Subtype").name.clone();
        f.base = strip_subset(&doc.get_key(fd, "BaseFont").name);
        let mut desc_font = fd.clone();
        if f.subtype == "Type0" {
            f.composite = true;
            let enc = doc.get_key(fd, "Encoding");
            if enc.k == K::Name {
                f.cmap = CMap::identity(enc.name.ends_with("-V"));
                if !enc.name.starts_with("Identity") {
                    // a predefined CMap is not available: two-byte codes as CIDs
                    f.cmap.name = enc.name.clone();
                }
            } else if enc.k == K::Stream {
                let data = doc.stream_data(&enc);
                f.cmap = CMap::parse(data);
                if f.cmap.space_len.is_empty() {
                    f.cmap = CMap::identity(f.cmap.vertical);
                }
            } else {
                f.cmap = CMap::identity(false);
            }
            f.vertical = f.cmap.vertical;
            let df = doc.get_key(fd, "DescendantFonts");
            let d0 = if df.k == K::Arr { doc.resolve(&df.item(0)) } else { df.clone() };
            if d0.is_dict() {
                desc_font = d0.clone();
                f.read_cid_widths(doc, &d0);
                let ctg = doc.get_key(&d0, "CIDToGIDMap");
                if ctg.k == K::Stream {
                    let data = doc.stream_data(&ctg);
                    let mut i: usize = 0;
                    while i + 1 < data.len() {
                        f.cid_to_gid.push(((data[i] as i64) << 8) | (data[i + 1] as i64));
                        i += 2;
                    }
                    f.cid_to_gid_identity = false;
                }
                if f.base.is_empty() {
                    f.base = strip_subset(&doc.get_key(&d0, "BaseFont").name);
                }
            }
        }
        let desc = doc.get_key(&desc_font, "FontDescriptor");
        if desc.is_dict() {
            f.flags = doc.get_key(&desc, "Flags").i();
            f.missing_width = doc.get_key(&desc, "MissingWidth").f();
            f.symbolic = (f.flags & 4) != 0;
            f.italic = (f.flags & 64) != 0 || doc.get_key(&desc, "ItalicAngle").f() != 0.0;
            f.bold = (f.flags & 262144) != 0 || doc.get_key(&desc, "FontWeight").f() >= 600.0;
        }
        let lb = lower(&f.base);
        if has(&lb, "bold") || has(&lb, "black") || has(&lb, "heavy") || has(&lb, "semibold") || has(&lb, "demi") {
            f.bold = true;
        }
        if has(&lb, "italic") || has(&lb, "oblique") {
            f.italic = true;
        }
        f.family = Font::family_of(&lb, f.flags);
        f.std_name = standard_name(&f.base, f.bold, f.italic, "");
        if f.subtype == "Type3" {
            f.matrix = doc.get_key(fd, "FontMatrix").nums();
            if f.matrix.len() != 6 {
                f.matrix = vec![0.001, 0.0, 0.0, 0.001, 0.0, 0.0];
            }
            f.procs = doc.get_key(fd, "CharProcs");
            f.resources = doc.get_key(fd, "Resources");
        }
        if !f.composite {
            f.read_simple_widths(doc, fd);
            f.read_encoding(doc, g, fd);
        }
        let tu = doc.get_key(fd, "ToUnicode");
        if tu.k == K::Stream {
            let data = doc.stream_data(&tu);
            f.to_unicode = CMap::parse(data);
            f.has_to_unicode = !f.to_unicode.uni.is_empty();
        }
        if desc.is_dict() {
            f.read_program(doc, &desc);
        }
        if f.prog == PROG_NONE && f.subtype != "Type3" {
            f.substitute(files);
        }
        if f.prog == PROG_TT && !f.composite {
            let mut code: i64 = 0;
            while code < 256 {
                let u = g.unicode(&f.enc[code as usize]);
                let mut cp: i64 = -1;
                if !u.is_empty() {
                    let cs: Vec<char> = u.chars().collect();
                    cp = cs[0] as i64;
                }
                let gid = f.tt_simple_gid(code, cp);
                f.simple_gid.push(gid);
                code += 1;
            }
        }
        if !f.has_widths && !f.composite {
            // the standard font's metrics
            let sn = if f.std_name.is_empty() { standard_name(&f.base, f.bold, f.italic, &f.family.clone()) } else { f.std_name.clone() };
            let w = crate::pdf_fontdata::std_widths(&sn);
            for item in w.split(';') {
                let parts: Vec<&str> = item.split(' ').collect();
                if parts.len() == 2 {
                    f.std_w.insert(parts[0].to_string(), crate::pdf_cff::parse_int(parts[1]) as f64);
                }
            }
            if !f.std_w.is_empty() && f.std_name.is_empty() {
                f.std_name = sn;
            }
        }
        f
    }

    fn family_of(l: &str, flags: i64) -> String {
        if has(l, "dingbats") {
            return "dingbats".to_string();
        }
        if l == "symbol" || has(l, "symbolmt") {
            return "symbol".to_string();
        }
        if has(l, "courier") || has(l, "mono") || has(l, "consolas") || (flags & 1) != 0 {
            return "mono".to_string();
        }
        if has(l, "times") || has(l, "serif") && !has(l, "sans") || has(l, "georgia") || has(l, "garamond") || has(l, "roman") || has(l, "minion") || has(l, "palatino") || has(l, "bookman") || has(l, "cambria") || has(l, "century") {
            return "serif".to_string();
        }
        if has(l, "arial") || has(l, "helvetica") || has(l, "sans") || has(l, "verdana") || has(l, "calibri") || has(l, "tahoma") {
            return "sans".to_string();
        }
        if (flags & 2) != 0 {
            return "serif".to_string();
        }
        "sans".to_string()
    }

    fn read_simple_widths(&mut self, doc: &mut Doc, fd: &Obj) {
        let w = doc.get_key(fd, "Widths");
        if w.k == K::Arr {
            self.first_char = doc.get_key(fd, "FirstChar").i();
            for x in w.items.iter() {
                let v = doc.resolve(x);
                self.widths.push(v.f());
            }
            self.has_widths = true;
        }
    }

    fn read_cid_widths(&mut self, doc: &mut Doc, d0: &Obj) {
        let dw = doc.get_key(d0, "DW");
        if dw.is_num() {
            self.dw = dw.f();
        }
        let w = doc.get_key(d0, "W");
        if w.k != K::Arr {
            return;
        }
        let mut i: usize = 0;
        let n = w.items.len();
        while i < n {
            let a = doc.resolve(&w.items[i]);
            if i + 1 >= n {
                break;
            }
            let b = doc.resolve(&w.items[i + 1]);
            if b.k == K::Arr {
                let mut c = a.i();
                for x in b.items.iter() {
                    let v = doc.resolve(x);
                    self.cid_w.insert(c, v.f());
                    c += 1;
                }
                i += 2;
            } else {
                if i + 2 >= n {
                    break;
                }
                let v = doc.resolve(&w.items[i + 2]);
                let mut c = a.i();
                let last = b.i();
                if last - c > 65536 {
                    i += 3;
                    continue;
                }
                while c <= last {
                    self.cid_w.insert(c, v.f());
                    c += 1;
                }
                i += 3;
            }
        }
        self.has_widths = true;
    }

    fn read_encoding(&mut self, doc: &mut Doc, g: &Glyphs, fd: &Obj) {
        let enc = doc.get_key(fd, "Encoding");
        let lb = lower(&self.base);
        // the base: the named encoding, or the font's own
        let mut base = "StandardEncoding".to_string();
        if has(&lb, "symbol") && !self.composite {
            base = "Symbol".to_string();
        }
        if has(&lb, "dingbats") {
            base = "ZapfDingbats".to_string();
        }
        if self.subtype == "TrueType" && !self.symbolic {
            base = "WinAnsiEncoding".to_string();
        }
        let mut named = false;
        if enc.k == K::Name {
            base = enc.name.clone();
            named = true;
        } else if enc.is_dict() {
            let be = doc.get_key(&enc, "BaseEncoding");
            if be.k == K::Name {
                base = be.name.clone();
                named = true;
            }
        }
        let _ = named;
        self.enc = g.encoding(&base);
        if enc.is_dict() {
            let d = doc.get_key(&enc, "Differences");
            let mut code: i64 = 0;
            for x in d.items.iter() {
                let v = doc.resolve(x);
                if v.k == K::Num {
                    code = v.i();
                } else if v.k == K::Name {
                    if code >= 0 && code < 256 {
                        self.enc[code as usize] = v.name.clone();
                        self.differences[code as usize] = true;
                    }
                    code += 1;
                }
            }
        }
    }

    fn read_program(&mut self, doc: &mut Doc, desc: &Obj) {
        let f2 = doc.get_key(desc, "FontFile2");
        if f2.k == K::Stream {
            let data = doc.stream_data(&f2);
            self.load_sfnt(data);
            if self.prog != PROG_NONE {
                self.embedded = true;
                return;
            }
        }
        let f3 = doc.get_key(desc, "FontFile3");
        if f3.k == K::Stream {
            let data = doc.stream_data(&f3);
            let st = doc.get_key(&f3, "Subtype").name.clone();
            if st == "OpenType" || (data.len() > 4 && data[0] == 79 && data[1] == 84) || (data.len() > 4 && data[0] == 0 && data[1] == 1) {
                self.load_sfnt(data);
            } else {
                self.load_cff(data);
            }
            if self.prog != PROG_NONE {
                self.embedded = true;
                return;
            }
        }
        let f1 = doc.get_key(desc, "FontFile");
        if f1.k == K::Stream {
            let data = doc.stream_data(&f1);
            let l1 = doc.get_key(&f1, "Length1").i();
            if data.len() > 4 && data[0] == 1 && data[1] == 0 && data[2] == 4 {
                // a CFF font in a /FontFile
                self.load_cff(data);
            } else {
                let t = Type1::parse(&data, l1);
                if t.ok {
                    self.t1 = t;
                    self.prog = PROG_T1;
                }
            }
            if self.prog != PROG_NONE {
                self.embedded = true;
            }
        }
    }

    fn load_sfnt(&mut self, data: Vec<u8>) {
        let t = TrueType::parse(data);
        if !t.ok {
            return;
        }
        if t.glyf >= 0 && t.loca >= 0 {
            self.tt = t;
            self.prog = PROG_TT;
            return;
        }
        if t.cff_off >= 0 {
            let mut cd: Vec<u8> = vec![];
            let mut i = t.cff_off as usize;
            let end = (t.cff_off + t.cff_len) as usize;
            while i < end && i < t.data.len() {
                cd.push(t.data[i]);
                i += 1;
            }
            self.tt = t;
            self.load_cff(cd);
        }
    }

    fn load_cff(&mut self, data: Vec<u8>) {
        let c = Cff::parse(data);
        if c.ok {
            self.cff = c;
            self.prog = PROG_CFF;
        }
    }

    fn substitute(&mut self, files: &FontFiles) {
        let mut key = self.family.clone();
        if key == "sans" || key == "serif" || key == "mono" {
            if self.bold && self.italic {
                key = format!("{}-bolditalic", key);
            } else if self.bold {
                key = format!("{}-bold", key);
            } else if self.italic {
                key = format!("{}-italic", key);
            }
        }
        let mut i = files.find(&key);
        if i < 0 {
            i = files.find(&self.family);
        }
        if i < 0 {
            return;
        }
        let data = files.data[i as usize].clone();
        if data.len() > 4 && data[0] == 1 && data[1] == 0 && data[2] >= 4 {
            // a bare CFF (pdf.js' standard fonts are CFF, whatever their name)
            self.load_cff(data);
            return;
        }
        if data.len() > 2 && (data[0] == 128 || data[0] == 37) {
            let t = Type1::parse(&data, 0);
            if t.ok {
                self.t1 = t;
                self.prog = PROG_T1;
            }
        } else {
            self.load_sfnt(data);
        }
    }

    /// The codes of string `s` with their widths and text.
    pub fn codes(&self, g: &Glyphs, s: &Vec<u8>) -> Vec<Code> {
        let mut out: Vec<Code> = vec![];
        let mut pos: usize = 0;
        while pos < s.len() {
            let mut cl: Vec<i64> = vec![];
            if self.composite {
                self.cmap.next_code(s, pos, &mut cl);
            } else {
                cl.push(s[pos] as i64);
                cl.push(1);
            }
            let code = cl[0];
            let len = cl[1];
            pos += len as usize;
            let w = self.width(code);
            let t = self.text_of(g, code);
            out.push(Code { code: code, len: len, width: w, text: t, space: len == 1 && code == 32 });
        }
        out
    }

    pub fn width(&self, code: i64) -> f64 {
        if self.composite {
            let cid = self.cmap.cid(code);
            match self.cid_w.get(&cid) {
                Some(w) => {
                    return *w / 1000.0;
                }
                None => {}
            }
            return self.dw / 1000.0;
        }
        let scale = if self.subtype == "Type3" { 1.0 } else { 0.001 };
        if self.has_widths {
            let i = code - self.first_char;
            if i >= 0 && (i as usize) < self.widths.len() {
                return self.widths[i as usize] * scale;
            }
            return self.missing_width * scale;
        }
        if code >= 0 && code < 256 {
            let name = self.enc[code as usize].clone();
            match self.std_w.get(&name) {
                Some(w) => {
                    return *w / 1000.0;
                }
                None => {}
            }
        }
        if self.prog == PROG_TT {
            let gid = self.gid(code);
            return self.tt.advance(gid) / self.tt.units_per_em;
        }
        if self.prog == PROG_CFF {
            let gid = self.gid(code);
            return self.cff.glyph_width(gid) * self.cff.matrix[0];
        }
        if self.missing_width > 0.0 {
            return self.missing_width / 1000.0;
        }
        0.5
    }

    pub fn text_of(&self, g: &Glyphs, code: i64) -> String {
        if self.has_to_unicode {
            match self.to_unicode.uni.get(&code) {
                Some(t) => {
                    return t.clone();
                }
                None => {}
            }
        }
        if self.composite {
            // without a ToUnicode: an identity mapping often is Unicode for UCS2 CMaps
            if self.cmap.name.starts_with("Uni") && self.cmap.name.contains("UCS2") {
                return crate::pdf_obj::ch(code as u32).to_string();
            }
            if self.prog == PROG_CFF && !self.cff.cid {
                let n = self.cff.glyph_name(self.gid(code));
                let u = g.unicode(&n);
                if !u.is_empty() {
                    return u;
                }
            }
            return code_char(code);
        }
        if code < 0 || code > 255 {
            return String::new();
        }
        let name = self.enc[code as usize].clone();
        if !name.is_empty() {
            let u = g.unicode(&name);
            if !u.is_empty() {
                return u;
            }
        }
        // the font's own names
        if self.prog == PROG_CFF {
            let n = self.cff.glyph_name(self.gid(code));
            let u = g.unicode(&n);
            if !u.is_empty() {
                return u;
            }
        }
        if self.prog == PROG_T1 && !self.t1.encoding[code as usize].is_empty() {
            let u = g.unicode(&self.t1.encoding[code as usize]);
            if !u.is_empty() {
                return u;
            }
        }
        code_char(code)
    }

    /// The glyph of `code` in the embedded (or substitute) program.
    fn gid(&self, code: i64) -> i64 {
        if self.prog == PROG_TT {
            if self.composite {
                let cid = self.cmap.cid(code);
                if !self.cid_to_gid_identity {
                    if cid >= 0 && (cid as usize) < self.cid_to_gid.len() {
                        return self.cid_to_gid[cid as usize];
                    }
                    return 0;
                }
                return cid;
            }
            if code >= 0 && (code as usize) < self.simple_gid.len() {
                return self.simple_gid[code as usize];
            }
            return 0;
        }
        if self.prog == PROG_CFF {
            if self.composite {
                let cid = self.cmap.cid(code);
                if self.cff.cid {
                    return self.cff.glyph_by_cid(cid);
                }
                return cid;
            }
            if code >= 0 && code < 256 {
                let name = self.enc[code as usize].clone();
                if self.differences[code as usize] || !self.symbolic {
                    let g = self.cff.glyph_by_name(&name);
                    if g > 0 {
                        return g;
                    }
                }
                if (code as usize) < self.cff.encoding.len() && self.cff.encoding[code as usize] > 0 {
                    return self.cff.encoding[code as usize];
                }
                let g2 = self.cff.glyph_by_name(&name);
                if g2 > 0 {
                    return g2;
                }
            }
            if self.cff.cid {
                return self.cff.glyph_by_cid(code);
            }
            return 0;
        }
        0
    }

    fn tt_simple_gid(&self, code: i64, uni: i64) -> i64 {
        let t = &self.tt;
        let name = if code >= 0 && code < 256 { self.enc[code as usize].clone() } else { String::new() };
        if !self.symbolic || self.differences[(code & 255) as usize] {
            // through the glyph name's Unicode and the (3,1) cmap
            if t.has_cmap(3, 1) && uni >= 0 {
                let gid = t.lookup(3, 1, uni);
                if gid > 0 {
                    return gid;
                }
            }
            if !name.is_empty() {
                let gp = t.glyph_by_name(&name);
                if gp > 0 {
                    return gp;
                }
            }
        }
        if t.has_cmap(3, 0) {
            let g = t.lookup(3, 0, 0xF000 + code);
            if g > 0 {
                return g;
            }
            let g2 = t.lookup(3, 0, code);
            if g2 > 0 {
                return g2;
            }
        }
        if t.has_cmap(1, 0) {
            let g = t.lookup(1, 0, code);
            if g > 0 {
                return g;
            }
        }
        if t.has_cmap(3, 1) {
            let g = t.lookup(3, 1, code);
            if g > 0 {
                return g;
            }
        }
        let g = t.lookup_any(code);
        if g > 0 {
            return g;
        }
        if !t.has_any_cmap() {
            return code;
        }
        0
    }

    pub fn has_outlines(&self) -> bool {
        self.prog != PROG_NONE
    }

    /// The outline of `code` in text space at size 1 (y up), or an empty path.
    pub fn outline(&mut self, code: i64) -> Path {
        match self.cache.get(&code) {
            Some(p) => {
                return p.clone();
            }
            None => {}
        }
        let mut p = Path::new();
        if self.prog == PROG_TT {
            let gid = self.gid(code);
            let raw = self.tt.glyph_path(gid);
            let s = 1.0 / self.tt.units_per_em;
            let m: Vec<f64> = vec![s, 0.0, 0.0, s, 0.0, 0.0];
            p.append(&raw, &m);
        } else if self.prog == PROG_CFF {
            let gid = self.gid(code);
            let raw = self.cff.glyph_path(gid);
            let m = self.cff.matrix.clone();
            p.append(&raw, &m);
        } else if self.prog == PROG_T1 {
            let mut name = if code >= 0 && code < 256 { self.enc[code as usize].clone() } else { String::new() };
            if self.symbolic && !self.differences[(code & 255) as usize] && code >= 0 && code < 256 && !self.t1.encoding[code as usize].is_empty() && self.embedded {
                name = self.t1.encoding[code as usize].clone();
            }
            if (name.is_empty() || !self.t1.has_glyph(&name)) && code >= 0 && code < 256 {
                name = self.t1.encoding[code as usize].clone();
            }
            let mut w: Vec<f64> = vec![];
            let raw = self.t1.glyph_path(&name, &mut w);
            let m = self.t1.matrix.clone();
            p.append(&raw, &m);
        }
        self.cache.insert(code, p.clone());
        p
    }
}


/// The text of a code nothing maps to Unicode: the code itself as a
/// character, as pdf.js gives it (it keeps the text's length and find).
fn code_char(code: i64) -> String {
    if code <= 0 || (code >= 0xD800 && code < 0xE000) || code > 0xFFFF {
        return String::new();
    }
    crate::pdf_obj::ch(code as u32).to_string()
}
