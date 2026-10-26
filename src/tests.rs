//! Unit tests: small PDFs written here, so every target runs them without files.

use crate::check::Check;
use crate::pdf_doc::Doc;
use crate::pdf_filters;
use crate::pdf_hash;
use crate::pdf_inflate;
use crate::pdf_lexer::PdfLexer;
use crate::pdf_obj;
use crate::pdf_obj::K;

fn hex(b: &Vec<u8>) -> String {
    let digits: Vec<char> = "0123456789abcdef".chars().collect();
    let mut s = String::new();
    for x in b.iter() {
        let v = *x as usize;
        s.push(digits[v / 16]);
        s.push(digits[v % 16]);
    }
    s
}

/// A PDF with a correct cross-reference table for `objs` (object i+1 is objs[i]).
pub fn build_pdf(objs: &Vec<String>, trailer_extra: &str) -> Vec<u8> {
    let mut out = String::from("%PDF-1.4\n");
    let mut offs: Vec<usize> = vec![];
    let mut i: usize = 0;
    while i < objs.len() {
        offs.push(out.chars().count());
        out.push_str(&format!("{} 0 obj\n{}\nendobj\n", i + 1, objs[i]));
        i += 1;
    }
    let xref = out.chars().count();
    out.push_str(&format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1));
    for o in offs.iter() {
        let mut n = format!("{}", o);
        while n.chars().count() < 10 {
            n = format!("0{}", n);
        }
        out.push_str(&format!("{} 00000 n \n", n));
    }
    out.push_str(&format!("trailer\n<< /Size {} /Root 1 0 R {}>>\nstartxref\n{}\n%%EOF\n", objs.len() + 1, trailer_extra, xref));
    pdf_obj::bytes_of(&out)
}

pub fn simple_pdf(content: &str) -> Vec<u8> {
    let objs: Vec<String> = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 200 100] >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>".to_string(),
        format!("<< /Length {} >>\nstream\n{}\nendstream", content.chars().count(), content),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
    ];
    build_pdf(&objs, "")
}

fn lexer_tests(c: &mut Check) {
    c.test("pdf:lexer", "objects");
    let mut lx = PdfLexer::new(pdf_obj::bytes_of("<< /A 1 /B [2 0 R 3.5 -4 (x\\(y\\)) <414> /N#20m] /C true >> %c\n12 0 R"));
    let d = lx.object(0);
    c.ok("dict", d.is_dict());
    c.eqi("A", d.at("A").i(), 1);
    let b = d.at("B");
    c.eqi("B len", b.len() as i64, 6);
    c.ok("ref", b.item(0).k == K::Ref && b.item(0).i() == 2);
    c.eq("real", &format!("{}", b.item(1).f()), "3.5");
    c.eqi("neg", b.item(2).i(), -4);
    c.eq("string", &b.item(3).text(), "x(y)");
    c.eq("hex", &b.item(4).text(), "A@");
    c.eq("name escape", &b.item(5).name, "N m");
    c.ok("bool", d.at("C").k == K::Bool && d.at("C").n == 1.0);
    let r = lx.object(0);
    c.ok("top ref", r.k == K::Ref && r.i() == 12);

    c.test("pdf:lexer", "string escapes");
    let mut l2 = PdfLexer::new(pdf_obj::bytes_of("(a\\nb\\101\\\\c(d)e) <FEFF00e9>"));
    let s = l2.object(0);
    c.eq("escapes", &hex(&s.s), "610a62415c6328642965");
    let u = l2.object(0);
    c.eq("utf16", &u.text(), "é");
}

