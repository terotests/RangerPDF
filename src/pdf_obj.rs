//! PDF objects (ISO 32000-1 §7.3) as a Rust enum with data. Ranger writes
//! it as each target's own sum type (a sealed interface on Kotlin, a
//! std::variant on C++, a tagged struct on Go), so an object carries only
//! the fields of its kind. `K` names the kinds for the callers that test
//! one (`o.kind() == K::Num`).

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
pub enum Obj {
    Null,
    Bool(bool),
    /// `int`: written without a decimal point
    Num { n: f64, int: bool },
    /// a string's bytes
    Str(Vec<u8>),
    /// a name, without the slash
    Name(String),
    Arr(Vec<Obj>),
    /// values parallel to `keys`
    Dict { keys: Vec<String>, items: Vec<Obj> },
    /// a stream's dictionary, its raw (still encoded) data, and the number
    /// and generation of the object it came from
    Stream { keys: Vec<String>, items: Vec<Obj>, data: Vec<u8>, num: i64, gen: i64 },
    Ref { num: i64, gen: i64 },
    Cmd(String),
    Eof,
}

static NO_ITEMS: Vec<Obj> = Vec::new();
static NO_KEYS: Vec<String> = Vec::new();
static NO_BYTES: Vec<u8> = Vec::new();
static NO_WORD: String = String::new();

/// An empty object of a kind.
pub fn make(k: K) -> Obj {
    match k {
        K::Null => Obj::Null,
        K::Bool => Obj::Bool(false),
        K::Num => Obj::Num { n: 0.0, int: false },
        K::Str => Obj::Str(vec![]),
        K::Name => Obj::Name(String::new()),
        K::Arr => Obj::Arr(vec![]),
        K::Dict => Obj::Dict { keys: vec![], items: vec![] },
        K::Stream => Obj::Stream { keys: vec![], items: vec![], data: vec![], num: 0, gen: 0 },
        K::Ref => Obj::Ref { num: 0, gen: 0 },
        K::Cmd => Obj::Cmd(String::new()),
        K::Eof => Obj::Eof,
    }
}

pub fn null() -> Obj {
    Obj::Null
}

pub fn boolean(b: bool) -> Obj {
    Obj::Bool(b)
}

pub fn num(n: f64) -> Obj {
    Obj::Num { n: n, int: false }
}

pub fn int(n: i64) -> Obj {
    Obj::Num { n: n as f64, int: true }
}

pub fn string(b: Vec<u8>) -> Obj {
    Obj::Str(b)
}

pub fn name(s: &str) -> Obj {
    Obj::Name(s.to_string())
}

pub fn cmd(s: &str) -> Obj {
    Obj::Cmd(s.to_string())
}

pub fn reference(num: i64, gen: i64) -> Obj {
    Obj::Ref { num: num, gen: gen }
}

pub fn dict() -> Obj {
    make(K::Dict)
}

pub fn arr() -> Obj {
    make(K::Arr)
}

/// A stream of the dictionary `d` (its entries; an object that is not a
/// dictionary gives none).
pub fn stream(d: Obj, data: Vec<u8>, num: i64, gen: i64) -> Obj {
    match d {
        Obj::Dict { keys, items } => Obj::Stream { keys: keys, items: items, data: data, num: num, gen: gen },
        Obj::Stream { keys, items, .. } => Obj::Stream { keys: keys, items: items, data: data, num: num, gen: gen },
        _ => Obj::Stream { keys: vec![], items: vec![], data: data, num: num, gen: gen },
    }
}

fn put(keys: &mut Vec<String>, items: &mut Vec<Obj>, key: &str, v: Obj) {
    let mut i: usize = 0;
    while i < keys.len() {
        if keys[i] == key {
            items[i] = v;
            return;
        }
        i += 1;
    }
    keys.push(key.to_string());
    items.push(v);
}

