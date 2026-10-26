//! Tests of rendering: content streams to display lists, fonts, text and pixels.

use crate::check::Check;
use crate::pdf_display;
use crate::pdf_inflate;
use crate::pdf_obj;
use crate::pdf::PdfDocument;
use crate::pdf_picture;
use crate::pdf_png;
use crate::pdf_raster;
use crate::pdf_type1::Type1;
use crate::tests::build_pdf;
use crate::tests::simple_pdf;

fn page_with(content: &str, resources: &str, extra: &Vec<String>) -> Vec<u8> {
    let mut objs: Vec<String> = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 200 100] >>".to_string(),
        format!("<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources {} >>", resources),
        format!("<< /Length {} >>\nstream\n{}\nendstream", content.chars().count(), content),
    ];
    for e in extra.iter() {
        objs.push(e.clone());
    }
    build_pdf(&objs, "")
}

fn pixel(r: &pdf_raster::Raster, x: i64, y: i64) -> String {
    let i = ((y * r.w + x) * 4) as usize;
    format!("{},{},{}", r.px[i], r.px[i + 1], r.px[i + 2])
}

fn paths(c: &mut Check) {
    c.test("pdf:render:path pdf:render:color", "filled rectangle");
    let mut d = PdfDocument::open(page_with("1 0 0 rg 10 10 50 30 re f", "<< >>", &vec![]), "");
    let p = d.render(0);
    c.eqi("cmds", p.cmds.len() as i64, 1);
    let f = p.cmds[0].clone();
    c.eqi("op", f.op, pdf_display::FILL);
    c.eq("color", &format!("{} {} {}", f.r, f.g, f.b), "255 0 0");
    // y is flipped: user y 10..40 is device y 60..90
    c.eq("pts", &format!("{:?}", f.pts), "[10.0, 90.0, 60.0, 90.0, 60.0, 60.0, 10.0, 60.0]");
    let ras = pdf_raster::rasterize(&p, 1.0);
    c.eq("inside", &pixel(&ras, 30, 75), "255,0,0");
    c.eq("outside", &pixel(&ras, 5, 5), "255,255,255");

    c.test("pdf:render:cm pdf:render:q", "transform, save and restore");
    let mut d2 = PdfDocument::open(page_with("q 2 0 0 2 0 0 cm 0 0 1 rg 0 0 10 10 re f Q 0 g 0 0 5 5 re f", "<< >>", &vec![]), "");
    let p2 = d2.render(0);
    c.eqi("cmds", p2.cmds.len() as i64, 2);
    c.eq("scaled", &format!("{:?}", p2.cmds[0].pts), "[0.0, 100.0, 20.0, 100.0, 20.0, 80.0, 0.0, 80.0]");
    c.eq("restored", &format!("{} {:?}", p2.cmds[1].b, p2.cmds[1].pts), "0 [0.0, 100.0, 5.0, 100.0, 5.0, 95.0, 0.0, 95.0]");

    c.test("pdf:render:stroke", "stroke width and dash");
    let mut d3 = PdfDocument::open(page_with("2 w [3 1] 0 d 0 0 1 RG 10 50 m 190 50 l S", "<< >>", &vec![]), "");
    let p3 = d3.render(0);
    c.eqi("op", p3.cmds[0].op, pdf_display::STROKE);
    c.eq("width", &format!("{}", p3.cmds[0].width), "2");
    c.eq("dash", &format!("{:?}", p3.cmds[0].dash), "[3.0, 1.0]");
    let r3 = pdf_raster::rasterize(&p3, 1.0);
    c.eq("on the line", &pixel(&r3, 11, 50), "0,0,255");
    c.eq("in a gap", &pixel(&r3, 13, 50), "255,255,255");

    c.test("pdf:render:clip", "clipping");
    let mut d4 = PdfDocument::open(page_with("q 0 0 100 100 re W n 1 0 0 rg 0 0 200 100 re f Q", "<< >>", &vec![]), "");
    let p4 = d4.render(0);
    let r4 = pdf_raster::rasterize(&p4, 1.0);
    c.eq("clipped in", &pixel(&r4, 50, 50), "255,0,0");
    c.eq("clipped out", &pixel(&r4, 150, 50), "255,255,255");
    c.eqi("unclip at end", p4.cmds[p4.cmds.len() - 1].op, pdf_display::UNCLIP);

    c.test("pdf:render:curve pdf:render:evenodd", "curves and even-odd");
    let mut d5 = PdfDocument::open(page_with("0 g 100 50 m 100 77.6 77.6 100 50 100 c 22.4 100 0 77.6 0 50 c 0 22.4 22.4 0 50 0 c 77.6 0 100 22.4 100 50 c h 25 25 50 50 re f*", "<< >>", &vec![]), "");
    let p5 = d5.render(0);
    let r5 = pdf_raster::rasterize(&p5, 1.0);
    c.eq("ring", &pixel(&r5, 50, 10), "0,0,0");
    c.eq("hole", &pixel(&r5, 50, 50), "255,255,255");
    c.eq("corner outside the circle", &pixel(&r5, 3, 3), "255,255,255");
}

