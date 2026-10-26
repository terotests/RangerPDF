//! Paths and matrices: a path in user space (moveto, lineto, cubic curveto,
//! closepath), transformed and flattened into polygon rings in device space.

/// [a b c d e f]: x' = a x + c y + e, y' = b x + d y + f
pub fn mat_mul(m: &Vec<f64>, n: &Vec<f64>) -> Vec<f64> {
    // m then n
    vec![
        m[0] * n[0] + m[1] * n[2],
        m[0] * n[1] + m[1] * n[3],
        m[2] * n[0] + m[3] * n[2],
        m[2] * n[1] + m[3] * n[3],
        m[4] * n[0] + m[5] * n[2] + n[4],
        m[4] * n[1] + m[5] * n[3] + n[5],
    ]
}

pub fn identity() -> Vec<f64> {
    vec![1.0, 0.0, 0.0, 1.0, 0.0, 0.0]
}

pub fn invert(m: &Vec<f64>) -> Vec<f64> {
    let det = m[0] * m[3] - m[1] * m[2];
    if det == 0.0 {
        return identity();
    }
    let a = m[3] / det;
    let b = (0.0 - m[1]) / det;
    let c = (0.0 - m[2]) / det;
    let d = m[0] / det;
    let e = (0.0 - m[4]) * a - m[5] * c;
    let f = (0.0 - m[4]) * b - m[5] * d;
    vec![a, b, c, d, e, f]
}

pub fn tx(m: &Vec<f64>, x: f64, y: f64) -> f64 {
    m[0] * x + m[2] * y + m[4]
}

pub fn ty(m: &Vec<f64>, x: f64, y: f64) -> f64 {
    m[1] * x + m[3] * y + m[5]
}

/// The scale of a matrix: the square root of its determinant's magnitude.
pub fn mat_scale(m: &Vec<f64>) -> f64 {
    let d = m[0] * m[3] - m[1] * m[2];
    let a = if d < 0.0 { 0.0 - d } else { d };
    a.sqrt()
}

pub fn fabs(x: f64) -> f64 {
    if x < 0.0 {
        return 0.0 - x;
    }
    x
}

pub fn fmax(a: f64, b: f64) -> f64 {
    if a > b {
        return a;
    }
    b
}

pub fn fmin(a: f64, b: f64) -> f64 {
    if a < b {
        return a;
    }
    b
}

pub const MOVE: i64 = 0;
pub const LINE: i64 = 1;
pub const CURVE: i64 = 2;
pub const CLOSE: i64 = 3;

#[derive(Clone, Debug)]
pub struct Path {
    pub ops: Vec<i64>,
    /// 2 numbers for move and line, 6 for curve, none for close
    pub xy: Vec<f64>,
    pub cx: f64,
    pub cy: f64,
    pub sx: f64,
    pub sy: f64,
}

impl Path {
    pub fn new() -> Path {
        Path { ops: vec![], xy: vec![], cx: 0.0, cy: 0.0, sx: 0.0, sy: 0.0 }
    }

    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    pub fn move_to(&mut self, x: f64, y: f64) {
        self.ops.push(MOVE);
        self.xy.push(x);
        self.xy.push(y);
        self.cx = x;
        self.cy = y;
        self.sx = x;
        self.sy = y;
    }

    pub fn line_to(&mut self, x: f64, y: f64) {
        if self.ops.is_empty() {
            self.move_to(x, y);
            return;
        }
        self.ops.push(LINE);
        self.xy.push(x);
        self.xy.push(y);
        self.cx = x;
        self.cy = y;
    }

    pub fn curve_to(&mut self, x1: f64, y1: f64, x2: f64, y2: f64, x3: f64, y3: f64) {
        if self.ops.is_empty() {
            self.move_to(x1, y1);
        }
        self.ops.push(CURVE);
        self.xy.push(x1);
        self.xy.push(y1);
        self.xy.push(x2);
        self.xy.push(y2);
        self.xy.push(x3);
        self.xy.push(y3);
        self.cx = x3;
        self.cy = y3;
    }

    /// A quadratic curve, as the cubic it equals.
    pub fn quad_to(&mut self, qx: f64, qy: f64, x: f64, y: f64) {
        let x0 = self.cx;
        let y0 = self.cy;
        self.curve_to(x0 + 2.0 * (qx - x0) / 3.0, y0 + 2.0 * (qy - y0) / 3.0, x + 2.0 * (qx - x) / 3.0, y + 2.0 * (qy - y) / 3.0, x, y);
    }

    pub fn close(&mut self) {
        if self.ops.is_empty() {
            return;
        }
        let last = self.ops[self.ops.len() - 1];
        if last == CLOSE {
            return;
        }
        self.ops.push(CLOSE);
        self.cx = self.sx;
        self.cy = self.sy;
    }

    pub fn rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        self.move_to(x, y);
        self.line_to(x + w, y);
        self.line_to(x + w, y + h);
        self.line_to(x, y + h);
        self.close();
    }

    /// `other` transformed by `m`, appended.
    pub fn append(&mut self, other: &Path, m: &Vec<f64>) {
        let mut k: usize = 0;
        for op in other.ops.iter() {
            if *op == MOVE {
                self.move_to(tx(m, other.xy[k], other.xy[k + 1]), ty(m, other.xy[k], other.xy[k + 1]));
                k += 2;
            } else if *op == LINE {
                self.line_to(tx(m, other.xy[k], other.xy[k + 1]), ty(m, other.xy[k], other.xy[k + 1]));
                k += 2;
            } else if *op == CURVE {
                let x1 = tx(m, other.xy[k], other.xy[k + 1]);
                let y1 = ty(m, other.xy[k], other.xy[k + 1]);
                let x2 = tx(m, other.xy[k + 2], other.xy[k + 3]);
                let y2 = ty(m, other.xy[k + 2], other.xy[k + 3]);
                let x3 = tx(m, other.xy[k + 4], other.xy[k + 5]);
                let y3 = ty(m, other.xy[k + 4], other.xy[k + 5]);
                self.curve_to(x1, y1, x2, y2, x3, y3);
                k += 6;
            } else {
                self.close();
            }
        }
    }
}

