//! PDF functions (ISO 32000-1 §7.10): sampled (type 0), exponential (2),
//! stitching (3) and PostScript calculator (4). A function is read once
//! into a `Func` and evaluated many times.

use crate::pdf_doc::Doc;
use crate::pdf_lexer::PdfLexer;
use crate::pdf_obj::Obj;
use crate::pdf_obj::K;

#[derive(Clone, Debug)]
pub struct Func {
    pub ftype: i64,
    pub domain: Vec<f64>,
    pub range: Vec<f64>,
    pub n_out: usize,
    // type 0
    size: Vec<i64>,
    bps: i64,
    encode: Vec<f64>,
    decode: Vec<f64>,
    samples: Vec<f64>,
    // type 2
    c0: Vec<f64>,
    c1: Vec<f64>,
    exp_n: f64,
    // type 3
    subs: Vec<Func>,
    bounds: Vec<f64>,
    // type 4: the program as a flat list; ops are names, numbers are numbers
    prog: Vec<Obj>,
    /// a list of functions, one per output (an array of 1-out functions)
    pub parts: Vec<Func>,
    pub ok: bool,
}

fn empty() -> Func {
    Func {
        ftype: -1,
        domain: vec![],
        range: vec![],
        n_out: 0,
        size: vec![],
        bps: 8,
        encode: vec![],
        decode: vec![],
        samples: vec![],
        c0: vec![],
        c1: vec![],
        exp_n: 1.0,
        subs: vec![],
        bounds: vec![],
        prog: vec![],
        parts: vec![],
        ok: false,
    }
}

fn clip(x: f64, a: f64, b: f64) -> f64 {
    if x < a {
        return a;
    }
    if x > b {
        return b;
    }
    x
}

fn interp(x: f64, x0: f64, x1: f64, y0: f64, y1: f64) -> f64 {
    if x1 == x0 {
        return y0;
    }
    y0 + (x - x0) * (y1 - y0) / (x1 - x0)
}

fn powf(x: f64, n: f64) -> f64 {
    if n == 1.0 {
        return x;
    }
    if x <= 0.0 {
        return 0.0;
    }
    (n * x.ln()).exp()
}

