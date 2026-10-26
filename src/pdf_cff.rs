//! CFF fonts (Adobe Technical Notes 5176 and 5177): the /FontFile3 of
//! Type1C and CIDFontType0C fonts, and the CFF table of OpenType. Glyph
//! outlines come from the Type 2 charstrings, glyph names from the
//! charset, and codes from the built-in encoding.

use crate::pdf_doc::be;
use crate::pdf_path::Path;

/// The 391 standard strings (Technical Note 5176, Appendix A).
const STD_STRINGS: &str = ".notdef space exclam quotedbl numbersign dollar percent ampersand quoteright parenleft parenright asterisk plus comma hyphen period slash zero one two three four five six seven eight nine colon semicolon less equal greater question at A B C D E F G H I J K L M N O P Q R S T U V W X Y Z bracketleft backslash bracketright asciicircum underscore quoteleft a b c d e f g h i j k l m n o p q r s t u v w x y z braceleft bar braceright asciitilde exclamdown cent sterling fraction yen florin section currency quotesingle quotedblleft guillemotleft guilsinglleft guilsinglright fi fl endash dagger daggerdbl periodcentered paragraph bullet quotesinglbase quotedblbase quotedblright guillemotright ellipsis perthousand questiondown grave acute circumflex tilde macron breve dotaccent dieresis ring cedilla hungarumlaut ogonek caron emdash AE ordfeminine Lslash Oslash OE ordmasculine ae dotlessi lslash oslash oe germandbls onesuperior logicalnot mu trademark Eth onehalf plusminus Thorn onequarter divide brokenbar degree thorn threequarters twosuperior registered minus eth multiply threesuperior copyright Aacute Acircumflex Adieresis Agrave Aring Atilde Ccedilla Eacute Ecircumflex Edieresis Egrave Iacute Icircumflex Idieresis Igrave Ntilde Oacute Ocircumflex Odieresis Ograve Otilde Scaron Uacute Ucircumflex Udieresis Ugrave Yacute Ydieresis Zcaron aacute acircumflex adieresis agrave aring atilde ccedilla eacute ecircumflex edieresis egrave iacute icircumflex idieresis igrave ntilde oacute ocircumflex odieresis ograve otilde scaron uacute ucircumflex udieresis ugrave yacute ydieresis zcaron exclamsmall Hungarumlautsmall dollaroldstyle dollarsuperior ampersandsmall Acutesmall parenleftsuperior parenrightsuperior twodotenleader onedotenleader zerooldstyle oneoldstyle twooldstyle threeoldstyle fouroldstyle fiveoldstyle sixoldstyle sevenoldstyle eightoldstyle nineoldstyle commasuperior threequartersemdash periodsuperior questionsmall asuperior bsuperior centsuperior dsuperior esuperior isuperior lsuperior msuperior nsuperior osuperior rsuperior ssuperior tsuperior ff ffi ffl parenleftinferior parenrightinferior Circumflexsmall hyphensuperior Gravesmall Asmall Bsmall Csmall Dsmall Esmall Fsmall Gsmall Hsmall Ismall Jsmall Ksmall Lsmall Msmall Nsmall Osmall Psmall Qsmall Rsmall Ssmall Tsmall Usmall Vsmall Wsmall Xsmall Ysmall Zsmall colonmonetary onefitted rupiah Tildesmall exclamdownsmall centoldstyle Lslashsmall Scaronsmall Zcaronsmall Dieresissmall Brevesmall Caronsmall Dotaccentsmall Macronsmall figuredash hypheninferior Ogoneksmall Ringsmall Cedillasmall questiondownsmall oneeighth threeeighths fiveeighths seveneighths onethird twothirds zerosuperior foursuperior fivesuperior sixsuperior sevensuperior eightsuperior ninesuperior zeroinferior oneinferior twoinferior threeinferior fourinferior fiveinferior sixinferior seveninferior eightinferior nineinferior centinferior dollarinferior periodinferior commainferior Agravesmall Aacutesmall Acircumflexsmall Atildesmall Adieresissmall Aringsmall AEsmall Ccedillasmall Egravesmall Eacutesmall Ecircumflexsmall Edieresissmall Igravesmall Iacutesmall Icircumflexsmall Idieresissmall Ethsmall Ntildesmall Ogravesmall Oacutesmall Ocircumflexsmall Otildesmall Odieresissmall OEsmall Oslashsmall Ugravesmall Uacutesmall Ucircumflexsmall Udieresissmall Yacutesmall Thornsmall Ydieresissmall 001.000 001.001 001.002 001.003 Black Bold Book Light Medium Regular Roman Semibold";

