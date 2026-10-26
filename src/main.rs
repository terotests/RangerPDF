//! web/pdf test runner. The last line is `ALL PASS <n>` when every test passed.

mod pdf_cff;
mod check;
mod pdf_cmap;
mod pdf_color;
mod pdf_crypt;
mod pdf_display;
mod pdf_doc;
mod pdf_filters;
mod pdf_font;
mod pdf_fontdata;
mod pdf_func;
mod pdf_gif;
mod pdf_glyphs;
mod pdf_hash;
mod pdf_image;
mod pdf_inflate;
mod pdf_jpeg;
mod pdf_lexer;
mod pdf_obj;
mod pdf_path;
mod pdf;
mod pdf_picture;
mod pdf_png;
mod pdf_raster;
mod pdf_render;
mod tests;
mod tests2;
mod pdf_tiff;
mod pdf_truetype;
mod pdf_type1;

fn main() {
    let mut c = check::Check::new();
    tests::run_all(&mut c);
    tests2::run_all(&mut c);
    c.done();
}
