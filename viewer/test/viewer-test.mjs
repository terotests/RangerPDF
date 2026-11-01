// The online viewer in headless Chromium (test/cdp.mjs, no Playwright): opens
// the sample PDF and checks drawing, text layer, search, zoom, rotation,
// sidebar, bookmarks, the pager and the iframe hand-over. Prints PASS/FAIL
// per check and `ALL PASS <n>` at the end.
// usage: node viewer/test/viewer-test.mjs [dist dir] [--shots=<dir>]
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { launch } from './cdp.mjs';
import { serve } from '../serve.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const args = process.argv.slice(2);
const dist = path.resolve(args.find((a) => !a.startsWith('--')) || path.join(here, '..', 'dist'));
const shots = (args.find((a) => a.startsWith('--shots=')) || '').slice(8);
const server = await serve(dist, 0);
const base = `http://127.0.0.1:${server.address().port}/`;
fs.writeFileSync(path.join(dist, 'host-test.html'), `<!doctype html><body style="margin:0">
<iframe id="f" src="index.html?embed=1" style="width:800px;height:700px;border:0"></iframe>
<script>
window.got = [];
addEventListener('message', async (e) => {
  got.push(e.data.type);
  if (e.data.type === 'rangerpdf:ready') {
    const b = await (await fetch('sample.pdf')).arrayBuffer();
    document.getElementById('f').contentWindow.postMessage({ type: 'rangerpdf:open', data: b, name: 'handed.pdf' }, '*', [b]);
  }
  if (e.data.type === 'rangerpdf:opened') window.opened = e.data;
});
</script></body>`);