#[derive(Clone, Debug)]
pub struct Cff {
    pub data: Vec<u8>,
    pub ok: bool,
    pub cid: bool,
    /// the font matrix (0.001 scale unless the font says otherwise)
    pub matrix: Vec<f64>,
    char_strings: Vec<i64>,
    gsubrs: Vec<i64>,
    /// per font dict (one for a font that is not CID-keyed): local subrs offsets, widths
    lsubrs: Vec<Vec<i64>>,
    default_w: Vec<f64>,
    nominal_w: Vec<f64>,
    /// glyph -> font dict
    fd_select: Vec<i64>,
    /// glyph -> SID (or CID for a CID-keyed font)
    pub charset: Vec<i64>,
    strings: Vec<String>,
    /// code -> glyph (the built-in encoding), -1 when unset
    pub encoding: Vec<i64>,
}

/// An INDEX: the offsets of its items, the last being the end; the next
/// position in `pos` is returned as the last element + 1 slot.
fn index(d: &Vec<u8>, pos: usize) -> Vec<i64> {
    let mut out: Vec<i64> = vec![];
    if pos + 2 > d.len() {
        out.push(pos as i64);
        return out;
    }
    let count = be(d, pos, 2) as usize;
    if count == 0 {
        out.push((pos + 2) as i64);
        return out;
    }
    let osz = d[pos + 2] as usize;
    let base = pos + 3 + (count + 1) * osz - 1;
    let mut i: usize = 0;
    while i <= count {
        let o = be(d, pos + 3 + i * osz, osz);
        out.push((base as i64) + o);
        i += 1;
    }
    out
}

fn index_end(ix: &Vec<i64>) -> usize {
    ix[ix.len() - 1] as usize
}

fn index_count(ix: &Vec<i64>) -> usize {
    if ix.len() <= 1 {
        return 0;
    }
    ix.len() - 1
}

/// A DICT: operator -> operands, as parallel lists (operator 12 x is 1200 + x).
struct CffDict {
    ops: Vec<i64>,
    args: Vec<Vec<f64>>,
}

impl CffDict {
    fn get(&self, op: i64) -> Vec<f64> {
        let mut i: usize = 0;
        while i < self.ops.len() {
            if self.ops[i] == op {
                return self.args[i].clone();
            }
            i += 1;
        }
        vec![]
    }

    fn num(&self, op: i64, dflt: f64) -> f64 {
        let a = self.get(op);
        if a.is_empty() {
            return dflt;
        }
        a[a.len() - 1]
    }
}

fn read_dict(d: &Vec<u8>, from: usize, to: usize) -> CffDict {
    let mut r = CffDict { ops: vec![], args: vec![] };
    let mut stack: Vec<f64> = vec![];
    let mut p = from;
    while p < to && p < d.len() {
        let b = d[p] as i64;
        if b <= 21 {
            let mut op = b;
            p += 1;
            if b == 12 && p < d.len() {
                op = 1200 + (d[p] as i64);
                p += 1;
            }
            r.ops.push(op);
            r.args.push(stack.clone());
            stack = vec![];
        } else if b == 28 {
            let v = be(d, p + 1, 2);
            stack.push((if v >= 32768 { v - 65536 } else { v }) as f64);
            p += 3;
        } else if b == 29 {
            let v = be(d, p + 1, 4);
            let f = v as f64;
            stack.push(if f >= 2147483648.0 { f - 4294967296.0 } else { f });
            p += 5;
        } else if b == 30 {
            // a real number in nibbles
            let mut s = String::new();
            p += 1;
            let mut done = false;
            while p < d.len() && !done {
                let byte = d[p] as i64;
                p += 1;
                let mut k = 0;
                while k < 2 {
                    let nib = if k == 0 { byte >> 4 } else { byte & 15 };
                    if nib <= 9 {
                        s.push(crate::pdf_obj::ch((48 + nib) as u32));
                    } else if nib == 10 {
                        s.push('.');
                    } else if nib == 11 {
                        s.push('E');
                    } else if nib == 12 {
                        s.push('E');
                        s.push('-');
                    } else if nib == 14 {
                        s.push('-');
                    } else if nib == 15 {
                        done = true;
                        break;
                    }
                    k += 1;
                }
            }
            stack.push(parse_real(&s));
        } else if b >= 32 && b <= 246 {
            stack.push((b - 139) as f64);
            p += 1;
        } else if b >= 247 && b <= 250 {
            let w = if p + 1 < d.len() { d[p + 1] as i64 } else { 0 };
            stack.push(((b - 247) * 256 + w + 108) as f64);
            p += 2;
        } else if b >= 251 && b <= 254 {
            let w = if p + 1 < d.len() { d[p + 1] as i64 } else { 0 };
            stack.push((0 - (b - 251) * 256 - w - 108) as f64);
            p += 2;
        } else {
            p += 1;
        }
    }
    r
}

