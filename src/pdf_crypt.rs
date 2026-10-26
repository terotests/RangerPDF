//! The standard security handler (ISO 32000-1 §7.6, ISO 32000-2 §7.6.4):
//! revisions 2 to 6, RC4 and AES. The user password is tried first, then
//! the same text as the owner password.

use crate::pdf_hash;
use crate::pdf_hash::Aes;
use crate::pdf_obj;
use crate::pdf_obj::Obj;
use crate::pdf_obj::K;

const PAD: [u8; 32] = [0x28, 0xBF, 0x4E, 0x5E, 0x4E, 0x75, 0x8A, 0x41, 0x64, 0x00, 0x4E, 0x56, 0xFF, 0xFA, 0x01, 0x08, 0x2E, 0x2E, 0x00, 0xB6, 0xD0, 0x68, 0x3E, 0x80, 0x2F, 0x0C, 0xA9, 0xFE, 0x64, 0x53, 0x69, 0x7A];

pub struct Crypt {
    pub active: bool,
    pub error: String,
    key: Vec<u8>,
    /// "RC4", "AESV2", "AESV3" or "None" for strings and for streams
    str_method: String,
    stm_method: String,
    enc_num: i64,
    encrypt_metadata: bool,
    pub revision: i64,
}

fn padded(pw: &Vec<u8>) -> Vec<u8> {
    let mut v: Vec<u8> = vec![];
    for b in pw.iter() {
        if v.len() < 32 {
            v.push(*b);
        }
    }
    let mut i: usize = 0;
    while v.len() < 32 {
        v.push(PAD[i]);
        i += 1;
    }
    v
}

fn cat(a: &Vec<u8>, b: &Vec<u8>) -> Vec<u8> {
    let mut v = a.clone();
    for x in b.iter() {
        v.push(*x);
    }
    v
}

fn slice(a: &Vec<u8>, from: usize, to: usize) -> Vec<u8> {
    let mut v: Vec<u8> = vec![];
    let mut i = from;
    while i < to && i < a.len() {
        v.push(a[i]);
        i += 1;
    }
    v
}

