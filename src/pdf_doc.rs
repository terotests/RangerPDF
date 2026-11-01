//! A PDF document (ISO 32000-1 §7.5, §7.7): the cross-reference table or
//! streams with their /Prev chain, object streams, the trailer, and the page
//! tree with inherited attributes. A file whose cross-reference is missing
//! or wrong is read again by scanning for its objects, as readers do.
//!
//! Sans-IO: the document is the bytes it was given. A document can also be
//! opened from its size alone (Doc::sparse): the bytes come in chunks the
//! host fetches (a vault download read at an offset, an HTTP Range). Every
//! read checks that its bytes are there; when they are not, the chunks are
//! listed in `need`, `short` is set and nothing read from the hole is kept.
//! The host feeds the chunks and does the same step again (open, a page).

use std::collections::HashMap;

use crate::pdf_crypt::Crypt;
use crate::pdf_filters;
use crate::pdf_lexer::PdfLexer;
use crate::pdf_obj;
use crate::pdf_obj::Obj;
use crate::pdf_obj::K;

#[derive(Clone, Debug)]
pub struct Page {
    pub dict: Obj,
    /// [x0 y0 x1 y1], normalized so that x0 < x1 and y0 < y1
    pub media: Vec<f64>,
    pub crop: Vec<f64>,
    /// 0, 90, 180 or 270
    pub rotate: i64,
    pub resources: Obj,
    /// the page object's number, -1 when it is not an indirect object
    pub num: i64,
    /// a sparse document's page whose dictionary is not read yet: its size
    /// is the inherited one until realize() reads it
    pub lazy: bool,
    /// the attributes it inherits (Resources, MediaBox, CropBox, Rotate)
    pub inh: Obj,
}

impl Page {
    /// The page's size in points as shown (after /Rotate).
    pub fn width(&self) -> f64 {
        if self.rotate == 90 || self.rotate == 270 {
            return self.crop[3] - self.crop[1];
        }
        self.crop[2] - self.crop[0]
    }

    pub fn height(&self) -> f64 {
        if self.rotate == 90 || self.rotate == 270 {
            return self.crop[2] - self.crop[0];
        }
        self.crop[3] - self.crop[1]
    }
}

pub struct Doc {
    pub lex: PdfLexer,
    /// per object number: 0 none/free, 1 at an offset, 2 in an object stream
    x_type: Vec<i64>,
    /// the offset, or the object stream's number
    x_a: Vec<i64>,
    /// the generation, or the index in the object stream
    x_b: Vec<i64>,
    pub trailer: Obj,
    cache: HashMap<i64, Obj>,
    busy: Vec<i64>,
    pub pages: Vec<Page>,
    pub version: String,
    /// what was wrong with the file and has been worked around
    pub warnings: Vec<String>,
    pub repaired: bool,
    pub crypt: Crypt,
    /// set when the document cannot be read at all (bad password, no pages)
    pub error: String,
    /// the bytes come in chunks (Doc::sparse); `have` per chunk
    pub sparse: bool,
    pub chunk: usize,
    have: Vec<bool>,
    /// chunks a read wanted and did not have, in the order asked
    pub need: Vec<i64>,
    /// a read since begin() wanted bytes that were not there
    pub short: bool,
    /// reads that came short, so that an object read from a hole is not kept
    misses: i64,
    /// read every page's dictionary when the page tree is read (a tree that
    /// did not keep the shape lazy pages expect)
    no_lazy: bool,
}

fn norm_box(b: &Obj) -> Vec<f64> {
    let v = b.nums();
    if v.len() < 4 {
        return vec![0.0, 0.0, 612.0, 792.0];
    }
    let mut x0 = v[0];
    let mut y0 = v[1];
    let mut x1 = v[2];
    let mut y1 = v[3];
    if x0 > x1 {
        let t = x0;
        x0 = x1;
        x1 = t;
    }
    if y0 > y1 {
        let t = y0;
        y0 = y1;
        y1 = t;
    }
    vec![x0, y0, x1, y1]
}

fn starts_with_at(buf: &Vec<u8>, pos: usize, word: &str) -> bool {
    let mut i = pos;
    for c in word.chars() {
        if i >= buf.len() || (buf[i] as u32) != (c as u32) {
            return false;
        }
        i += 1;
    }
    true
}

fn is_digit(c: u8) -> bool {
    c >= 48 && c <= 57
}

impl Doc {
    pub fn open(data: Vec<u8>, password: &str) -> Doc {
        let mut d = Doc::blank(data);
        d.start(password);
        d
    }

    /// A document of `size` bytes none of which are here yet; the host
    /// feeds chunks of `chunk` bytes and calls start() until `short` is off.
    pub fn sparse(size: usize, chunk: usize, password: &str) -> Doc {
        // one buffer of the file's size (pushes copy it in Go)
        let buf: Vec<u8> = vec![0; size];
        let mut d = Doc::blank(buf);
        d.sparse = true;
        d.chunk = if chunk < 1024 { 1024 } else { chunk };
        let n = (size + d.chunk - 1) / d.chunk;
        let mut h: Vec<bool> = vec![];
        let mut j: usize = 0;
        while j < n {
            h.push(false);
            j += 1;
        }
        d.have = h;
        d.start(password);
        d
    }

