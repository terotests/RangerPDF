//! A software rasterizer for the display list: antialiased polygon fill
//! (nonzero and even-odd), strokes with caps, joins and dashes, clip masks
//! and images. The viewer draws with EVG; this is for thumbnails and for
//! comparing pages with a reference renderer pixel by pixel.

use crate::pdf_display;
use crate::pdf_display::Cmd;
use crate::pdf_display::PageOut;
use crate::pdf_path;

/// Vertical subsamples per pixel row.
const SS: i64 = 4;

pub struct Raster {
    pub w: i64,
    pub h: i64,
    /// w * h * 4, RGBA
    pub px: Vec<u8>,
    clips: Vec<Vec<u8>>,
    /// JPEG images are not decoded here: they are drawn in this grey
    pub jpeg_gray: i64,
}

struct Edge {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    dir: i64,
}

fn edges_of(pts: &Vec<f64>, ends: &Vec<i64>, s: f64, out: &mut Vec<Edge>) {
    let mut start: usize = 0;
    for e in ends.iter() {
        let end = *e as usize;
        if end > start + 1 {
            let mut i = start;
            while i < end {
                let j = if i + 1 < end { i + 1 } else { start };
                let ax = pts[i * 2] * s;
                let ay = pts[i * 2 + 1] * s;
                let bx = pts[j * 2] * s;
                let by = pts[j * 2 + 1] * s;
                if ay != by {
                    if ay < by {
                        out.push(Edge { x0: ax, y0: ay, x1: bx, y1: by, dir: 1 });
                    } else {
                        out.push(Edge { x0: bx, y0: by, x1: ax, y1: ay, dir: -1 });
                    }
                }
                i += 1;
            }
        }
        start = end;
    }
}

impl Raster {
    pub fn new(w: i64, h: i64) -> Raster {
        let mut px: Vec<u8> = vec![255; (w * h * 4) as usize];
        let _ = &mut px;
        Raster { w: w, h: h, px: px, clips: vec![], jpeg_gray: 200 }
    }