let pass = 0, fail = 0;
const check = (name, ok, info = '') => {
  if (ok) pass++; else fail++;
  console.log(`${ok ? 'PASS' : 'FAIL'} ${name}${info ? '  ' + info : ''}`);
};
const b = await launch({ width: 1280, height: 900 });
try {
  const p = await b.page(base + 'index.html?file=sample.pdf');
  const S = (e) => p.eval(`(() => { const S = rangerPdf.S; return ${e}; })()`);
  await p.waitFor('rangerPdf.S.info && rangerPdf.S.renders >= 1');
  check('open', await S('S.info.pages === 3 && S.info.title === "Acme NDA signed"'), JSON.stringify(await S('S.info')));
  // the light blue box at the top of page 1 and black title text are drawn
  const px = await p.eval(`(() => {
    const c = document.querySelector('.pv-page canvas'); const g = c.getContext('2d');
    const s = c.width / 595; const at = (x, y) => Array.from(g.getImageData(Math.round(x * s), Math.round(y * s), 1, 1).data);
    let dark = 0; const d = g.getImageData(0, 0, c.width, Math.round(130 * s)).data;
    for (let i = 0; i < d.length; i += 4) if (d[i] < 60 && d[i + 1] < 60 && d[i + 2] < 60) dark++;
    return { box: at(520, 175), white: at(20, 20), dark, w: c.width };
  })()`);
  check('render', px.box[2] > 240 && px.box[0] < 245 && px.white.join() === '255,255,255,255' && px.dark > 200, JSON.stringify(px));
  const fw = await S(`[document.querySelector(".pv-page").offsetWidth, document.getElementById("pv-pages").clientWidth]`);
  check("fit width", Math.abs(fw[0] - (fw[1] - 40)) < 6, JSON.stringify(fw));
  check('text layer', await p.waitFor(`document.querySelector('.pv-text') && document.querySelector('.pv-text').textContent.includes('Non-Disclosure Agreement')`));
  // text selection: the layer's run lies over the drawn title
  const sel = await p.eval(`(() => {
    const sp = [...document.querySelectorAll('.pv-page .pv-text span')].find((s) => s.textContent === 'Non-Disclosure Agreement');
    const r = sp.getBoundingClientRect(), c = document.querySelector('.pv-page canvas').getBoundingClientRect();
    const range = document.createRange(); range.selectNodeContents(sp); getSelection().removeAllRanges(); getSelection().addRange(range);
    return { x: (r.left - c.left) / (c.width / 595), w: r.width / (c.width / 595), sel: String(getSelection()) };
  })()`);
  check('text selection', Math.abs(sel.x - 70) < 3 && sel.w > 300 && sel.w < 360 && sel.sel === 'Non-Disclosure Agreement', JSON.stringify(sel));
  // search
  await p.key('f', { ctrl: true });
  check('find opens', await p.eval(`!document.getElementById('pv-search').hidden && document.activeElement.id === 'pv-q'`));
  await p.type('confidential');
  await p.key('Enter');
  await p.waitFor(`document.getElementById('pv-count').textContent === '1/3'`, 5000).catch(() => {});
  check('find count', await p.eval(`document.getElementById('pv-count').textContent`) === '1/3', await p.eval(`document.getElementById('pv-count').textContent`));
  check('find highlights', await p.eval(`document.querySelectorAll('.pv-hit').length === 3 && document.querySelectorAll('.pv-hit.now').length === 1`));
  await p.key('Enter'); await p.key('Enter');
  check('find next', await p.eval(`document.getElementById('pv-count').textContent === '3/3' && rangerPdf.S.now.page === 1 && rangerPdf.S.cur === 1`),
    await p.eval(`document.getElementById('pv-count').textContent + ' cur ' + rangerPdf.S.cur`));
  if (shots) await p.screenshot(path.join(shots, '2-find.png'));
  await p.key('Enter');
  check('find wraps', await p.eval(`document.getElementById('pv-count').textContent === '1/3'`));
  await p.key('Escape');
  check('find closes', await p.eval(`document.getElementById('pv-search').hidden && document.querySelectorAll('.pv-hit').length === 0`));
  // pager
  await p.eval(`document.getElementById('pv-page-next').click()`);
  await p.eval(`document.getElementById('pv-page-next').click()`);
  await p.waitFor('rangerPdf.S.cur === 2');
  check('pager', await p.eval(`document.getElementById('pv-page').value === '3' && document.getElementById('pv-pages-n').textContent === '3'`));
  await p.key('Home');
  check('keys', await p.waitFor('rangerPdf.S.cur === 0').catch(() => false));
  // zoom
  await p.eval(`rangerPdf.setZoom(1)`);
  const w100 = await p.eval(`document.querySelector('.pv-page canvas').getBoundingClientRect().width`);
  await p.eval(`document.getElementById('pv-zoom-in').click()`);
  const z = await p.eval(`document.getElementById('pv-zoom-val').textContent`);
  await p.waitFor(`rangerPdf.S.pages[0].drawn.startsWith((1.1 * 96 / 72).toFixed(4))`);
  const zoomed = await p.eval(`({ css: document.querySelector('.pv-page canvas').getBoundingClientRect().width, px: document.querySelector('.pv-page canvas').width })`);
  check('zoom 100 %', Math.abs(w100 - 595 * 96 / 72) < 2, String(w100));
  check('zoom in', z === '110 %' && Math.abs(zoomed.css - 595 * 96 / 72 * 1.1) < 2 && Math.abs(zoomed.px - zoomed.css) < 2, z + ' ' + JSON.stringify(zoomed));
  await p.eval(`document.getElementById('pv-zoom-btn').click()`);
  await p.eval(`[...document.querySelectorAll('#pv-pop button')].find((b) => b.textContent === 'Page fit').click()`);
  const fitH = await p.eval(`document.querySelector('.pv-sheet').offsetHeight <= document.getElementById('pv-pages').clientHeight`);
  check('page fit', fitH && await p.eval(`rangerPdf.S.fit === 'page'`));
  // rotate
  await p.eval(`document.getElementById('pv-view-btn').click()`);
  await p.eval(`[...document.querySelectorAll('#pv-pop button')].find((b) => b.textContent === 'Rotate clockwise').click()`);
  await p.waitFor(`rangerPdf.S.pages[0].drawn.endsWith('/1')`);
  const rot = await p.eval(`(() => { const c = document.querySelector('.pv-page canvas'); const g = c.getContext('2d');
    const s = c.height / 595; const at = (x, y) => Array.from(g.getImageData(Math.round(x), Math.round(y), 1, 1).data);
    return { w: c.width, h: c.height, box: at(c.width - 130 * s, 300 * s), span: (() => { const sp = [...document.querySelectorAll('.pv-page .pv-text span')].find((x) => x.textContent === 'Non-Disclosure Agreement'); const r = sp.getBoundingClientRect(); return [r.width, r.height]; })() }; })()`);
  check('rotate', rot.w > rot.h && rot.box[2] > 240 && rot.box[0] < 245 && rot.span[1] > rot.span[0], JSON.stringify(rot));
  if (shots) await p.screenshot(path.join(shots, '3-rotated.png'));
  await p.eval(`rangerPdf.rotate(-1)`);
  await p.eval(`rangerPdf.setZoom('width')`);
  // sidebar: thumbnails and bookmarks
  await p.eval(`document.getElementById('pv-side-btn').click()`);
  await p.waitFor(`[...document.querySelectorAll('.pv-thumb')].filter((t) => t.dataset.drawn).length === 3`);
  check('thumbnails', await p.eval(`document.querySelectorAll('.pv-thumb').length === 3 && document.querySelector('.pv-thumb.now').dataset.i === '0'`));
  await p.eval(`document.getElementById('pv-tab-outline').click()`);
  check('bookmarks', await p.eval(`[...document.querySelectorAll('.pv-bm')].map((b) => b.textContent).join('|') === 'Non-Disclosure Agreement|Appendix A|Appendix B: schedule'`));
  await p.eval(`document.querySelectorAll('.pv-bm')[2].click()`);
  check('bookmark goes to page', await p.waitFor('rangerPdf.S.cur === 2').catch(() => false));
  if (shots) await p.screenshot(path.join(shots, '1-viewer.png'));
  // the properties dialog
  await p.eval(`document.getElementById('pv-menu-btn').click()`);
  await p.eval(`[...document.querySelectorAll('#pv-pop button')].find((b) => b.textContent === 'Document properties').click()`);
  check('properties', await p.eval(`document.getElementById('pv-dialog-body').textContent.includes('210 × 297 mm')`));
  await p.key('Enter');
  // pan tool
  await p.eval(`rangerPdf.setZoom(2); document.getElementById('pv-pan').click()`);
  const before = await p.eval(`document.getElementById('pv-pages').scrollTop`);
  for (const type of ['mousePressed', 'mouseMoved', 'mouseReleased']) {
    await p.send('Input.dispatchMouseEvent', { type, x: 700, y: type === 'mousePressed' ? 600 : 400, button: 'left', buttons: type === 'mouseReleased' ? 0 : 1, clickCount: 1 });
  }
  const after = await p.eval(`document.getElementById('pv-pages').scrollTop`);
  check('pan', after - before > 150, `${before} -> ${after}`);
  // a broken file
  await p.eval(`rangerPdf.open(new TextEncoder().encode('not a pdf'), 'bad.pdf')`);
  check('broken file', await p.waitFor(`document.querySelector('.pv-msg.err')`).catch(() => false));

  // in an iframe, the file handed over by the host page
  const h = await b.page(base + 'host-test.html');
  await h.waitFor('window.opened', 15000).catch(() => {});
  const got = await h.eval(`({ got: window.got, opened: window.opened })`);
  check('iframe hand-over', got.opened && got.opened.pages === 3 && got.got[0] === 'rangerpdf:ready', JSON.stringify(got));
  if (shots) await h.screenshot(path.join(shots, '4-embedded.png'));
} catch (e) {
  fail++;
  console.log('FAIL exception ' + (e.stack || e));
} finally {
  await b.close();
  server.close();
  fs.rmSync(path.join(dist, 'host-test.html'), { force: true });
}
console.log(fail ? `FAILED ${fail} of ${pass + fail}` : `ALL PASS ${pass}`);
process.exit(fail ? 1 : 0);
