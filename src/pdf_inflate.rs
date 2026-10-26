//! Inflate (RFC 1951) with the zlib framing (RFC 1950) PDF's /FlateDecode
//! uses. A damaged stream gives what was decoded before the damage, as
//! readers do; `ok` tells whether it ended properly.

pub struct Inflated {
    pub data: Vec<u8>,
    pub ok: bool,
}

struct Bits {
    pos: usize,
    bit: i64,
    nbits: i64,
    eof: bool,
}

/// A canonical Huffman code: `counts[len]` codes of each length, `symbols`
/// in code order.
struct Huff {
    counts: Vec<i64>,
    symbols: Vec<i64>,
}

fn need(b: &mut Bits, src: &Vec<u8>, n: i64) -> i64 {
    while b.nbits < n {
        if b.pos >= src.len() {
            b.eof = true;
            return 0;
        }
        b.bit = b.bit | ((src[b.pos] as i64) << b.nbits);
        b.pos += 1;
        b.nbits += 8;
    }
    let v = b.bit & ((1 << n) - 1);
    b.bit = b.bit >> n;
    b.nbits -= n;
    v
}

fn build(lengths: &Vec<i64>) -> Huff {
    let mut counts: Vec<i64> = vec![0; 16];
    for l in lengths.iter() {
        counts[*l as usize] += 1;
    }
    counts[0] = 0;
    let mut offs: Vec<i64> = vec![0; 16];
    let mut i: usize = 1;
    while i < 16 {
        offs[i] = offs[i - 1] + counts[i - 1];
        i += 1;
    }
    let mut symbols: Vec<i64> = vec![0; lengths.len()];
    let mut s: usize = 0;
    while s < lengths.len() {
        let l = lengths[s] as usize;
        if l != 0 {
            symbols[offs[l] as usize] = s as i64;
            offs[l] += 1;
        }
        s += 1;
    }
    Huff { counts: counts, symbols: symbols }
}

fn decode_sym(b: &mut Bits, src: &Vec<u8>, h: &Huff) -> i64 {
    let mut code: i64 = 0;
    let mut first: i64 = 0;
    let mut index: i64 = 0;
    let mut len: usize = 1;
    while len < 16 {
        code = code | need(b, src, 1);
        if b.eof {
            return -1;
        }
        let count = h.counts[len];
        if code - count < first {
            return h.symbols[(index + (code - first)) as usize];
        }
        index += count;
        first += count;
        first = first << 1;
        code = code << 1;
        len += 1;
    }
    -1
}

const LBASE: [i64; 29] = [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
const LEXT: [i64; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const DBASE: [i64; 30] = [1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577];
const DEXT: [i64; 30] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];
const CLORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];

fn codes(b: &mut Bits, src: &Vec<u8>, out: &mut Vec<u8>, lit: &Huff, dist: &Huff) -> bool {
    loop {
        let sym = decode_sym(b, src, lit);
        if sym < 0 {
            return false;
        }
        if sym < 256 {
            out.push(sym as u8);
        } else if sym == 256 {
            return true;
        } else {
            let li = (sym - 257) as usize;
            if li >= 29 {
                return false;
            }
            let len = LBASE[li] + need(b, src, LEXT[li]);
            let ds = decode_sym(b, src, dist);
            if ds < 0 || ds >= 30 {
                return false;
            }
            let d = DBASE[ds as usize] + need(b, src, DEXT[ds as usize]);
            if b.eof || (d as usize) > out.len() {
                return false;
            }
            let start = out.len() - (d as usize);
            let mut k: usize = 0;
            while k < (len as usize) {
                let v = out[start + k];
                out.push(v);
                k += 1;
            }
        }
    }
}