    fn blank(data: Vec<u8>) -> Doc {
        Doc {
            lex: PdfLexer::new(data),
            x_type: vec![],
            x_a: vec![],
            x_b: vec![],
            trailer: pdf_obj::dict(),
            cache: HashMap::new(),
            busy: vec![],
            pages: vec![],
            version: String::new(),
            warnings: vec![],
            repaired: false,
            crypt: Crypt::none(),
            error: String::new(),
            sparse: false,
            chunk: 65536,
            have: vec![],
            need: vec![],
            short: false,
            misses: 0,
            no_lazy: false,
        }
    }

    /// Reads the document's structure (cross-reference, trailer, pages)
    /// from the bytes it has; on a sparse document again after a short one.
    pub fn start(&mut self, password: &str) {
        self.x_type = vec![];
        self.x_a = vec![];
        self.x_b = vec![];
        self.trailer = pdf_obj::dict();
        self.cache = HashMap::new();
        self.busy = vec![];
        self.pages = vec![];
        self.warnings = vec![];
        self.repaired = false;
        self.crypt = Crypt::none();
        self.error = String::new();
        self.no_lazy = false;
        self.begin();
        self.read_header();
        let ok = self.read_xref();
        if self.short {
            return;
        }
        if !ok {
            self.repair();
        }
        if !self.short && !self.trailer.at("Root").is_dict() && self.trailer.at("Root").k != K::Ref {
            self.repair();
        }
        if self.short {
            return;
        }
        self.setup_crypt(password);
        if !self.error.is_empty() || self.short {
            return;
        }
        self.read_pages();
        if self.short {
            self.pages = vec![];
            return;
        }
        if self.pages.is_empty() && !self.repaired {
            self.repair();
            self.cache = HashMap::new();
            self.read_pages();
        }
        if self.pages.is_empty() && !self.short {
            self.pages_by_scan();
        }
        if self.short {
            self.pages = vec![];
            return;
        }
        if self.pages.is_empty() {
            self.error = "no pages".to_string();
        }
    }

    /// A new step: forget what the last one needed.
    pub fn begin(&mut self) {
        self.need = vec![];
        self.short = false;
    }

    /// Every chunk is here (always so for a document opened from its bytes).
    pub fn complete(&self) -> bool {
        if !self.sparse {
            return true;
        }
        for h in self.have.iter() {
            if !*h {
                return false;
            }
        }
        true
    }

    /// Bytes from offset `at`: the chunks they fill completely (or that end
    /// at the end of the file) count as here.
    pub fn feed(&mut self, at: usize, bytes: &Vec<u8>) {
        let n = self.lex.buf.len();
        let mut k: usize = 0;
        while k < bytes.len() && at + k < n {
            self.lex.buf[at + k] = bytes[k];
            k += 1;
        }
        if !self.sparse {
            return;
        }
        let end = at + k;
        let mut c = (at + self.chunk - 1) / self.chunk;
        while c < self.have.len() {
            let from = c * self.chunk;
            let mut to = from + self.chunk;
            if to > n {
                to = n;
            }
            if from < at || to > end {
                break;
            }
            self.have[c] = true;
            c += 1;
        }
    }

    fn want(&mut self, c: usize) {
        let ci = c as i64;
        if !self.need.contains(&ci) {
            self.need.push(ci);
        }
        self.short = true;
        self.misses += 1;
    }

    /// Whether bytes [from, to) are here; the missing chunks go to `need`
    /// (all of them, or only the first when `all` is false: a read that ran
    /// on through a hole does not know how far it really needs).
    fn here(&mut self, from: usize, to: usize, all: bool) -> bool {
        if !self.sparse {
            return true;
        }
        let n = self.lex.buf.len();
        let mut t = to;
        if t > n {
            t = n;
        }
        if from >= t {
            return true;
        }
        let mut ok = true;
        let mut c = from / self.chunk;
        let last = (t - 1) / self.chunk;
        while c <= last && c < self.have.len() {
            if !self.have[c] {
                self.want(c);
                ok = false;
                if !all {
                    return false;
                }
            }
            c += 1;
        }
        ok
    }

    fn warn(&mut self, s: &str) {
        if self.warnings.len() < 50 {
            self.warnings.push(s.to_string());
        }
    }

    fn read_header(&mut self) {
        let n = self.lex.buf.len();
        if !self.here(0, 1032, true) {
            return;
        }
        let mut i: usize = 0;
        while i + 8 < n && i < 1024 {
            if starts_with_at(&self.lex.buf, i, "%PDF-") {
                let mut v = String::new();
                let mut k = i + 5;
                while k < n && k < i + 9 && (is_digit(self.lex.buf[k]) || self.lex.buf[k] == 46) {
                    v.push(pdf_obj::ch(self.lex.buf[k] as u32));
                    k += 1;
                }
                self.version = v;
                return;
            }
            i += 1;
        }
        self.warn("no %PDF header");
    }

