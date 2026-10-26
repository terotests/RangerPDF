#!/usr/bin/env node
// The PDF viewer's parity benchmark: pdf.js' own test corpus (test/pdfs at the
// commit ranger-deps.json pins) opened by the viewer's es6 build and by
// pdf.js (pdfjs-dist, the npm version pinned next to it), with pdf.js as the
// reference. Behaviour is compared, no pdf.js code is used by the viewer.
//
//   open    the file opens and has as many pages as pdf.js finds
//   text    per page (first 5), the text without whitespace: exact match and
//           a similarity (Dice coefficient of character bigrams)
//   pixels  page 1 at 72 dpi on white: the share of pixels whose channels
//           differ from pdf.js' by at most 40; `ink` is the same share among
//           the pixels that are not white in either rendering
//
// usage: node web/pdf/tools/bench.js [--limit=N] [--only=substr] [--jobs=N]
//   --corpus=<pdf.js checkout> --ref=<dir with node_modules/pdfjs-dist>
//   (default .deps/pdfjs and .deps/pdfjs-ref, fetched when missing)
//
// Writes web/pdf/parity/results.tsv (one line per file) and summary.json and
// prints the summary. Needs the es6 build: SUITES=pdf web/test.sh es6.
'use strict';
const fs = require('fs');
const path = require('path');
const vm = require('vm');
const zlib = require('zlib');
const { execFileSync } = require('child_process');
const { Worker, isMainThread, parentPort, workerData } = require('worker_threads');

const ROOT = path.resolve(__dirname, '../../..');
const BUILD = path.join(ROOT, 'tmp/web-pdf/es6/core.js');
const CACHE = path.join(ROOT, 'tmp/web-pdf/bench');
const OUT = path.join(ROOT, 'web/pdf/parity');
const PAGES = 5;
const TOL = 40;

const FONT_FILES = {
  sans: 'LiberationSans-Regular.ttf', 'sans-bold': 'LiberationSans-Bold.ttf',
  'sans-italic': 'LiberationSans-Italic.ttf', 'sans-bolditalic': 'LiberationSans-BoldItalic.ttf',
  serif: 'FoxitSerif.pfb', 'serif-bold': 'FoxitSerifBold.pfb',
  'serif-italic': 'FoxitSerifItalic.pfb', 'serif-bolditalic': 'FoxitSerifBoldItalic.pfb',
  mono: 'FoxitFixed.pfb', 'mono-bold': 'FoxitFixedBold.pfb',
  'mono-italic': 'FoxitFixedItalic.pfb', 'mono-bolditalic': 'FoxitFixedBoldItalic.pfb',
  symbol: 'FoxitSymbol.pfb', dingbats: 'FoxitDingbats.pfb',
};

const norm = (s) => s.replace(/\s+/g, '');

function dice(a, b) {
  if (a === b) return 1;
  if (a.length < 2 || b.length < 2) return 0;
  const m = new Map();
  for (let i = 0; i + 1 < a.length; i++) {
    const k = a.substr(i, 2);
    m.set(k, (m.get(k) || 0) + 1);
  }
  let hit = 0;
  for (let i = 0; i + 1 < b.length; i++) {
    const k = b.substr(i, 2);
    const n = m.get(k) || 0;
    if (n > 0) { hit++; m.set(k, n - 1); }
  }
  return (2 * hit) / (a.length + b.length - 2);
}

// ---- worker: the viewer's es6 build on one file at a time ----
if (!isMainThread) {
  const src = fs.readFileSync(workerData.build, 'utf8').replace(/\n__js_main\(\);\s*$/, '\n');
  const ctx = vm.createContext({ console: { log() {}, error() {} }, TextEncoder, TextDecoder });
  vm.runInContext(src + '\n;globalThis.P = { PdfDocument, FontFiles, R: PdfRaster };', ctx);
  const { PdfDocument, FontFiles, R } = ctx.P;
  const fonts = FontFiles.new_();
  for (const [k, f] of Object.entries(FONT_FILES)) {
    fonts.add(k, Array.from(fs.readFileSync(path.join(workerData.fonts, f))));
  }
  parentPort.on('message', ({ file, password }) => {
    const t0 = Date.now();
    const r = { pages: 0, error: '', text: [], w: 0, h: 0, px: null, jpeg: false };
    try {
      const d = PdfDocument.open_with_fonts(Array.from(fs.readFileSync(file)), password || '', fonts);
      r.error = d.error();
      r.pages = d.page_count();
      for (let i = 0; i < Math.min(PAGES, r.pages); i++) {
        const p = d.render(i);
        r.text.push(p.plain_text());
        if (i === 0) {
          const ras = R.rasterize(p, 1.0);
          r.w = ras.w; r.h = ras.h;
          r.px = Buffer.from(ras.px);
          r.jpeg = p.images.some((im) => im.jpeg && im.jpeg.length > 0);
          r.unsup = p.warnings.some((w) => /^image codec/.test(w));
        }
      }
    } catch (e) {
      r.error = 'exception: ' + String(e && e.message || e).slice(0, 200);
    }
    r.ms = Date.now() - t0;
    parentPort.postMessage(r);
  });
  return;
}

