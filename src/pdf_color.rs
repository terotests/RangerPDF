//! Colour spaces (ISO 32000-1 §8.6): device, CIE-based (as their device
//! equivalents), ICCBased (by component count or its alternate), Indexed,
//! Separation and DeviceN (through the tint transform), and Pattern.
//! Colours come out as sRGB in 0..1.

use crate::pdf_doc::Doc;
use crate::pdf_func;
use crate::pdf_func::Func;
use crate::pdf_obj::Obj;
use crate::pdf_obj::K;

pub const CS_GRAY: i64 = 0;
pub const CS_RGB: i64 = 1;
pub const CS_CMYK: i64 = 2;
pub const CS_LAB: i64 = 3;
pub const CS_INDEXED: i64 = 4;
pub const CS_SEP: i64 = 5;
pub const CS_PATTERN: i64 = 6;

#[derive(Clone, Debug)]
pub struct ColorSpace {
    pub kind: i64,
    /// components of a colour in this space
    pub n: usize,
    /// Indexed: the base; Separation/DeviceN: the alternate; Pattern: the underlying space
    pub base: Vec<ColorSpace>,
    pub hival: i64,
    pub lookup: Vec<u8>,
    pub tint: Func,
    /// Separation /None: paints nothing
    pub none: bool,
    /// Lab: white point and range
    pub range: Vec<f64>,
}

pub fn device(kind: i64) -> ColorSpace {
    let n = if kind == CS_GRAY { 1 } else if kind == CS_CMYK { 4 } else { 3 };
    ColorSpace { kind: kind, n: n, base: vec![], hival: 0, lookup: vec![], tint: crate::pdf_func::read_none(), none: false, range: vec![] }
}

fn by_n(n: i64) -> ColorSpace {
    if n == 1 {
        return device(CS_GRAY);
    }
    if n == 4 {
        return device(CS_CMYK);
    }
    device(CS_RGB)
}

/// The colour space `o` names or is (a name, an array, or a resource name
/// resolved through `res`).
pub fn read(doc: &mut Doc, o: &Obj, res: &Obj, depth: i64) -> ColorSpace {
    let v = doc.resolve(o);
    if depth > 8 {
        return device(CS_GRAY);
    }
    if v.k == K::Name {
        let n = v.name.clone();
        if n == "DeviceGray" || n == "G" || n == "CalGray" {
            return device(CS_GRAY);
        }
        if n == "DeviceRGB" || n == "RGB" || n == "CalRGB" {
            return device(CS_RGB);
        }
        if n == "DeviceCMYK" || n == "CMYK" {
            return device(CS_CMYK);
        }
        if n == "Pattern" {
            let mut p = device(CS_PATTERN);
            p.n = 0;
            return p;
        }
        if n == "Indexed" || n == "I" {
            return device(CS_RGB);
        }
        // a name from the resources' ColorSpace dictionary
        let csd = doc.get_key(res, "ColorSpace");
        if csd.is_dict() && csd.has(&n) {
            let x = csd.at(&n);
            return read(doc, &x, res, depth + 1);
        }
        return device(CS_RGB);
    }
    if v.k != K::Arr || v.items.is_empty() {
        return device(CS_RGB);
    }
    let fam = doc.resolve(&v.items[0]).name.clone();
    if fam == "ICCBased" {
        let s = doc.resolve(&v.item(1));
        let n = doc.get_key(&s, "N").i();
        if n == 1 || n == 3 || n == 4 {
            return by_n(n);
        }
        let alt = doc.get_key(&s, "Alternate");
        if !alt.is_null() {
            return read(doc, &alt, res, depth + 1);
        }
        return device(CS_RGB);
    }
    if fam == "CalGray" {
        return device(CS_GRAY);
    }
    if fam == "CalRGB" {
        return device(CS_RGB);
    }
    if fam == "Lab" {
        let mut l = device(CS_LAB);
        let d = doc.resolve(&v.item(1));
        l.range = doc.get_key(&d, "Range").nums();
        if l.range.len() < 4 {
            l.range = vec![-100.0, 100.0, -100.0, 100.0];
        }
        return l;
    }
    if fam == "Indexed" || fam == "I" {
        let base = read(doc, &v.item(1), res, depth + 1);
        let mut c = device(CS_INDEXED);
        c.n = 1;
        c.hival = doc.resolve(&v.item(2)).i();
        let lk = doc.resolve(&v.item(3));
        if lk.k == K::Stream {
            c.lookup = doc.stream_data(&lk);
        } else {
            c.lookup = lk.s.clone();
        }
        c.base.push(base);
        return c;
    }
    if fam == "Separation" || fam == "DeviceN" {
        let mut c = device(CS_SEP);
        let names = doc.resolve(&v.item(1));
        if fam == "Separation" {
            c.n = 1;
            c.none = names.is_name("None");
        } else {
            c.n = if names.k == K::Arr { names.items.len() } else { 1 };
            let mut all_none = names.k == K::Arr && !names.items.is_empty();
            for x in names.items.iter() {
                if !x.is_name("None") {
                    all_none = false;
                }
            }
            c.none = all_none;
        }
        let alt = read(doc, &v.item(2), res, depth + 1);
        c.tint = pdf_func::read(doc, &v.item(3), 0);
        c.base.push(alt);
        return c;
    }
    if fam == "Pattern" {
        let mut p = device(CS_PATTERN);
        p.n = 0;
        if v.items.len() > 1 {
            let u = read(doc, &v.item(1), res, depth + 1);
            p.n = u.n;
            p.base.push(u);
        }
        return p;
    }
    if fam == "DeviceGray" || fam == "G" {
        return device(CS_GRAY);
    }
    if fam == "DeviceCMYK" || fam == "CMYK" {
        return device(CS_CMYK);
    }
    device(CS_RGB)
}