/// The function `o` (a dictionary, a stream, or an array of them).
pub fn read(doc: &mut Doc, o: &Obj, depth: i64) -> Func {
    let f = doc.resolve(o);
    let mut r = empty();
    if depth > 8 {
        return r;
    }
    if f.k == K::Arr {
        r.ftype = 100;
        for x in f.items.iter() {
            let p = read(doc, x, depth + 1);
            r.n_out += 1;
            r.parts.push(p);
        }
        r.ok = !r.parts.is_empty();
        return r;
    }
    if !f.is_dict() {
        return r;
    }
    r.ftype = doc.get_key(&f, "FunctionType").i();
    r.domain = doc.get_key(&f, "Domain").nums();
    if r.domain.len() < 2 {
        r.domain = vec![0.0, 1.0];
    }
    r.range = doc.get_key(&f, "Range").nums();
    if r.ftype == 0 {
        if f.k != K::Stream {
            return r;
        }
        r.size = vec![];
        for x in doc.get_key(&f, "Size").items.iter() {
            r.size.push(x.i());
        }
        r.bps = doc.get_key(&f, "BitsPerSample").i();
        r.n_out = r.range.len() / 2;
        let m = r.domain.len() / 2;
        if r.size.len() < m || r.n_out == 0 || r.bps <= 0 {
            return r;
        }
        r.encode = doc.get_key(&f, "Encode").nums();
        if r.encode.len() < 2 * m {
            r.encode = vec![];
            let mut i: usize = 0;
            while i < m {
                r.encode.push(0.0);
                r.encode.push((r.size[i] - 1) as f64);
                i += 1;
            }
        }
        r.decode = doc.get_key(&f, "Decode").nums();
        if r.decode.len() < 2 * r.n_out {
            r.decode = r.range.clone();
        }
        let data = doc.stream_data(&f);
        let mut total: usize = r.n_out;
        for s in r.size.iter() {
            total = total * (*s as usize);
        }
        let maxv = powf(2.0, r.bps as f64) - 1.0;
        let mut bitpos: usize = 0;
        let mut i: usize = 0;
        while i < total {
            let mut v: f64 = 0.0;
            let mut k: i64 = 0;
            while k < r.bps {
                let byte = bitpos / 8;
                let bit = 7 - (bitpos % 8);
                let mut b: i64 = 0;
                if byte < data.len() {
                    b = ((data[byte] as i64) >> bit) & 1;
                }
                v = v * 2.0 + (b as f64);
                bitpos += 1;
                k += 1;
            }
            r.samples.push(v / maxv);
            i += 1;
        }
        r.ok = true;
        return r;
    }
    if r.ftype == 2 {
        r.c0 = doc.get_key(&f, "C0").nums();
        r.c1 = doc.get_key(&f, "C1").nums();
        if r.c0.is_empty() {
            r.c0 = vec![0.0];
        }
        if r.c1.is_empty() {
            r.c1 = vec![1.0];
        }
        r.exp_n = doc.get_key(&f, "N").f();
        r.n_out = r.c0.len();
        r.ok = true;
        return r;
    }
    if r.ftype == 3 {
        let fs = doc.get_key(&f, "Functions");
        for x in fs.items.iter() {
            let p = read(doc, x, depth + 1);
            if p.n_out > r.n_out {
                r.n_out = p.n_out;
            }
            r.subs.push(p);
        }
        r.bounds = doc.get_key(&f, "Bounds").nums();
        r.encode = doc.get_key(&f, "Encode").nums();
        r.ok = !r.subs.is_empty();
        return r;
    }
    if r.ftype == 4 {
        if f.k != K::Stream {
            return r;
        }
        let data = doc.stream_data(&f);
        let mut lx = PdfLexer::new(data);
        loop {
            let t = lx.token();
            if t.k == K::Eof {
                break;
            }
            r.prog.push(t);
        }
        r.n_out = r.range.len() / 2;
        r.ok = true;
        return r;
    }
    r
}

impl Func {
    pub fn eval(&self, input: &Vec<f64>) -> Vec<f64> {
        if self.ftype == 100 {
            let mut out: Vec<f64> = vec![];
            for p in self.parts.iter() {
                let v = p.eval(input);
                if v.is_empty() {
                    out.push(0.0);
                } else {
                    out.push(v[0]);
                }
            }
            return out;
        }
        let mut x: Vec<f64> = vec![];
        let mut i: usize = 0;
        while i < input.len() && 2 * i + 1 < self.domain.len() {
            x.push(clip(input[i], self.domain[2 * i], self.domain[2 * i + 1]));
            i += 1;
        }
        if x.is_empty() {
            x.push(0.0);
        }
        let mut out = if self.ftype == 0 {
            self.eval0(&x)
        } else if self.ftype == 2 {
            self.eval2(x[0])
        } else if self.ftype == 3 {
            self.eval3(x[0])
        } else if self.ftype == 4 {
            self.eval4(&x)
        } else {
            vec![]
        };
        if self.range.len() >= 2 {
            let mut j: usize = 0;
            while j < out.len() && 2 * j + 1 < self.range.len() {
                out[j] = clip(out[j], self.range[2 * j], self.range[2 * j + 1]);
                j += 1;
            }
        }
        out
    }