    /// Coverage (0..255) of the polygon rings for rows y_from..y_to, one
    /// row of `w` after another (row y_from first).
    fn coverage(&self, edges: &Vec<Edge>, even_odd: bool, y_from: i64, y_to: i64) -> Vec<u8> {
        let w = self.w;
        let rows = if y_to > y_from { y_to - y_from } else { 0 };
        let mut cov: Vec<u8> = vec![0; (w * rows) as usize];
        if edges.is_empty() || rows == 0 {
            return cov;
        }
        // the edges by the row they start in, then an active list per row
        let mut starts: Vec<Vec<usize>> = vec![];
        let mut r: i64 = 0;
        while r < rows {
            let none: Vec<usize> = vec![];
            starts.push(none);
            r += 1;
        }
        let mut ei: usize = 0;
        while ei < edges.len() {
            let mut sr = edges[ei].y0.floor() as i64 - y_from;
            if sr < 0 {
                sr = 0;
            }
            if sr < rows {
                starts[sr as usize].push(ei);
            }
            ei += 1;
        }
        let mut active: Vec<usize> = vec![];
        let mut acc: Vec<f64> = vec![0.0; (w + 2) as usize];
        let mut xs: Vec<f64> = vec![];
        let mut ds: Vec<i64> = vec![];
        let mut y = y_from;
        while y < y_to {
            let ri = (y - y_from) as usize;
            // drop the edges that ended above this row, add those starting in it
            let mut keep: Vec<usize> = vec![];
            for k in active.iter() {
                if edges[*k].y1 > (y as f64) {
                    keep.push(*k);
                }
            }
            for k in starts[ri].iter() {
                keep.push(*k);
            }
            active = keep;
            if active.is_empty() {
                y += 1;
                continue;
            }
            let mut lo = w + 1;
            let mut hi: i64 = -1;
            let mut any = false;
            let mut sub: i64 = 0;
            while sub < SS {
                let sy = (y as f64) + ((sub as f64) + 0.5) / (SS as f64);
                xs.clear();
                ds.clear();
                for k in active.iter() {
                    let e = &edges[*k];
                    if sy >= e.y0 && sy < e.y1 {
                        let t = (sy - e.y0) / (e.y1 - e.y0);
                        xs.push(e.x0 + t * (e.x1 - e.x0));
                        ds.push(e.dir);
                    }
                }
                // sort the crossings (insertion sort: few per row)
                let n = xs.len();
                let mut i: usize = 1;
                while i < n {
                    let x = xs[i];
                    let d = ds[i];
                    let mut j = i;
                    while j > 0 && xs[j - 1] > x {
                        xs[j] = xs[j - 1];
                        ds[j] = ds[j - 1];
                        j -= 1;
                    }
                    xs[j] = x;
                    ds[j] = d;
                    i += 1;
                }
                let mut wind: i64 = 0;
                let mut xa = 0.0;
                let mut i2: usize = 0;
                while i2 < n {
                    let before = if even_odd { wind % 2 != 0 } else { wind != 0 };
                    wind += ds[i2];
                    let after = if even_odd { wind % 2 != 0 } else { wind != 0 };
                    if !before && after {
                        xa = xs[i2];
                    } else if before && !after {
                        span(&mut acc, xa, xs[i2], w);
                        let a0 = xa.floor() as i64;
                        let b0 = xs[i2].ceil() as i64 + 1;
                        if a0 < lo {
                            lo = a0;
                        }
                        if b0 > hi {
                            hi = b0;
                        }
                        any = true;
                    }
                    i2 += 1;
                }
                sub += 1;
            }
            if any {
                if lo < 0 {
                    lo = 0;
                }
                if hi > w {
                    hi = w;
                }
                let row = (ri as i64 * w) as usize;
                let mut x = lo;
                while x < hi {
                    let xu = x as usize;
                    let v = acc[xu] / (SS as f64);
                    if v > 0.0 {
                        let b = (v * 255.0 + 0.5).floor();
                        cov[row + xu] = if b > 255.0 { 255 } else { b as u8 };
                    }
                    acc[xu] = 0.0;
                    x += 1;
                }
                // the cells span() may touch beyond the row's range
                acc[w as usize] = 0.0;
                acc[(w + 1) as usize] = 0.0;
            }
            y += 1;
        }
        cov
    }

    fn rows_of(&self, edges: &Vec<Edge>) -> Vec<i64> {
        let mut y0 = 1.0e30;
        let mut y1 = -1.0e30;
        for e in edges.iter() {
            y0 = pdf_path::fmin(y0, e.y0);
            y1 = pdf_path::fmax(y1, e.y1);
        }
        let mut a = y0.floor() as i64;
        let mut b = y1.ceil() as i64 + 1;
        if a < 0 {
            a = 0;
        }
        if b > self.h {
            b = self.h;
        }
        vec![a, b]
    }

    fn paint(&mut self, cov: &Vec<u8>, rows: &Vec<i64>, r: i64, g: i64, b: i64, a: f64) {
        let w = self.w;
        let has_clip = !self.clips.is_empty();
        let top = self.clips.len();
        let mut y = rows[0];
        while y < rows[1] {
            let mut x: i64 = 0;
            let band = ((y - rows[0]) * w) as usize;
            while x < w {
                let i = (y * w + x) as usize;
                let c = cov[band + (x as usize)] as i64;
                if c > 0 {
                    let mut alpha = (c as f64) / 255.0 * a;
                    if has_clip {
                        alpha = alpha * (self.clips[top - 1][i] as f64) / 255.0;
                    }
                    if alpha > 0.0 {
                        let p = i * 4;
                        self.px[p] = blend(self.px[p], r, alpha);
                        self.px[p + 1] = blend(self.px[p + 1], g, alpha);
                        self.px[p + 2] = blend(self.px[p + 2], b, alpha);
                    }
                }
                x += 1;
            }
            y += 1;
        }
    }

