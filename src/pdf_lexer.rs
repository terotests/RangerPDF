//! The PDF lexer and object parser (ISO 32000-1 §7.2, §7.3): tokens over a
//! byte buffer, and objects built from them. Streams are read by doc.rs,
//! which knows where the data of an indirect object ends.

use crate::pdf_obj;
use crate::pdf_obj::Obj;
use crate::pdf_obj::K;

pub fn is_ws(c: u8) -> bool {
    c == 32 || c == 10 || c == 13 || c == 9 || c == 12 || c == 0
}

pub fn is_delim(c: u8) -> bool {
    c == 40 || c == 41 || c == 60 || c == 62 || c == 91 || c == 93 || c == 123 || c == 125 || c == 47 || c == 37
}

pub fn hexval(c: u8) -> i64 {
    let n = c as i64;
    if n >= 48 && n <= 57 {
        return n - 48;
    }
    if n >= 97 && n <= 102 {
        return n - 87;
    }
    if n >= 65 && n <= 70 {
        return n - 55;
    }
    -1
}

pub struct PdfLexer {
    pub buf: Vec<u8>,
    pub pos: usize,
}

impl PdfLexer {
    pub fn new(buf: Vec<u8>) -> PdfLexer {
        PdfLexer { buf: buf, pos: 0 }
    }

    pub fn at_end(&self) -> bool {
        self.pos >= self.buf.len()
    }

    pub fn peek(&self) -> i64 {
        if self.pos < self.buf.len() {
            return self.buf[self.pos] as i64;
        }
        -1
    }

    pub fn skip_ws(&mut self) {
        while self.pos < self.buf.len() {
            let c = self.buf[self.pos];
            if c == 37 {
                // a comment, to the end of the line
                while self.pos < self.buf.len() && self.buf[self.pos] != 10 && self.buf[self.pos] != 13 {
                    self.pos += 1;
                }
            } else if is_ws(c) {
                self.pos += 1;
            } else {
                return;
            }
        }
    }

    /// The next token: a number, string, name, keyword, or one of the
    /// delimiters `[ ] << >> { }` (as a Cmd).
    pub fn token(&mut self) -> Obj {
        self.skip_ws();
        if self.pos >= self.buf.len() {
            return pdf_obj::make(K::Eof);
        }
        let c = self.buf[self.pos];
        if c == 40 {
            self.pos += 1;
            return self.literal();
        }
        if c == 60 {
            if self.pos + 1 < self.buf.len() && self.buf[self.pos + 1] == 60 {
                self.pos += 2;
                return pdf_obj::cmd("<<");
            }
            self.pos += 1;
            return self.hex_string();
        }
        if c == 62 {
            if self.pos + 1 < self.buf.len() && self.buf[self.pos + 1] == 62 {
                self.pos += 2;
                return pdf_obj::cmd(">>");
            }
            self.pos += 1;
            return pdf_obj::cmd(">");
        }
        if c == 91 {
            self.pos += 1;
            return pdf_obj::cmd("[");
        }
        if c == 93 {
            self.pos += 1;
            return pdf_obj::cmd("]");
        }
        if c == 123 {
            self.pos += 1;
            return pdf_obj::cmd("{");
        }
        if c == 125 {
            self.pos += 1;
            return pdf_obj::cmd("}");
        }
        if c == 41 {
            self.pos += 1;
            return pdf_obj::cmd(")");
        }
        if c == 47 {
            self.pos += 1;
            return self.name();
        }
        if (c >= 48 && c <= 57) || c == 43 || c == 45 || c == 46 {
            let save = self.pos;
            let n = self.number();
            if n.k == K::Num {
                return n;
            }
            self.pos = save;
        }
        let mut s = String::new();
        while self.pos < self.buf.len() {
            let d = self.buf[self.pos];
            if is_ws(d) || is_delim(d) {
                break;
            }
            s.push(pdf_obj::ch(d as u32));
            self.pos += 1;
        }
        if s.is_empty() {
            self.pos += 1;
            return pdf_obj::cmd("?");
        }
        if s == "true" {
            let mut b = pdf_obj::make(K::Bool);
            b.n = 1.0;
            return b;
        }
        if s == "false" {
            return pdf_obj::make(K::Bool);
        }
        if s == "null" {
            return pdf_obj::null();
        }
        pdf_obj::cmd(&s)
    }

    fn number(&mut self) -> Obj {
        let mut neg = false;
        let mut int_part: f64 = 0.0;
        let mut frac: f64 = 0.0;
        let mut scale: f64 = 1.0;
        let mut seen_dot = false;
        let mut digits = 0;
        // signs may repeat in broken files (pdf.js reads "--5" as -5)
        while self.pos < self.buf.len() && (self.buf[self.pos] == 45 || self.buf[self.pos] == 43) {
            if self.buf[self.pos] == 45 {
                neg = true;
            }
            self.pos += 1;
        }
        while self.pos < self.buf.len() {
            let c = self.buf[self.pos];
            if c >= 48 && c <= 57 {
                let d = (c - 48) as f64;
                if seen_dot {
                    scale = scale / 10.0;
                    frac = frac + d * scale;
                } else {
                    int_part = int_part * 10.0 + d;
                }
                digits += 1;
            } else if c == 46 && !seen_dot {
                seen_dot = true;
            } else if c == 45 && digits == 0 {
                // "-.-5" and the like
            } else {
                break;
            }
            self.pos += 1;
        }
        if digits == 0 {
            if seen_dot || neg {
                // a lone "-" or "." is 0 in pdf.js
                return pdf_obj::int(0);
            }
            return pdf_obj::make(K::Eof);
        }
        let mut v = int_part + frac;
        if neg {
            v = 0.0 - v;
        }
        let mut o = pdf_obj::num(v);
        if !seen_dot {
            o.g = 1;
        }
        o
    }