fn colors(c: &mut Check) {
    c.test("pdf:color:cmyk pdf:color:separation pdf:func:2", "device and separation colours");
    let extra: Vec<String> = vec!["[/Separation /Spot /DeviceCMYK << /FunctionType 2 /Domain [0 1] /C0 [0 0 0 0] /C1 [0 1 1 0] /N 1 >>]".to_string()];
    let mut d = PdfDocument::open(page_with("0 0 0 1 k 0 0 10 10 re f /CS0 cs 1 sc 10 0 10 10 re f 0.5 g 20 0 10 10 re f", "<< /ColorSpace << /CS0 5 0 R >> >>", &extra), "");
    let p = d.render(0);
    c.eq("cmyk black", &format!("{} {} {}", p.cmds[0].r, p.cmds[0].g, p.cmds[0].b), "0 0 0");
    c.eq("separation red", &format!("{} {} {}", p.cmds[1].r, p.cmds[1].g, p.cmds[1].b), "255 0 0");
    c.eq("gray", &format!("{} {} {}", p.cmds[2].r, p.cmds[2].g, p.cmds[2].b), "128 128 128");

    c.test("pdf:func:4 pdf:color:devicen", "PostScript function");
    let fnc = "{ dup 0.5 gt { pop 1 0 0 } { pop 0 0 1 } ifelse }";
    let extra2: Vec<String> = vec![
        "[/DeviceN [/A] /DeviceRGB 6 0 R]".to_string(),
        format!("<< /FunctionType 4 /Domain [0 1] /Range [0 1 0 1 0 1] /Length {} >>\nstream\n{}\nendstream", fnc.chars().count(), fnc),
    ];
    let mut d2 = PdfDocument::open(page_with("/C cs 0.7 scn 0 0 10 10 re f 0.2 scn 0 0 10 10 re f", "<< /ColorSpace << /C 5 0 R >> >>", &extra2), "");
    let p2 = d2.render(0);
    c.eq("above", &format!("{} {} {}", p2.cmds[0].r, p2.cmds[0].g, p2.cmds[0].b), "255 0 0");
    c.eq("below", &format!("{} {} {}", p2.cmds[1].r, p2.cmds[1].g, p2.cmds[1].b), "0 0 255");

    c.test("pdf:color:indexed pdf:image", "indexed image");
    let extra3: Vec<String> = vec![format!("<< /Type /XObject /Subtype /Image /Width 2 /Height 1 /BitsPerComponent 8 /ColorSpace [/Indexed /DeviceRGB 1 <FF000000FF00>] /Length 2 >>\nstream\n{}{}\nendstream", pdf_obj::ch(0), pdf_obj::ch(1))];
    let mut d3 = PdfDocument::open(page_with("q 100 0 0 50 0 0 cm /Im0 Do Q", "<< /XObject << /Im0 5 0 R >> >>", &extra3), "");
    let p3 = d3.render(0);
    c.eqi("images", p3.images.len() as i64, 1);
    if p3.images.len() == 1 {
        c.eq("pixels", &format!("{:?}", p3.images[0].rgba), "[255, 0, 0, 255, 0, 255, 0, 255]");
    }
    let r3 = pdf_raster::rasterize(&p3, 1.0);
    c.eq("left", &pixel(&r3, 25, 75), "255,0,0");
    c.eq("right", &pixel(&r3, 75, 75), "0,255,0");
}

fn text(c: &mut Check) {
    c.test("pdf:text:simple pdf:font:std14", "standard font text and widths");
    let mut d = PdfDocument::open(simple_pdf("BT /F1 10 Tf 10 50 Td (Hello) Tj ET"), "");
    let p = d.render(0);
    c.eq("text", &p.plain_text(), "Hello");
    c.eqi("chars", p.text.len() as i64, 5);
    // Helvetica: H is 722/1000 em
    c.eq("H width", &fx(2, p.text[0].w), "7.22");
    c.eq("e x", &fx(2, p.text[1].x), "17.22");
    // no substitute font: the host draws the text
    c.eqi("text cmd", p.cmds[0].op, pdf_display::TEXT);
    c.eq("family", &p.cmds[0].family, "sans");

    c.test("pdf:text:TJ pdf:text:spacing", "kerning, word and character spacing");
    let mut d2 = PdfDocument::open(simple_pdf("BT /F1 10 Tf 2 Tw 1 Tc 0 50 Td [(A) -1000 (B)] TJ ( C) Tj ET"), "");
    let p2 = d2.render(0);
    c.eq("text", &p2.plain_text(), "A B C");
    // A 667 + Tc 1 + 10 (the -1000 adjustment) = 17.67
    c.eq("B x", &fx(2, p2.text[1].x), "17.67");

    c.test("pdf:text:tounicode pdf:font:type0", "Type0 font with ToUnicode");
    let cmap = "/CIDInit /ProcSet findresource begin 12 dict begin begincmap 1 begincodespacerange <0000> <FFFF> endcodespacerange 2 beginbfchar <0001> <0048> <0002> <00E4> endbfchar 1 beginbfrange <0003> <0004> <0061> endbfrange endcmap end end";
    let extra: Vec<String> = vec![
        "<< /Type /Font /Subtype /Type0 /BaseFont /X /Encoding /Identity-H /DescendantFonts [6 0 R] /ToUnicode 7 0 R >>".to_string(),
        "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /X /DW 500 /W [1 [600 700]] >>".to_string(),
        format!("<< /Length {} >>\nstream\n{}\nendstream", cmap.chars().count(), cmap),
    ];
    let mut d3 = PdfDocument::open(page_with("BT /F0 10 Tf 0 50 Td <0001000200030004> Tj ET", "<< /Font << /F0 5 0 R >> >>", &extra), "");
    let p3 = d3.render(0);
    c.eq("text", &p3.plain_text(), "Häab");
    c.eq("x of b", &fx(1, p3.text[3].x), "18.0");

    c.test("pdf:font:differences pdf:font:encoding", "encoding with differences");
    let extra4: Vec<String> = vec!["<< /Type /Font /Subtype /Type1 /BaseFont /Times-Roman /Encoding << /BaseEncoding /WinAnsiEncoding /Differences [65 /Euro /adieresis] >> >>".to_string()];
    let mut d4 = PdfDocument::open(page_with("BT /F0 12 Tf 0 50 Td (AB\\200\\344) Tj ET", "<< /Font << /F0 5 0 R >> >>", &extra4), "");
    let p4 = d4.render(0);
    c.eq("text", &p4.plain_text(), "€ä€ä");
}

