//! MD5, SHA-256, SHA-384/512, RC4 and AES, for the standard security
//! handler (crypt.rs). A 32-bit word is kept in an f64: JavaScript's bit
//! operators are 32-bit signed, so the
//! bit operations are done on 16-bit halves, which every target computes
//! the same.

const T32: f64 = 4294967296.0;

fn halves(x: f64) -> Vec<i64> {
    let hi = (x / 65536.0).floor();
    let lo = x - hi * 65536.0;
    vec![hi as i64, lo as i64]
}

fn join(hi: i64, lo: i64) -> f64 {
    (hi as f64) * 65536.0 + (lo as f64)
}

pub fn add(a: f64, b: f64) -> f64 {
    let mut s = a + b;
    while s >= T32 {
        s -= T32;
    }
    s
}

pub fn xor(a: f64, b: f64) -> f64 {
    let x = halves(a);
    let y = halves(b);
    join(x[0] ^ y[0], x[1] ^ y[1])
}

pub fn and(a: f64, b: f64) -> f64 {
    let x = halves(a);
    let y = halves(b);
    join(x[0] & y[0], x[1] & y[1])
}

pub fn or(a: f64, b: f64) -> f64 {
    let x = halves(a);
    let y = halves(b);
    join(x[0] | y[0], x[1] | y[1])
}

pub fn not(a: f64) -> f64 {
    (T32 - 1.0) - a
}

fn pow2(n: i64) -> f64 {
    let mut p = 1.0;
    let mut i = 0;
    while i < n {
        p = p * 2.0;
        i += 1;
    }
    p
}

pub fn shr(x: f64, n: i64) -> f64 {
    (x / pow2(n)).floor()
}

pub fn shl(x: f64, n: i64) -> f64 {
    let p = pow2(32 - n);
    let hi = (x / p).floor();
    (x - hi * p) * pow2(n)
}

pub fn rotl(x: f64, n: i64) -> f64 {
    let n2 = n % 32;
    if n2 == 0 {
        return x;
    }
    shl(x, n2) + shr(x, 32 - n2)
}

pub fn rotr(x: f64, n: i64) -> f64 {
    rotl(x, 32 - (n % 32))
}

fn word_le(d: &Vec<u8>, i: usize) -> f64 {
    (d[i] as f64) + (d[i + 1] as f64) * 256.0 + (d[i + 2] as f64) * 65536.0 + (d[i + 3] as f64) * 16777216.0
}

fn word_be(d: &Vec<u8>, i: usize) -> f64 {
    (d[i + 3] as f64) + (d[i + 2] as f64) * 256.0 + (d[i + 1] as f64) * 65536.0 + (d[i] as f64) * 16777216.0
}

/// `w` as 4 little-endian bytes. Helpers return new bytes: a Ranger buffer
/// (`Vec<u8>`) cannot grow through a `&mut` parameter.
fn le_bytes(w: f64) -> Vec<u8> {
    let mut out: Vec<u8> = vec![];
    let mut x = w;
    let mut k = 0;
    while k < 4 {
        let b = x - (x / 256.0).floor() * 256.0;
        out.push(b as u8);
        x = (x / 256.0).floor();
        k += 1;
    }
    out
}

fn be_bytes(w: f64) -> Vec<u8> {
    let t = le_bytes(w);
    vec![t[3], t[2], t[1], t[0]]
}

/// The message padded to 64-byte blocks (MD5 and SHA-256), the bit length
/// little- or big-endian.
fn pad64(m: &Vec<u8>, little: bool) -> Vec<u8> {
    let mut d = m.clone();
    d.push(128);
    while d.len() % 64 != 56 {
        d.push(0);
    }
    let bits = (m.len() as f64) * 8.0;
    let lo = bits - (bits / T32).floor() * T32;
    let hi = (bits / T32).floor();
    if little {
        for b in le_bytes(lo).iter() {
            d.push(*b);
        }
        for b in le_bytes(hi).iter() {
            d.push(*b);
        }
    } else {
        for b in be_bytes(hi).iter() {
            d.push(*b);
        }
        for b in be_bytes(lo).iter() {
            d.push(*b);
        }
    }
    d
}

const MD5_S: [i64; 64] = [7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21];

