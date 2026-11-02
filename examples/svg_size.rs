// The size of each page as an SVG file (pdf_svg) and as the PNG the
// desktop preview sends, both raw and gzipped (gzip -6 by the shell):
//   cargo run --release --example svg_size -- file.pdf [width_px] [outdir]
use pdf::pdf::PdfDocument;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let b = std::fs::read(&a[1]).unwrap();
    let px: f64 = a.get(2).map(|s| s.parse().unwrap()).unwrap_or(1024.0);
    let out = a.get(3).cloned().unwrap_or("/tmp".to_string());
    let mut d = PdfDocument::open(b, "");
    let n = d.page_count().min(5);
    for i in 0..n {
        let t = std::time::Instant::now();
        let svg = d.page_svg(i, 0);
        let ts = t.elapsed().as_millis();
        let t2 = std::time::Instant::now();
        let png = d.page_png(i, px);
        let tp = t2.elapsed().as_millis();
        let name = std::path::Path::new(&a[1]).file_stem().unwrap().to_string_lossy().to_string();
        std::fs::write(format!("{}/{}-{}.svg", out, name, i), &svg).unwrap();
        std::fs::write(format!("{}/{}-{}.png", out, name, i), &png).unwrap();
        println!("{} p{} svg {} B {} ms | png {} B {} ms", name, i, svg.len(), ts, png.len(), tp);
    }
}