fn filter_tests(c: &mut Check) {
    c.test("pdf:filter:ASCIIHexDecode", "hex");
    c.eq("ahx", &pdf_obj::latin1(&pdf_filters::ascii_hex(&pdf_obj::bytes_of("48 65 6c6C 6F7>"))), "Hellop");
    c.test("pdf:filter:ASCII85Decode", "a85");
    c.eq("a85", &pdf_obj::latin1(&pdf_filters::ascii85(&pdf_obj::bytes_of("87cURD]i,\"Ebo80~>"))), "Hello World!");
    c.eq("a85 z", &hex(&pdf_filters::ascii85(&pdf_obj::bytes_of("z~>"))), "00000000");
    c.test("pdf:filter:RunLengthDecode", "rl");
    let rl: Vec<u8> = vec![2, 65, 66, 67, 254, 68, 128];
    c.eq("rl", &pdf_obj::latin1(&pdf_filters::run_length(&rl)), "ABCDDD");
    c.test("pdf:filter:FlateDecode", "stored and fixed blocks");
    // zlib of "hello hello hello" with fixed Huffman codes (python zlib.compress)
    let z: Vec<u8> = vec![120, 156, 203, 72, 205, 201, 201, 87, 200, 64, 144, 0, 58, 46, 6, 125];
    let r = pdf_inflate::inflate(&z);
    c.eq("fixed", &pdf_obj::latin1(&r.data), "hello hello hello");
    c.ok("ok", r.ok);
    let st: Vec<u8> = vec![120, 1, 1, 3, 0, 252, 255, 97, 98, 99, 2, 77, 1, 39];
    c.eq("stored", &pdf_obj::latin1(&pdf_inflate::inflate(&st).data), "abc");
    c.test("pdf:filter:FlateDecode", "dynamic block");
    let dz = pdf_filters::ascii_hex(&pdf_obj::bytes_of("78da95d53d4e03300c86e1bda7c811e2bffc7446cc5c01894854aa180a6a0ba74708295bedcfb3dfe9511c9f4f1fabd463f97a5fe5e5e9b95c4febb62ee5b25edf3ecbe1fc37a507d3fbf7cf7fc08f83dd88dbec4ca36c9706943b6e58bcfbee828c08640220543111229884386342924421f59f89452cd410970eba0cdc65665cb8265d985c17e6707f047061c55cd860176e29979e7519becb8c5ca4221f0b612ec2b08b48c64534e922e6ba480b5d3ae232409709bb68cdb828255d945d1795c845157051032f51c35d7aca65645da6eb62357231025c8c311713fc446bc6c52ce962cd77e9a1cb405cc2fd38fc02112c418c"));
    let dr = pdf_inflate::inflate(&dz);
    c.eqi("length", dr.data.len() as i64, 2372);
    c.eq("md5", &hex(&pdf_hash::md5(&dr.data)), "5a9e2a9872fbfa421ae4f2eec995b149");
    c.test("pdf:filter:LZWDecode", "lzw");
    // the example of ISO 32000-1 §7.4.4.2: "-----A---B"
    let lz: Vec<u8> = vec![0x80, 0x0B, 0x60, 0x50, 0x22, 0x0C, 0x0C, 0x85, 0x01];
    c.eq("lzw", &pdf_obj::latin1(&pdf_filters::lzw(&lz, 1)), "-----A---B");
    c.test("pdf:filter:predictor", "png up");
    let mut p = pdf_obj::dict();
    p.set("Predictor", pdf_obj::int(12));
    p.set("Columns", pdf_obj::int(2));
    let pr: Vec<u8> = vec![2, 1, 2, 2, 1, 1];
    c.eq("up", &hex(&pdf_filters::predict(pr, &p)), "01020203");
}