pub fn md5(m: &Vec<u8>) -> Vec<u8> {
    let mut kt: Vec<f64> = vec![];
    let mut i: i64 = 0;
    while i < 64 {
        let s = ((i + 1) as f64).sin();
        let a = if s < 0.0 { 0.0 - s } else { s };
        kt.push((a * T32).floor());
        i += 1;
    }
    let d = pad64(m, true);
    let mut a0: f64 = 1732584193.0;
    let mut b0: f64 = 4023233417.0;
    let mut c0: f64 = 2562383102.0;
    let mut d0: f64 = 271733878.0;
    let mut off: usize = 0;
    while off < d.len() {
        let mut w: Vec<f64> = vec![];
        let mut j: usize = 0;
        while j < 16 {
            w.push(word_le(&d, off + j * 4));
            j += 1;
        }
        let mut a = a0;
        let mut b = b0;
        let mut c = c0;
        let mut dd = d0;
        let mut r: usize = 0;
        while r < 64 {
            let mut f: f64 = 0.0;
            let mut g: usize = 0;
            if r < 16 {
                f = or(and(b, c), and(not(b), dd));
                g = r;
            } else if r < 32 {
                f = or(and(dd, b), and(not(dd), c));
                g = (5 * r + 1) % 16;
            } else if r < 48 {
                f = xor(xor(b, c), dd);
                g = (3 * r + 5) % 16;
            } else {
                f = xor(c, or(b, not(dd)));
                g = (7 * r) % 16;
            }
            let f2 = add(add(add(f, a), kt[r]), w[g]);
            a = dd;
            dd = c;
            c = b;
            b = add(b, rotl(f2, MD5_S[r]));
            r += 1;
        }
        a0 = add(a0, a);
        b0 = add(b0, b);
        c0 = add(c0, c);
        d0 = add(d0, dd);
        off += 64;
    }
    let mut out: Vec<u8> = vec![];
    for b in le_bytes(a0).iter() {
        out.push(*b);
    }
    for b in le_bytes(b0).iter() {
        out.push(*b);
    }
    for b in le_bytes(c0).iter() {
        out.push(*b);
    }
    for b in le_bytes(d0).iter() {
        out.push(*b);
    }
    out
}

fn primes(n: usize) -> Vec<f64> {
    let mut p: Vec<f64> = vec![];
    let mut x: i64 = 2;
    while p.len() < n {
        let mut prime = true;
        let mut k: i64 = 2;
        while k * k <= x {
            if x % k == 0 {
                prime = false;
                break;
            }
            k += 1;
        }
        if prime {
            p.push(x as f64);
        }
        x += 1;
    }
    p
}

/// The first 32 bits of the fractional part of x^(1/root) (x a prime):
/// SHA-256's constants, computed rather than listed.
fn frac32(x: f64, root: f64) -> f64 {
    let mut r = x.sqrt();
    if root == 3.0 {
        // Newton's method: cbrt is not the same on every target
        r = x / 3.0;
        let mut i = 0;
        while i < 100 {
            r = r - (r * r * r - x) / (3.0 * r * r);
            i += 1;
        }
    }
    let f = r - r.floor();
    (f * T32).floor()
}

pub fn sha256(m: &Vec<u8>) -> Vec<u8> {
    let ps = primes(64);
    let mut k: Vec<f64> = vec![];
    for p in ps.iter() {
        k.push(frac32(*p, 3.0));
    }
    let mut h: Vec<f64> = vec![];
    let mut i: usize = 0;
    while i < 8 {
        h.push(frac32(ps[i], 2.0));
        i += 1;
    }
    let d = pad64(m, false);
    let mut off: usize = 0;
    while off < d.len() {
        let mut w: Vec<f64> = vec![];
        let mut j: usize = 0;
        while j < 64 {
            if j < 16 {
                w.push(word_be(&d, off + j * 4));
            } else {
                let x = w[j - 15];
                let s0 = xor(xor(rotr(x, 7), rotr(x, 18)), shr(x, 3));
                let y = w[j - 2];
                let s1 = xor(xor(rotr(y, 17), rotr(y, 19)), shr(y, 10));
                w.push(add(add(add(w[j - 16], s0), w[j - 7]), s1));
            }
            j += 1;
        }
        let mut a = h[0];
        let mut b = h[1];
        let mut c = h[2];
        let mut dd = h[3];
        let mut e = h[4];
        let mut f = h[5];
        let mut g = h[6];
        let mut hh = h[7];
        let mut r: usize = 0;
        while r < 64 {
            let s1 = xor(xor(rotr(e, 6), rotr(e, 11)), rotr(e, 25));
            let ch = xor(and(e, f), and(not(e), g));
            let t1 = add(add(add(add(hh, s1), ch), k[r]), w[r]);
            let s0 = xor(xor(rotr(a, 2), rotr(a, 13)), rotr(a, 22));
            let mj = xor(xor(and(a, b), and(a, c)), and(b, c));
            let t2 = add(s0, mj);
            hh = g;
            g = f;
            f = e;
            e = add(dd, t1);
            dd = c;
            c = b;
            b = a;
            a = add(t1, t2);
            r += 1;
        }
        h[0] = add(h[0], a);
        h[1] = add(h[1], b);
        h[2] = add(h[2], c);
        h[3] = add(h[3], dd);
        h[4] = add(h[4], e);
        h[5] = add(h[5], f);
        h[6] = add(h[6], g);
        h[7] = add(h[7], hh);
        off += 64;
    }
    let mut out: Vec<u8> = vec![];
    for x in h.iter() {
        for b in be_bytes(*x).iter() {
            out.push(*b);
        }
    }
    out
}