fn parse_real(s: &str) -> f64 {
    let mut mant: f64 = 0.0;
    let mut scale: f64 = 1.0;
    let mut neg = false;
    let mut dot = false;
    let mut exp: i64 = 0;
    let mut eneg = false;
    let mut in_exp = false;
    for c in s.chars() {
        if c == '-' {
            if in_exp {
                eneg = true;
            } else {
                neg = true;
            }
        } else if c == '.' {
            dot = true;
        } else if c == 'E' {
            in_exp = true;
        } else {
            let v = ((c as u32) - 48) as i64;
            if in_exp {
                exp = exp * 10 + v;
            } else if dot {
                scale = scale / 10.0;
                mant = mant + (v as f64) * scale;
            } else {
                mant = mant * 10.0 + (v as f64);
            }
        }
    }
    let mut e = 0;
    while e < exp {
        if eneg {
            mant = mant / 10.0;
        } else {
            mant = mant * 10.0;
        }
        e += 1;
    }
    if neg {
        return 0.0 - mant;
    }
    mant
}

fn bias(n: usize) -> i64 {
    if n < 1240 {
        return 107;
    }
    if n < 33900 {
        return 1131;
    }
    32768
}

impl Cff {
    pub fn parse(data: Vec<u8>) -> Cff {
        let mut c = Cff {
            data: data,
            ok: false,
            cid: false,
            matrix: vec![0.001, 0.0, 0.0, 0.001, 0.0, 0.0],
            char_strings: vec![],
            gsubrs: vec![],
            lsubrs: vec![],
            default_w: vec![],
            nominal_w: vec![],
            fd_select: vec![],
            charset: vec![],
            strings: vec![],
            encoding: vec![],
        };
        let d = c.data.clone();
        if d.len() < 4 || d[0] != 1 {
            return c;
        }
        let hdr = d[2] as usize;
        let names = index(&d, hdr);
        let tops = index(&d, index_end(&names));
        let strs = index(&d, index_end(&tops));
        let gs = index(&d, index_end(&strs));
        if index_count(&tops) < 1 {
            return c;
        }
        let mut i: usize = 0;
        while i < index_count(&strs) {
            let mut s = String::new();
            let mut k = strs[i] as usize;
            while k < (strs[i + 1] as usize) && k < d.len() {
                s.push(crate::pdf_obj::ch(d[k] as u32));
                k += 1;
            }
            c.strings.push(s);
            i += 1;
        }
        c.gsubrs = gs;
        let top = read_dict(&d, tops[0] as usize, tops[1] as usize);
        let fm = top.get(1207);
        if fm.len() == 6 {
            c.matrix = fm;
        }
        let cs_off = top.num(17, -1.0);
        if cs_off < 0.0 {
            return c;
        }
        c.char_strings = index(&d, cs_off as usize);
        let nglyphs = index_count(&c.char_strings);
        c.cid = !top.get(1230).is_empty();
        if c.cid {
            let fda = top.num(1236, -1.0);
            if fda >= 0.0 {
                let fds = index(&d, fda as usize);
                let mut j: usize = 0;
                while j < index_count(&fds) {
                    let fd = read_dict(&d, fds[j] as usize, fds[j + 1] as usize);
                    c.add_private(&fd);
                    j += 1;
                }
            }
            let fs = top.num(1237, -1.0);
            if fs >= 0.0 {
                c.read_fd_select(fs as usize, nglyphs);
            }
        } else {
            c.add_private(&top);
        }
        if c.lsubrs.is_empty() {
            c.lsubrs.push(vec![]);
            c.default_w.push(0.0);
            c.nominal_w.push(0.0);
        }
        c.read_charset(top.num(15, 0.0) as usize, nglyphs);
        if !c.cid {
            c.read_encoding(top.num(16, 0.0) as usize);
        }
        c.ok = nglyphs > 0;
        c
    }