fn hash_tests(c: &mut Check) {
    c.test("pdf:crypt:md5", "md5");
    c.eq("md5 empty", &hex(&pdf_hash::md5(&vec![])), "d41d8cd98f00b204e9800998ecf8427e");
    c.eq("md5 abc", &hex(&pdf_hash::md5(&pdf_obj::bytes_of("abc"))), "900150983cd24fb0d6963f7d28e17f72");
    c.test("pdf:crypt:sha256", "sha256");
    c.eq("sha256 abc", &hex(&pdf_hash::sha256(&pdf_obj::bytes_of("abc"))), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    c.test("pdf:crypt:sha512", "sha384 sha512");
    c.eq("sha512 abc", &hex(&pdf_hash::sha512(&pdf_obj::bytes_of("abc"))), "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f");
    c.eq("sha384 abc", &hex(&pdf_hash::sha384(&pdf_obj::bytes_of("abc"))), "cb00753f45a35e8bb5a03d699ac65007272c32ab0eded1631a8b605a43ff5bed8086072ba1e7cc2358baeca134c825a7");
    c.test("pdf:crypt:rc4", "rc4");
    c.eq("rc4", &hex(&pdf_hash::rc4(&pdf_obj::bytes_of("Key"), &pdf_obj::bytes_of("Plaintext"))), "bbf316e8d940af0ad3");
    c.test("pdf:crypt:aes", "aes-128 and aes-256 blocks");
    let k128: Vec<u8> = vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
    let pt: Vec<i64> = vec![0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff];
    let a = pdf_hash::Aes::new(&k128);
    let ct = a.encrypt_block(&pt);
    let mut cb: Vec<u8> = vec![];
    for x in ct.iter() {
        cb.push(*x as u8);
    }
    c.eq("aes128 enc", &hex(&cb), "69c4e0d86a7b0430d8cdb78070b4c55a");
    let back = a.decrypt_block(&ct);
    c.eq("aes128 dec", &format!("{:?}", back), &format!("{:?}", pt));
    let mut k256: Vec<u8> = vec![];
    let mut i = 0;
    while i < 32 {
        k256.push(i as u8);
        i += 1;
    }
    let a2 = pdf_hash::Aes::new(&k256);
    let ct2 = a2.encrypt_block(&pt);
    let mut cb2: Vec<u8> = vec![];
    for x in ct2.iter() {
        cb2.push(*x as u8);
    }
    c.eq("aes256 enc", &hex(&cb2), "8ea2b7ca516745bfeafc49904b496089");
    c.eq("aes256 dec", &format!("{:?}", a2.decrypt_block(&ct2)), &format!("{:?}", pt));
}

fn doc_tests(c: &mut Check) {
    c.test("pdf:doc:xref-table pdf:doc:pages", "simple file");
    let mut d = Doc::open(simple_pdf("BT /F1 12 Tf 10 50 Td (Hi) Tj ET"), "");
    c.eq("error", &d.error, "");
    c.eqi("pages", d.page_count() as i64, 1);
    c.ok("not repaired", !d.repaired);
    c.eq("version", &d.version, "1.4");
    c.eq("media", &format!("{:?}", d.pages[0].media), "[0.0, 0.0, 200.0, 100.0]");
    let content = pdf_obj::latin1(&d.page_content(0));
    c.eq("content", &content, "BT /F1 12 Tf 10 50 Td (Hi) Tj ET");

    c.test("pdf:doc:repair", "broken offsets");
    let mut bad = simple_pdf("0 0 m 10 10 l S");
    // break the startxref offset
    let n = bad.len();
    let mut i = n - 8;
    while i > 0 {
        if bad[i] >= 48 && bad[i] <= 57 {
            bad[i] = 49;
            break;
        }
        i -= 1;
    }
    let mut d2 = Doc::open(bad, "");
    c.eqi("pages", d2.page_count() as i64, 1);
    c.ok("repaired", d2.repaired);
    c.eq("content", &pdf_obj::latin1(&d2.page_content(0)), "0 0 m 10 10 l S");

    c.test("pdf:doc:inherit", "inherited attributes and rotate");
    let objs: Vec<String> = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 /MediaBox [0 0 300 400] /Rotate 90 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R >>".to_string(),
        "<< /Type /Pages /Parent 2 0 R /Kids [5 0 R] /Count 1 /CropBox [10 10 110 210] >>".to_string(),
        "<< /Type /Page /Parent 4 0 R /Rotate -90 >>".to_string(),
    ];
    let d3 = Doc::open(build_pdf(&objs, ""), "");
    c.eqi("pages", d3.page_count() as i64, 2);
    c.eqi("rotate", d3.pages[0].rotate, 90);
    c.eq("size", &format!("{}x{}", d3.pages[0].width(), d3.pages[0].height()), "400x300");
    c.eqi("rotate 2", d3.pages[1].rotate, 270);
    c.eq("crop", &format!("{:?}", d3.pages[1].crop), "[10.0, 10.0, 110.0, 210.0]");
}

pub fn run_all(c: &mut Check) {
    lexer_tests(c);
    filter_tests(c);
    hash_tests(c);
    doc_tests(c);
}
