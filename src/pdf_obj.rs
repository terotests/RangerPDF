//! PDF objects (ISO 32000-1 §7.3). A struct rather than an enum, as in
//! web/graphql/src/json.rs: one shape for every kind, so that a value can
//! be changed in place and copied the same way on every target.

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum K {
    Null,
    Bool,
    Num,
    Str,
    Name,
    Arr,
    Dict,
    Stream,
    Ref,
    /// a bare keyword: an operator in a content stream, or `obj`, `R`, ...
    Cmd,
    /// end of the data, or a token that could not be read
    Eof,
}

#[derive(Clone, Debug)]
pub struct Obj {
    pub k: K,
    /// a number, a boolean (1 or 0), or a reference's object number
    pub n: f64,
    /// a reference's generation; for a number, 1 when it was written as an integer
    pub g: i64,
    /// a string's bytes, or a stream's raw (still encoded) data
    pub s: Vec<u8>,
    /// a name (without the slash) or a keyword
    pub name: String,
    /// array items, or dictionary values (parallel to `keys`)
    pub items: Vec<Obj>,
    pub keys: Vec<String>,
}

pub fn make(k: K) -> Obj {
    Obj { k: k, n: 0.0, g: 0, s: vec![], name: String::new(), items: vec![], keys: vec![] }
}

pub fn null() -> Obj {
    make(K::Null)
}

pub fn num(n: f64) -> Obj {
    let mut o = make(K::Num);
    o.n = n;
    o
}

pub fn int(n: i64) -> Obj {
    let mut o = make(K::Num);
    o.n = n as f64;
    o.g = 1;
    o
}

pub fn name(s: &str) -> Obj {
    let mut o = make(K::Name);
    o.name = s.to_string();
    o
}

pub fn cmd(s: &str) -> Obj {
    let mut o = make(K::Cmd);
    o.name = s.to_string();
    o
}

pub fn reference(num: i64, gen: i64) -> Obj {
    let mut o = make(K::Ref);
    o.n = num as f64;
    o.g = gen;
    o
}

pub fn dict() -> Obj {
    make(K::Dict)
}

pub fn arr() -> Obj {
    make(K::Arr)
}

/// Bytes of an ASCII (or Latin-1) text.
pub fn bytes_of(s: &str) -> Vec<u8> {
    let mut v: Vec<u8> = vec![];
    for c in s.chars() {
        let n = c as i64;
        if n < 256 {
            v.push(n as u8);
        } else {
            v.push(63);
        }
    }
    v
}

/// The character of a code point; '?' for one that is not.
pub fn ch(u: u32) -> char {
    match char::from_u32(u) {
        Some(c) => c,
        None => '?',
    }
}

/// Bytes as Latin-1 text.
pub fn latin1(b: &Vec<u8>) -> String {
    let mut s = String::new();
    for x in b.iter() {
        let n = *x as u32;
        s.push(ch(n));
    }
    s
}

impl Obj {
    pub fn is(&self, k: K) -> bool {
        self.k == k
    }

    pub fn is_null(&self) -> bool {
        self.k == K::Null || self.k == K::Eof
    }

    pub fn is_num(&self) -> bool {
        self.k == K::Num
    }

    pub fn is_dict(&self) -> bool {
        self.k == K::Dict || self.k == K::Stream
    }

    pub fn is_name(&self, s: &str) -> bool {
        self.k == K::Name && self.name == s
    }

    pub fn is_cmd(&self, s: &str) -> bool {
        self.k == K::Cmd && self.name == s
    }

    pub fn f(&self) -> f64 {
        if self.k == K::Num || self.k == K::Bool {
            return self.n;
        }
        0.0
    }

    pub fn i(&self) -> i64 {
        if self.k == K::Num || self.k == K::Bool || self.k == K::Ref {
            let n = self.n;
            if n >= 0.0 {
                return n.floor() as i64;
            }
            return 0 - ((0.0 - n).floor() as i64);
        }
        0
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn item(&self, i: usize) -> Obj {
        if i < self.items.len() {
            return self.items[i].clone();
        }
        null()
    }

    fn index_of(&self, key: &str) -> i64 {
        let mut i: usize = 0;
        while i < self.keys.len() {
            if self.keys[i] == key {
                return i as i64;
            }
            i += 1;
        }
        -1
    }

    pub fn has(&self, key: &str) -> bool {
        self.index_of(key) >= 0
    }

    /// The value of `key` in a dictionary or stream (unresolved).
    pub fn at(&self, key: &str) -> Obj {
        let i = self.index_of(key);
        if i >= 0 {
            return self.items[i as usize].clone();
        }
        null()
    }

    pub fn set(&mut self, key: &str, v: Obj) {
        let i = self.index_of(key);
        if i >= 0 {
            self.items[i as usize] = v;
            return;
        }
        self.keys.push(key.to_string());
        self.items.push(v);
    }

    pub fn push(&mut self, v: Obj) {
        self.items.push(v);
    }

    /// The text of a string (Latin-1, or UTF-16BE after a byte-order mark).
    pub fn text(&self) -> String {
        if self.k == K::Name || self.k == K::Cmd {
            return self.name.clone();
        }
        if self.s.len() >= 2 && self.s[0] == 254 && self.s[1] == 255 {
            let mut out = String::new();
            let mut i: usize = 2;
            while i + 1 < self.s.len() {
                let mut u = ((self.s[i] as u32) << 8) | (self.s[i + 1] as u32);
                i += 2;
                if u >= 0xD800 && u < 0xDC00 && i + 1 < self.s.len() {
                    let lo = ((self.s[i] as u32) << 8) | (self.s[i + 1] as u32);
                    i += 2;
                    u = 0x10000 + ((u - 0xD800) << 10) + (lo - 0xDC00);
                }
                out.push(ch(u));
            }
            return out;
        }
        latin1(&self.s)
    }

    /// A number array as floats.
    pub fn nums(&self) -> Vec<f64> {
        let mut v: Vec<f64> = vec![];
        for x in self.items.iter() {
            v.push(x.f());
        }
        v
    }
}
