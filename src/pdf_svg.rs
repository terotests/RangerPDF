//! A rendered page (`PageOut`) as an SVG file: the same fills, strokes,
//! clips, images and text the raster draws, for a host that draws vectors
//! itself (a browser): the file is small, and stays sharp at any zoom.
//! Coordinates are points, 1/100 of a point exact; a quarter turn is a
//! transform of the whole page. Images go in as data URLs: a JPEG as it
//! is, other pixels as a PNG.

use crate::pdf_display;
use crate::pdf_display::PageOut;
use crate::pdf_path;
use crate::pdf_png;

/// `v` with at most two decimals, without trailing zeros
pub fn num(v: f64) -> String {
    let neg = v < 0.0;
    let mut n = ((if neg { 0.0 - v } else { v }) * 100.0 + 0.5) as i64;
    if n == 0 {
        return "0".to_string();
    }
    let whole = n / 100;
    n = n % 100;
    let mut s = if neg { format!("-{}", whole) } else { format!("{}", whole) };
    if n > 0 {
        if n % 10 == 0 {
            s = format!("{}.{}", s, n / 10);
        } else if n < 10 {
            s = format!("{}.0{}", s, n);
        } else {
            s = format!("{}.{}", s, n);
        }
    }
    s
}

fn hex2(v: i64) -> String {
    let d = "0123456789abcdef";
    let c = if v < 0 { 0 } else if v > 255 { 255 } else { v };
    let hi = (c / 16) as usize;
    let lo = (c % 16) as usize;
    format!("{}{}", &d[hi..hi + 1], &d[lo..lo + 1])
}

fn color(r: i64, g: i64, b: i64) -> String {
    format!("#{}{}{}", hex2(r), hex2(g), hex2(b))
}

/// The rings of a command as path data: relative moves and lines, each
/// ring closed when `closed` says so (fills always)
fn path_data(pts: &Vec<f64>, ends: &Vec<i64>, closed: &Vec<bool>, fill: bool) -> String {
    let mut out: Vec<String> = vec![];
    let mut start: usize = 0;
    let mut k: usize = 0;
    let mut px = 0.0;
    let mut py = 0.0;
    while k < ends.len() {
        let end = ends[k] as usize;
        let mut i = start;
        while i < end {
            let x = pts[i * 2];
            let y = pts[i * 2 + 1];
            if i == start {
                if out.is_empty() {
                    out.push(format!("M{} {}", num(x), num(y)));
                } else {
                    out.push(format!("m{} {}", num(x - px), num(y - py)));
                }
            } else if i == start + 1 {
                out.push(format!("l{} {}", num(x - px), num(y - py)));
            } else {
                out.push(format!(" {} {}", num(x - px), num(y - py)));
            }
            // relative to the rounded point, so that the errors do not add up
            px = ((x * 100.0).round()) / 100.0;
            py = ((y * 100.0).round()) / 100.0;
            i += 1;
        }
        let shut = fill || (k < closed.len() && closed[k]);
        if shut && end > start {
            out.push("z".to_string());
            // after z the point is the ring's start
            px = ((pts[start * 2] * 100.0).round()) / 100.0;
            py = ((pts[start * 2 + 1] * 100.0).round()) / 100.0;
        }
        start = end;
        k += 1;
    }
    out.join("")
}

/// A glyph's outline under the linear part of `m` (no translation): path
/// data in points, curves kept
fn glyph_data(g: &pdf_path::Path, m: &Vec<f64>) -> String {
    let mut out: Vec<String> = vec![];
    let mut j: usize = 0;
    let mut k: usize = 0;
    while k < g.ops.len() {
        let op = g.ops[k];
        if op == pdf_path::CLOSE {
            out.push("z".to_string());
        } else {
            let n = if op == pdf_path::CURVE { 3 } else { 1 };
            let mut parts: Vec<String> = vec![];
            let mut p = 0;
            while p < n {
                let x = g.xy[j];
                let y = g.xy[j + 1];
                parts.push(format!("{} {}", num(m[0] * x + m[2] * y), num(m[1] * x + m[3] * y)));
                j += 2;
                p += 1;
            }
            let letter = if op == pdf_path::MOVE { "M" } else if op == pdf_path::LINE { "L" } else { "C" };
            out.push(format!("{}{}", letter, parts.join(" ")));
        }
        k += 1;
    }
    out.join("")
}

fn alpha_attr(name: &str, a: f64) -> String {
    if a >= 0.999 {
        return String::new();
    }
    format!(" {}=\"{}\"", name, num(a))
}