fn api(c: &mut Check) {
    c.test("pdf:api", "page sizes and errors");
    let d = PdfDocument::open(simple_pdf("0 0 m"), "");
    c.eqi("pages", d.page_count(), 1);
    c.eq("size", &format!("{}x{}", d.page_width(0), d.page_height(0)), "200x100");
    let bad = PdfDocument::open(pdf_obj::bytes_of("not a pdf"), "");
    c.eq("error", &bad.error(), "no pages");
}

fn viewer(c: &mut Check) {
    c.test("pdf:png", "page as PNG");
    let px: Vec<u8> = vec![255, 0, 0, 255];
    let f = pdf_png::encode_rgba(1, 1, &px);
    c.eq("signature", &format!("{:?}", part(&f, 0, 8)), "[137, 80, 78, 71, 13, 10, 26, 10]");
    let n = f.len();
    c.eq("IEND crc", &format!("{:?}", part(&f, n - 4, n)), "[174, 66, 96, 130]");
    // IHDR is 8 + 25 bytes; IDAT's data starts after its length and type
    let len = ((f[33] as usize) << 24) | ((f[34] as usize) << 16) | ((f[35] as usize) << 8) | (f[36] as usize);
    let z = part(&f, 41, 41 + len);
    let raw = pdf_inflate::inflate(&z);
    c.ok("inflates", raw.ok);
    c.eq("scanline", &format!("{:?}", raw.data), "[0, 255, 0, 0, 255]");
    let mut d = PdfDocument::open(simple_pdf("1 0 0 rg 0 0 100 100 re f"), "");
    let pf = d.page_png(0, 400.0);
    c.eq("size", &format!("{:?}", part(&pf, 16, 24)), "[0, 0, 1, 144, 0, 0, 0, 200]");

    c.test("pdf:find", "find on a page");
    let mut d2 = PdfDocument::open(simple_pdf("BT /F1 10 Tf 10 50 Td (Hello World) Tj ET"), "");
    let hits = d2.find(0, "WORLD");
    c.eqi("hits", hits.len() as i64, 1);
    if hits.len() == 1 {
        c.eq("x", &fx(1, hits[0].x), "35.6");
        c.eq("w", &fx(1, hits[0].w), "26.1");
    }
    c.eqi("none", d2.find(0, "xyz").len() as i64, 0);

    c.test("pdf:outline", "bookmarks");
    let objs: Vec<String> = vec![
        "<< /Type /Catalog /Pages 2 0 R /Outlines 5 0 R /Names << /Dests 8 0 R >> >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 /MediaBox [0 0 200 100] >>".to_string(),
        "<< /Type /Page /Parent 2 0 R >>".to_string(),
        "<< /Type /Page /Parent 2 0 R >>".to_string(),
        "<< /Type /Outlines /First 6 0 R >>".to_string(),
        "<< /Title (One) /Dest [4 0 R /Fit] /Next 7 0 R /First 9 0 R >>".to_string(),
        "<< /Title (Two) /Dest (nd) >>".to_string(),
        "<< /Names [(nd) [3 0 R /XYZ 0 0 0]] >>".to_string(),
        "<< /Title (Child) /A << /S /GoTo /D [3 0 R /Fit] >> >>".to_string(),
    ];
    let mut d3 = PdfDocument::open(build_pdf(&objs, ""), "");
    let mut got = String::new();
    for it in d3.outline().iter() {
        got.push_str(&format!("{}:{}:{} ", it.title, it.level, it.page));
    }
    c.eq("items", &got, "One:1:1 Child:2:0 Two:1:0 ");
}

fn annots(c: &mut Check) {
    c.test("pdf:annot", "annotation appearances");
    // a 10x10 appearance fitted to a 40x20 Rect; the hidden one is not drawn
    let objs: Vec<String> = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 200 100] >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /Annots [4 0 R 6 0 R] >>".to_string(),
        "<< /Type /Annot /Subtype /Widget /Rect [100 10 140 30] /AP << /N 5 0 R >> >>".to_string(),
        "<< /Type /XObject /Subtype /Form /BBox [0 0 10 10] /Length 18 >>\nstream\n0 0 1 rg 0 0 10 10 re f\nendstream".to_string(),
        "<< /Type /Annot /Subtype /Square /F 2 /Rect [0 0 50 50] /AP << /N 5 0 R >> >>".to_string(),
    ];
    let mut d = PdfDocument::open(build_pdf(&objs, ""), "");
    let p = d.render(0);
    let r = pdf_raster::rasterize(&p, 1.0);
    c.eq("inside", &pixel(&r, 120, 80), "0,0,255");
    c.eq("outside", &pixel(&r, 95, 80), "255,255,255");
    c.eq("hidden", &pixel(&r, 20, 80), "255,255,255");
}

/// Type 1 encryption: `key` 55665 for eexec, 4330 for charstrings.
fn t1_encrypt(plain: &Vec<u8>, key: i64) -> Vec<u8> {
    let mut r = key;
    let mut out: Vec<u8> = vec![];
    for b in plain.iter() {
        let c = ((*b as i64) ^ (r >> 8)) & 255;
        r = ((c + r) * 52845 + 22719) & 65535;
        out.push(c as u8);
    }
    out
}