    fn eval0(&self, x: &Vec<f64>) -> Vec<f64> {
        // multilinear interpolation of the samples
        let m = self.size.len();
        let mut e: Vec<f64> = vec![];
        let mut i: usize = 0;
        while i < m {
            let xi = if i < x.len() { x[i] } else { 0.0 };
            let mut v = interp(xi, self.domain[2 * i], self.domain[2 * i + 1], self.encode[2 * i], self.encode[2 * i + 1]);
            v = clip(v, 0.0, (self.size[i] - 1) as f64);
            e.push(v);
            i += 1;
        }
        let mut out: Vec<f64> = vec![];
        let mut j: usize = 0;
        while j < self.n_out {
            // walk the 2^m corners
            let corners: usize = 1 << m;
            let mut acc = 0.0;
            let mut c: usize = 0;
            while c < corners {
                let mut wgt = 1.0;
                let mut idx: usize = 0;
                let mut stride: usize = 1;
                let mut d: usize = 0;
                while d < m {
                    let lo = e[d].floor();
                    let frac = e[d] - lo;
                    let mut k = lo as usize;
                    if ((c >> d) & 1) == 1 {
                        if (k as i64) + 1 < self.size[d] {
                            k += 1;
                        }
                        wgt = wgt * frac;
                    } else {
                        wgt = wgt * (1.0 - frac);
                    }
                    idx += k * stride;
                    stride = stride * (self.size[d] as usize);
                    d += 1;
                }
                let si = idx * self.n_out + j;
                if wgt > 0.0 && si < self.samples.len() {
                    acc += wgt * self.samples[si];
                }
                c += 1;
            }
            let v = if 2 * j + 1 < self.decode.len() { self.decode[2 * j] + acc * (self.decode[2 * j + 1] - self.decode[2 * j]) } else { acc };
            out.push(v);
            j += 1;
        }
        out
    }

    fn eval2(&self, x: f64) -> Vec<f64> {
        let p = powf(x, self.exp_n);
        let mut out: Vec<f64> = vec![];
        let mut i: usize = 0;
        while i < self.c0.len() {
            let b = if i < self.c1.len() { self.c1[i] } else { 1.0 };
            out.push(self.c0[i] + p * (b - self.c0[i]));
            i += 1;
        }
        out
    }

    fn eval3(&self, x: f64) -> Vec<f64> {
        let k = self.subs.len();
        let mut i: usize = 0;
        while i < self.bounds.len() && x >= self.bounds[i] {
            i += 1;
        }
        if i >= k {
            i = k - 1;
        }
        let lo = if i == 0 { self.domain[0] } else { self.bounds[i - 1] };
        let hi = if i < self.bounds.len() { self.bounds[i] } else { self.domain[1] };
        let mut e0 = 0.0;
        let mut e1 = 1.0;
        if 2 * i + 1 < self.encode.len() {
            e0 = self.encode[2 * i];
            e1 = self.encode[2 * i + 1];
        }
        let t = interp(x, lo, hi, e0, e1);
        let input: Vec<f64> = vec![t];
        self.subs[i].eval(&input)
    }

    fn eval4(&self, x: &Vec<f64>) -> Vec<f64> {
        let mut st: Vec<f64> = x.clone();
        let mut pc = Pc { i: 0 };
        // skip the outer brace
        if !self.prog.is_empty() && self.prog[0].is_cmd("{") {
            pc.i = 1;
        }
        self.run4(&mut st, &mut pc, 0);
        let n = self.n_out;
        let mut out: Vec<f64> = vec![];
        if st.len() >= n {
            let start = st.len() - n;
            let mut i = start;
            while i < st.len() {
                out.push(st[i]);
                i += 1;
            }
        }
        out
    }