    fn set_entry(&mut self, num: i64, t: i64, a: i64, b: i64, force: bool) {
        if num < 0 || num > 8000000 {
            return;
        }
        let u = num as usize;
        while self.x_type.len() <= u {
            self.x_type.push(-1);
            self.x_a.push(0);
            self.x_b.push(0);
        }
        // the newest section comes first: an entry already set stays
        if self.x_type[u] >= 0 && !force {
            return;
        }
        self.x_type[u] = t;
        self.x_a[u] = a;
        self.x_b[u] = b;
    }

    fn find_startxref(&self) -> i64 {
        let buf = &self.lex.buf;
        let n = buf.len();
        let mut i: i64 = (n as i64) - 9;
        let stop: i64 = if (n as i64) > 4096 { (n as i64) - 4096 } else { 0 };
        while i >= stop {
            if starts_with_at(buf, i as usize, "startxref") {
                let mut k = (i as usize) + 9;
                while k < n && !is_digit(buf[k]) {
                    k += 1;
                }
                let mut v: i64 = 0;
                let mut any = false;
                while k < n && is_digit(buf[k]) {
                    v = v * 10 + ((buf[k] - 48) as i64);
                    any = true;
                    k += 1;
                }
                if any {
                    return v;
                }
                return -1;
            }
            i -= 1;
        }
        -1
    }

    fn merge_trailer(&mut self, t: &Obj) {
        let mut i: usize = 0;
        while i < t.keys.len() {
            let k = t.keys[i].clone();
            if !self.trailer.has(&k) {
                self.trailer.set(&k, t.items[i].clone());
            }
            i += 1;
        }
    }

    /// The cross-reference sections from startxref along /Prev; false when
    /// they could not be read.
    fn read_xref(&mut self) -> bool {
        let n = self.lex.buf.len();
        let tail = if n > 4096 { n - 4096 } else { 0 };
        if !self.here(tail, n, true) {
            return false;
        }
        let start = self.find_startxref();
        if start < 0 {
            self.warn("no startxref");
            return false;
        }
        let mut off = start;
        let mut seen: Vec<i64> = vec![];
        let mut sections = 0;
        while off >= 0 {
            if seen.contains(&off) || sections > 100 {
                break;
            }
            seen.push(off);
            sections += 1;
            let t = self.read_section(off);
            if self.short {
                return false;
            }
            if !t.is_dict() {
                if sections == 1 {
                    self.warn("bad startxref offset");
                    return false;
                }
                self.warn("bad /Prev offset");
                break;
            }
            // a hybrid file's /XRefStm comes before its /Prev
            if t.at("XRefStm").is_num() {
                let xs = t.at("XRefStm").i();
                if !seen.contains(&xs) {
                    seen.push(xs);
                    self.read_section(xs);
                }
            }
            self.merge_trailer(&t);
            if t.at("Prev").is_num() {
                off = t.at("Prev").i();
            } else {
                off = -1;
            }
        }
        if !self.trailer.has("Root") {
            return false;
        }
        true
    }

    /// One section at `off`: a table with its trailer, or a stream. The
    /// trailer (or the stream's dictionary) comes back, or Null.
    fn read_section(&mut self, off: i64) -> Obj {
        if off < 0 || (off as usize) >= self.lex.buf.len() {
            return pdf_obj::null();
        }
        if !self.here(off as usize, (off as usize) + 1, true) {
            return pdf_obj::null();
        }
        self.lex.pos = off as usize;
        let t = self.lex.token();
        if t.is_cmd("xref") {
            let tr = self.read_table();
            // the table and its trailer, read through to here
            let to = self.lex.pos + 1;
            if !self.here(off as usize, to, false) {
                return pdf_obj::null();
            }
            return tr;
        }
        // a cross-reference stream: `num gen obj << ... >> stream`
        self.lex.pos = off as usize;
        let o = self.read_indirect_at(off as usize, -1);
        if o.k != K::Stream || !o.at("Type").is_name("XRef") {
            return pdf_obj::null();
        }
        self.read_xref_stream(&o);
        o
    }

    fn read_table(&mut self) -> Obj {
        loop {
            let save = self.lex.pos;
            let a = self.lex.token();
            if a.is_cmd("trailer") {
                let t = self.lex.object(0);
                return t;
            }
            if a.k != K::Num {
                self.lex.pos = save;
                return pdf_obj::null();
            }
            let b = self.lex.token();
            if b.k != K::Num {
                return pdf_obj::null();
            }
            let first = a.i();
            let count = b.i();
            let mut i: i64 = 0;
            while i < count {
                let o = self.lex.token();
                let g = self.lex.token();
                let f = self.lex.token();
                if o.k != K::Num || g.k != K::Num || f.k != K::Cmd {
                    return pdf_obj::null();
                }
                let num = first + i;
                if f.name == "n" {
                    self.set_entry(num, 1, o.i(), g.i(), false);
                } else {
                    self.set_entry(num, 0, 0, 0, false);
                }
                i += 1;
            }
        }
    }