// SHA-512 on 64-bit words as (hi, lo) pairs of 32-bit words

fn add64(ah: f64, al: f64, bh: f64, bl: f64, out: &mut Vec<f64>) {
    let mut l = al + bl;
    let mut carry = 0.0;
    if l >= T32 {
        l -= T32;
        carry = 1.0;
    }
    let h = add(add(ah, bh), carry);
    out.push(h);
    out.push(l);
}

/// rotate right a 64-bit word (hi, lo) by n
fn rotr64(h: f64, l: f64, n: i64, out: &mut Vec<f64>) {
    if n < 32 {
        if n == 0 {
            out.push(h);
            out.push(l);
            return;
        }
        out.push(or(shr(h, n), shl(l, 32 - n)));
        out.push(or(shr(l, n), shl(h, 32 - n)));
    } else {
        let m = n - 32;
        if m == 0 {
            out.push(l);
            out.push(h);
            return;
        }
        out.push(or(shr(l, m), shl(h, 32 - m)));
        out.push(or(shr(h, m), shl(l, 32 - m)));
    }
}

fn shr64(h: f64, l: f64, n: i64, out: &mut Vec<f64>) {
    out.push(shr(h, n));
    out.push(or(shr(l, n), shl(h, 32 - n)));
}

/// The fractional part of x^(1/root) to 64 bits, as (hi, lo): computed
/// exactly with integer square and cube roots on 16-bit limbs would be long,
/// so the constants are listed (FIPS 180-4 §4.2.3, §5.3.4, §5.3.5).
const K512: [&str; 80] = [
    "428a2f98d728ae22", "7137449123ef65cd", "b5c0fbcfec4d3b2f", "e9b5dba58189dbbc", "3956c25bf348b538", "59f111f1b605d019", "923f82a4af194f9b", "ab1c5ed5da6d8118",
    "d807aa98a3030242", "12835b0145706fbe", "243185be4ee4b28c", "550c7dc3d5ffb4e2", "72be5d74f27b896f", "80deb1fe3b1696b1", "9bdc06a725c71235", "c19bf174cf692694",
    "e49b69c19ef14ad2", "efbe4786384f25e3", "0fc19dc68b8cd5b5", "240ca1cc77ac9c65", "2de92c6f592b0275", "4a7484aa6ea6e483", "5cb0a9dcbd41fbd4", "76f988da831153b5",
    "983e5152ee66dfab", "a831c66d2db43210", "b00327c898fb213f", "bf597fc7beef0ee4", "c6e00bf33da88fc2", "d5a79147930aa725", "06ca6351e003826f", "142929670a0e6e70",
    "27b70a8546d22ffc", "2e1b21385c26c926", "4d2c6dfc5ac42aed", "53380d139d95b3df", "650a73548baf63de", "766a0abb3c77b2a8", "81c2c92e47edaee6", "92722c851482353b",
    "a2bfe8a14cf10364", "a81a664bbc423001", "c24b8b70d0f89791", "c76c51a30654be30", "d192e819d6ef5218", "d69906245565a910", "f40e35855771202a", "106aa07032bbd1b8",
    "19a4c116b8d2d0c8", "1e376c085141ab53", "2748774cdf8eeb99", "34b0bcb5e19b48a8", "391c0cb3c5c95a63", "4ed8aa4ae3418acb", "5b9cca4f7763e373", "682e6ff3d6b2b8a3",
    "748f82ee5defb2fc", "78a5636f43172f60", "84c87814a1f0ab72", "8cc702081a6439ec", "90befffa23631e28", "a4506cebde82bde9", "bef9a3f7b2c67915", "c67178f2e372532b",
    "ca273eceea26619c", "d186b8c721c0c207", "eada7dd6cde0eb1e", "f57d4f7fee6ed178", "06f067aa72176fba", "0a637dc5a2c898a6", "113f9804bef90dae", "1b710b35131c471b",
    "28db77f523047d84", "32caab7b40c72493", "3c9ebe0a15c9bebc", "431d67c49c100d4c", "4cc5d4becb3e42b6", "597f299cfc657e2a", "5fcb6fab3ad6faec", "6c44198c4a475817",
];