fn t1_charstring(ops: Vec<i64>) -> Vec<u8> {
    let mut plain: Vec<u8> = vec![0, 0, 0, 0];
    for o in ops.iter() {
        plain.push(*o as u8);
    }
    t1_encrypt(&plain, 4330)
}

fn push_text(v: &mut Vec<u8>, s: &str) {
    for b in pdf_obj::bytes_of(s).iter() {
        v.push(*b);
    }
}

fn push_entry(v: &mut Vec<u8>, head: &str, cs: &Vec<u8>, tail: &str) {
    push_text(v, &format!("{} {} RD ", head, cs.len()));
    for b in cs.iter() {
        v.push(*b);
    }
    push_text(v, tail);
}

fn type1(c: &mut Check) {
    c.test("pdf:type1", "embedded Type 1 font with Subrs and an encoding");
    // as Word, TeX and Ghostscript write them: `/Subrs N array`, `/name len RD`
    let subr = t1_charstring(vec![248, 136, 139, 5, 11]); // 500 0 rlineto return
    let a = t1_charstring(vec![139, 248, 136, 13, 239, 239, 21, 139, 10, 139, 248, 136, 5, 9, 14]);
    let notdef = t1_charstring(vec![139, 248, 136, 13, 14]);
    let mut pr: Vec<u8> = vec![0, 0, 0, 0];
    push_text(&mut pr, "dup /Private 8 dict dup begin\n/lenIV 4 def\n/Subrs 1 array\n");
    push_entry(&mut pr, "dup 0", &subr, " NP\n");
    push_text(&mut pr, "ND\n2 index /CharStrings 2 dict dup begin\n");
    push_entry(&mut pr, "/.notdef", &notdef, " ND\n");
    push_entry(&mut pr, "/A", &a, " ND\n");
    push_text(&mut pr, "end\nend\n");
    let enc = t1_encrypt(&pr, 55665);
    let mut f: Vec<u8> = vec![];
    push_text(&mut f, "%!PS-AdobeFont-1.0: Test\n/FontMatrix [0.001 0 0 0.001 0 0] readonly def\n/Encoding 256 array\n0 1 255 {1 index exch /.notdef put} for\ndup 65 /A put\nreadonly def\ncurrentfile eexec\n");
    let hexd = "0123456789abcdef";
    let hx: Vec<char> = hexd.chars().collect();
    let mut h = String::new();
    for b in enc.iter() {
        h.push(hx[(*b as usize) / 16]);
        h.push(hx[(*b as usize) % 16]);
    }
    push_text(&mut f, &h);
    push_text(&mut f, "\ncleartomark\n");
    let t = Type1::parse(&f, 0);
    c.eq("ok", if t.ok { "yes" } else { "no" }, "yes");
    c.eq("encoding", &t.encoding[65], "A");
    c.eq("has A", if t.has_glyph("A") { "yes" } else { "no" }, "yes");
    let mut w: Vec<f64> = vec![];
    let p = t.glyph_path("A", &mut w);
    c.eq("width", &fx(0, if w.is_empty() { -1.0 } else { w[0] }), "500.0");
    // moveto, the subr's lineto, lineto, close
    c.eq("ops", &format!("{}", p.ops.len()), "4");
    c.eq("subr end", &format!("{},{}", fx(0, p.xy[2]), fx(0, p.xy[3])), "600.0,100.0");
}

/// Bytes from a hex string (`to_digit` is not on every target).
fn unhex(s: &str) -> Vec<u8> {
    let digits: Vec<char> = "0123456789abcdef".chars().collect();
    let cs: Vec<char> = s.chars().collect();
    let mut out: Vec<u8> = vec![];
    let mut i: usize = 0;
    while i + 1 < cs.len() {
        let mut hi: i64 = 0;
        let mut lo: i64 = 0;
        let mut k: usize = 0;
        while k < 16 {
            if digits[k] == cs[i] {
                hi = k as i64;
            }
            if digits[k] == cs[i + 1] {
                lo = k as i64;
            }
            k += 1;
        }
        out.push((hi * 16 + lo) as u8);
        i += 2;
    }
    out
}

/// Pixels (1,1) (8,7) (0,0) (11,9) (6,4) of a 12x10 RGBA picture; each one
/// within `tol` per channel of `want` is written as `want` has it.
fn pic_pixels(rgba: &Vec<u8>, w: i64, want: &str, tol: i64) -> String {
    let pts: Vec<i64> = vec![1, 1, 8, 7, 0, 0, 11, 9, 6, 4];
    let wants: Vec<&str> = want.split(';').collect();
    let mut out = String::new();
    let mut i: usize = 0;
    while i < 5 {
        let at = ((pts[i * 2 + 1] * w + pts[i * 2]) * 4) as usize;
        let parts: Vec<&str> = wants[i].split(',').collect();
        let mut close = true;
        let mut k: usize = 0;
        while k < 4 {
            let wv = crate::pdf_cff::parse_int(parts[k]);
            let gv = rgba[at + k] as i64;
            if gv - wv > tol || wv - gv > tol {
                close = false;
            }
            k += 1;
        }
        if i > 0 {
            out.push(';');
        }
        if close {
            out.push_str(wants[i]);
        } else {
            out.push_str(&format!("{},{},{},{}", rgba[at], rgba[at + 1], rgba[at + 2], rgba[at + 3]));
        }
        i += 1;
    }
    out
}