    fn name(&mut self) -> Obj {
        let mut raw: Vec<u8> = vec![];
        while self.pos < self.buf.len() {
            let c = self.buf[self.pos];
            if is_ws(c) || is_delim(c) {
                break;
            }
            if c == 35 && self.pos + 2 < self.buf.len() {
                let h = hexval(self.buf[self.pos + 1]);
                let l = hexval(self.buf[self.pos + 2]);
                if h >= 0 && l >= 0 {
                    raw.push((h * 16 + l) as u8);
                    self.pos += 3;
                    continue;
                }
            }
            raw.push(c);
            self.pos += 1;
        }
        pdf_obj::name(&pdf_obj::latin1(&raw))
    }

    fn literal(&mut self) -> Obj {
        let mut o = pdf_obj::make(K::Str);
        let mut depth = 1;
        while self.pos < self.buf.len() {
            let c = self.buf[self.pos];
            self.pos += 1;
            if c == 92 {
                if self.pos >= self.buf.len() {
                    break;
                }
                let e = self.buf[self.pos];
                self.pos += 1;
                if e == 110 {
                    o.s.push(10);
                } else if e == 114 {
                    o.s.push(13);
                } else if e == 116 {
                    o.s.push(9);
                } else if e == 98 {
                    o.s.push(8);
                } else if e == 102 {
                    o.s.push(12);
                } else if e == 13 {
                    if self.pos < self.buf.len() && self.buf[self.pos] == 10 {
                        self.pos += 1;
                    }
                } else if e == 10 {
                    // a line continuation
                } else if e >= 48 && e <= 55 {
                    let mut v = (e - 48) as i64;
                    let mut k = 0;
                    while k < 2 && self.pos < self.buf.len() && self.buf[self.pos] >= 48 && self.buf[self.pos] <= 55 {
                        v = v * 8 + ((self.buf[self.pos] - 48) as i64);
                        self.pos += 1;
                        k += 1;
                    }
                    o.s.push((v & 255) as u8);
                } else {
                    o.s.push(e);
                }
            } else if c == 40 {
                depth += 1;
                o.s.push(c);
            } else if c == 41 {
                depth -= 1;
                if depth == 0 {
                    return o;
                }
                o.s.push(c);
            } else {
                o.s.push(c);
            }
        }
        o
    }

    fn hex_string(&mut self) -> Obj {
        let mut o = pdf_obj::make(K::Str);
        let mut hi: i64 = -1;
        while self.pos < self.buf.len() {
            let c = self.buf[self.pos];
            self.pos += 1;
            if c == 62 {
                break;
            }
            let v = hexval(c);
            if v < 0 {
                continue;
            }
            if hi < 0 {
                hi = v;
            } else {
                o.s.push((hi * 16 + v) as u8);
                hi = -1;
            }
        }
        if hi >= 0 {
            o.s.push((hi * 16) as u8);
        }
        o
    }

    /// The next object: a dictionary or array is read whole, and
    /// `num gen R` becomes a reference. A keyword comes back as a Cmd.
    pub fn object(&mut self, depth: i64) -> Obj {
        let t = self.token();
        self.object_from(t, depth)
    }

    pub fn object_from(&mut self, t: Obj, depth: i64) -> Obj {
        if t.k == K::Cmd {
            if t.name == "[" {
                let mut a = pdf_obj::arr();
                if depth > 100 {
                    return a;
                }
                loop {
                    let x = self.token();
                    if x.k == K::Eof || x.is_cmd("]") {
                        break;
                    }
                    if x.is_cmd(">>") || x.is_cmd("endobj") || x.is_cmd("stream") {
                        // a broken array: stop where the object ends
                        self.pos -= x.name.chars().count();
                        break;
                    }
                    if x.is_cmd("R") {
                        // `a b R` inside an array
                        let n = a.items.len();
                        if n >= 2 && a.items[n - 1].k == K::Num && a.items[n - 2].k == K::Num {
                            let gen = a.items[n - 1].i();
                            let num = a.items[n - 2].i();
                            a.items.truncate(n - 2);
                            a.items.push(pdf_obj::reference(num, gen));
                        }
                        continue;
                    }
                    let v = self.object_from(x, depth + 1);
                    a.items.push(v);
                }
                return a;
            }
            if t.name == "<<" {
                let mut d = pdf_obj::dict();
                if depth > 100 {
                    return d;
                }
                loop {
                    let key = self.token();
                    if key.k == K::Eof || key.is_cmd(">>") {
                        break;
                    }
                    if key.k != K::Name {
                        if key.is_cmd("endobj") || key.is_cmd("stream") {
                            self.pos -= key.name.chars().count();
                            break;
                        }
                        continue;
                    }
                    let save = self.pos;
                    let vt = self.token();
                    if vt.is_cmd(">>") {
                        d.set(&key.name, pdf_obj::null());
                        break;
                    }
                    if vt.k == K::Name {
                        // a value that is a name, or a missing value before the next key
                        d.set(&key.name, vt);
                        continue;
                    }
                    if vt.is_cmd("endobj") || vt.is_cmd("stream") {
                        self.pos = save;
                        break;
                    }
                    let v = self.object_from(vt, depth + 1);
                    d.set(&key.name, v);
                }
                return d;
            }
            return t;
        }
        if t.k == K::Num && t.g == 1 && t.n >= 0.0 {
            // `num gen R`?
            let save = self.pos;
            let t2 = self.token();
            if t2.k == K::Num && t2.g == 1 {
                let t3 = self.token();
                if t3.is_cmd("R") {
                    return pdf_obj::reference(t.i(), t2.i());
                }
            }
            self.pos = save;
        }
        t
    }
}