    fn read_xref_stream(&mut self, s: &Obj) {
        let d = self.stream_data(s);
        let w = s.at("W").nums();
        if w.len() < 3 {
            return;
        }
        let w0 = w[0] as usize;
        let w1 = w[1] as usize;
        let w2 = w[2] as usize;
        let row = w0 + w1 + w2;
        if row == 0 {
            return;
        }
        let mut index: Vec<i64> = vec![];
        let idx = s.at("Index");
        if idx.k == K::Arr {
            for x in idx.items.iter() {
                index.push(x.i());
            }
        } else {
            index.push(0);
            index.push(s.at("Size").i());
        }
        let mut pos: usize = 0;
        let mut j: usize = 0;
        while j + 1 < index.len() {
            let first = index[j];
            let count = index[j + 1];
            let mut i: i64 = 0;
            while i < count && pos + row <= d.len() {
                let mut t: i64 = 1;
                if w0 > 0 {
                    t = be(&d, pos, w0);
                }
                let a = be(&d, pos + w0, w1);
                let b = be(&d, pos + w0 + w1, w2);
                pos += row;
                if t == 0 {
                    self.set_entry(first + i, 0, 0, 0, false);
                } else if t == 1 {
                    self.set_entry(first + i, 1, a, b, false);
                } else if t == 2 {
                    self.set_entry(first + i, 2, a, b, false);
                }
                i += 1;
            }
            j += 2;
        }
    }

    /// Reads the cross-reference again by scanning the file for `n g obj`.
    pub fn repair(&mut self) {
        if self.repaired {
            return;
        }
        if !self.complete() {
            // scanning reads the whole file
            let n = self.lex.buf.len();
            self.here(0, n, true);
            return;
        }
        self.repaired = true;
        self.warn("cross-reference rebuilt by scanning");
        self.x_type = vec![];
        self.x_a = vec![];
        self.x_b = vec![];
        self.cache = HashMap::new();
        let n = self.lex.buf.len();
        let mut trailers: Vec<usize> = vec![];
        let mut i: usize = 0;
        while i + 3 <= n {
            let c = self.lex.buf[i];
            if c == 111 && starts_with_at(&self.lex.buf, i, "obj") && (i + 3 == n || crate::pdf_lexer::is_ws(self.lex.buf[i + 3]) || crate::pdf_lexer::is_delim(self.lex.buf[i + 3])) {
                // back over `num ws gen ws`
                let mut k = i;
                let mut ok = true;
                let mut gen: i64 = 0;
                let mut num: i64 = 0;
                if k == 0 || !crate::pdf_lexer::is_ws(self.lex.buf[k - 1]) {
                    ok = false;
                }
                while ok && k > 0 && crate::pdf_lexer::is_ws(self.lex.buf[k - 1]) {
                    k -= 1;
                }
                let gen_end = k;
                while ok && k > 0 && is_digit(self.lex.buf[k - 1]) {
                    k -= 1;
                }
                if ok && k < gen_end {
                    gen = digits_at(&self.lex.buf, k, gen_end);
                } else {
                    ok = false;
                }
                let mut sp = k;
                while ok && sp > 0 && crate::pdf_lexer::is_ws(self.lex.buf[sp - 1]) {
                    sp -= 1;
                }
                if sp == k {
                    ok = false;
                }
                let num_end = sp;
                while ok && sp > 0 && is_digit(self.lex.buf[sp - 1]) {
                    sp -= 1;
                }
                if ok && sp < num_end && num_end - sp < 10 {
                    num = digits_at(&self.lex.buf, sp, num_end);
                } else {
                    ok = false;
                }
                if ok {
                    // a later definition replaces an earlier one
                    self.set_entry(num, 1, sp as i64, gen, true);
                }
                i += 3;
                continue;
            }
            if c == 116 && starts_with_at(&self.lex.buf, i, "trailer") {
                trailers.push(i + 7);
            }
            i += 1;
        }
        // the trailers, last first
        self.trailer = pdf_obj::dict();
        let mut t = trailers.len();
        while t > 0 {
            t -= 1;
            self.lex.pos = trailers[t];
            let d = self.lex.object(0);
            if d.is_dict() {
                self.merge_trailer(&d);
            }
        }
        // objects in object streams, cross-reference streams' trailers, and the catalog
        let count = self.x_type.len();
        let mut num: usize = 0;
        let mut catalog: i64 = -1;
        while num < count {
            if self.x_type[num] == 1 {
                let off = self.x_a[num] as usize;
                let o = self.read_indirect_at(off, num as i64);
                if o.k == K::Stream && o.at("Type").is_name("ObjStm") {
                    self.index_objstm(num as i64, &o);
                }
                if o.k == K::Stream && o.at("Type").is_name("XRef") {
                    self.merge_trailer(&o);
                }
                if o.is_dict() && o.at("Type").is_name("Catalog") && o.has("Pages") {
                    catalog = num as i64;
                }
            }
            num += 1;
        }
        self.trailer.set("Type", pdf_obj::null());
        if catalog >= 0 && !self.root_ok() {
            self.trailer.set("Root", pdf_obj::reference(catalog, 0));
        }
        self.cache = HashMap::new();
    }