const IV512: [&str; 8] = ["6a09e667f3bcc908", "bb67ae8584caa73b", "3c6ef372fe94f82b", "a54ff53a5f1d36f1", "510e527fade682d1", "9b05688c2b3e6c1f", "1f83d9abfb41bd6b", "5be0cd19137e2179"];
const IV384: [&str; 8] = ["cbbb9d5dc1059ed8", "629a292a367cd507", "9159015a3070dd17", "152fecd8f70e5939", "67332667ffc00b31", "8eb44a8768581511", "db0c2e0d64f98fa7", "47b5481dbefa4fa4"];

fn hex32(s: &str, from: usize) -> f64 {
    let cs: Vec<char> = s.chars().collect();
    let mut v: f64 = 0.0;
    let mut i = from;
    while i < from + 8 {
        let c = cs[i] as u8;
        v = v * 16.0 + (crate::pdf_lexer::hexval(c) as f64);
        i += 1;
    }
    v
}

fn sha512_core(m: &Vec<u8>, iv: &Vec<String>, out_words: usize) -> Vec<u8> {
    let mut kh: Vec<f64> = vec![];
    let mut kl: Vec<f64> = vec![];
    for s in K512.iter() {
        kh.push(hex32(s, 0));
        kl.push(hex32(s, 8));
    }
    let mut hh: Vec<f64> = vec![];
    let mut hl: Vec<f64> = vec![];
    for s in iv.iter() {
        hh.push(hex32(s, 0));
        hl.push(hex32(s, 8));
    }
    let mut d = m.clone();
    d.push(128);
    while d.len() % 128 != 112 {
        d.push(0);
    }
    let bits = (m.len() as f64) * 8.0;
    for b in be_bytes(0.0).iter() {
        d.push(*b);
    }
    for b in be_bytes(0.0).iter() {
        d.push(*b);
    }
    for b in be_bytes((bits / T32).floor()).iter() {
        d.push(*b);
    }
    for b in be_bytes(bits - (bits / T32).floor() * T32).iter() {
        d.push(*b);
    }
    let mut off: usize = 0;
    while off < d.len() {
        let mut wh: Vec<f64> = vec![];
        let mut wl: Vec<f64> = vec![];
        let mut j: usize = 0;
        while j < 80 {
            if j < 16 {
                wh.push(word_be(&d, off + j * 8));
                wl.push(word_be(&d, off + j * 8 + 4));
            } else {
                let mut t: Vec<f64> = vec![];
                rotr64(wh[j - 15], wl[j - 15], 1, &mut t);
                rotr64(wh[j - 15], wl[j - 15], 8, &mut t);
                shr64(wh[j - 15], wl[j - 15], 7, &mut t);
                let s0h = xor(xor(t[0], t[2]), t[4]);
                let s0l = xor(xor(t[1], t[3]), t[5]);
                let mut u: Vec<f64> = vec![];
                rotr64(wh[j - 2], wl[j - 2], 19, &mut u);
                rotr64(wh[j - 2], wl[j - 2], 61, &mut u);
                shr64(wh[j - 2], wl[j - 2], 6, &mut u);
                let s1h = xor(xor(u[0], u[2]), u[4]);
                let s1l = xor(xor(u[1], u[3]), u[5]);
                let mut r: Vec<f64> = vec![];
                add64(wh[j - 16], wl[j - 16], s0h, s0l, &mut r);
                add64(r[0], r[1], wh[j - 7], wl[j - 7], &mut r);
                add64(r[2], r[3], s1h, s1l, &mut r);
                wh.push(r[4]);
                wl.push(r[5]);
            }
            j += 1;
        }
        let mut ah = hh.clone();
        let mut al = hl.clone();
        let mut i: usize = 0;
        while i < 80 {
            // ah/al: a b c d e f g h
            let mut t: Vec<f64> = vec![];
            rotr64(ah[4], al[4], 14, &mut t);
            rotr64(ah[4], al[4], 18, &mut t);
            rotr64(ah[4], al[4], 41, &mut t);
            let s1h = xor(xor(t[0], t[2]), t[4]);
            let s1l = xor(xor(t[1], t[3]), t[5]);
            let chh = xor(and(ah[4], ah[5]), and(not(ah[4]), ah[6]));
            let chl = xor(and(al[4], al[5]), and(not(al[4]), al[6]));
            let mut r: Vec<f64> = vec![];
            add64(ah[7], al[7], s1h, s1l, &mut r);
            add64(r[0], r[1], chh, chl, &mut r);
            add64(r[2], r[3], kh[i], kl[i], &mut r);
            add64(r[4], r[5], wh[i], wl[i], &mut r);
            let t1h = r[6];
            let t1l = r[7];
            let mut u: Vec<f64> = vec![];
            rotr64(ah[0], al[0], 28, &mut u);
            rotr64(ah[0], al[0], 34, &mut u);
            rotr64(ah[0], al[0], 39, &mut u);
            let s0h = xor(xor(u[0], u[2]), u[4]);
            let s0l = xor(xor(u[1], u[3]), u[5]);
            let mjh = xor(xor(and(ah[0], ah[1]), and(ah[0], ah[2])), and(ah[1], ah[2]));
            let mjl = xor(xor(and(al[0], al[1]), and(al[0], al[2])), and(al[1], al[2]));
            let mut q: Vec<f64> = vec![];
            add64(s0h, s0l, mjh, mjl, &mut q);
            add64(ah[3], al[3], t1h, t1l, &mut q);
            add64(t1h, t1l, q[0], q[1], &mut q);
            ah[7] = ah[6];
            al[7] = al[6];
            ah[6] = ah[5];
            al[6] = al[5];
            ah[5] = ah[4];
            al[5] = al[4];
            ah[4] = q[2];
            al[4] = q[3];
            ah[3] = ah[2];
            al[3] = al[2];
            ah[2] = ah[1];
            al[2] = al[1];
            ah[1] = ah[0];
            al[1] = al[0];
            ah[0] = q[4];
            al[0] = q[5];
            i += 1;
        }
        let mut k: usize = 0;
        while k < 8 {
            let mut r: Vec<f64> = vec![];
            add64(hh[k], hl[k], ah[k], al[k], &mut r);
            hh[k] = r[0];
            hl[k] = r[1];
            k += 1;
        }
        off += 128;
    }
    let mut out: Vec<u8> = vec![];
    let mut k: usize = 0;
    while k < out_words {
        for b in be_bytes(hh[k]).iter() {
            out.push(*b);
        }
        for b in be_bytes(hl[k]).iter() {
            out.push(*b);
        }
        k += 1;
    }
    out
}