    fn add_private(&mut self, fd: &CffDict) {
        let p = fd.get(18);
        let mut subrs: Vec<i64> = vec![];
        let mut dw = 0.0;
        let mut nw = 0.0;
        if p.len() == 2 {
            let size = p[0] as usize;
            let off = p[1] as usize;
            let pd = read_dict(&self.data, off, off + size);
            dw = pd.num(20, 0.0);
            nw = pd.num(21, 0.0);
            let so = pd.num(19, -1.0);
            if so >= 0.0 {
                subrs = index(&self.data, off + (so as usize));
            }
        }
        self.lsubrs.push(subrs);
        self.default_w.push(dw);
        self.nominal_w.push(nw);
    }

    fn read_fd_select(&mut self, off: usize, n: usize) {
        let d = &self.data;
        if off >= d.len() {
            return;
        }
        let fmt = d[off];
        if fmt == 0 {
            let mut i: usize = 0;
            while i < n {
                let v = if off + 1 + i < d.len() { d[off + 1 + i] as i64 } else { 0 };
                self.fd_select.push(v);
                i += 1;
            }
        } else if fmt == 3 {
            let nr = be(d, off + 1, 2) as usize;
            let mut sel: Vec<i64> = vec![0; n];
            let mut r: usize = 0;
            while r < nr {
                let first = be(d, off + 3 + r * 3, 2) as usize;
                let fd = if off + 5 + r * 3 < d.len() { d[off + 5 + r * 3] as i64 } else { 0 };
                let next = be(d, off + 6 + r * 3, 2) as usize;
                let mut g = first;
                while g < next && g < n {
                    sel[g] = fd;
                    g += 1;
                }
                r += 1;
            }
            self.fd_select = sel;
        }
    }

    fn read_charset(&mut self, off: usize, n: usize) {
        self.charset = vec![0];
        let d = self.data.clone();
        if off <= 2 {
            // ISOAdobe (and, approximately, the expert sets)
            let mut i: usize = 1;
            while i < n {
                self.charset.push(i as i64);
                i += 1;
            }
            return;
        }
        if off >= d.len() {
            return;
        }
        let fmt = d[off];
        let mut p = off + 1;
        if fmt == 0 {
            while self.charset.len() < n && p + 1 < d.len() {
                self.charset.push(be(&d, p, 2));
                p += 2;
            }
        } else {
            let wide = fmt == 2;
            while self.charset.len() < n && p + 2 < d.len() {
                let first = be(&d, p, 2);
                let left = if wide { be(&d, p + 2, 2) } else { d[p + 2] as i64 };
                p += if wide { 4 } else { 3 };
                let mut k: i64 = 0;
                while k <= left && self.charset.len() < n {
                    self.charset.push(first + k);
                    k += 1;
                }
            }
        }
    }

    fn read_encoding(&mut self, off: usize) {
        self.encoding = vec![-1; 256];
        let d = self.data.clone();
        if off <= 1 {
            // the standard encoding: through glyph names
            let se = crate::pdf_fontdata::standard_encoding();
            for item in se.split(';') {
                let parts: Vec<&str> = item.split(' ').collect();
                if parts.len() == 2 {
                    let code = parse_int(parts[0]);
                    let g = self.glyph_by_name(parts[1]);
                    if code >= 0 && code < 256 && g > 0 {
                        self.encoding[code as usize] = g;
                    }
                }
            }
            return;
        }
        if off >= d.len() {
            return;
        }
        let fmt = d[off] & 127;
        if fmt == 0 {
            let n = d[off + 1] as usize;
            let mut i: usize = 0;
            while i < n && off + 2 + i < d.len() {
                self.encoding[d[off + 2 + i] as usize] = (i + 1) as i64;
                i += 1;
            }
        } else if fmt == 1 {
            let nr = d[off + 1] as usize;
            let mut g: i64 = 1;
            let mut r: usize = 0;
            while r < nr && off + 3 + r * 2 < d.len() {
                let first = d[off + 2 + r * 2] as i64;
                let left = d[off + 3 + r * 2] as i64;
                let mut k: i64 = 0;
                while k <= left {
                    if first + k < 256 {
                        self.encoding[(first + k) as usize] = g;
                    }
                    g += 1;
                    k += 1;
                }
                r += 1;
            }
        }
    }