fn clamp01(x: f64) -> f64 {
    if x < 0.0 {
        return 0.0;
    }
    if x > 1.0 {
        return 1.0;
    }
    x
}

pub fn cmyk_to_rgb(c: f64, m: f64, y: f64, k: f64) -> Vec<f64> {
    vec![clamp01((1.0 - c) * (1.0 - k)), clamp01((1.0 - m) * (1.0 - k)), clamp01((1.0 - y) * (1.0 - k))]
}

fn lab_f(t: f64) -> f64 {
    if t > 6.0 / 29.0 {
        return t * t * t;
    }
    (108.0 / 841.0) * (t - 4.0 / 29.0)
}

fn srgb(x: f64) -> f64 {
    let c = clamp01(x);
    if c <= 0.0031308 {
        return 12.92 * c;
    }
    1.055 * (c.ln() / 2.4).exp() - 0.055
}

impl ColorSpace {
    /// The initial colour (§8.6.8): black, or index 0, or tint 1.
    pub fn initial(&self) -> Vec<f64> {
        if self.kind == CS_CMYK {
            return vec![0.0, 0.0, 0.0, 1.0];
        }
        if self.kind == CS_SEP {
            let mut v: Vec<f64> = vec![];
            let mut i: usize = 0;
            while i < self.n {
                v.push(1.0);
                i += 1;
            }
            return v;
        }
        let mut v: Vec<f64> = vec![];
        let mut i: usize = 0;
        while i < self.n {
            v.push(0.0);
            i += 1;
        }
        v
    }

    pub fn to_rgb(&self, c: &Vec<f64>) -> Vec<f64> {
        let at0 = if c.len() > 0 { c[0] } else { 0.0 };
        let at1 = if c.len() > 1 { c[1] } else { 0.0 };
        let at2 = if c.len() > 2 { c[2] } else { 0.0 };
        let at3 = if c.len() > 3 { c[3] } else { 0.0 };
        if self.kind == CS_GRAY {
            let v = clamp01(at0);
            return vec![v, v, v];
        }
        if self.kind == CS_RGB {
            return vec![clamp01(at0), clamp01(at1), clamp01(at2)];
        }
        if self.kind == CS_CMYK {
            return cmyk_to_rgb(at0, at1, at2, at3);
        }
        if self.kind == CS_LAB {
            // D50 white, to XYZ, to sRGB (an approximation without adaptation)
            let l = at0;
            let a = at1;
            let b = at2;
            let fy = (l + 16.0) / 116.0;
            let x = 0.9642 * lab_f(fy + a / 500.0);
            let y = lab_f(fy);
            let z = 0.8249 * lab_f(fy - b / 200.0);
            let r = 3.1339 * x - 1.6169 * y - 0.4906 * z;
            let gg = -0.9785 * x + 1.9160 * y + 0.0333 * z;
            let bb = 0.0720 * x - 0.2290 * y + 1.4057 * z;
            return vec![srgb(r), srgb(gg), srgb(bb)];
        }
        if self.kind == CS_INDEXED {
            if self.base.is_empty() {
                return vec![0.0, 0.0, 0.0];
            }
            let b = &self.base[0];
            let mut i = (at0 + 0.5).floor() as i64;
            if i < 0 {
                i = 0;
            }
            if i > self.hival {
                i = self.hival;
            }
            let mut comps: Vec<f64> = vec![];
            let mut k: usize = 0;
            while k < b.n {
                let pos = (i as usize) * b.n + k;
                let v = if pos < self.lookup.len() { (self.lookup[pos] as f64) / 255.0 } else { 0.0 };
                comps.push(v);
                k += 1;
            }
            if b.kind == CS_LAB {
                // the lookup holds Lab scaled to bytes
                let l = comps[0] * 100.0;
                let a0 = b.range[0] + comps[1] * (b.range[1] - b.range[0]);
                let b0 = b.range[2] + comps[2] * (b.range[3] - b.range[2]);
                let lab: Vec<f64> = vec![l, a0, b0];
                return b.to_rgb(&lab);
            }
            return b.to_rgb(&comps);
        }
        if self.kind == CS_SEP {
            if self.base.is_empty() {
                return vec![0.0, 0.0, 0.0];
            }
            let out = if self.tint.ok { self.tint.eval(c) } else { vec![] };
            if out.is_empty() {
                // no tint transform: tint 1 is black
                let v = 1.0 - clamp01(at0);
                return vec![v, v, v];
            }
            return self.base[0].to_rgb(&out);
        }
        if self.kind == CS_PATTERN && !self.base.is_empty() {
            return self.base[0].to_rgb(c);
        }
        vec![0.0, 0.0, 0.0]
    }
}