    pub fn fill(&mut self, pts: &Vec<f64>, ends: &Vec<i64>, even_odd: bool, s: f64, r: i64, g: i64, b: i64, a: f64) {
        let mut edges: Vec<Edge> = vec![];
        edges_of(pts, ends, s, &mut edges);
        if edges.is_empty() {
            return;
        }
        let rows = self.rows_of(&edges);
        if rows[1] <= rows[0] {
            return;
        }
        let cov = self.coverage(&edges, even_odd, rows[0], rows[1]);
        self.paint(&cov, &rows, r, g, b, a);
    }

    pub fn push_clip(&mut self, pts: &Vec<f64>, ends: &Vec<i64>, even_odd: bool, s: f64) {
        let mut edges: Vec<Edge> = vec![];
        edges_of(pts, ends, s, &mut edges);
        let rows = if edges.is_empty() { vec![0, 0] } else { self.rows_of(&edges) };
        let band = self.coverage(&edges, even_odd, rows[0], rows[1]);
        let mut cov: Vec<u8> = vec![0; (self.w * self.h) as usize];
        let off = (rows[0] * self.w) as usize;
        let mut q: usize = 0;
        while q < band.len() {
            cov[off + q] = band[q];
            q += 1;
        }
        if !self.clips.is_empty() {
            let top = self.clips.len() - 1;
            let mut i: usize = 0;
            while i < cov.len() {
                let v = (cov[i] as i64) * (self.clips[top][i] as i64) / 255;
                cov[i] = v as u8;
                i += 1;
            }
        }
        self.clips.push(cov);
    }

    pub fn pop_clip(&mut self) {
        if !self.clips.is_empty() {
            let n = self.clips.len() - 1;
            self.clips.truncate(n);
        }
    }

    pub fn image(&mut self, img: &crate::pdf_image::Img, m: &Vec<f64>, s: f64, alpha: f64) {
        // device pixel -> image pixel through the inverse of the image matrix
        let ms: Vec<f64> = vec![m[0] * s, m[1] * s, m[2] * s, m[3] * s, m[4] * s, m[5] * s];
        let inv = pdf_path::invert(&ms);
        let xs: Vec<f64> = vec![ms[4], ms[0] + ms[4], ms[0] + ms[2] + ms[4], ms[2] + ms[4]];
        let ys: Vec<f64> = vec![ms[5], ms[1] + ms[5], ms[1] + ms[3] + ms[5], ms[3] + ms[5]];
        let mut x0 = xs[0];
        let mut x1 = xs[0];
        let mut y0 = ys[0];
        let mut y1 = ys[0];
        let mut k: usize = 1;
        while k < 4 {
            x0 = pdf_path::fmin(x0, xs[k]);
            x1 = pdf_path::fmax(x1, xs[k]);
            y0 = pdf_path::fmin(y0, ys[k]);
            y1 = pdf_path::fmax(y1, ys[k]);
            k += 1;
        }
        let mut ya = y0.floor() as i64;
        let mut yb = y1.ceil() as i64;
        let mut xa = x0.floor() as i64;
        let mut xb = x1.ceil() as i64;
        if ya < 0 {
            ya = 0;
        }
        if xa < 0 {
            xa = 0;
        }
        if yb > self.h {
            yb = self.h;
        }
        if xb > self.w {
            xb = self.w;
        }
        let iw = img.w;
        let ih = img.h;
        let jpeg = img.rgba.is_empty();
        let top = self.clips.len();
        let mut y = ya;
        while y < yb {
            let mut x = xa;
            while x < xb {
                let cx = (x as f64) + 0.5;
                let cy = (y as f64) + 0.5;
                let u = pdf_path::tx(&inv, cx, cy);
                let v = pdf_path::ty(&inv, cx, cy);
                if u >= 0.0 && u < 1.0 && v >= 0.0 && v < 1.0 {
                    let sx = (u * (iw as f64)).floor() as i64;
                    let sy = ((1.0 - v) * (ih as f64)).floor() as i64;
                    let sxc = if sx >= iw { iw - 1 } else { sx };
                    let syc = if sy >= ih { ih - 1 } else { sy };
                    let si = (syc * iw + sxc) as usize;
                    let mut r: i64 = self.jpeg_gray;
                    let mut g: i64 = self.jpeg_gray;
                    let mut b: i64 = self.jpeg_gray;
                    let mut a = alpha;
                    if !jpeg {
                        r = img.rgba[si * 4] as i64;
                        g = img.rgba[si * 4 + 1] as i64;
                        b = img.rgba[si * 4 + 2] as i64;
                        a = a * (img.rgba[si * 4 + 3] as f64) / 255.0;
                    } else if !img.alpha.is_empty() {
                        a = a * (img.alpha[si] as f64) / 255.0;
                    }
                    let i = (y * self.w + x) as usize;
                    if top > 0 {
                        a = a * (self.clips[top - 1][i] as f64) / 255.0;
                    }
                    if a > 0.0 {
                        let p = i * 4;
                        self.px[p] = blend(self.px[p], r, a);
                        self.px[p + 1] = blend(self.px[p + 1], g, a);
                        self.px[p + 2] = blend(self.px[p + 2], b, a);
                    }
                }
                x += 1;
            }
            y += 1;
        }
    }
}