    fn root_ok(&mut self) -> bool {
        let r = self.trailer.at("Root");
        let c = self.resolve(&r);
        c.is_dict() && c.has("Pages")
    }

    fn index_objstm(&mut self, stm: i64, s: &Obj) {
        let d = self.stream_data(s);
        let n = s.at("N").i();
        let mut lx = PdfLexer::new(d);
        let mut i: i64 = 0;
        while i < n {
            let a = lx.token();
            let _b = lx.token();
            if a.k != K::Num {
                break;
            }
            let num = a.i();
            if num >= 0 && ((num as usize) >= self.x_type.len() || self.x_type[num as usize] < 0) {
                self.set_entry(num, 2, stm, i, false);
            }
            i += 1;
        }
    }

    /// The indirect object at `off`; `want` is its expected number (-1 for any).
    fn read_indirect_at(&mut self, off: usize, want: i64) -> Obj {
        if off >= self.lex.buf.len() {
            return pdf_obj::make(K::Eof);
        }
        if !self.here(off, off + 1, true) {
            return pdf_obj::make(K::Eof);
        }
        self.lex.pos = off;
        let a = self.lex.token();
        let b = self.lex.token();
        let c = self.lex.token();
        if a.k != K::Num || b.k != K::Num || !c.is_cmd("obj") {
            return pdf_obj::make(K::Eof);
        }
        if want >= 0 && a.i() != want {
            return pdf_obj::make(K::Eof);
        }
        let mut o = self.lex.object(0);
        let save = self.lex.pos;
        let t = self.lex.token();
        // the object's head (its dictionary and the keyword after it)
        let head_end = self.lex.pos + 1;
        if !self.here(off, head_end, false) {
            return pdf_obj::make(K::Eof);
        }
        if t.is_cmd("stream") && o.is_dict() {
            // the data starts after the end of line
            let mut p = self.lex.pos;
            if p < self.lex.buf.len() && self.lex.buf[p] == 13 {
                p += 1;
            }
            if p < self.lex.buf.len() && self.lex.buf[p] == 10 {
                p += 1;
            }
            let len_obj = o.at("Length");
            let mut len: i64 = -1;
            if len_obj.k == K::Num {
                len = len_obj.i();
            } else if len_obj.k == K::Ref {
                let here = self.lex.pos;
                let m = self.misses;
                let l = self.resolve(&len_obj);
                self.lex.pos = here;
                if self.misses > m {
                    return pdf_obj::make(K::Eof);
                }
                if l.k == K::Num {
                    len = l.i();
                }
            }
            let n = self.lex.buf.len();
            if len >= 0 && p + (len as usize) <= n {
                // the data and the endstream after it
                if !self.here(p, p + (len as usize) + 32, true) {
                    return pdf_obj::make(K::Eof);
                }
            }
            let mut end: usize = 0;
            let mut good = false;
            if len >= 0 && p + (len as usize) <= n {
                end = p + (len as usize);
                // "endstream" should follow, after optional white space
                let mut q = end;
                while q < n && crate::pdf_lexer::is_ws(self.lex.buf[q]) {
                    q += 1;
                }
                good = starts_with_at(&self.lex.buf, q, "endstream");
            }
            if !good {
                // the length is wrong: search for endstream
                let mut q = p;
                let mut found = false;
                while q + 9 <= n {
                    if self.lex.buf[q] == 101 && starts_with_at(&self.lex.buf, q, "endstream") {
                        found = true;
                        break;
                    }
                    q += 1;
                }
                if !self.here(p, q + 9, false) {
                    return pdf_obj::make(K::Eof);
                }
                if found {
                    end = q;
                    // the end of line before endstream is not data
                    if end > p && self.lex.buf[end - 1] == 10 {
                        end -= 1;
                    }
                    if end > p && self.lex.buf[end - 1] == 13 {
                        end -= 1;
                    }
                } else if len >= 0 {
                    end = if p + (len as usize) <= n { p + (len as usize) } else { n };
                } else {
                    end = n;
                }
            }
            // in a buffer of its final size (pushes copy it in Go)
            let dn: usize = if end > p { end - p } else { 0 };
            let mut data: Vec<u8> = vec![0; dn];
            let mut k = p;
            while k < end {
                data[k - p] = self.lex.buf[k];
                k += 1;
            }
            o.k = K::Stream;
            o.s = data;
            o.n = a.i() as f64;
            o.g = b.i();
            self.lex.pos = end;
        } else {
            self.lex.pos = save;
        }
        o
    }