// ---- main ----
const args = Object.fromEntries(process.argv.slice(2).map((a) => {
  const m = /^--([^=]+)(?:=(.*))?$/.exec(a);
  return m ? [m[1], m[2] === undefined ? true : m[2]] : [a, true];
}));
const pins = require(path.join(ROOT, 'ranger-deps.json'));
const corpus = path.resolve(args.corpus || path.join(ROOT, '.deps/pdfjs'));
const refDir = path.resolve(args.ref || path.join(ROOT, '.deps/pdfjs-ref'));

function fetchDeps() {
  const pin = pins.pdfjs;
  if (!fs.existsSync(path.join(corpus, 'test/test_manifest.json'))) {
    console.error(`==> pdf.js corpus -> ${pin.rev.slice(0, 7)} (${corpus})`);
    fs.mkdirSync(corpus, { recursive: true });
    const git = (...a) => execFileSync('git', ['-C', corpus, ...a], { stdio: 'inherit' });
    git('init', '-q');
    git('remote', 'add', 'origin', pin.git);
    git('sparse-checkout', 'set', ...pin.sparse.split(' '));
    git('fetch', '-q', '--depth', '1', '--filter=blob:none', 'origin', pin.rev);
    git('checkout', '-q', '--detach', 'FETCH_HEAD');
  }
  if (!fs.existsSync(path.join(refDir, 'node_modules/pdfjs-dist'))) {
    console.error(`==> pdfjs-dist@${pin.npm} (${refDir})`);
    fs.mkdirSync(refDir, { recursive: true });
    if (!fs.existsSync(path.join(refDir, 'package.json'))) fs.writeFileSync(path.join(refDir, 'package.json'), '{"private":true}\n');
    execFileSync('npm', ['install', '--no-audit', '--no-fund', '--silent', `pdfjs-dist@${pin.npm}`], { cwd: refDir, stdio: 'inherit' });
  }
  if (!fs.existsSync(BUILD)) {
    execFileSync(path.join(ROOT, 'web/test.sh'), ['es6'], { env: { ...process.env, SUITES: 'pdf' }, stdio: 'inherit' });
  }
}

// pdf.js' answers, cached per file (the corpus does not change under a pin);
// each file in a process of its own, as pdf.js can take minutes and
// gigabytes on some of its own test files
const refKey = (file) => path.join(CACHE, 'ref', path.basename(file) + '.json');

async function reference(file, password, fontsDir) {
  const key = refKey(file);
  if (!fs.existsSync(key)) {
    const { execFile } = require('child_process');
    await new Promise((resolve) => {
      execFile(process.execPath, ['--max-old-space-size=2048', __filename, `--ref-one=${file}`, `--password=${password}`,
        `--fonts=${fontsDir}`, `--ref=${refDir}`], { timeout: 120000, killSignal: 'SIGKILL', maxBuffer: 1 << 20 }, () => resolve());
    });
    if (!fs.existsSync(key)) {
      fs.mkdirSync(path.dirname(key), { recursive: true });
      fs.writeFileSync(key, JSON.stringify({ pages: 0, error: 'pdf.js timeout or crash', text: [], w: 0, h: 0, px: null }));
    }
  }
  const c = JSON.parse(fs.readFileSync(key, 'utf8'));
  if (c.px) c.px = zlib.gunzipSync(Buffer.from(c.px, 'base64'));
  return c;
}