fn blend(dst: u8, src: i64, a: f64) -> u8 {
    let d = dst as f64;
    let v = d + ((src as f64) - d) * (if a > 1.0 { 1.0 } else { a });
    let r = (v + 0.5).floor();
    if r < 0.0 {
        return 0;
    }
    if r > 255.0 {
        return 255;
    }
    r as u8
}

/// Adds the horizontal span xa..xb (one subsample row) to `acc`, with the
/// partial pixels at its ends.
fn span(acc: &mut Vec<f64>, xa0: f64, xb0: f64, w: i64) {
    let mut xa = xa0;
    let mut xb = xb0;
    if xa < 0.0 {
        xa = 0.0;
    }
    if xb > (w as f64) {
        xb = w as f64;
    }
    if xb <= xa {
        return;
    }
    let ia = xa.floor() as i64;
    let ib = xb.floor() as i64;
    if ia == ib {
        acc[ia as usize] += xb - xa;
        return;
    }
    acc[ia as usize] += (ia as f64) + 1.0 - xa;
    let mut x = ia + 1;
    while x < ib {
        acc[x as usize] += 1.0;
        x += 1;
    }
    if ib < w {
        acc[ib as usize] += xb - (ib as f64);
    }
}

/// The polygons that cover a stroke: a quad per segment, joins and caps.
pub fn stroke_polys(c: &Cmd, out_pts: &mut Vec<f64>, out_ends: &mut Vec<i64>) {
    let hw = c.width / 2.0;
    let mut start: usize = 0;
    let mut ri: usize = 0;
    for e in c.ends.iter() {
        let end = *e as usize;
        let closed = ri < c.closed.len() && c.closed[ri];
        ri += 1;
        let mut ring: Vec<f64> = vec![];
        let mut i = start;
        while i < end {
            ring.push(c.pts[i * 2]);
            ring.push(c.pts[i * 2 + 1]);
            i += 1;
        }
        if closed && ring.len() >= 4 {
            let x = ring[0];
            let y = ring[1];
            ring.push(x);
            ring.push(y);
        }
        let mut pieces: Vec<Vec<f64>> = vec![];
        if c.dash.is_empty() {
            pieces.push(ring);
        } else {
            pieces = dashes(&ring, &c.dash, c.phase);
        }
        for pc in pieces.iter() {
            stroke_line(pc, hw, c.cap, c.join, closed && c.dash.is_empty(), out_pts, out_ends);
        }
        start = end;
    }
}

