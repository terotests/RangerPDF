// The online viewer's engine in a Worker: the reader as WebAssembly
// (rangerpdf.wasm, built from ../wasm.rs) with no imports. The page posts
// {id, op, ...} and gets {id, ok, ...} back; pixels come as a transferred
// ArrayBuffer.
'use strict';

const FONT_FILES = {
  sans: 'LiberationSans-Regular.ttf', 'sans-bold': 'LiberationSans-Bold.ttf',
  'sans-italic': 'LiberationSans-Italic.ttf', 'sans-bolditalic': 'LiberationSans-BoldItalic.ttf',
  serif: 'FoxitSerif.pfb', 'serif-bold': 'FoxitSerifBold.pfb',
  'serif-italic': 'FoxitSerifItalic.pfb', 'serif-bolditalic': 'FoxitSerifBoldItalic.pfb',
  mono: 'FoxitFixed.pfb', 'mono-bold': 'FoxitFixedBold.pfb',
  'mono-italic': 'FoxitFixedItalic.pfb', 'mono-bolditalic': 'FoxitFixedBoldItalic.pfb',
  symbol: 'FoxitSymbol.pfb', dingbats: 'FoxitDingbats.pfb',
};

let wasm = null;
const enc = new TextEncoder();
const dec = new TextDecoder();

function put(bytes) {
  const p = wasm.pv_alloc(bytes.length);
  new Uint8Array(wasm.memory.buffer, p, bytes.length).set(bytes);
  return p;
}
function out() {
  return new Uint8Array(wasm.memory.buffer, wasm.pv_out_ptr(), wasm.pv_out_len());
}
const outText = () => dec.decode(out());
const outJson = () => JSON.parse(outText());
function str(s) {
  const b = enc.encode(s);
  return [put(b), b.length];
}

async function init(base) {
  const r = await fetch(new URL('rangerpdf.wasm', base));
  const { instance } = await WebAssembly.instantiate(await r.arrayBuffer(), {});
  wasm = instance.exports;
  // the substitute faces for fonts a file does not embed; a missing one only
  // means such text is not drawn
  await Promise.all(Object.entries(FONT_FILES).map(async ([key, file]) => {
    try {
      const f = await fetch(new URL('fonts/' + file, base));
      if (!f.ok) return;
      const b = new Uint8Array(await f.arrayBuffer());
      const [kp, kl] = str(key);
      wasm.pv_font(kp, kl, put(b), b.length);
    } catch (e) { /* no font */ }
  }));
}

const ops = {
  async init(m) { await init(m.base); return {}; },
  open(m) {
    const b = new Uint8Array(m.data);
    const [pp, pl] = str(m.password || '');
    const t0 = performance.now();
    wasm.pv_open(put(b), b.length, pp, pl);
    return { info: outJson(), ms: performance.now() - t0 };
  },
  render(m) {
    const t0 = performance.now();
    if (!wasm.pv_render(m.page, m.scale, m.quarter || 0)) return { w: 0, h: 0 };
    const w = wasm.pv_out_w(), h = wasm.pv_out_h();
    const px = out().slice().buffer;
    return { w, h, px, ms: performance.now() - t0, transfer: [px] };
  },
  text(m) { wasm.pv_text(m.page); return { items: outJson() }; },
  plain(m) { wasm.pv_plain(m.page); return { text: outText() }; },
  find(m) {
    const [qp, ql] = str(m.query);
    wasm.pv_find(m.page, qp, ql);
    return { boxes: outJson() };
  },
  outline() { wasm.pv_outline(); return { items: outJson() }; },
  forget() { wasm.pv_forget(); return {}; },
};

self.onmessage = async (e) => {
  const m = e.data;
  try {
    const r = await ops[m.op](m);
    const transfer = r.transfer || [];
    delete r.transfer;
    self.postMessage({ id: m.id, ok: true, ...r }, transfer);
  } catch (err) {
    self.postMessage({ id: m.id, ok: false, error: String(err && err.message || err) });
  }
};