    /// Object `num` (resolved through the cross-reference), or Null.
    pub fn object(&mut self, num: i64) -> Obj {
        if num < 0 {
            return pdf_obj::null();
        }
        match self.cache.get(&num) {
            Some(o) => {
                return o.clone();
            }
            None => {}
        }
        if self.busy.contains(&num) || self.busy.len() > 64 {
            return pdf_obj::null();
        }
        self.busy.push(num);
        let m = self.misses;
        let mut o = self.load(num);
        if self.misses > m {
            // read from a hole: not kept, and not a reason to repair
            let nb0 = self.busy.len() - 1;
            self.busy.truncate(nb0);
            return pdf_obj::null();
        }
        if o.k == K::Eof && !self.repaired {
            // the offset was wrong: read the cross-reference again by scanning
            self.repair();
            o = self.load(num);
        }
        let nb = self.busy.len() - 1;
        self.busy.truncate(nb);
        if o.k == K::Eof {
            o = pdf_obj::null();
        }
        if self.misses > m {
            return o;
        }
        self.cache.insert(num, o.clone());
        o
    }

    fn load(&mut self, num: i64) -> Obj {
        let u = num as usize;
        if u >= self.x_type.len() {
            return pdf_obj::null();
        }
        let t = self.x_type[u];
        if t == 1 {
            let off = self.x_a[u] as usize;
            let mut o = self.read_indirect_at(off, num);
            if o.k != K::Eof && self.crypt.active {
                let gen = self.x_b[u];
                o = self.crypt.decrypt_obj(o, num, gen);
            }
            return o;
        }
        if t == 2 {
            let stm = self.x_a[u];
            let idx = self.x_b[u];
            return self.from_objstm(stm, idx, num);
        }
        if t < 0 {
            return pdf_obj::make(K::Eof);
        }
        pdf_obj::null()
    }

    fn from_objstm(&mut self, stm: i64, idx: i64, num: i64) -> Obj {
        let s = self.object(stm);
        if s.k != K::Stream {
            return pdf_obj::make(K::Eof);
        }
        let d = self.stream_data(&s);
        let n = s.at("N").i();
        let first = s.at("First").i();
        let mut lx = PdfLexer::new(d);
        let mut nums: Vec<i64> = vec![];
        let mut offs: Vec<i64> = vec![];
        let mut i: i64 = 0;
        while i < n {
            let a = lx.token();
            let b = lx.token();
            if a.k != K::Num || b.k != K::Num {
                break;
            }
            nums.push(a.i());
            offs.push(b.i());
            i += 1;
        }
        // all the objects of the stream go to the cache at once
        let mut found = pdf_obj::make(K::Eof);
        let mut k: usize = 0;
        while k < nums.len() {
            lx.pos = (first + offs[k]) as usize;
            let o = lx.object(0);
            if nums[k] == num && ((k as i64) == idx || found.k == K::Eof) {
                found = o.clone();
            }
            let other = nums[k];
            if other != num && (other as usize) < self.x_type.len() && self.x_type[other as usize] == 2 && self.x_a[other as usize] == stm {
                self.cache.insert(other, o);
            }
            k += 1;
        }
        found
    }

    /// `o` itself, or the object it refers to.
    pub fn resolve(&mut self, o: &Obj) -> Obj {
        if o.k == K::Ref {
            return self.object(o.i());
        }
        o.clone()
    }

    /// The resolved value of `key` in dictionary `d`.
    pub fn get_key(&mut self, d: &Obj, key: &str) -> Obj {
        let v = d.at(key);
        self.resolve(&v)
    }

    /// A stream's data with its filters applied (image codecs left).
    pub fn stream_data(&mut self, s: &Obj) -> Vec<u8> {
        let r = self.stream_decoded(s);
        r.data
    }

    pub fn stream_decoded(&mut self, s: &Obj) -> pdf_filters::Decoded {
        let f = self.get_key(s, "Filter");
        let mut p = self.get_key(s, "DecodeParms");
        if p.is_null() {
            p = self.get_key(s, "DP");
        }
        let mut pr = p.clone();
        if p.k == K::Arr {
            pr = pdf_obj::arr();
            for x in p.items.iter() {
                let y = self.resolve(x);
                pr.push(y);
            }
        }
        let mut fr = f.clone();
        if f.k == K::Arr {
            fr = pdf_obj::arr();
            for x in f.items.iter() {
                let y = self.resolve(x);
                fr.push(y);
            }
        }
        if f.is_null() {
            let mut ff = self.get_key(s, "F");
            if ff.k != K::Name && ff.k != K::Arr {
                ff = pdf_obj::null();
            }
            fr = ff;
        }
        pdf_filters::decode(&s.s, &fr, &pr)
    }