pub fn sha512(m: &Vec<u8>) -> Vec<u8> {
    let mut iv: Vec<String> = vec![];
    for s in IV512.iter() {
        iv.push(s.to_string());
    }
    sha512_core(m, &iv, 8)
}

pub fn sha384(m: &Vec<u8>) -> Vec<u8> {
    let mut iv: Vec<String> = vec![];
    for s in IV384.iter() {
        iv.push(s.to_string());
    }
    sha512_core(m, &iv, 6)
}

pub fn rc4(key: &Vec<u8>, data: &Vec<u8>) -> Vec<u8> {
    let mut s: Vec<i64> = vec![];
    let mut i: i64 = 0;
    while i < 256 {
        s.push(i);
        i += 1;
    }
    if key.is_empty() {
        return data.clone();
    }
    let mut j: i64 = 0;
    let mut k: usize = 0;
    while k < 256 {
        j = (j + s[k] + (key[k % key.len()] as i64)) & 255;
        let t = s[k];
        s[k] = s[j as usize];
        s[j as usize] = t;
        k += 1;
    }
    let mut out: Vec<u8> = vec![];
    let mut a: i64 = 0;
    let mut b: i64 = 0;
    for x in data.iter() {
        a = (a + 1) & 255;
        b = (b + s[a as usize]) & 255;
        let t = s[a as usize];
        s[a as usize] = s[b as usize];
        s[b as usize] = t;
        let kk = s[((s[a as usize] + s[b as usize]) & 255) as usize];
        out.push(((*x as i64) ^ kk) as u8);
    }
    out
}