/// Polygon rings: points as x, y pairs; `ends[i]` is the point count after
/// ring i; `closed[i]` whether the ring was closed by the path.
#[derive(Clone, Debug)]
pub struct Rings {
    pub pts: Vec<f64>,
    pub ends: Vec<i64>,
    pub closed: Vec<bool>,
}

impl Rings {
    pub fn new() -> Rings {
        Rings { pts: vec![], ends: vec![], closed: vec![] }
    }

    fn npts(&self) -> i64 {
        (self.pts.len() / 2) as i64
    }

    fn start_of_last(&self) -> i64 {
        if self.ends.is_empty() {
            return 0;
        }
        self.ends[self.ends.len() - 1]
    }

    fn end_ring(&mut self, closed: bool) {
        let n = self.npts();
        if n > self.start_of_last() {
            self.ends.push(n);
            self.closed.push(closed);
        }
    }

    pub fn bbox(&self) -> Vec<f64> {
        if self.pts.is_empty() {
            return vec![0.0, 0.0, 0.0, 0.0];
        }
        let mut x0 = self.pts[0];
        let mut y0 = self.pts[1];
        let mut x1 = x0;
        let mut y1 = y0;
        let mut i: usize = 0;
        while i + 1 < self.pts.len() {
            let x = self.pts[i];
            let y = self.pts[i + 1];
            if x < x0 {
                x0 = x;
            }
            if x > x1 {
                x1 = x;
            }
            if y < y0 {
                y0 = y;
            }
            if y > y1 {
                y1 = y;
            }
            i += 2;
        }
        vec![x0, y0, x1, y1]
    }
}

/// The path transformed by `m` and flattened: curves become line segments
/// no further than `tol` from the curve.
pub fn flatten(p: &Path, m: &Vec<f64>, tol: f64) -> Rings {
    let mut r = Rings::new();
    let mut k: usize = 0;
    let mut cx = 0.0;
    let mut cy = 0.0;
    for op in p.ops.iter() {
        if *op == MOVE {
            r.end_ring(false);
            cx = tx(m, p.xy[k], p.xy[k + 1]);
            cy = ty(m, p.xy[k], p.xy[k + 1]);
            r.pts.push(cx);
            r.pts.push(cy);
            k += 2;
        } else if *op == LINE {
            cx = tx(m, p.xy[k], p.xy[k + 1]);
            cy = ty(m, p.xy[k], p.xy[k + 1]);
            r.pts.push(cx);
            r.pts.push(cy);
            k += 2;
        } else if *op == CURVE {
            let x1 = tx(m, p.xy[k], p.xy[k + 1]);
            let y1 = ty(m, p.xy[k], p.xy[k + 1]);
            let x2 = tx(m, p.xy[k + 2], p.xy[k + 3]);
            let y2 = ty(m, p.xy[k + 2], p.xy[k + 3]);
            let x3 = tx(m, p.xy[k + 4], p.xy[k + 5]);
            let y3 = ty(m, p.xy[k + 4], p.xy[k + 5]);
            // the segment count from the control polygon's second differences
            let ddx = fmax(fabs(cx - 2.0 * x1 + x2), fabs(x1 - 2.0 * x2 + x3));
            let ddy = fmax(fabs(cy - 2.0 * y1 + y2), fabs(y1 - 2.0 * y2 + y3));
            let dd = (ddx * ddx + ddy * ddy).sqrt();
            let mut n = ((dd * 0.75 / tol).sqrt()).ceil() as i64;
            if n < 1 {
                n = 1;
            }
            if n > 200 {
                n = 200;
            }
            let mut i: i64 = 1;
            while i <= n {
                let t = (i as f64) / (n as f64);
                let u = 1.0 - t;
                let a = u * u * u;
                let b = 3.0 * u * u * t;
                let c = 3.0 * u * t * t;
                let d = t * t * t;
                r.pts.push(a * cx + b * x1 + c * x2 + d * x3);
                r.pts.push(a * cy + b * y1 + c * y2 + d * y3);
                i += 1;
            }
            cx = x3;
            cy = y3;
            k += 6;
        } else {
            // back to the ring's start, where a following segment begins a new ring
            let st = r.start_of_last() as usize;
            if (r.npts() as usize) > st {
                cx = r.pts[st * 2];
                cy = r.pts[st * 2 + 1];
            }
            r.end_ring(true);
            r.pts.push(cx);
            r.pts.push(cy);
        }
    }
    r.end_ring(false);
    // a ring of one point (a lone moveto) draws nothing
    let mut out = Rings::new();
    let mut start: i64 = 0;
    let mut i: usize = 0;
    while i < r.ends.len() {
        let e = r.ends[i];
        if e - start >= 2 {
            let mut j = start;
            while j < e {
                out.pts.push(r.pts[(j * 2) as usize]);
                out.pts.push(r.pts[(j * 2 + 1) as usize]);
                j += 1;
            }
            out.ends.push(out.npts());
            out.closed.push(r.closed[i]);
        }
        start = e;
        i += 1;
    }
    out
}