    pub fn glyph_count(&self) -> i64 {
        index_count(&self.char_strings) as i64
    }

    pub fn sid_name(&self, sid: i64) -> String {
        if sid < 391 {
            let names: Vec<&str> = STD_STRINGS.split(' ').collect();
            if sid >= 0 && (sid as usize) < names.len() {
                return names[sid as usize].to_string();
            }
            return String::new();
        }
        let i = (sid - 391) as usize;
        if i < self.strings.len() {
            return self.strings[i].clone();
        }
        String::new()
    }

    pub fn glyph_name(&self, gid: i64) -> String {
        if gid < 0 || (gid as usize) >= self.charset.len() || self.cid {
            return String::new();
        }
        self.sid_name(self.charset[gid as usize])
    }

    pub fn glyph_by_name(&self, name: &str) -> i64 {
        let mut i: usize = 0;
        while i < self.charset.len() {
            if self.sid_name(self.charset[i]) == name {
                return i as i64;
            }
            i += 1;
        }
        -1
    }

    /// The glyph of CID `cid` in a CID-keyed font (through the charset).
    pub fn glyph_by_cid(&self, cid: i64) -> i64 {
        if !self.cid {
            return cid;
        }
        let mut i: usize = 0;
        while i < self.charset.len() {
            if self.charset[i] == cid {
                return i as i64;
            }
            i += 1;
        }
        -1
    }

    /// Glyph `gid`'s outline in charstring units (scale with `matrix`).
    pub fn glyph_path(&self, gid: i64) -> Path {
        let mut st = Cs2Run { p: Path::new(), stack: vec![], x: 0.0, y: 0.0, nhints: 0, open: false, width: 0.0, have_width: false, done: false, fd: 0, depth: 0, trans: vec![0.0; 32] };
        if gid < 0 || (gid as usize) >= index_count(&self.char_strings) {
            return st.p;
        }
        if (gid as usize) < self.fd_select.len() {
            st.fd = self.fd_select[gid as usize] as usize;
        }
        if st.fd >= self.lsubrs.len() {
            st.fd = 0;
        }
        let from = self.char_strings[gid as usize] as usize;
        let to = self.char_strings[(gid + 1) as usize] as usize;
        self.run(&mut st, from, to);
        if st.open {
            st.p.close();
        }
        st.p
    }

    /// The glyph's advance in charstring units.
    pub fn glyph_width(&self, gid: i64) -> f64 {
        let mut st = Cs2Run { p: Path::new(), stack: vec![], x: 0.0, y: 0.0, nhints: 0, open: false, width: 0.0, have_width: false, done: false, fd: 0, depth: 0, trans: vec![0.0; 32] };
        if gid < 0 || (gid as usize) >= index_count(&self.char_strings) {
            return 0.0;
        }
        if (gid as usize) < self.fd_select.len() {
            st.fd = self.fd_select[gid as usize] as usize;
        }
        if st.fd >= self.lsubrs.len() {
            st.fd = 0;
        }
        let from = self.char_strings[gid as usize] as usize;
        let to = self.char_strings[(gid + 1) as usize] as usize;
        self.run(&mut st, from, to);
        if st.have_width {
            return st.width;
        }
        self.default_w[st.fd]
    }

    fn take_width(&self, st: &mut Cs2Run, expected_even: bool, nargs: usize) {
        // the first stack-clearing operator may carry the width first
        if st.have_width {
            return;
        }
        st.have_width = true;
        let n = st.stack.len();
        let odd = if expected_even { n % 2 == 1 } else { n > nargs };
        if odd && n > 0 {
            st.width = self.nominal_w[st.fd] + st.stack[0];
            let mut rest: Vec<f64> = vec![];
            let mut i: usize = 1;
            while i < n {
                rest.push(st.stack[i]);
                i += 1;
            }
            st.stack = rest;
        } else {
            st.width = self.default_w[st.fd];
        }
    }

    fn line(&self, st: &mut Cs2Run, dx: f64, dy: f64) {
        st.x += dx;
        st.y += dy;
        st.p.line_to(st.x, st.y);
    }