/// Raw deflate data from `start`.
pub fn inflate_raw(src: &Vec<u8>, start: usize) -> Inflated {
    let mut out: Vec<u8> = vec![];
    let mut b = Bits { pos: start, bit: 0, nbits: 0, eof: false };
    loop {
        let last = need(&mut b, src, 1);
        let kind = need(&mut b, src, 2);
        if b.eof {
            return Inflated { data: out, ok: false };
        }
        if kind == 0 {
            b.bit = 0;
            b.nbits = 0;
            if b.pos + 4 > src.len() {
                return Inflated { data: out, ok: false };
            }
            let len = (src[b.pos] as usize) | ((src[b.pos + 1] as usize) << 8);
            b.pos += 4;
            let mut k: usize = 0;
            while k < len && b.pos < src.len() {
                out.push(src[b.pos]);
                b.pos += 1;
                k += 1;
            }
            if k < len {
                return Inflated { data: out, ok: false };
            }
        } else if kind == 1 {
            let mut ll: Vec<i64> = vec![];
            let mut i = 0;
            while i < 288 {
                if i < 144 {
                    ll.push(8);
                } else if i < 256 {
                    ll.push(9);
                } else if i < 280 {
                    ll.push(7);
                } else {
                    ll.push(8);
                }
                i += 1;
            }
            let mut dl: Vec<i64> = vec![];
            let mut j = 0;
            while j < 30 {
                dl.push(5);
                j += 1;
            }
            let lit = build(&ll);
            let dist = build(&dl);
            if !codes(&mut b, src, &mut out, &lit, &dist) {
                return Inflated { data: out, ok: false };
            }
        } else if kind == 2 {
            let nlen = need(&mut b, src, 5) + 257;
            let ndist = need(&mut b, src, 5) + 1;
            let ncode = need(&mut b, src, 4) + 4;
            let mut cl: Vec<i64> = vec![0; 19];
            let mut i: usize = 0;
            while i < (ncode as usize) {
                cl[CLORDER[i]] = need(&mut b, src, 3);
                i += 1;
            }
            let clh = build(&cl);
            let mut lens: Vec<i64> = vec![];
            let total = (nlen + ndist) as usize;
            while lens.len() < total {
                let sym = decode_sym(&mut b, src, &clh);
                if sym < 0 {
                    return Inflated { data: out, ok: false };
                }
                if sym < 16 {
                    lens.push(sym);
                } else {
                    let mut rep: i64 = 0;
                    let mut val: i64 = 0;
                    if sym == 16 {
                        if lens.is_empty() {
                            return Inflated { data: out, ok: false };
                        }
                        val = lens[lens.len() - 1];
                        rep = 3 + need(&mut b, src, 2);
                    } else if sym == 17 {
                        rep = 3 + need(&mut b, src, 3);
                    } else {
                        rep = 11 + need(&mut b, src, 7);
                    }
                    let mut r = 0;
                    while r < rep && lens.len() < total {
                        lens.push(val);
                        r += 1;
                    }
                }
            }
            let mut ll: Vec<i64> = vec![];
            let mut dl: Vec<i64> = vec![];
            let mut k: usize = 0;
            while k < total {
                if k < (nlen as usize) {
                    ll.push(lens[k]);
                } else {
                    dl.push(lens[k]);
                }
                k += 1;
            }
            let lit = build(&ll);
            let dist = build(&dl);
            if !codes(&mut b, src, &mut out, &lit, &dist) {
                return Inflated { data: out, ok: false };
            }
        } else {
            return Inflated { data: out, ok: false };
        }
        if last == 1 {
            return Inflated { data: out, ok: true };
        }
    }
}

/// A zlib stream; without a valid zlib header the data is read as raw
/// deflate, which some writers produce.
pub fn inflate(src: &Vec<u8>) -> Inflated {
    if src.len() >= 2 {
        let cmf = src[0] as i64;
        let flg = src[1] as i64;
        if (cmf & 15) == 8 && ((cmf * 256 + flg) % 31) == 0 {
            return inflate_raw(src, 2);
        }
    }
    inflate_raw(src, 0)
}
