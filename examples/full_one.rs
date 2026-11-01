use pdf::pdf::PdfDocument;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let b = std::fs::read(&a[1]).unwrap();
    let t = std::time::Instant::now();
    let mut d = PdfDocument::open(b, "");
    println!("open {} pages {} ms", d.page_count(), t.elapsed().as_millis());
    if d.page_count() > 0 { d.text(0); }
    println!("text {} ms", t.elapsed().as_millis());
}