    /// Runs the program from `pc` to the matching `}`.
    fn run4(&self, st: &mut Vec<f64>, pc: &mut Pc, depth: i64) {
        while pc.i < self.prog.len() {
            let t = self.prog[pc.i].clone();
            pc.i += 1;
            if st.len() > 1000 || depth > 50 {
                return;
            }
            if t.k == K::Num || t.k == K::Bool {
                st.push(t.n);
                continue;
            }
            if t.k != K::Cmd {
                continue;
            }
            let op = t.name.clone();
            if op == "}" {
                return;
            }
            if op == "{" {
                // `{ a } if` or `{ a } { b } ifelse`
                let a_start = pc.i;
                self.skip_block(pc);
                let mut b_start: usize = 0;
                let mut has_b = false;
                if pc.i < self.prog.len() && self.prog[pc.i].is_cmd("{") {
                    pc.i += 1;
                    b_start = pc.i;
                    has_b = true;
                    self.skip_block(pc);
                }
                let mut kw = String::new();
                if pc.i < self.prog.len() {
                    kw = self.prog[pc.i].name.clone();
                    pc.i += 1;
                }
                let cond = pop(st);
                let mut p2 = Pc { i: 0 };
                if kw == "if" && cond != 0.0 {
                    p2.i = a_start;
                    self.run4(st, &mut p2, depth + 1);
                } else if kw == "ifelse" && has_b {
                    if cond != 0.0 {
                        p2.i = a_start;
                    } else {
                        p2.i = b_start;
                    }
                    self.run4(st, &mut p2, depth + 1);
                }
                continue;
            }
            op4(st, &op);
        }
    }

    fn skip_block(&self, pc: &mut Pc) {
        let mut d = 1;
        while pc.i < self.prog.len() {
            let t = self.prog[pc.i].clone();
            pc.i += 1;
            if t.is_cmd("{") {
                d += 1;
            } else if t.is_cmd("}") {
                d -= 1;
                if d == 0 {
                    return;
                }
            }
        }
    }
}

/// A program counter (a `&mut usize` is not shared on every target).
pub struct Pc {
    pub i: usize,
}

fn pop(st: &mut Vec<f64>) -> f64 {
    if st.is_empty() {
        return 0.0;
    }
    let v = st[st.len() - 1];
    let keep = st.len() - 1;
    st.truncate(keep);
    v
}

fn b2f(b: bool) -> f64 {
    if b {
        return 1.0;
    }
    0.0
}

fn to_i(x: f64) -> i64 {
    if x >= 0.0 {
        return x.floor() as i64;
    }
    0 - ((0.0 - x).floor() as i64)
}