    fn setup_crypt(&mut self, password: &str) {
        let e = self.trailer.at("Encrypt");
        if e.is_null() {
            return;
        }
        let ed = self.resolve(&e);
        if !ed.is_dict() {
            return;
        }
        let id = self.get_key(&self.trailer.clone(), "ID");
        let mut id0: Vec<u8> = vec![];
        if id.k == K::Arr && id.items.len() > 0 {
            id0 = id.items[0].s.clone();
        }
        let enc_num: i64 = if e.k == K::Ref { e.i() } else { -1 };
        let c = Crypt::open(&ed, &id0, password, enc_num);
        if !c.error.is_empty() {
            self.error = c.error.clone();
        }
        self.crypt = c;
        // objects read so far were read before the key was known
        self.cache = HashMap::new();
    }

    fn read_pages(&mut self) {
        self.pages = vec![];
        let root = self.get_key(&self.trailer.clone(), "Root");
        if !root.is_dict() {
            return;
        }
        let pages_ref = root.at("Pages");
        let mut visited: Vec<i64> = vec![];
        let inh = pdf_obj::dict();
        self.walk(&pages_ref, &inh, &mut visited, 0);
    }

    fn walk(&mut self, node_ref: &Obj, inherited: &Obj, visited: &mut Vec<i64>, depth: i64) {
        if depth > 64 || self.pages.len() > 100000 {
            return;
        }
        if node_ref.k == K::Ref {
            let n = node_ref.i();
            if visited.contains(&n) {
                self.warn("page tree loop");
                return;
            }
            visited.push(n);
        }
        let node = self.resolve(node_ref);
        if !node.is_dict() {
            return;
        }
        let mut inh = inherited.clone();
        let names: Vec<&str> = vec!["Resources", "MediaBox", "CropBox", "Rotate"];
        for k in names.iter() {
            if node.has(k) {
                let v = node.at(k);
                inh.set(k, v);
            }
        }
        let kids = self.get_key(&node, "Kids");
        let ty = node.at("Type");
        if ty.is_name("Page") || (kids.k != K::Arr && !ty.is_name("Pages")) {
            let num = if node_ref.k == K::Ref { node_ref.i() } else { -1 };
            self.add_page(&node, &inh, num);
            return;
        }
        if kids.k == K::Arr {
            // a sparse document's flat node (as many pages as kids): a kid
            // that is not here yet is a page read when it is shown, so that
            // opening does not read every page's dictionary
            let flat = self.sparse && !self.no_lazy && node.at("Count").is_num() && node.at("Count").i() == (kids.items.len() as i64);
            for kid in kids.items.iter() {
                if flat && kid.k == K::Ref && !self.object_here(kid.i()) {
                    self.add_lazy(kid.i(), &inh);
                } else {
                    self.walk(kid, &inh, visited, depth + 1);
                }
            }
        }
    }

    /// Whether object `num` can be read without a missing chunk: it is in
    /// the cache, or the chunk of its offset is here (an object in an
    /// object stream counts as not here).
    fn object_here(&self, num: i64) -> bool {
        if self.cache.contains_key(&num) {
            return true;
        }
        if num < 0 || (num as usize) >= self.x_type.len() || self.x_type[num as usize] != 1 {
            return false;
        }
        let off = self.x_a[num as usize] as usize;
        let c = off / self.chunk;
        c < self.have.len() && self.have[c]
    }

    fn add_lazy(&mut self, num: i64, inh: &Obj) {
        let mb = self.get_key(inh, "MediaBox");
        let mut media = norm_box(&self.resolve_arr(&mb));
        if media[2] - media[0] <= 0.0 || media[3] - media[1] <= 0.0 {
            media = vec![0.0, 0.0, 612.0, 792.0];
        }
        let crop = media.clone();
        let rot_o = self.get_key(inh, "Rotate");
        let mut rot = rot_o.i() % 360;
        if rot < 0 {
            rot += 360;
        }
        if rot % 90 != 0 {
            rot = 0;
        }
        self.pages.push(Page { dict: pdf_obj::null(), media: media, crop: crop, rotate: rot, resources: pdf_obj::null(), num: num, lazy: true, inh: inh.clone() });
    }

    /// Reads page i's own dictionary when it is lazy; false when its bytes
    /// are not here yet (short) or the page tree had to be read again.
    pub fn realize(&mut self, i: usize) -> bool {
        if i >= self.pages.len() || !self.pages[i].lazy {
            return true;
        }
        let num = self.pages[i].num;
        let node = self.object(num);
        if self.short {
            return false;
        }
        let ty = node.at("Type");
        let kids = node.at("Kids");
        if !node.is_dict() || ty.is_name("Pages") || kids.k == K::Arr {
            // not a page after all: the whole tree, every page read
            self.no_lazy = true;
            let old = self.pages.clone();
            self.cache = HashMap::new();
            self.read_pages();
            if self.short {
                self.pages = old;
                return false;
            }
            if self.pages.len() != old.len() {
                // the viewer has laid out the pages it was given
                self.pages = old;
                self.warn("page tree changed when read in full");
            }
            return true;
        }
        let mut inh = self.pages[i].inh.clone();
        let names: Vec<&str> = vec!["Resources", "MediaBox", "CropBox", "Rotate"];
        for k in names.iter() {
            if node.has(k) {
                let v = node.at(k);
                inh.set(k, v);
            }
        }
        let n = self.pages.len();
        self.add_page(&node, &inh, num);
        if self.short {
            self.pages.truncate(n);
            return false;
        }
        let made = self.pages[n].clone();
        self.pages.truncate(n);
        self.pages[i] = made;
        true
    }