    fn curve(&self, st: &mut Cs2Run, a: f64, b: f64, c: f64, d: f64, e: f64, f: f64) {
        let x1 = st.x + a;
        let y1 = st.y + b;
        let x2 = x1 + c;
        let y2 = y1 + d;
        st.x = x2 + e;
        st.y = y2 + f;
        st.p.curve_to(x1, y1, x2, y2, st.x, st.y);
    }

    fn move_to(&self, st: &mut Cs2Run, dx: f64, dy: f64) {
        if st.open {
            st.p.close();
        }
        st.x += dx;
        st.y += dy;
        st.p.move_to(st.x, st.y);
        st.open = true;
    }

    fn run(&self, st: &mut Cs2Run, from: usize, to: usize) {
        let d = &self.data;
        let mut p = from;
        st.depth += 1;
        if st.depth > 10 {
            st.depth -= 1;
            return;
        }
        while p < to && p < d.len() && !st.done {
            let b = d[p] as i64;
            p += 1;
            if b >= 32 || b == 28 {
                if b == 28 {
                    let v = be(d, p, 2);
                    st.stack.push((if v >= 32768 { v - 65536 } else { v }) as f64);
                    p += 2;
                } else if b <= 246 {
                    st.stack.push((b - 139) as f64);
                } else if b <= 250 {
                    let w = if p < d.len() { d[p] as i64 } else { 0 };
                    st.stack.push(((b - 247) * 256 + w + 108) as f64);
                    p += 1;
                } else if b <= 254 {
                    let w = if p < d.len() { d[p] as i64 } else { 0 };
                    st.stack.push((0 - (b - 251) * 256 - w - 108) as f64);
                    p += 1;
                } else {
                    // 16.16 fixed
                    let v = be(d, p, 4) as f64;
                    let s = if v >= 2147483648.0 { v - 4294967296.0 } else { v };
                    st.stack.push(s / 65536.0);
                    p += 4;
                }
                continue;
            }
            let s = st.stack.clone();
            let n = s.len();
            if b == 1 || b == 3 || b == 18 || b == 23 {
                // hstem vstem hstemhm vstemhm
                self.take_width(st, true, 0);
                st.nhints += (st.stack.len() / 2) as i64;
                st.stack = vec![];
            } else if b == 19 || b == 20 {
                // hintmask cntrmask: a vstem may be implied
                self.take_width(st, true, 0);
                st.nhints += (st.stack.len() / 2) as i64;
                st.stack = vec![];
                p += ((st.nhints + 7) / 8) as usize;
            } else if b == 21 {
                self.take_width(st, false, 2);
                let s2 = st.stack.clone();
                if s2.len() >= 2 {
                    self.move_to(st, s2[s2.len() - 2], s2[s2.len() - 1]);
                }
                st.stack = vec![];
            } else if b == 22 {
                self.take_width(st, false, 1);
                let s2 = st.stack.clone();
                if !s2.is_empty() {
                    self.move_to(st, s2[s2.len() - 1], 0.0);
                }
                st.stack = vec![];
            } else if b == 4 {
                self.take_width(st, false, 1);
                let s2 = st.stack.clone();
                if !s2.is_empty() {
                    self.move_to(st, 0.0, s2[s2.len() - 1]);
                }
                st.stack = vec![];
            } else if b == 5 {
                let mut i: usize = 0;
                while i + 1 < n {
                    self.line(st, s[i], s[i + 1]);
                    i += 2;
                }
                st.stack = vec![];
            } else if b == 6 || b == 7 {
                let mut horiz = b == 6;
                for v in s.iter() {
                    if horiz {
                        self.line(st, *v, 0.0);
                    } else {
                        self.line(st, 0.0, *v);
                    }
                    horiz = !horiz;
                }
                st.stack = vec![];
            } else if b == 8 {
                let mut i: usize = 0;
                while i + 5 < n {
                    self.curve(st, s[i], s[i + 1], s[i + 2], s[i + 3], s[i + 4], s[i + 5]);
                    i += 6;
                }
                st.stack = vec![];
            } else if b == 24 {
                // rcurveline
                let mut i: usize = 0;
                while i + 8 <= n {
                    self.curve(st, s[i], s[i + 1], s[i + 2], s[i + 3], s[i + 4], s[i + 5]);
                    i += 6;
                }
                if i + 1 < n {
                    self.line(st, s[i], s[i + 1]);
                }
                st.stack = vec![];
            } else if b == 25 {
                // rlinecurve
                let mut i: usize = 0;
                while i + 1 < n && n - i > 6 {
                    self.line(st, s[i], s[i + 1]);
                    i += 2;
                }
                if i + 5 < n {
                    self.curve(st, s[i], s[i + 1], s[i + 2], s[i + 3], s[i + 4], s[i + 5]);
                }
                st.stack = vec![];
            } else if b == 26 {
                // vvcurveto
                let mut i: usize = 0;
                let mut dx1 = 0.0;
                if n % 2 == 1 {
                    dx1 = s[0];
                    i = 1;
                }
                while i + 3 < n {
                    self.curve(st, dx1, s[i], s[i + 1], s[i + 2], 0.0, s[i + 3]);
                    dx1 = 0.0;
                    i += 4;
                }
                st.stack = vec![];
            } else if b == 27 {
                // hhcurveto
                let mut i: usize = 0;
                let mut dy1 = 0.0;
                if n % 2 == 1 {
                    dy1 = s[0];
                    i = 1;
                }
                while i + 3 < n {
                    self.curve(st, s[i], dy1, s[i + 1], s[i + 2], s[i + 3], 0.0);
                    dy1 = 0.0;
                    i += 4;
                }
                st.stack = vec![];
            } else if b == 30 || b == 31 {
                // vhcurveto hvcurveto
                let mut horiz = b == 31;
                let mut i: usize = 0;
                while i + 3 < n {
                    let last = i + 4 == n - 1;
                    let extra = if last { s[n - 1] } else { 0.0 };
                    if horiz {
                        self.curve(st, s[i], 0.0, s[i + 1], s[i + 2], extra, s[i + 3]);
                    } else {
                        self.curve(st, 0.0, s[i], s[i + 1], s[i + 2], s[i + 3], extra);
                    }
                    horiz = !horiz;
                    i += 4;
                }
                st.stack = vec![];
            } else if b == 10 || b == 29 {
                // callsubr callgsubr
                if n == 0 {
                    continue;
                }
                let mut rest = s.clone();
                let idx = rest[n - 1] as i64;
                rest.truncate(n - 1);
                st.stack = rest;
                let subrs = if b == 10 { self.lsubrs[st.fd].clone() } else { self.gsubrs.clone() };
                let cnt = index_count(&subrs);
                let k = idx + bias(cnt);
                if k >= 0 && (k as usize) < cnt {
                    self.run(st, subrs[k as usize] as usize, subrs[(k + 1) as usize] as usize);
                }
            } else if b == 11 {
                // return
                st.depth -= 1;
                return;
            } else if b == 14 {
                // endchar, perhaps with an accented character (seac)
                self.take_width(st, false, 0);
                let s2 = st.stack.clone();
                if s2.len() >= 4 {
                    let n2 = s2.len();
                    self.seac(st, s2[n2 - 4], s2[n2 - 3], s2[n2 - 2] as i64, s2[n2 - 1] as i64);
                }
                if st.open {
                    st.p.close();
                    st.open = false;
                }
                st.done = true;
            } else if b == 12 {
                let e = if p < d.len() { d[p] as i64 } else { 0 };
                p += 1;
                self.escape(st, e, &s);
            } else {
                st.stack = vec![];
            }
        }
        st.depth -= 1;
    }