// AES (FIPS 197)

pub struct Aes {
    sbox: Vec<i64>,
    inv: Vec<i64>,
    /// the round keys, 4 bytes per word
    w: Vec<i64>,
    rounds: usize,
}

fn xtime(x: i64) -> i64 {
    let y = x << 1;
    if (y & 256) != 0 {
        return (y ^ 0x11b) & 255;
    }
    y
}

fn gmul(a: i64, b: i64) -> i64 {
    let mut p: i64 = 0;
    let mut x = a;
    let mut y = b;
    while y > 0 {
        if (y & 1) != 0 {
            p = p ^ x;
        }
        x = xtime(x);
        y = y >> 1;
    }
    p
}

impl Aes {
    pub fn new(key: &Vec<u8>) -> Aes {
        // the S-box from the multiplicative inverse and the affine map
        let mut sbox: Vec<i64> = vec![0; 256];
        let mut inv: Vec<i64> = vec![0; 256];
        // inverses through the powers of the generator 3
        let mut exp: Vec<i64> = vec![0; 256];
        let mut log: Vec<i64> = vec![0; 256];
        let mut p: i64 = 1;
        let mut e: usize = 0;
        while e < 255 {
            exp[e] = p;
            log[p as usize] = e as i64;
            p = p ^ xtime(p);
            e += 1;
        }
        let mut x: i64 = 0;
        while x < 256 {
            let mut v: i64 = 0;
            if x != 0 {
                v = exp[((255 - log[x as usize]) % 255) as usize];
            }
            let mut s = v;
            let mut r = v;
            let mut k = 0;
            while k < 4 {
                r = ((r << 1) | (r >> 7)) & 255;
                s = s ^ r;
                k += 1;
            }
            s = s ^ 0x63;
            sbox[x as usize] = s;
            inv[s as usize] = x;
            x += 1;
        }
        let nk = key.len() / 4;
        let rounds = nk + 6;
        let mut w: Vec<i64> = vec![];
        for b in key.iter() {
            w.push(*b as i64);
        }
        let mut rcon: i64 = 1;
        let mut i = nk;
        while i < 4 * (rounds + 1) {
            let mut t: Vec<i64> = vec![w[(i - 1) * 4], w[(i - 1) * 4 + 1], w[(i - 1) * 4 + 2], w[(i - 1) * 4 + 3]];
            if i % nk == 0 {
                let t0 = t[0];
                t[0] = sbox[t[1] as usize] ^ rcon;
                t[1] = sbox[t[2] as usize];
                t[2] = sbox[t[3] as usize];
                t[3] = sbox[t0 as usize];
                rcon = xtime(rcon);
            } else if nk > 6 && i % nk == 4 {
                let mut k: usize = 0;
                while k < 4 {
                    t[k] = sbox[t[k] as usize];
                    k += 1;
                }
            }
            let mut k: usize = 0;
            while k < 4 {
                let v = w[(i - nk) * 4 + k] ^ t[k];
                w.push(v);
                k += 1;
            }
            i += 1;
        }
        Aes { sbox: sbox, inv: inv, w: w, rounds: rounds }
    }

    fn add_key(&self, s: &mut Vec<i64>, round: usize) {
        let mut k: usize = 0;
        while k < 16 {
            s[k] = s[k] ^ self.w[round * 16 + k];
            k += 1;
        }
    }

    pub fn encrypt_block(&self, input: &Vec<i64>) -> Vec<i64> {
        let mut s = input.clone();
        self.add_key(&mut s, 0);
        let mut r: usize = 1;
        while r <= self.rounds {
            let mut t: Vec<i64> = vec![0; 16];
            // SubBytes + ShiftRows
            let mut c: usize = 0;
            while c < 4 {
                let mut row: usize = 0;
                while row < 4 {
                    t[c * 4 + row] = self.sbox[s[((c + row) % 4) * 4 + row] as usize];
                    row += 1;
                }
                c += 1;
            }
            if r < self.rounds {
                let mut c2: usize = 0;
                while c2 < 4 {
                    let a0 = t[c2 * 4];
                    let a1 = t[c2 * 4 + 1];
                    let a2 = t[c2 * 4 + 2];
                    let a3 = t[c2 * 4 + 3];
                    t[c2 * 4] = xtime(a0) ^ (xtime(a1) ^ a1) ^ a2 ^ a3;
                    t[c2 * 4 + 1] = a0 ^ xtime(a1) ^ (xtime(a2) ^ a2) ^ a3;
                    t[c2 * 4 + 2] = a0 ^ a1 ^ xtime(a2) ^ (xtime(a3) ^ a3);
                    t[c2 * 4 + 3] = (xtime(a0) ^ a0) ^ a1 ^ a2 ^ xtime(a3);
                    c2 += 1;
                }
            }
            s = t;
            self.add_key(&mut s, r);
            r += 1;
        }
        s
    }