fn same(a: &Vec<u8>, b: &Vec<u8>, n: usize) -> bool {
    if a.len() < n || b.len() < n {
        return false;
    }
    let mut i: usize = 0;
    while i < n {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

fn xor_key(k: &Vec<u8>, i: i64) -> Vec<u8> {
    let mut v: Vec<u8> = vec![];
    for b in k.iter() {
        v.push(((*b as i64) ^ i) as u8);
    }
    v
}

fn le32(p: i64) -> Vec<u8> {
    // /P is a signed 32-bit number; its bytes, little-endian
    let mut x: f64 = p as f64;
    if x < 0.0 {
        x = x + 4294967296.0;
    }
    let mut v: Vec<u8> = vec![];
    let mut k = 0;
    while k < 4 {
        let b = x - (x / 256.0).floor() * 256.0;
        v.push(b as u8);
        x = (x / 256.0).floor();
        k += 1;
    }
    v
}

fn utf8(s: &str) -> Vec<u8> {
    let mut v: Vec<u8> = vec![];
    for c in s.chars() {
        let u = c as u32;
        if u < 0x80 {
            v.push(u as u8);
        } else if u < 0x800 {
            v.push((0xC0 | (u >> 6)) as u8);
            v.push((0x80 | (u & 63)) as u8);
        } else if u < 0x10000 {
            v.push((0xE0 | (u >> 12)) as u8);
            v.push((0x80 | ((u >> 6) & 63)) as u8);
            v.push((0x80 | (u & 63)) as u8);
        } else {
            v.push((0xF0 | (u >> 18)) as u8);
            v.push((0x80 | ((u >> 12) & 63)) as u8);
            v.push((0x80 | ((u >> 6) & 63)) as u8);
            v.push((0x80 | (u & 63)) as u8);
        }
    }
    v
}

impl Crypt {
    pub fn none() -> Crypt {
        Crypt { active: false, error: String::new(), key: vec![], str_method: "None".to_string(), stm_method: "None".to_string(), enc_num: -1, encrypt_metadata: true, revision: 0 }
    }

    pub fn open(e: &Obj, id0: &Vec<u8>, password: &str, enc_num: i64) -> Crypt {
        let mut c = Crypt::none();
        c.enc_num = enc_num;
        let filter = e.at("Filter");
        if !filter.is_name("Standard") {
            c.error = format!("unsupported security handler {}", filter.name);
            return c;
        }
        let v = e.at("V").i();
        let r = e.at("R").i();
        c.revision = r;
        let em = e.at("EncryptMetadata");
        if em.k == K::Bool && em.n == 0.0 {
            c.encrypt_metadata = false;
        }
        let mut length = e.at("Length").i();
        if length == 0 {
            length = 40;
        }
        let mut method = "RC4".to_string();
        if v >= 4 {
            let cf = e.at("CF");
            let stmf = e.at("StmF");
            let strf = e.at("StrF");
            c.stm_method = Crypt::cf_method(&cf, &stmf);
            c.str_method = Crypt::cf_method(&cf, &strf);
            if c.stm_method == "AESV2" || c.str_method == "AESV2" {
                method = "AESV2".to_string();
                length = 128;
            }
            if c.stm_method == "AESV3" || c.str_method == "AESV3" {
                method = "AESV3".to_string();
            }
            let cfl = Crypt::cf_length(&cf, &stmf);
            if cfl > 0 && method == "RC4" {
                length = cfl;
            }
        } else {
            c.stm_method = "RC4".to_string();
            c.str_method = "RC4".to_string();
        }
        let o = e.at("O").s.clone();
        let u = e.at("U").s.clone();
        if r >= 5 {
            let pw = slice(&utf8(password), 0, 127);
            let oe = e.at("OE").s.clone();
            let ue = e.at("UE").s.clone();
            let u48 = slice(&u, 0, 48);
            let iv: Vec<u8> = vec![0; 16];
            let empty: Vec<u8> = vec![];
            if same(&Crypt::hash_r6(&pw, &slice(&u, 32, 40), &empty, r), &u, 32) {
                let k = Crypt::hash_r6(&pw, &slice(&u, 40, 48), &empty, r);
                c.key = Aes::new(&k).cbc_decrypt(&iv, &ue, false);
            } else if same(&Crypt::hash_r6(&pw, &slice(&o, 32, 40), &u48, r), &o, 32) {
                let k = Crypt::hash_r6(&pw, &slice(&o, 40, 48), &u48, r);
                c.key = Aes::new(&k).cbc_decrypt(&iv, &oe, false);
            } else {
                c.error = "password required".to_string();
                return c;
            }
            if c.stm_method == "None" && c.str_method == "None" {
                c.stm_method = "AESV3".to_string();
                c.str_method = "AESV3".to_string();
            }
            c.active = true;
            return c;
        }
        let n = if r == 2 { 5 } else { (length / 8) as usize };
        let p = e.at("P").i();
        let pw = pdf_obj::bytes_of(password);
        let mut key = Crypt::file_key(&padded(&pw), &o, p, id0, r, n, c.encrypt_metadata);
        if !Crypt::check_user(&key, &u, id0, r) {
            // the password as the owner password: it gives the user password
            let mut h = pdf_hash::md5(&padded(&pw));
            if r >= 3 {
                let mut i = 0;
                while i < 50 {
                    h = pdf_hash::md5(&h);
                    i += 1;
                }
            }
            let ok = slice(&h, 0, n);
            let mut user = o.clone();
            if r == 2 {
                user = pdf_hash::rc4(&ok, &user);
            } else {
                let mut i: i64 = 19;
                while i >= 0 {
                    user = pdf_hash::rc4(&xor_key(&ok, i), &user);
                    i -= 1;
                }
            }
            key = Crypt::file_key(&user, &o, p, id0, r, n, c.encrypt_metadata);
            if !Crypt::check_user(&key, &u, id0, r) {
                c.error = "password required".to_string();
                return c;
            }
        }
        c.key = key;
        c.active = true;
        c
    }

    fn cf_method(cf: &Obj, name: &Obj) -> String {
        if !name.is(K::Name) || name.name == "Identity" {
            return "None".to_string();
        }
        let f = cf.at(&name.name);
        let m = f.at("CFM");
        if m.is_name("AESV2") {
            return "AESV2".to_string();
        }
        if m.is_name("AESV3") {
            return "AESV3".to_string();
        }
        if m.is_name("None") {
            return "None".to_string();
        }
        "RC4".to_string()
    }

    fn cf_length(cf: &Obj, name: &Obj) -> i64 {
        if !name.is(K::Name) {
            return 0;
        }
        let l = cf.at(&name.name).at("Length").i();
        // some writers give the length in bytes
        if l > 0 && l <= 16 {
            return l * 8;
        }
        l
    }

    fn file_key(pw32: &Vec<u8>, o: &Vec<u8>, p: i64, id0: &Vec<u8>, r: i64, n: usize, meta: bool) -> Vec<u8> {
        let mut m = padded(pw32);
        m = cat(&m, &slice(o, 0, 32));
        m = cat(&m, &le32(p));
        m = cat(&m, id0);
        if r >= 4 && !meta {
            let ff: Vec<u8> = vec![255, 255, 255, 255];
            m = cat(&m, &ff);
        }
        let mut h = pdf_hash::md5(&m);
        if r >= 3 {
            let mut i = 0;
            while i < 50 {
                h = pdf_hash::md5(&slice(&h, 0, n));
                i += 1;
            }
        }
        slice(&h, 0, n)
    }

    fn check_user(key: &Vec<u8>, u: &Vec<u8>, id0: &Vec<u8>, r: i64) -> bool {
        let pad: Vec<u8> = padded(&vec![]);
        if r == 2 {
            let x = pdf_hash::rc4(key, &pad);
            return same(&x, u, 32);
        }
        let mut x = pdf_hash::rc4(key, &pdf_hash::md5(&cat(&pad, id0)));
        let mut i: i64 = 1;
        while i <= 19 {
            x = pdf_hash::rc4(&xor_key(key, i), &x);
            i += 1;
        }
        same(&x, u, 16)
    }

    /// The hash of revisions 5 (SHA-256) and 6 (ISO 32000-2 Algorithm 2.B).
    fn hash_r6(pw: &Vec<u8>, salt: &Vec<u8>, udata: &Vec<u8>, r: i64) -> Vec<u8> {
        let mut k = pdf_hash::sha256(&cat(&cat(pw, salt), udata));
        if r == 5 {
            return k;
        }
        let mut round: i64 = 0;
        loop {
            let one = cat(&cat(pw, &k), udata);
            let mut k1: Vec<u8> = vec![];
            let mut j = 0;
            while j < 64 {
                for b in one.iter() {
                    k1.push(*b);
                }
                j += 1;
            }
            let aes = Aes::new(&slice(&k, 0, 16));
            let e = aes.cbc_encrypt(&slice(&k, 16, 32), &k1);
            let mut sum: i64 = 0;
            let mut i: usize = 0;
            while i < 16 {
                sum += e[i] as i64;
                i += 1;
            }
            let m = sum % 3;
            if m == 0 {
                k = pdf_hash::sha256(&e);
            } else if m == 1 {
                k = pdf_hash::sha384(&e);
            } else {
                k = pdf_hash::sha512(&e);
            }
            let last = e[e.len() - 1] as i64;
            if round >= 63 && last <= round - 32 {
                break;
            }
            round += 1;
            if round > 1000 {
                break;
            }
        }
        slice(&k, 0, 32)
    }

    fn obj_key(&self, num: i64, gen: i64, aes: bool) -> Vec<u8> {
        if self.revision >= 5 {
            return self.key.clone();
        }
        let mut m = self.key.clone();
        m.push((num & 255) as u8);
        m.push(((num >> 8) & 255) as u8);
        m.push(((num >> 16) & 255) as u8);
        m.push((gen & 255) as u8);
        m.push(((gen >> 8) & 255) as u8);
        if aes {
            let salt: Vec<u8> = vec![0x73, 0x41, 0x6C, 0x54];
            m = cat(&m, &salt);
        }
        let h = pdf_hash::md5(&m);
        let mut n = self.key.len() + 5;
        if n > 16 {
            n = 16;
        }
        slice(&h, 0, n)
    }

    pub fn decrypt_bytes(&self, data: &Vec<u8>, num: i64, gen: i64, method: &str) -> Vec<u8> {
        if method == "None" {
            return data.clone();
        }
        if method == "RC4" {
            let k = self.obj_key(num, gen, false);
            return pdf_hash::rc4(&k, data);
        }
        if data.len() < 16 {
            return vec![];
        }
        let k = self.obj_key(num, gen, true);
        let aes = Aes::new(&k);
        aes.cbc_decrypt(&slice(data, 0, 16), &slice(data, 16, data.len()), true)
    }

    fn strings(&self, o: Obj, num: i64, gen: i64) -> Obj {
        let mut r = o;
        if r.k == K::Str {
            let m = self.str_method.clone();
            r.s = self.decrypt_bytes(&r.s, num, gen, &m);
            return r;
        }
        if r.k == K::Arr || r.k == K::Dict || r.k == K::Stream {
            let mut items: Vec<Obj> = vec![];
            for x in r.items.iter() {
                items.push(self.strings(x.clone(), num, gen));
            }
            r.items = items;
        }
        r
    }

    /// Object `num` as read from the file, with its strings and stream data decrypted.
    pub fn decrypt_obj(&self, o: Obj, num: i64, gen: i64) -> Obj {
        if !self.active || num == self.enc_num {
            return o;
        }
        if o.k == K::Stream {
            if o.at("Type").is_name("XRef") {
                return o;
            }
            if o.at("Type").is_name("Metadata") && !self.encrypt_metadata {
                return o;
            }
            let data = o.s.clone();
            let mut r = self.strings(o, num, gen);
            let m = self.stm_method.clone();
            r.s = self.decrypt_bytes(&data, num, gen, &m);
            return r;
        }
        self.strings(o, num, gen)
    }
}