    fn escape(&self, st: &mut Cs2Run, e: i64, s: &Vec<f64>) {
        let n = s.len();
        if e == 35 && n >= 13 {
            // flex
            self.curve(st, s[0], s[1], s[2], s[3], s[4], s[5]);
            self.curve(st, s[6], s[7], s[8], s[9], s[10], s[11]);
        } else if e == 34 && n >= 7 {
            // hflex
            let y0 = st.y;
            self.curve(st, s[0], 0.0, s[1], s[2], s[3], 0.0);
            let dy = y0 - st.y;
            self.curve(st, s[4], 0.0, s[5], dy, s[6], 0.0);
        } else if e == 36 && n >= 9 {
            // hflex1
            let y0 = st.y;
            self.curve(st, s[0], s[1], s[2], s[3], s[4], 0.0);
            let dy = y0 - st.y - s[7];
            self.curve(st, s[5], 0.0, s[6], s[7], s[8], dy);
        } else if e == 37 && n >= 11 {
            // flex1
            let x0 = st.x;
            let y0 = st.y;
            let dx = s[0] + s[2] + s[4] + s[6] + s[8];
            let dy = s[1] + s[3] + s[5] + s[7] + s[9];
            self.curve(st, s[0], s[1], s[2], s[3], s[4], s[5]);
            let ax = if dx < 0.0 { 0.0 - dx } else { dx };
            let ay = if dy < 0.0 { 0.0 - dy } else { dy };
            if ax > ay {
                let fy = y0 - (st.y + s[7] + s[9]);
                self.curve(st, s[6], s[7], s[8], s[9], s[10], fy);
            } else {
                let fx = x0 - (st.x + s[6] + s[8]);
                self.curve(st, s[6], s[7], s[8], s[9], fx, s[10]);
            }
        } else if e == 0 {
            // dotsection (Type 1 leftover)
        } else {
            // arithmetic operators: a simple subset
            let mut stack = s.clone();
            if e == 9 && n >= 1 {
                let v = stack[n - 1];
                stack[n - 1] = if v < 0.0 { 0.0 - v } else { v };
            } else if e == 10 && n >= 2 {
                let v = stack[n - 2] + stack[n - 1];
                stack.truncate(n - 2);
                stack.push(v);
            } else if e == 11 && n >= 2 {
                let v = stack[n - 2] - stack[n - 1];
                stack.truncate(n - 2);
                stack.push(v);
            } else if e == 12 && n >= 2 {
                let v = if stack[n - 1] != 0.0 { stack[n - 2] / stack[n - 1] } else { 0.0 };
                stack.truncate(n - 2);
                stack.push(v);
            } else if e == 14 && n >= 1 {
                stack[n - 1] = 0.0 - stack[n - 1];
            } else if e == 18 && n >= 1 {
                stack.truncate(n - 1);
            } else if e == 24 && n >= 2 {
                let v = stack[n - 2] * stack[n - 1];
                stack.truncate(n - 2);
                stack.push(v);
            } else if e == 26 && n >= 1 {
                stack[n - 1] = stack[n - 1].sqrt();
            } else if e == 27 && n >= 1 {
                let v = stack[n - 1];
                stack.push(v);
            } else if e == 28 && n >= 2 {
                let a = stack[n - 2];
                stack[n - 2] = stack[n - 1];
                stack[n - 1] = a;
            } else if e == 20 && n >= 2 {
                // put
                let i = stack[n - 1] as usize;
                if i < 32 {
                    st.trans[i] = stack[n - 2];
                }
                stack.truncate(n - 2);
            } else if e == 21 && n >= 1 {
                // get
                let i = stack[n - 1] as usize;
                stack[n - 1] = if i < 32 { st.trans[i] } else { 0.0 };
            } else {
                stack = vec![];
            }
            st.stack = stack;
            return;
        }
        st.stack = vec![];
    }