fn dashes(ring: &Vec<f64>, dash: &Vec<f64>, phase: f64) -> Vec<Vec<f64>> {
    let mut out: Vec<Vec<f64>> = vec![];
    let mut total = 0.0;
    for d in dash.iter() {
        total += *d;
    }
    if total <= 0.0 {
        out.push(ring.clone());
        return out;
    }
    // where the phase puts us in the pattern
    let mut di: usize = 0;
    let mut left = dash[0];
    let mut on = true;
    let mut ph = phase - (phase / total).floor() * total;
    while ph > 0.0 {
        if ph >= left {
            ph -= left;
            di = (di + 1) % dash.len();
            left = dash[di];
            on = !on;
        } else {
            left -= ph;
            ph = 0.0;
        }
    }
    let mut cur: Vec<f64> = vec![];
    if on && ring.len() >= 2 {
        cur.push(ring[0]);
        cur.push(ring[1]);
    }
    let mut i: usize = 2;
    let mut guard = 0;
    while i + 1 < ring.len() && guard < 100000 {
        guard += 1;
        let ax = ring[i - 2];
        let ay = ring[i - 1];
        let bx = ring[i];
        let by = ring[i + 1];
        let seg = ((bx - ax) * (bx - ax) + (by - ay) * (by - ay)).sqrt();
        let mut pos = 0.0;
        while seg - pos > left {
            pos += left;
            let t = pos / seg;
            let px = ax + t * (bx - ax);
            let py = ay + t * (by - ay);
            if on {
                cur.push(px);
                cur.push(py);
                out.push(cur);
                cur = vec![];
            } else {
                cur = vec![px, py];
            }
            on = !on;
            di = (di + 1) % dash.len();
            left = dash[di];
            if left <= 0.0 && !on {
                left = 0.0;
            }
            if out.len() > 20000 {
                return out;
            }
        }
        left -= seg - pos;
        if on {
            cur.push(bx);
            cur.push(by);
        }
        i += 2;
    }
    if on && cur.len() >= 4 {
        out.push(cur);
    }
    out
}

fn push_poly(p: &Vec<f64>, out_pts: &mut Vec<f64>, out_ends: &mut Vec<i64>) {
    // the same winding for every piece, so that nonzero fill joins them
    let mut area = 0.0;
    let n = p.len() / 2;
    let mut i: usize = 0;
    while i < n {
        let j = (i + 1) % n;
        area += p[i * 2] * p[j * 2 + 1] - p[j * 2] * p[i * 2 + 1];
        i += 1;
    }
    if area < 0.0 {
        let mut k = n;
        while k > 0 {
            k -= 1;
            out_pts.push(p[k * 2]);
            out_pts.push(p[k * 2 + 1]);
        }
    } else {
        for v in p.iter() {
            out_pts.push(*v);
        }
    }
    out_ends.push((out_pts.len() / 2) as i64);
}

fn disc(x: f64, y: f64, r: f64, out_pts: &mut Vec<f64>, out_ends: &mut Vec<i64>) {
    let mut n = (r * 2.0) as i64 + 8;
    if n > 64 {
        n = 64;
    }
    let mut p: Vec<f64> = vec![];
    let mut i = 0;
    while i < n {
        let a = (i as f64) * 6.283185307179586 / (n as f64);
        p.push(x + r * a.cos());
        p.push(y + r * a.sin());
        i += 1;
    }
    push_poly(&p, out_pts, out_ends);
}