async function referenceOne(pdfjs, file, password, fontsDir) {
  const key = refKey(file);
  const r = { pages: 0, error: '', text: [], w: 0, h: 0, px: null };
  try {
    const task = pdfjs.getDocument({
      data: new Uint8Array(fs.readFileSync(file)), password,
      standardFontDataUrl: fontsDir + '/', isEvalSupported: false, verbosity: 0,
    });
    const doc = await withTimeout(task.promise, 30000);
    r.pages = doc.numPages;
    for (let i = 1; i <= Math.min(PAGES, r.pages); i++) {
      const page = await doc.getPage(i);
      const tc = await withTimeout(page.getTextContent(), 30000);
      r.text.push(tc.items.map((it) => it.str || '').join(''));
      if (i === 1) {
        const vp = page.getViewport({ scale: 1 });
        const w = Math.ceil(vp.width), h = Math.ceil(vp.height);
        const cv = doc.canvasFactory.create(w, h);
        cv.context.fillStyle = '#fff';
        cv.context.fillRect(0, 0, w, h);
        await withTimeout(page.render({ canvasContext: cv.context, viewport: vp }).promise, 60000);
        r.w = w; r.h = h;
        r.px = Buffer.from(cv.context.getImageData(0, 0, w, h).data.buffer);
      }
    }
    await doc.destroy();
  } catch (e) {
    r.error = String(e && e.message || e).slice(0, 200);
  }
  fs.mkdirSync(path.dirname(key), { recursive: true });
  fs.writeFileSync(key, JSON.stringify({ ...r, px: r.px ? zlib.gzipSync(r.px).toString('base64') : null }));
}

function withTimeout(p, ms) {
  let t;
  return Promise.race([p, new Promise((_, rej) => { t = setTimeout(() => rej(new Error('timeout')), ms); })])
    .finally(() => clearTimeout(t));
}

function pixelScore(a, b) {
  if (!a.px || !b.px) return null;
  if (Math.abs(a.w - b.w) > 1 || Math.abs(a.h - b.h) > 1) return { all: 0, ink: 0 };
  const w = Math.min(a.w, b.w), h = Math.min(a.h, b.h);
  let ok = 0, ink = 0, inkOk = 0;
  const white = (p, i) => p[i] >= 240 && p[i + 1] >= 240 && p[i + 2] >= 240;
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      const i = (y * a.w + x) * 4, j = (y * b.w + x) * 4;
      const same = Math.abs(a.px[i] - b.px[j]) <= TOL && Math.abs(a.px[i + 1] - b.px[j + 1]) <= TOL &&
          Math.abs(a.px[i + 2] - b.px[j + 2]) <= TOL;
      if (same) ok++;
      if (!white(a.px, i) || !white(b.px, j)) {
        ink++;
        if (same) inkOk++;
      }
    }
  }
  if (w * h === 0) return null;
  return { all: ok / (w * h), ink: ink ? inkOk / ink : 1 };
}

class Pool {
  constructor(n, data) { this.data = data; this.free = []; for (let i = 0; i < n; i++) this.free.push(this.spawn()); this.wait = []; }
  spawn() { return new Worker(__filename, { workerData: this.data, resourceLimits: { maxOldGenerationSizeMb: 2048 } }); }
  async run(job, ms) {
    const w = this.free.length ? this.free.pop() : await new Promise((r) => this.wait.push(r));
    return new Promise((resolve) => {
      const done = (r, worker) => {
        clearTimeout(t);
        w.removeAllListeners('message');
        w.removeAllListeners('error');
        const next = this.wait.shift();
        if (next) next(worker); else this.free.push(worker);
        resolve(r);
      };
      const t = setTimeout(() => { w.terminate(); done({ timeout: true, error: 'timeout', pages: 0, text: [] }, this.spawn()); }, ms);
      w.once('message', (r) => done(r, w));
      w.once('error', (e) => done({ error: 'crash: ' + e.message, pages: 0, text: [] }, this.spawn()));
      w.postMessage(job);
    });
  }
  close() { for (const w of this.free) w.terminate(); }
}

