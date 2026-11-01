//! Checks sparse (chunked) reading against reading the whole file, over a
//! directory of PDFs: page count, page sizes and the first and last page's
//! text must be the same; prints the bytes each needed.
//!   cargo run --release --example sparse_corpus -- <dir> [chunk]
use pdf::pdf::PdfDocument;
use pdf::pdf_font::FontFiles;
use std::time::Instant;

fn feed(d: &mut PdfDocument, full: &Vec<u8>, fed: &mut i64) {
    let r = d.needs();
    let mut i = 0;
    while i + 1 < r.len() {
        let from = r[i] as usize;
        let to = (r[i + 1] as usize).min(full.len());
        let b: Vec<i64> = full[from..to].iter().map(|x| *x as i64).collect();
        *fed += b.len() as i64;
        d.feed_ints(r[i], &b);
        i += 2;
    }
}

fn text(d: &mut PdfDocument, i: i64, full: &Vec<u8>, fed: &mut i64) -> String {
    let mut t = d.text(i);
    let mut n = 0;
    while d.short() && n < 500 {
        feed(d, full, fed);
        t = d.text(i);
        n += 1;
    }
    t
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let dir = &args[1];
    let chunk: i64 = if args.len() > 2 { args[2].parse().unwrap() } else { 65536 };
    let mut names: Vec<_> = if std::path::Path::new(dir).is_file() { vec![std::path::PathBuf::from(dir)] } else { std::fs::read_dir(dir).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().map(|x| x == "pdf").unwrap_or(false)).collect() };
    names.sort();
    let (mut same, mut diff, mut total, mut read_open, mut read_first) = (0, 0, 0i64, 0i64, 0i64);
    for p in names.iter() {
        let full = std::fs::read(p).unwrap();
        if full.len() > 40_000_000 { continue; }
        let t0 = Instant::now();
        let mut a = PdfDocument::open(full.clone(), "");
        let n = a.page_count();
        let fa = if n > 0 { a.text(0) } else { String::new() };
        let la = if n > 1 { a.text(n - 1) } else { String::new() };
        let ta = t0.elapsed().as_millis();
        let mut fed: i64 = 0;
        let mut b = PdfDocument::open_sparse(full.len() as i64, chunk, "", FontFiles::new());
        let mut k = 0;
        while b.short() && k < 500 {
            feed(&mut b, &full, &mut fed);
            b.restart("");
            k += 1;
        }
        let at_open = fed;
        let fb = if b.page_count() > 0 { text(&mut b, 0, &full, &mut fed) } else { String::new() };
        let at_first = fed;
        let bn = b.page_count(); let lb = if bn > 1 { text(&mut b, bn - 1, &full, &mut fed) } else { String::new() };
        let mut sizes_ok = b.page_count() == n;
        let mut i = 0;
        while sizes_ok && i < n {
            // a lazy page's own size comes when it is realized
            let mut k2 = 0;
            while !b.realize(i) && k2 < 100 {
                feed(&mut b, &full, &mut fed);
                k2 += 1;
            }
            sizes_ok = a.page_width(i) == b.page_width(i) && a.page_height(i) == b.page_height(i);
            i += 1;
        }
        let ok = sizes_ok && fa == fb && la == lb && a.error() == b.error();
        let name = p.file_name().unwrap().to_string_lossy();
        if ok { same += 1; } else {
            diff += 1;
            println!("DIFF {} pages {}/{} first {} last {} err '{}' '{}'", name, n, b.page_count(), fa == fb, la == lb, a.error(), b.error());
        }
        total += full.len() as i64;
        read_open += at_open;
        read_first += at_first;
        eprintln!("{} {} kB: open {} kB, first page {} kB ({} ms full, {} ms sparse)", name, full.len() / 1024, at_open / 1024, at_first / 1024, ta, t0.elapsed().as_millis() - ta);
    }
    println!("same {} diff {}; {} MB in all, open read {} MB, first page {} MB", same, diff, total >> 20, read_open >> 20, read_first >> 20);
}