fn escape(s: &str) -> String {
    let mut out = String::new();
    for ch in s.chars() {
        if ch == '&' {
            out.push_str("&amp;");
        } else if ch == '<' {
            out.push_str("&lt;");
        } else if ch == '>' {
            out.push_str("&gt;");
        } else if ch == '"' {
            out.push_str("&quot;");
        } else {
            out.push(ch);
        }
    }
    out
}

pub fn base64(d: &Vec<u8>) -> String {
    let abc: Vec<char> = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/".chars().collect();
    let mut out = String::new();
    let mut i: usize = 0;
    while i + 2 < d.len() {
        let n = ((d[i] as usize) << 16) | ((d[i + 1] as usize) << 8) | (d[i + 2] as usize);
        out.push(abc[(n >> 18) & 63]);
        out.push(abc[(n >> 12) & 63]);
        out.push(abc[(n >> 6) & 63]);
        out.push(abc[n & 63]);
        i += 3;
    }
    let left = d.len() - i;
    if left == 1 {
        let n = (d[i] as usize) << 16;
        out.push(abc[(n >> 18) & 63]);
        out.push(abc[(n >> 12) & 63]);
        out.push('=');
        out.push('=');
    } else if left == 2 {
        let n = ((d[i] as usize) << 16) | ((d[i + 1] as usize) << 8);
        out.push(abc[(n >> 18) & 63]);
        out.push(abc[(n >> 12) & 63]);
        out.push(abc[(n >> 6) & 63]);
        out.push('=');
    }
    out
}

/// An image's data URL: the JPEG as it is (its mask is left out), else
/// the pixels as a PNG
fn image_url(img: &crate::pdf_image::Img) -> String {
    if !img.jpeg.is_empty() && img.alpha.is_empty() {
        return format!("data:image/jpeg;base64,{}", base64(&img.jpeg));
    }
    if img.rgba.is_empty() || img.w <= 0 || img.h <= 0 {
        return String::new();
    }
    format!("data:image/png;base64,{}", base64(&pdf_png::encode_rgba_small(img.w, img.h, &img.rgba)))
}