function parityMd(s, rows) {
  const pct = (a, b) => (b ? ((100 * a) / b).toFixed(1) + ' %' : '-');
  const L = [];
  L.push('# PDF viewer parity with pdf.js', '');
  L.push(`GENERATED by \`web/pdf/tools/bench.js\`. Corpus: ${s.corpus} (the files in its test manifest that are in the repository, ${s.files}); reference: ${s.reference}. The viewer runs as its es6 build. Per file: page 1 is rendered at 72 dpi on white by both and compared pixel by pixel (a pixel matches when no channel differs by more than ${TOL}); the text of the first ${PAGES} pages is compared without white space. Per-file numbers: \`parity/results.tsv\`.`, '');
  L.push('| metric | result |', '|---|---|');
  L.push(`| opens with pdf.js' page count | ${s.open} / ${s.ref_opens} (${pct(s.open, s.ref_opens)}) |`);
  L.push(`| exceptions / timeouts (60 s) | ${s.exceptions} / ${s.timeouts} |`);
  L.push(`| text: pages identical | ${s.text_exact_pages} / ${s.text_pages} (${pct(s.text_exact_pages, s.text_pages)}) |`);
  L.push(`| text: mean similarity (bigram Dice) | ${(100 * s.text_similarity).toFixed(1)} % |`);
  L.push(`| pixels: mean share matching | ${(100 * s.pixels).toFixed(1)} % (${s.pixel_files} files) |`);
  L.push(`| pixels: files with at least 95 % | ${s.pixels_95} / ${s.pixel_files} |`);
  L.push(`| ink (pixels not white in either): mean share matching | ${(100 * s.ink).toFixed(1)} % |`);
  L.push(`| ink: files with at least 90 % | ${s.ink_90} / ${s.pixel_files} |`);
  L.push(`| ink without files that have a JPEG image not decoded | ${(100 * s.ink_no_jpeg).toFixed(1)} % (${s.pixel_files_no_jpeg} files) |`);
  L.push('');
  L.push('Not implemented, and so behind most of the differences: JPEG 2000, JBIG2 and CCITT image decoding (those images are drawn grey; a JPEG that does not decode is too), mesh shadings, blend modes and soft masks, predefined CJK CMaps, annotations without an appearance stream. Glyphs are anti-aliased with 4x4 samples, pdf.js uses the browser canvas, so edges of text differ slightly everywhere.');
  L.push('');
  const undecoded = rows.filter((r) => r.unsup).length;
  const worst = rows.filter((r) => r.ink !== null && r.ref_pages > 0 && !r.unsup && !r.jpeg).sort((a, b) => a.ink - b.ink).slice(0, 30);
  L.push('## Lowest ink scores', '', `Files whose page 1 has a JPEG that does not decode (${rows.filter((r) => r.jpeg).length}) or an image in a format not decoded (${undecoded}: JPEG 2000, JBIG2, CCITT) are left out of this list.`, '', '| file | ink | text | note |', '|---|---|---|---|');
  for (const r of worst) {
    L.push(`| ${r.name} | ${(100 * r.ink).toFixed(0)} % | ${r.text_sim === null ? '-' : (100 * r.text_sim).toFixed(0) + ' %'} | ${r.jpeg ? 'JPEG' : ''}${r.error ? ' ' + r.error : ''} |`);
  }
  L.push('');
  return L.join('\n');
}