/// Bytes of an ASCII (or Latin-1) text.
pub fn bytes_of(s: &str) -> Vec<u8> {
    // sized first and set by index: a push on a Ranger buffer copies it on es6
    // (the length in a local: es6 re-evaluates a `vec![x; n]` count per element)
    let n_chars = s.chars().count();
    let mut v: Vec<u8> = vec![0; n_chars];
    let mut i: usize = 0;
    for c in s.chars() {
        let n = c as i64;
        v[i] = if n < 256 { n as u8 } else { 63 };
        i += 1;
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
    pub fn kind(&self) -> K {
        match self {
            Obj::Null => K::Null,
            Obj::Bool(_) => K::Bool,
            Obj::Num { .. } => K::Num,
            Obj::Str(_) => K::Str,
            Obj::Name(_) => K::Name,
            Obj::Arr(_) => K::Arr,
            Obj::Dict { .. } => K::Dict,
            Obj::Stream { .. } => K::Stream,
            Obj::Ref { .. } => K::Ref,
            Obj::Cmd(_) => K::Cmd,
            Obj::Eof => K::Eof,
        }
    }

    pub fn is(&self, k: K) -> bool {
        self.kind() == k
    }

    pub fn is_null(&self) -> bool {
        match self {
            Obj::Null => true,
            Obj::Eof => true,
            _ => false,
        }
    }

    pub fn is_num(&self) -> bool {
        match self {
            Obj::Num { .. } => true,
            _ => false,
        }
    }

    /// A number written without a decimal point.
    pub fn is_int(&self) -> bool {
        match self {
            Obj::Num { int, .. } => *int,
            _ => false,
        }
    }

    pub fn is_dict(&self) -> bool {
        match self {
            Obj::Dict { .. } => true,
            Obj::Stream { .. } => true,
            _ => false,
        }
    }

    pub fn is_name(&self, s: &str) -> bool {
        match self {
            Obj::Name(n) => n == s,
            _ => false,
        }
    }

    pub fn is_cmd(&self, s: &str) -> bool {
        match self {
            Obj::Cmd(n) => n == s,
            _ => false,
        }
    }

    /// A number, or a boolean as 1 or 0.
    pub fn f(&self) -> f64 {
        match self {
            Obj::Num { n, .. } => *n,
            Obj::Bool(b) => {
                if *b {
                    1.0
                } else {
                    0.0
                }
            }
            _ => 0.0,
        }
    }

    /// A number (truncated toward zero), a boolean, or a reference's object number.
    pub fn i(&self) -> i64 {
        match self {
            Obj::Ref { num, .. } => *num,
            Obj::Num { n, .. } => {
                let v = *n;
                if v >= 0.0 {
                    return v.floor() as i64;
                }
                0 - ((0.0 - v).floor() as i64)
            }
            Obj::Bool(b) => {
                if *b {
                    1
                } else {
                    0
                }
            }
            _ => 0,
        }
    }

    /// A reference's (or a stream's object's) generation.
    pub fn gen(&self) -> i64 {
        match self {
            Obj::Ref { gen, .. } => *gen,
            Obj::Stream { gen, .. } => *gen,
            _ => 0,
        }
    }

    /// Array items, or dictionary values (parallel to `keys`).
    pub fn items(&self) -> &Vec<Obj> {
        match self {
            Obj::Arr(v) => v,
            Obj::Dict { items, .. } => items,
            Obj::Stream { items, .. } => items,
            _ => &NO_ITEMS,
        }
    }

    pub fn keys(&self) -> &Vec<String> {
        match self {
            Obj::Dict { keys, .. } => keys,
            Obj::Stream { keys, .. } => keys,
            _ => &NO_KEYS,
        }
    }

    /// A string's bytes, or a stream's raw (still encoded) data.
    pub fn raw(&self) -> &Vec<u8> {
        match self {
            Obj::Str(b) => b,
            Obj::Stream { data, .. } => data,
            _ => &NO_BYTES,
        }
    }

    /// A name or a keyword; "" for another kind.
    pub fn word(&self) -> &String {
        match self {
            Obj::Name(s) => s,
            Obj::Cmd(s) => s,
            _ => &NO_WORD,
        }
    }

    /// Replaces a string's bytes or a stream's data.
    pub fn set_raw(&mut self, b: Vec<u8>) {
        match self {
            Obj::Str(s) => *s = b,
            Obj::Stream { data, .. } => *data = b,
            _ => {}
        }
    }

    pub fn len(&self) -> usize {
        self.items().len()
    }

    pub fn item(&self, i: usize) -> Obj {
        let items = self.items();
        if i < items.len() {
            return items[i].clone();
        }
        null()
    }

    fn index_of(&self, key: &str) -> i64 {
        let keys = self.keys();
        let mut i: usize = 0;
        while i < keys.len() {
            if keys[i] == key {
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
            return self.items()[i as usize].clone();
        }
        null()
    }

    /// Sets `key` of a dictionary or stream; nothing on another kind.
    pub fn set(&mut self, key: &str, v: Obj) {
        match self {
            Obj::Dict { keys, items } => put(keys, items, key, v),
            Obj::Stream { keys, items, .. } => put(keys, items, key, v),
            _ => {}
        }
    }

    /// Appends to an array; nothing on another kind.
    pub fn push(&mut self, v: Obj) {
        match self {
            Obj::Arr(a) => a.push(v),
            _ => {}
        }
    }

    /// Replaces item `i` of an array, dictionary or stream.
    pub fn set_item(&mut self, i: usize, v: Obj) {
        match self {
            Obj::Arr(a) => a[i] = v,
            Obj::Dict { items, .. } => items[i] = v,
            Obj::Stream { items, .. } => items[i] = v,
            _ => {}
        }
    }

    /// The text of a string (Latin-1, or UTF-16BE after a byte-order mark).
    pub fn text(&self) -> String {
        let k = self.kind();
        if k == K::Name || k == K::Cmd {
            return self.word().clone();
        }
        let s = self.raw();
        if s.len() >= 2 && s[0] == 254 && s[1] == 255 {
            let mut out = String::new();
            let mut i: usize = 2;
            while i + 1 < s.len() {
                let mut u = ((s[i] as u32) << 8) | (s[i + 1] as u32);
                i += 2;
                if u >= 0xD800 && u < 0xDC00 && i + 1 < s.len() {
                    let lo = ((s[i] as u32) << 8) | (s[i + 1] as u32);
                    i += 2;
                    u = 0x10000 + ((u - 0xD800) << 10) + (lo - 0xDC00);
                }
                out.push(ch(u));
            }
            return out;
        }
        latin1(s)
    }

    /// A number array as floats.
    pub fn nums(&self) -> Vec<f64> {
        let mut v: Vec<f64> = vec![];
        for x in self.items().iter() {
            v.push(x.f());
        }
        v
    }
}