/// The page as an SVG file, turned clockwise by `quarter` quarter turns.
pub fn page_svg(page: &PageOut, quarter: i64) -> String {
    let q = ((quarter % 4) + 4) % 4;
    let w = page.width;
    let h = page.height;
    let (vw, vh) = if q % 2 == 1 { (h, w) } else { (w, h) };
    let mut out: Vec<String> = vec![];
    out.push(format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" viewBox=\"0 0 {} {}\" width=\"{}\" height=\"{}\">",
        num(vw), num(vh), num(vw), num(vh)
    ));
    out.push(format!("<rect width=\"{}\" height=\"{}\" fill=\"#fff\"/>", num(vw), num(vh)));
    if q == 1 {
        out.push(format!("<g transform=\"matrix(0 1 -1 0 {} 0)\">", num(h)));
    } else if q == 2 {
        out.push(format!("<g transform=\"matrix(-1 0 0 -1 {} {})\">", num(w), num(h)));
    } else if q == 3 {
        out.push(format!("<g transform=\"matrix(0 -1 1 0 0 {})\">", num(w)));
    } else {
        out.push("<g>".to_string());
    }
    // images used more than once go in once
    let mut urls: Vec<String> = vec![];
    let mut defined: Vec<bool> = vec![];
    let mut n = 0;
    while n < page.images.len() {
        urls.push(String::new());
        defined.push(false);
        n += 1;
    }
    // glyphs by outline and size: defined once, each use placed by x y
    let mut defs: Vec<String> = vec![];
    let mut glyph_keys: Vec<String> = vec![];
    let mut clips = 0;
    let mut open = 0;
    for c in page.cmds.iter() {
        if c.op == pdf_display::FILL {
            if c.ends.is_empty() {
                continue;
            }
            let rule = if c.even_odd { " fill-rule=\"evenodd\"" } else { "" };
            out.push(format!(
                "<path d=\"{}\" fill=\"{}\"{}{}/>",
                path_data(&c.pts, &c.ends, &c.closed, true), color(c.r, c.g, c.b), rule, alpha_attr("fill-opacity", c.a)
            ));
        } else if c.op == pdf_display::STROKE {
            if c.ends.is_empty() {
                continue;
            }
            let mut extra = String::new();
            if c.cap == 1 {
                extra.push_str(" stroke-linecap=\"round\"");
            } else if c.cap == 2 {
                extra.push_str(" stroke-linecap=\"square\"");
            }
            if c.join == 1 {
                extra.push_str(" stroke-linejoin=\"round\"");
            } else if c.join == 2 {
                extra.push_str(" stroke-linejoin=\"bevel\"");
            } else if c.miter != 4.0 {
                extra.push_str(&format!(" stroke-miterlimit=\"{}\"", num(c.miter)));
            }
            if !c.dash.is_empty() {
                let mut parts: Vec<String> = vec![];
                for d in c.dash.iter() {
                    parts.push(num(*d));
                }
                extra.push_str(&format!(" stroke-dasharray=\"{}\"", parts.join(" ")));
                if c.phase != 0.0 {
                    extra.push_str(&format!(" stroke-dashoffset=\"{}\"", num(c.phase)));
                }
            }
            out.push(format!(
                "<path d=\"{}\" fill=\"none\" stroke=\"{}\" stroke-width=\"{}\"{}{}/>",
                path_data(&c.pts, &c.ends, &c.closed, false), color(c.r, c.g, c.b), num(c.width), extra, alpha_attr("stroke-opacity", c.a)
            ));
        } else if c.op == pdf_display::CLIP {
            let rule = if c.even_odd { " clip-rule=\"evenodd\"" } else { "" };
            out.push(format!(
                "<clipPath id=\"c{}\"><path d=\"{}\"{}/></clipPath><g clip-path=\"url(#c{})\">",
                clips, path_data(&c.pts, &c.ends, &c.closed, true), rule, clips
            ));
            clips += 1;
            open += 1;
        } else if c.op == pdf_display::UNCLIP {
            if open > 0 {
                out.push("</g>".to_string());
                open -= 1;
            }
        } else if c.op == pdf_display::IMAGE {
            if c.image < 0 || (c.image as usize) >= page.images.len() || c.m.len() < 6 {
                continue;
            }
            let k = c.image as usize;
            if !defined[k] {
                urls[k] = image_url(&page.images[k]);
                defined[k] = true;
            }
            if urls[k].is_empty() {
                continue;
            }
            // the unit square, y up, into the parallelogram of m
            let m = &c.m;
            out.push(format!(
                "<image width=\"1\" height=\"1\" preserveAspectRatio=\"none\" transform=\"matrix({} {} {} {} {} {})\"{} xlink:href=\"{}\"/>",
                num(m[0]), num(m[1]), num(0.0 - m[2]), num(0.0 - m[3]), num(m[2] + m[4]), num(m[3] + m[5]), alpha_attr("opacity", c.a), urls[k]
            ));
        } else if c.op == pdf_display::GLYPH {
            if c.image < 0 || (c.image as usize) >= page.glyphs.len() || c.m.len() < 6 {
                continue;
            }
            let m = &c.m;
            let key = format!("{}|{}|{}|{}|{}", c.image, num(m[0] * 1000.0), num(m[1] * 1000.0), num(m[2] * 1000.0), num(m[3] * 1000.0));
            let mut at: i64 = -1;
            let mut k: usize = 0;
            while k < glyph_keys.len() {
                if glyph_keys[k] == key {
                    at = k as i64;
                    break;
                }
                k += 1;
            }
            if at < 0 {
                at = glyph_keys.len() as i64;
                glyph_keys.push(key);
                defs.push(format!("<path id=\"g{}\" d=\"{}\"/>", at, glyph_data(&page.glyphs[c.image as usize], m)));
            }
            let fill = color(c.r, c.g, c.b);
            let fill_attr = if fill == "#000000" { String::new() } else { format!(" fill=\"{}\"", fill) };
            out.push(format!("<use xlink:href=\"#g{}\" x=\"{}\" y=\"{}\"{}{}/>", at, num(m[4]), num(m[5]), fill_attr, alpha_attr("fill-opacity", c.a)));
        } else if c.op == pdf_display::TEXT {
            if c.m.len() < 6 {
                continue;
            }
            let m = &c.m;
            let mut style = String::new();
            if c.bold {
                style.push_str(" font-weight=\"bold\"");
            }
            if c.italic {
                style.push_str(" font-style=\"italic\"");
            }
            out.push(format!(
                "<text transform=\"matrix({} {} {} {} {} {})\" font-size=\"1\" font-family=\"{}, sans-serif\" fill=\"{}\"{}{}>{}</text>",
                num(m[0]), num(m[1]), num(0.0 - m[2]), num(0.0 - m[3]), num(m[4]), num(m[5]), escape(&c.family), color(c.r, c.g, c.b), style, alpha_attr("fill-opacity", c.a), escape(&c.text)
            ));
        }
    }
    while open > 0 {
        out.push("</g>".to_string());
        open -= 1;
    }
    out.push("</g></svg>".to_string());
    if !defs.is_empty() {
        // after the root and the white ground
        let mut all: Vec<String> = vec![];
        let mut i: usize = 0;
        while i < out.len() {
            if i == 2 {
                all.push(format!("<defs>{}</defs>", defs.join("")));
            }
            all.push(out[i].clone());
            i += 1;
        }
        return all.join("\n");
    }
    out.join("\n")
}