    pub fn decrypt_block(&self, input: &Vec<i64>) -> Vec<i64> {
        let mut s = input.clone();
        self.add_key(&mut s, self.rounds);
        let mut r = self.rounds;
        while r > 0 {
            r -= 1;
            let mut t: Vec<i64> = vec![0; 16];
            // InvShiftRows + InvSubBytes
            let mut c: usize = 0;
            while c < 4 {
                let mut row: usize = 0;
                while row < 4 {
                    t[((c + row) % 4) * 4 + row] = self.inv[s[c * 4 + row] as usize];
                    row += 1;
                }
                c += 1;
            }
            s = t;
            self.add_key(&mut s, r);
            if r > 0 {
                let mut c2: usize = 0;
                while c2 < 4 {
                    let a0 = s[c2 * 4];
                    let a1 = s[c2 * 4 + 1];
                    let a2 = s[c2 * 4 + 2];
                    let a3 = s[c2 * 4 + 3];
                    s[c2 * 4] = gmul(a0, 14) ^ gmul(a1, 11) ^ gmul(a2, 13) ^ gmul(a3, 9);
                    s[c2 * 4 + 1] = gmul(a0, 9) ^ gmul(a1, 14) ^ gmul(a2, 11) ^ gmul(a3, 13);
                    s[c2 * 4 + 2] = gmul(a0, 13) ^ gmul(a1, 9) ^ gmul(a2, 14) ^ gmul(a3, 11);
                    s[c2 * 4 + 3] = gmul(a0, 11) ^ gmul(a1, 13) ^ gmul(a2, 9) ^ gmul(a3, 14);
                    c2 += 1;
                }
            }
        }
        s
    }

    /// CBC decryption; with `unpad` the PKCS#5 padding is removed.
    pub fn cbc_decrypt(&self, iv: &Vec<u8>, data: &Vec<u8>, unpad: bool) -> Vec<u8> {
        let mut out: Vec<u8> = vec![];
        let mut prev: Vec<i64> = vec![];
        for b in iv.iter() {
            prev.push(*b as i64);
        }
        while prev.len() < 16 {
            prev.push(0);
        }
        let mut off: usize = 0;
        while off + 16 <= data.len() {
            let mut blk: Vec<i64> = vec![];
            let mut k: usize = 0;
            while k < 16 {
                blk.push(data[off + k] as i64);
                k += 1;
            }
            let p = self.decrypt_block(&blk);
            let mut k2: usize = 0;
            while k2 < 16 {
                out.push(((p[k2] ^ prev[k2]) & 255) as u8);
                k2 += 1;
            }
            prev = blk;
            off += 16;
        }
        if unpad && !out.is_empty() {
            let n = out[out.len() - 1] as usize;
            if n >= 1 && n <= 16 && n <= out.len() {
                let keep = out.len() - n;
                out.truncate(keep);
            }
        }
        out
    }

    /// CBC encryption without padding (the data is whole blocks).
    pub fn cbc_encrypt(&self, iv: &Vec<u8>, data: &Vec<u8>) -> Vec<u8> {
        let mut out: Vec<u8> = vec![];
        let mut prev: Vec<i64> = vec![];
        for b in iv.iter() {
            prev.push(*b as i64);
        }
        let mut off: usize = 0;
        while off + 16 <= data.len() {
            let mut blk: Vec<i64> = vec![];
            let mut k: usize = 0;
            while k < 16 {
                blk.push((data[off + k] as i64) ^ prev[k]);
                k += 1;
            }
            let c = self.encrypt_block(&blk);
            for v in c.iter() {
                out.push(*v as u8);
            }
            prev = c;
            off += 16;
        }
        out
    }
}