async function main() {
  if (args['ref-one']) {
    const pdfjs = await import(path.join(refDir, 'node_modules/pdfjs-dist/legacy/build/pdf.mjs'));
    await referenceOne(pdfjs, args['ref-one'], args.password === true ? '' : (args.password || ''), args.fonts);
    process.exit(0);
  }
  fetchDeps();
  const fontsDir = path.join(corpus, 'external/standard_fonts');
  const manifest = JSON.parse(fs.readFileSync(path.join(corpus, 'test/test_manifest.json'), 'utf8'));
  const seen = new Set();
  let files = [];
  for (const m of manifest) {
    if (m.link || seen.has(m.file)) continue;
    const f = path.join(corpus, 'test', m.file);
    if (!fs.existsSync(f)) continue;
    seen.add(m.file);
    files.push({ file: f, name: path.basename(m.file), password: m.password || '' });
  }
  files.sort((a, b) => a.name.localeCompare(b.name));
  if (args.only) files = files.filter((f) => f.name.includes(args.only));
  if (args.limit) files = files.slice(0, +args.limit);

  const pool = new Pool(+(args.jobs || Math.max(1, require('os').cpus().length - 1)), { build: BUILD, fonts: fontsDir });
  const rows = [];
  let n = 0;
  const queue = files.slice();
  const lanes = Math.max(2, pool.free.length + 1);
  const one = async (f) => {
    const ref = await reference(f.file, f.password, fontsDir);
    const ours = await pool.run({ file: f.file, password: f.password }, 60000);
    const row = { name: f.name, ref_pages: ref.pages, pages: ours.pages, ms: ours.ms || 0, error: ours.error || '', ref_error: ref.error };
    row.open = ref.pages > 0 && ours.pages === ref.pages;
    const k = Math.min(ref.text.length, ours.text.length);
    let exact = 0, sim = 0, cmp = 0;
    for (let i = 0; i < k; i++) {
      const a = norm(ref.text[i]), b = norm(ours.text[i]);
      if (a.length === 0 && b.length === 0) continue;
      cmp++;
      if (a === b) exact++;
      sim += dice(a, b);
    }
    row.text_pages = cmp;
    row.text_exact = exact;
    row.text_sim = cmp ? sim / cmp : null;
    const px = pixelScore(ours, ref);
    row.pixels = px ? px.all : null;
    row.ink = px ? px.ink : null;
    row.jpeg = !!ours.jpeg;
    row.unsup = !!ours.unsup;
    rows.push(row);
    if (++n % 50 === 0) console.error(`  ${n}/${files.length}`);
  };
  await Promise.all(Array.from({ length: lanes }, async () => {
    while (queue.length) await one(queue.shift());
  }));
  pool.close();
  rows.sort((a, b) => a.name.localeCompare(b.name));

  const refOk = rows.filter((r) => r.ref_pages > 0);
  const avg = (xs) => (xs.length ? xs.reduce((s, x) => s + x, 0) / xs.length : 0);
  const textRows = refOk.filter((r) => r.text_pages > 0);
  const pxRows = refOk.filter((r) => r.pixels !== null);
  const pxNoJpeg = pxRows.filter((r) => !r.jpeg);
  const s = {
    corpus: `pdf.js test/pdfs @ ${pins.pdfjs.rev.slice(0, 7)}`, reference: `pdfjs-dist ${pins.pdfjs.npm}`,
    files: rows.length, ref_opens: refOk.length,
    open: refOk.filter((r) => r.open).length,
    exceptions: rows.filter((r) => /^(exception|crash)/.test(r.error)).length,
    timeouts: rows.filter((r) => r.error === 'timeout').length,
    text_files: textRows.length,
    text_pages: textRows.reduce((s, r) => s + r.text_pages, 0),
    text_exact_pages: textRows.reduce((s, r) => s + r.text_exact, 0),
    text_similarity: +avg(textRows.map((r) => r.text_sim)).toFixed(4),
    pixel_files: pxRows.length,
    pixels: +avg(pxRows.map((r) => r.pixels)).toFixed(4),
    pixels_95: pxRows.filter((r) => r.pixels >= 0.95).length,
    ink: +avg(pxRows.map((r) => r.ink)).toFixed(4),
    ink_90: pxRows.filter((r) => r.ink >= 0.9).length,
    pixel_files_no_jpeg: pxNoJpeg.length,
    pixels_no_jpeg: +avg(pxNoJpeg.map((r) => r.pixels)).toFixed(4),
    ink_no_jpeg: +avg(pxNoJpeg.map((r) => r.ink)).toFixed(4),
    ms_total: rows.reduce((s, r) => s + r.ms, 0),
  };
  if (!args.only && !args.limit) {
    fs.mkdirSync(OUT, { recursive: true });
    const f = (x) => (x === null || x === undefined ? '-' : typeof x === 'number' && !Number.isInteger(x) ? x.toFixed(3) : String(x));
    const lines = ['file\tref_pages\tpages\ttext_pages\ttext_exact\ttext_sim\tpixels\tink\tjpeg\tms\terror'];
    for (const r of rows) {
      lines.push([r.name, r.ref_pages, r.pages, r.text_pages, r.text_exact, r.text_sim, r.pixels, r.ink, r.jpeg ? 1 : 0, r.ms, (r.error || '').replace(/\s+/g, ' ')].map(f).join('\t'));
    }
    fs.writeFileSync(path.join(OUT, 'results.tsv'), lines.join('\n') + '\n');
    fs.writeFileSync(path.join(OUT, 'summary.json'), JSON.stringify(s, null, 2) + '\n');
    fs.writeFileSync(path.join(ROOT, 'web/pdf/PARITY.md'), parityMd(s, rows));
  } else if (args.v) {
    for (const r of rows) console.log(JSON.stringify(r));
  }
  console.log(JSON.stringify(s, null, 2));
}

main().catch((e) => { console.error(e); process.exit(1); });