    fn add_page(&mut self, node: &Obj, inh: &Obj, num: i64) {
        let mb = self.get_key(inh, "MediaBox");
        let mut media = norm_box(&self.resolve_arr(&mb));
        if media[2] - media[0] <= 0.0 || media[3] - media[1] <= 0.0 {
            media = vec![0.0, 0.0, 612.0, 792.0];
        }
        let mut crop = media.clone();
        let cb = self.get_key(inh, "CropBox");
        if cb.k == K::Arr {
            let c = norm_box(&self.resolve_arr(&cb));
            // the crop box is clipped to the media box
            let x0 = if c[0] > media[0] { c[0] } else { media[0] };
            let y0 = if c[1] > media[1] { c[1] } else { media[1] };
            let x1 = if c[2] < media[2] { c[2] } else { media[2] };
            let y1 = if c[3] < media[3] { c[3] } else { media[3] };
            if x1 > x0 && y1 > y0 {
                crop = vec![x0, y0, x1, y1];
            }
        }
        let rot_o = self.get_key(inh, "Rotate");
        let mut rot = rot_o.i() % 360;
        if rot < 0 {
            rot += 360;
        }
        if rot % 90 != 0 {
            rot = 0;
        }
        let res = self.get_key(inh, "Resources");
        self.pages.push(Page { dict: node.clone(), media: media, crop: crop, rotate: rot, resources: res, num: num, lazy: false, inh: pdf_obj::null() });
    }

    fn resolve_arr(&mut self, a: &Obj) -> Obj {
        let mut r = pdf_obj::arr();
        for x in a.items.iter() {
            let y = self.resolve(x);
            r.push(y);
        }
        r
    }

    /// Pages found by looking at every object, for a file whose page tree is lost.
    fn pages_by_scan(&mut self) {
        if !self.repaired {
            self.repair();
        }
        if self.short {
            return;
        }
        let n = self.x_type.len();
        let mut num: usize = 0;
        let inh = pdf_obj::dict();
        while num < n {
            if self.x_type[num] >= 1 {
                let o = self.object(num as i64);
                if o.is_dict() && o.at("Type").is_name("Page") {
                    let mut i2 = inh.clone();
                    let names: Vec<&str> = vec!["Resources", "MediaBox", "CropBox", "Rotate"];
                    for k in names.iter() {
                        if o.has(k) {
                            i2.set(k, o.at(k));
                        }
                    }
                    self.add_page(&o, &i2, num as i64);
                }
            }
            num += 1;
        }
        if !self.pages.is_empty() {
            self.warn("pages found by scanning");
        }
    }

    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// The page's content streams, decoded and joined.
    pub fn page_content(&mut self, i: usize) -> Vec<u8> {
        let mut out: Vec<u8> = vec![];
        if i >= self.pages.len() || !self.realize(i) {
            return out;
        }
        let pd = self.pages[i].dict.clone();
        let c = self.get_key(&pd, "Contents");
        if c.k == K::Stream {
            return self.stream_data(&c);
        }
        if c.k == K::Arr {
            for x in c.items.iter() {
                let s = self.resolve(x);
                if s.k == K::Stream {
                    let d = self.stream_data(&s);
                    for b in d.iter() {
                        out.push(*b);
                    }
                    out.push(10);
                }
            }
        }
        out
    }

    /// The document information dictionary's string `key` (Title, Author, ...).
    pub fn info(&mut self, key: &str) -> String {
        let t = self.trailer.clone();
        let inf = self.get_key(&t, "Info");
        if !inf.is_dict() {
            return String::new();
        }
        let v = self.get_key(&inf, key);
        if v.k == K::Str {
            return v.text();
        }
        String::new()
    }
}

fn digits_at(buf: &Vec<u8>, from: usize, to: usize) -> i64 {
    let mut v: i64 = 0;
    let mut k = from;
    while k < to {
        v = v * 10 + ((buf[k] - 48) as i64);
        k += 1;
    }
    v
}

/// A big-endian unsigned number of `n` bytes at `pos`.
pub fn be(d: &Vec<u8>, pos: usize, n: usize) -> i64 {
    let mut v: i64 = 0;
    let mut k: usize = 0;
    while k < n {
        if pos + k < d.len() {
            v = v * 256 + (d[pos + k] as i64);
        }
        k += 1;
    }
    v
}