    fn seac(&self, st: &mut Cs2Run, adx: f64, ady: f64, bchar: i64, achar: i64) {
        let se = crate::pdf_fontdata::standard_encoding();
        let mut bname = String::new();
        let mut aname = String::new();
        for item in se.split(';') {
            let parts: Vec<&str> = item.split(' ').collect();
            if parts.len() == 2 {
                let c = parse_int(parts[0]);
                if c == bchar {
                    bname = parts[1].to_string();
                }
                if c == achar {
                    aname = parts[1].to_string();
                }
            }
        }
        let bg = self.glyph_by_name(&bname);
        let ag = self.glyph_by_name(&aname);
        if st.open {
            st.p.close();
            st.open = false;
        }
        if bg > 0 {
            let bp = self.glyph_path(bg);
            let m: Vec<f64> = vec![1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
            st.p.append(&bp, &m);
        }
        if ag > 0 {
            let ap = self.glyph_path(ag);
            let m: Vec<f64> = vec![1.0, 0.0, 0.0, 1.0, adx, ady];
            st.p.append(&ap, &m);
        }
    }
}

struct Cs2Run {
    p: Path,
    stack: Vec<f64>,
    x: f64,
    y: f64,
    nhints: i64,
    open: bool,
    width: f64,
    have_width: bool,
    done: bool,
    fd: usize,
    depth: i64,
    trans: Vec<f64>,
}

pub fn parse_int(s: &str) -> i64 {
    let mut v: i64 = 0;
    let mut neg = false;
    for c in s.chars() {
        if c == '-' {
            neg = true;
        } else if c >= '0' && c <= '9' {
            v = v * 10 + (((c as u32) - 48) as i64);
        }
    }
    if neg {
        return 0 - v;
    }
    v
}