fn stroke_line(r: &Vec<f64>, hw: f64, cap: i64, join: i64, closed: bool, out_pts: &mut Vec<f64>, out_ends: &mut Vec<i64>) {
    let n = r.len() / 2;
    if n == 0 {
        return;
    }
    if n == 1 || (n == 2 && r[0] == r[2] && r[1] == r[3]) {
        // a zero-length segment: a dot for round and square caps
        if cap == 1 {
            disc(r[0], r[1], hw, out_pts, out_ends);
        } else if cap == 2 {
            let p: Vec<f64> = vec![r[0] - hw, r[1] - hw, r[0] + hw, r[1] - hw, r[0] + hw, r[1] + hw, r[0] - hw, r[1] + hw];
            push_poly(&p, out_pts, out_ends);
        }
        return;
    }
    let mut i: usize = 0;
    while i + 1 < n {
        let mut ax = r[i * 2];
        let mut ay = r[i * 2 + 1];
        let mut bx = r[i * 2 + 2];
        let mut by = r[i * 2 + 3];
        let dx = bx - ax;
        let dy = by - ay;
        let len = (dx * dx + dy * dy).sqrt();
        if len <= 0.0 {
            i += 1;
            continue;
        }
        let ux = dx / len;
        let uy = dy / len;
        if cap == 2 && !closed {
            if i == 0 {
                ax -= ux * hw;
                ay -= uy * hw;
            }
            if i + 2 == n {
                bx += ux * hw;
                by += uy * hw;
            }
        }
        let nx = 0.0 - uy * hw;
        let ny = ux * hw;
        let p: Vec<f64> = vec![ax + nx, ay + ny, bx + nx, by + ny, bx - nx, by - ny, ax - nx, ay - ny];
        push_poly(&p, out_pts, out_ends);
        // the join at b with the next segment
        let has_next = i + 2 < n || closed;
        if has_next && hw > 0.3 {
            let k = if i + 2 < n { i + 2 } else { 1 };
            let cx = r[k * 2];
            let cy = r[k * 2 + 1];
            let ex = cx - r[i * 2 + 2];
            let ey = cy - r[i * 2 + 3];
            let el = (ex * ex + ey * ey).sqrt();
            let jx = r[i * 2 + 2];
            let jy = r[i * 2 + 3];
            if join == 1 {
                disc(jx, jy, hw, out_pts, out_ends);
            } else if el > 0.0 {
                let vx = ex / el;
                let vy = ey / el;
                let mx = 0.0 - vy * hw;
                let my = vx * hw;
                // the bevel between the two segments' outer corners, both sides
                let t1: Vec<f64> = vec![jx, jy, jx + nx, jy + ny, jx + mx, jy + my];
                push_poly(&t1, out_pts, out_ends);
                let t2: Vec<f64> = vec![jx, jy, jx - nx, jy - ny, jx - mx, jy - my];
                push_poly(&t2, out_pts, out_ends);
                if join == 0 {
                    // the miter point, unless the angle is too sharp
                    let cosv = ux * vx + uy * vy;
                    let half = ((1.0 + cosv) / 2.0).sqrt();
                    if half > 0.1 {
                        let bxm = (nx + mx) / 2.0;
                        let bym = (ny + my) / 2.0;
                        let bl = (bxm * bxm + bym * bym).sqrt();
                        if bl > 0.0 {
                            let ml = hw / half;
                            let sgn = if ux * vy - uy * vx > 0.0 { -1.0 } else { 1.0 };
                            let px = jx + bxm / bl * ml * sgn;
                            let py = jy + bym / bl * ml * sgn;
                            let q1: Vec<f64> = vec![jx, jy, jx + nx * sgn, jy + ny * sgn, px, py, jx + mx * sgn, jy + my * sgn];
                            push_poly(&q1, out_pts, out_ends);
                        }
                    }
                }
            }
        }
        i += 1;
    }
    if cap == 1 && !closed {
        disc(r[0], r[1], hw, out_pts, out_ends);
        disc(r[(n - 1) * 2], r[(n - 1) * 2 + 1], hw, out_pts, out_ends);
    }
}

/// The page drawn at `scale` pixels per point.
pub fn rasterize(page: &PageOut, scale: f64) -> Raster {
    let w = (page.width * scale).ceil() as i64;
    let h = (page.height * scale).ceil() as i64;
    let mut ras = Raster::new(if w < 1 { 1 } else { w }, if h < 1 { 1 } else { h });
    for c in page.cmds.iter() {
        if c.op == pdf_display::FILL {
            ras.fill(&c.pts, &c.ends, c.even_odd, scale, c.r, c.g, c.b, c.a);
        } else if c.op == pdf_display::STROKE {
            let mut pts: Vec<f64> = vec![];
            let mut ends: Vec<i64> = vec![];
            stroke_polys(c, &mut pts, &mut ends);
            ras.fill(&pts, &ends, false, scale, c.r, c.g, c.b, c.a);
        } else if c.op == pdf_display::CLIP {
            ras.push_clip(&c.pts, &c.ends, c.even_odd, scale);
        } else if c.op == pdf_display::UNCLIP {
            ras.pop_clip();
        } else if c.op == pdf_display::IMAGE {
            if c.image >= 0 && (c.image as usize) < page.images.len() {
                let img = &page.images[c.image as usize];
                ras.image(img, &c.m, scale, c.a);
            }
        }
    }
    ras
}