fn op4(st: &mut Vec<f64>, op: &str) {
    if op == "add" {
        let b = pop(st);
        let a = pop(st);
        st.push(a + b);
    } else if op == "sub" {
        let b = pop(st);
        let a = pop(st);
        st.push(a - b);
    } else if op == "mul" {
        let b = pop(st);
        let a = pop(st);
        st.push(a * b);
    } else if op == "div" {
        let b = pop(st);
        let a = pop(st);
        st.push(if b == 0.0 { 0.0 } else { a / b });
    } else if op == "idiv" {
        let b = to_i(pop(st));
        let a = to_i(pop(st));
        st.push(if b == 0 { 0.0 } else { (a / b) as f64 });
    } else if op == "mod" {
        let b = to_i(pop(st));
        let a = to_i(pop(st));
        st.push(if b == 0 { 0.0 } else { (a % b) as f64 });
    } else if op == "neg" {
        let a = pop(st);
        st.push(0.0 - a);
    } else if op == "abs" {
        let a = pop(st);
        st.push(if a < 0.0 { 0.0 - a } else { a });
    } else if op == "ceiling" {
        let a = pop(st);
        st.push(a.ceil());
    } else if op == "floor" {
        let a = pop(st);
        st.push(a.floor());
    } else if op == "round" {
        let a = pop(st);
        st.push((a + 0.5).floor());
    } else if op == "truncate" {
        let a = pop(st);
        st.push(to_i(a) as f64);
    } else if op == "sqrt" {
        let a = pop(st);
        st.push(if a > 0.0 { a.sqrt() } else { 0.0 });
    } else if op == "sin" {
        let a = pop(st);
        st.push((a * 3.141592653589793 / 180.0).sin());
    } else if op == "cos" {
        let a = pop(st);
        st.push((a * 3.141592653589793 / 180.0).cos());
    } else if op == "atan" {
        let b = pop(st);
        let a = pop(st);
        let mut d = a.atan2(b) * 180.0 / 3.141592653589793;
        if d < 0.0 {
            d += 360.0;
        }
        st.push(d);
    } else if op == "exp" {
        let b = pop(st);
        let a = pop(st);
        st.push(powf(a, b));
    } else if op == "ln" {
        let a = pop(st);
        st.push(if a > 0.0 { a.ln() } else { 0.0 });
    } else if op == "log" {
        let a = pop(st);
        st.push(if a > 0.0 { a.ln() / 2.302585092994046 } else { 0.0 });
    } else if op == "cvi" {
        let a = pop(st);
        st.push(to_i(a) as f64);
    } else if op == "cvr" {
    } else if op == "dup" {
        let a = pop(st);
        st.push(a);
        st.push(a);
    } else if op == "pop" {
        pop(st);
    } else if op == "exch" {
        let b = pop(st);
        let a = pop(st);
        st.push(b);
        st.push(a);
    } else if op == "copy" {
        let n = to_i(pop(st)) as usize;
        if n <= st.len() {
            let start = st.len() - n;
            let mut i = start;
            let end = st.len();
            while i < end {
                let v = st[i];
                st.push(v);
                i += 1;
            }
        }
    } else if op == "index" {
        let n = to_i(pop(st)) as usize;
        if n < st.len() {
            let v = st[st.len() - 1 - n];
            st.push(v);
        }
    } else if op == "roll" {
        let j = to_i(pop(st));
        let n = to_i(pop(st));
        if n > 0 && (n as usize) <= st.len() {
            let start = st.len() - (n as usize);
            let mut part: Vec<f64> = vec![];
            let mut i = start;
            while i < st.len() {
                part.push(st[i]);
                i += 1;
            }
            let mut k: usize = 0;
            while k < part.len() {
                let mut src = (k as i64) - j;
                src = ((src % n) + n) % n;
                st[start + k] = part[src as usize];
                k += 1;
            }
        }
    } else if op == "eq" {
        let b = pop(st);
        let a = pop(st);
        st.push(b2f(a == b));
    } else if op == "ne" {
        let b = pop(st);
        let a = pop(st);
        st.push(b2f(a != b));
    } else if op == "gt" {
        let b = pop(st);
        let a = pop(st);
        st.push(b2f(a > b));
    } else if op == "ge" {
        let b = pop(st);
        let a = pop(st);
        st.push(b2f(a >= b));
    } else if op == "lt" {
        let b = pop(st);
        let a = pop(st);
        st.push(b2f(a < b));
    } else if op == "le" {
        let b = pop(st);
        let a = pop(st);
        st.push(b2f(a <= b));
    } else if op == "and" {
        let b = to_i(pop(st));
        let a = to_i(pop(st));
        st.push((a & b) as f64);
    } else if op == "or" {
        let b = to_i(pop(st));
        let a = to_i(pop(st));
        st.push((a | b) as f64);
    } else if op == "xor" {
        let b = to_i(pop(st));
        let a = to_i(pop(st));
        st.push((a ^ b) as f64);
    } else if op == "not" {
        let a = pop(st);
        // booleans are 1/0; on an integer, the bitwise complement
        if a == 0.0 || a == 1.0 {
            st.push(1.0 - a);
        } else {
            st.push((0 - to_i(a) - 1) as f64);
        }
    } else if op == "bitshift" {
        let s = to_i(pop(st));
        let a = to_i(pop(st));
        if s >= 0 {
            st.push((a << s) as f64);
        } else {
            st.push((a >> (0 - s)) as f64);
        }
    } else if op == "true" {
        st.push(1.0);
    } else if op == "false" {
        st.push(0.0);
    }
}

/// A function that is not there (`ok` false).
pub fn read_none() -> Func {
    empty()
}