/// Checks a 12x10 picture file against Pillow's pixels.
fn pic_case(c: &mut Check, name: &str, hex: &str, want: &str, tol: i64) {
    let p = pdf_picture::decode(&unhex(hex));
    c.eq(&format!("{} ok", name), &format!("{} {}x{} {}", p.ok, p.w, p.h, p.error), "true 12x10 ");
    if p.ok {
        c.eq(&format!("{} pixels", name), &pic_pixels(&p.rgba, p.w, want, tol), want);
    }
}

fn pictures(c: &mut Check) {
    c.test("pdf:pictures", "PNG, JPEG, BMP, GIF and TIFF files to RGBA; JPEG images in PDF");
    pic_case(c, "png_pal_il", "89504e470d0a1a0a0000000d494844520000000c0000000a0803000001c0f57f5e000000206348524d00007a26000080840000fa00000080e8000075300000ea6000003a98000017709cba513c0000000c504c5445c81e1efafafa00a0001428b4f182aefa0000000774494d4507ea0a1715213aac0b48090000003a4944415408d7354c870d00300802fdffe7825013224b01820b681d288c2d7ba02967ab38df3ce58cce262b23af8d74dbcb1bd7c472d0537ef100238500a32f563c1f0000000049454e44ae426082", "200,30,30,255;20,40,180,255;200,30,30,255;20,40,180,255;0,160,0,255", 1);
    pic_case(c, "png_rgba", "89504e470d0a1a0a0000000d494844520000000c0000000a0806000000802cbffa0000006949444154789c958eb11180200c001fe498c04ea7701c96d1d24998c01d5c8249d268858788285fe63e9f28a5bb651f87998c29847c84f5165d924b586f1127983f22803801a82fc46a8a6e910194881c25b95fb762e871e1ad1c31a90854e56be1ab7a7ba9450638013b3a242614d27c4d0000000049454e44ae426082", "200,30,30,255;20,40,180,255;1,2,3,128;20,40,180,255;0,160,0,255", 1);
    pic_case(c, "jpeg_420", "ffd8ffe000104a46494600010100000100010000ffdb0043000201010101010201010102020202020403020202020504040304060506060605060606070908060709070606080b08090a0a0a0a0a06080b0c0b0a0c090a0a0affdb004301020202020202050303050a0706070a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0affc0001108000a000c03012200021101031101ffc4001f0000010501010101010100000000000000000102030405060708090a0bffc400b5100002010303020403050504040000017d01020300041105122131410613516107227114328191a1082342b1c11552d1f02433627282090a161718191a25262728292a3435363738393a434445464748494a535455565758595a636465666768696a737475767778797a838485868788898a92939495969798999aa2a3a4a5a6a7a8a9aab2b3b4b5b6b7b8b9bac2c3c4c5c6c7c8c9cad2d3d4d5d6d7d8d9dae1e2e3e4e5e6e7e8e9eaf1f2f3f4f5f6f7f8f9faffc4001f0100030101010101010101010000000000000102030405060708090a0bffc400b51100020102040403040705040400010277000102031104052131061241510761711322328108144291a1b1c109233352f0156272d10a162434e125f11718191a262728292a35363738393a434445464748494a535455565758595a636465666768696a737475767778797a82838485868788898a92939495969798999aa2a3a4a5a6a7a8a9aab2b3b4b5b6b7b8b9bac2c3c4c5c6c7c8c9cad2d3d4d5d6d7d8d9dae2e3e4e5e6e7e8e9eaf2f3f4f5f6f7f8f9faffda000c03010002110311003f00e9b42fd96be327fc11fbe2ec5fb60f8e7c67e05f19c5f0d370f17e8de08d46e350bcd022d4edce9d6f757504d15aa7facbf8248ed5ae2196e543f9656349a787d23fe1fc3ff0485f10ff00c4cfe347eccdf147e25ebadc4de27f883e00f0d5fdeb463848502dda436f0a8e91411c71962f232b4b2cb23fb27fc14b3f7dff0006e8de6ab2fcd75aa7813c1ba96a772dcc9777b75aae957173732b759269a796596491b2cf248eec4b3127f9f5afd7bc15f09b83f8ab84a78ecc69ca4dd5765ccf952f674e4b4d5392bbf7b7ed65a1e671e71de7fc7799d0ccf38e49c9d28a8c79128c63cd2925cb769c97334e6d5fb28a763fffd9", "196,28,28,255;17,41,173,255;195,31,32,255;28,37,176,255;82,99,107,255", 4);
    pic_case(c, "jpeg_prog", "ffd8ffe000104a46494600010100000100010000ffdb0043000201010101010201010102020202020403020202020504040304060506060605060606070908060709070606080b08090a0a0a0a0a06080b0c0b0a0c090a0a0affdb004301020202020202050303050a0706070a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0affc2001108000a000c03011100021101031101ffc400160001010100000000000000000000000000060509ffc4001801000203000000000000000000000000000507030406ffda000c03010002100310000001a61f7e91829f64365cfa7dd1ffc4001c1000010305000000000000000000000000070003050406081721ffda000801010001050262d6990fcbef810d4225f71d17ffc4002811000200030605050000000000000000000102051131030412132161000607517110223541d1ffda0008010301013f01e6cea1c262507b6b85d11b34e1a8592fb94cda4d3a5055b61365c30c7d6dd331bbb8527f00d8003eea49e2edf03647baa93e4c893e49d4efe9ffc40027110001030007090000000000000000000001020411000305213151810710131722356191b2ffda0008010201013f01b1ac67af6d84346ea4f10ce24c27a499540f43157812472c9f2efae356b39aaf3f10348cf1269b3eee2cce6927528249d4eeffc400251000020201020407000000000000000000010203040511410613325100101221313461ffda0008010100063f021c617ae51b831bf6e1a52348f0091796acc085ddc10bea05b6f6d48e666b86729929f7b390a159df4edd5a28fc000dfe493e1e53d52d1a7248dbb3b4b13331ee492493dcf97fffc4001b10010003010003000000000000000000000111213141001061ffda0008010100013f21900ed50a3dd6449a0f3ea8b75706097ea250a1d61bdaa075c16a2dbebfffda000c030100020003000000104d8fffc4001b11010003000301000000000000000000000111214100103181ffda0008010301013f107f5bd432ca8919ad08f27dfe8bb028064a250b174fab35941d428da9595ebfffc4001c1101000202030100000000000000000000011121415100103191ffda0008010201013f10517b140c8024d69e21cff41f4180a81a02650a1b33d41ca98da8aadaaadbd7ffc4001a100100030003000000000000000000000001001131215161ffda0008010100013f10adfb9f1d915606ba15d5bc3f3828c31c290efa189b5b758f0a65167fffd9", "201,25,28,255;17,41,175,255;198,29,32,255;21,40,179,255;0,162,0,255", 4);
    pic_case(c, "bmp_24", "424d9e0100000000000036000000280000000c0000000a000000010018000000000068010000c40e0000c40e0000000000000000000000a000fafafafafafafafafafafafafafafab42814b42814b42814b42814b42814b42814fafafa00a000fafafafafafafafafafafafab42814b42814b42814b42814b42814b42814fafafafafafa00a00000a000fafafafafafab42814b42814b42814b42814b42814b42814fafafafafafafafafafafafa00a000fafafab42814b42814b42814b42814b42814b42814fafafafafafafafafafafafafafafa00a000b42814b42814b42814b42814b42814b428141e1ec81e1ec81e1ec81e1ec81e1ec81e1ec800a000fafafafafafafafafafafafafafafa1e1ec81e1ec81e1ec81e1ec81e1ec81e1ec8fafafa00a000fafafafafafafafafafafafa1e1ec81e1ec81e1ec81e1ec81e1ec81e1ec8fafafafafafa00a00000a000fafafafafafa1e1ec81e1ec81e1ec81e1ec81e1ec81e1ec8fafafafafafafafafafafafa00a000fafafa1e1ec81e1ec81e1ec81e1ec81e1ec81e1ec8fafafafafafafafafafafafafafafa00a000", "200,30,30,255;20,40,180,255;200,30,30,255;20,40,180,255;0,160,0,255", 1);
    pic_case(c, "bmp_8", "424d3604000000000000be030000280000000c0000000a000000010008000000000078000000c40e0000c40e0000e2000000e200000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000330000006600000099000000cc000000ff00003300000033330000336600003399000033cc000033ff00006600000066330000666600006699000066cc000066ff00009900000099330000996600009999000099cc000099ff0000cc000000cc330000cc660000cc990000cccc0000ccff0000ff000000ff330000ff660000ff990000ffcc0000ffff00330000003300330033006600330099003300cc003300ff00333300003333330033336600333399003333cc003333ff00336600003366330033666600336699003366cc003366ff00339900003399330033996600339999003399cc003399ff0033cc000033cc330033cc660033cc990033cccc0033ccff0033ff000033ff330033ff660033ff990033ffcc0033ffff00660000006600330066006600660099006600cc006600ff00663300006633330066336600663399006633cc006633ff00666600006666330066666600666699006666cc006666ff00669900006699330066996600669999006699cc006699ff0066cc000066cc330066cc660066cc990066cccc0066ccff0066ff000066ff330066ff660066ff990066ffcc0066ffff00990000009900330099006600990099009900cc009900ff00993300009933330099336600993399009933cc009933ff00996600009966330099666600996699009966cc009966ff00999900009999330099996600999999009999cc009999ff0099cc000099cc330099cc660099cc990099cccc0099ccff0099ff000099ff330099ff660099ff990099ffcc0099ffff00cc000000cc003300cc006600cc009900cc00cc00cc00ff00cc330000cc333300cc336600cc339900cc33cc00cc33ff00cc660000cc663300cc666600cc669900cc66cc00cc66ff00cc990000cc993300cc996600cc999900cc99cc00cc99ff00cccc0000cccc3300cccc6600cccc9900cccccc00ccccff00ccff0000ccff3300ccff6600ccff9900ccffcc00ccffff00ff000000ff003300ff006600ff009900ff00cc00ff00ff00ff330000ff333300ff336600ff339900ff33cc00ff33ff00ff660000ff663300ff666600ff669900ff66cc00ff66ff00ff990000ff993300ff996600ff999900ff99cc00ff99ff00ffcc0000ffcc3300ffcc6600ffcc9900ffcccc00ffccff00ffff0000ffff3300ffff6600ffff9900ffffcc00ffffff001ce1e1e1e1e19a7da07da07ce11ce1e1e1e17ca07da076a0e1e11c1ce1e176a176a07da0e1e1e1e11ce1a17ca1a17ca1e1e1e1e1e11c7ca0767c9a7c380e380d38381ce1e1e1e1e1380e38380e0de11ce1e1e1e1380e380e3838e1e11c1ce1e1380e38380e38e1e1e1e11ce1380e380e380ee1e1e1e1e11c", "204,0,0,255;0,0,153,255;204,51,51,255;0,51,153,255;0,153,0,255", 1);
    pic_case(c, "gif_il", "4749463837610c000a00810000fafafa00a0001428b4c81e1e2c000000000c000a000008350007081c08a060c10003091a0c0020a1c0830c1b3a0410b1a0c301150d6a0c20a0a3c78d003c7ea4185164478a1a4d0ac818d26440003b", "200,30,30,255;20,40,180,255;200,30,30,255;20,40,180,255;0,160,0,255", 1);
    pic_case(c, "tiff_lzw", "49492a006e000000803203c1e8141207058440df50b864361c005000213068a44e1d177d440010c89c760f0a87c4635178f4563e1e8648e310d92c96552b984c633110a0a169359bcda713b9b4ca671b85cf27343a145e352f8650a953a9ec2e912ba5d1298b4a7cc6a3518080000a0000010300010000000c00000001010300010000000a0000000201030003000000ec00000003010300010000000500000006010300010000000200000011010400010000000800000015010300010000000300000016010300010000000a0000001701040001000000650000001c010300010000000100000000000000080008000800", "200,30,30,255;20,40,180,255;200,30,30,255;20,40,180,255;0,160,0,255", 1);
    pic_case(c, "tiff_pb", "49492a004c000000fb51fcfa005efb51fdfa015efafb51fffaff5efffafb5101fa5efdfafb51005efcfafcfa005efb32fdfa015efafb32fffaff5efffafb3201fa5efdfafb32005efcfafb32090000010300010000000c00000001010300010000000a00000002010300010000000800000003010300010000000580000006010300010000000100000011010400010000000800000016010300010000000a0000001701040001000000440000001c010300010000000100000000000000", "81,81,81,255;50,50,50,255;81,81,81,255;50,50,50,255;94,94,94,255", 1);
    c.eq("unknown", &pdf_picture::sniff(&vec![1, 2, 3]), "");
    // an icon file: a 4x4 32-bit entry and a 2x2 24-bit one whose AND mask
    // clears the top left pixel
    let ico = unhex("000001000200040400000100200078000000260000000202000001001800400000009e0000002800000004000000080000000100200000000000000000000000000000000000000000000000000000a0008000a0008000a0008000a0008000a0008000a0008000a0008000a0008000a0008000a0008000a0008000a0008000a0008000a0008000a0008000a000800000000000000000000000000000000028000000020000000400000001001800000000000000000000000000000000000000000000000000b428141e1ec800001e1ec8b4281400000000000080000000");
    let small = pdf_picture::decode_ico(&ico, 2);
    c.eq("ico small", &format!("{} {}x{} {},{},{},{} {},{},{},{}", small.ok, small.w, small.h, small.rgba[0], small.rgba[1], small.rgba[2], small.rgba[3], small.rgba[4], small.rgba[5], small.rgba[6], small.rgba[7]), "true 2x2 200,30,30,0 20,40,180,255");
    let mid = pdf_picture::decode_ico(&ico, 3);
    c.eq("ico nearest", &format!("{} {}x{} {},{},{},{}", mid.ok, mid.w, mid.h, mid.rgba[0], mid.rgba[1], mid.rgba[2], mid.rgba[3]), "true 4x4 0,160,0,128");
    let large = pdf_picture::decode_ico(&ico, 48);
    c.eq("ico largest", &format!("{}x{}", large.w, large.h), "4x4");
    c.eq("ico not", &format!("{}", pdf_picture::decode_ico(&vec![1, 2, 3], 16).ok), "false");
    let big = pdf_picture::decode(&unhex("ffd8ffe000104a46494600010100000100010000ffdb0043000201010101010201010102020202020403020202020504040304060506060605060606070908060709070606080b08090a0a0a0a0a06080b0c0b0a0c090a0a0affdb004301020202020202050303050a0706070a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0affc0001108000a000c03012200021101031101ffc4001f0000010501010101010100000000000000000102030405060708090a0bffc400b5100002010303020403050504040000017d01020300041105122131410613516107227114328191a1082342b1c11552d1f02433627282090a161718191a25262728292a3435363738393a434445464748494a535455565758595a636465666768696a737475767778797a838485868788898a92939495969798999aa2a3a4a5a6a7a8a9aab2b3b4b5b6b7b8b9bac2c3c4c5c6c7c8c9cad2d3d4d5d6d7d8d9dae1e2e3e4e5e6e7e8e9eaf1f2f3f4f5f6f7f8f9faffc4001f0100030101010101010101010000000000000102030405060708090a0bffc400b51100020102040403040705040400010277000102031104052131061241510761711322328108144291a1b1c109233352f0156272d10a162434e125f11718191a262728292a35363738393a434445464748494a535455565758595a636465666768696a737475767778797a82838485868788898a92939495969798999aa2a3a4a5a6a7a8a9aab2b3b4b5b6b7b8b9bac2c3c4c5c6c7c8c9cad2d3d4d5d6d7d8d9dae2e3e4e5e6e7e8e9eaf2f3f4f5f6f7f8f9faffda000c03010002110311003f00e9b42fd96be327fc11fbe2ec5fb60f8e7c67e05f19c5f0d370f17e8de08d46e350bcd022d4edce9d6f757504d15aa7facbf8248ed5ae2196e543f9656349a787d23fe1fc3ff0485f10ff00c4cfe347eccdf147e25ebadc4de27f883e00f0d5fdeb463848502dda436f0a8e91411c71962f232b4b2cb23fb27fc14b3f7dff0006e8de6ab2fcd75aa7813c1ba96a772dcc9777b75aae957173732b759269a796596491b2cf248eec4b3127f9f5afd7bc15f09b83f8ab84a78ecc69ca4dd5765ccf952f674e4b4d5392bbf7b7ed65a1e671e71de7fc7799d0ccf38e49c9d28a8c79128c63cd2925cb769c97334e6d5fb28a763fffd9"));
    let half = pdf_picture::shrink(&big, 6);
    c.eq("shrink", &format!("{}x{}", half.w, half.h), "6x5");
    // quarter turns clockwise: the left pixel of a 2x1 picture goes on top
    let two = pdf_picture::made("", 2, 1, vec![1, 0, 0, 255, 2, 0, 0, 255]);
    let t1 = pdf_picture::rotate(&two, 1);
    let t3 = pdf_picture::rotate(&two, 3);
    let t2 = pdf_picture::rotate(&two, 2);
    c.eq("turn", &format!("{}x{} {},{} {},{} {},{}", t1.w, t1.h, t1.rgba[0], t1.rgba[4], t3.rgba[0], t3.rgba[4], t2.rgba[0], t2.rgba[4]), "1x2 1,2 2,1 2,1");
    let png = big.png_ints_turned(5, 1);
    c.eq("turned png", &format!("{}x{}", png[19], png[23]), "5x6");
    let mut sd = PdfDocument::open(simple_pdf("0 0 1 rg 0 0 10 10 re f"), "");
    let pp = sd.page_png_turned(0, 100.0, 1);
    c.eq("turned page", &format!("{}x{}", (pp[18] as i64) * 256 + (pp[19] as i64), (pp[22] as i64) * 256 + (pp[23] as i64)), "100x200");
    // a DCTDecode image (behind ASCIIHex, to keep the file text) is drawn
    let objs: Vec<String> = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 12 10] >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << /XObject << /Im 5 0 R >> >> >>".to_string(),
        "<< /Length 25 >>\nstream\nq 12 0 0 10 0 0 cm /Im Do Q\nendstream".to_string(),
        format!("<< /Type /XObject /Subtype /Image /Width 12 /Height 10 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter [/ASCIIHexDecode /DCTDecode] /Length {} >>\nstream\n{}>\nendstream", 1735, "ffd8ffe000104a46494600010100000100010000ffdb0043000201010101010201010102020202020403020202020504040304060506060605060606070908060709070606080b08090a0a0a0a0a06080b0c0b0a0c090a0a0affdb004301020202020202050303050a0706070a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0affc0001108000a000c03012200021101031101ffc4001f0000010501010101010100000000000000000102030405060708090a0bffc400b5100002010303020403050504040000017d01020300041105122131410613516107227114328191a1082342b1c11552d1f02433627282090a161718191a25262728292a3435363738393a434445464748494a535455565758595a636465666768696a737475767778797a838485868788898a92939495969798999aa2a3a4a5a6a7a8a9aab2b3b4b5b6b7b8b9bac2c3c4c5c6c7c8c9cad2d3d4d5d6d7d8d9dae1e2e3e4e5e6e7e8e9eaf1f2f3f4f5f6f7f8f9faffc4001f0100030101010101010101010000000000000102030405060708090a0bffc400b51100020102040403040705040400010277000102031104052131061241510761711322328108144291a1b1c109233352f0156272d10a162434e125f11718191a262728292a35363738393a434445464748494a535455565758595a636465666768696a737475767778797a82838485868788898a92939495969798999aa2a3a4a5a6a7a8a9aab2b3b4b5b6b7b8b9bac2c3c4c5c6c7c8c9cad2d3d4d5d6d7d8d9dae2e3e4e5e6e7e8e9eaf2f3f4f5f6f7f8f9faffda000c03010002110311003f00e9b42fd96be327fc11fbe2ec5fb60f8e7c67e05f19c5f0d370f17e8de08d46e350bcd022d4edce9d6f757504d15aa7facbf8248ed5ae2196e543f9656349a787d23fe1fc3ff0485f10ff00c4cfe347eccdf147e25ebadc4de27f883e00f0d5fdeb463848502dda436f0a8e91411c71962f232b4b2cb23fb27fc14b3f7dff0006e8de6ab2fcd75aa7813c1ba96a772dcc9777b75aae957173732b759269a796596491b2cf248eec4b3127f9f5afd7bc15f09b83f8ab84a78ecc69ca4dd5765ccf952f674e4b4d5392bbf7b7ed65a1e671e71de7fc7799d0ccf38e49c9d28a8c79128c63cd2925cb769c97334e6d5fb28a763fffd9"),
    ];
    let mut d = PdfDocument::open(build_pdf(&objs, ""), "");
    let page = d.render(0);
    let r = pdf_raster::rasterize(&page, 1.0);
    c.eq("pdf jpeg", &pic_pixels(&r.px, r.w, "196,28,28,255;17,41,173,255;195,31,32,255;28,37,176,255;82,99,107,255", 6), "196,28,28,255;17,41,173,255;195,31,32,255;28,37,176,255;82,99,107,255");
}


pub fn run_all(c: &mut Check) {
    pictures(c);
    type1(c);
    annots(c);
    paths(c);
    colors(c);
    text(c);
    api(c);
    viewer(c);
}

/// `x` with `d` decimals (the `{:.N}` format is not on every target).
fn fx(d: i64, x: f64) -> String {
    let mut m: i64 = 1;
    let mut i: i64 = 0;
    while i < d {
        m *= 10;
        i += 1;
    }
    let neg = x < 0.0;
    let v = ((if neg { 0.0 - x } else { x }) * (m as f64)).round() as i64;
    let mut frac = format!("{}", v % m);
    while (frac.chars().count() as i64) < d {
        frac = format!("0{}", frac);
    }
    format!("{}{}.{}", if neg { "-" } else { "" }, v / m, frac)
}

fn part(v: &Vec<u8>, from: usize, to: usize) -> Vec<u8> {
    let mut out: Vec<u8> = vec![];
    let mut i = from;
    while i < to && i < v.len() {
        out.push(v[i]);
        i += 1;
    }
    out
}
