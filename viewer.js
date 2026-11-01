// The online PDF viewer: the page side. The reader runs as WebAssembly in a
// Worker (pdf-worker.js); this file lays out the pages, draws the pixels the
// worker sends, and puts a transparent text layer over each page for
// selection, plus the search hits. No libraries.
//
// Opening a file: ?file=<url> (&name=...), a dropped or chosen file, or from a
// host page (the viewer in an iframe) with
//   frame.contentWindow.postMessage({ type: "rangerpdf:open", data: arrayBuffer, name }, "*")
// The viewer posts {type: "rangerpdf:ready"} to its parent when it can take a
// file and {type: "rangerpdf:opened", pages, error} after each one.
// Other parameters: embed=1 (no header, for a host's preview pane),
// theme=light|dark, page=<n>, zoom=<percent>|width|page, q=<search>.
'use strict';
(() => {
  const $ = (id) => document.getElementById(id);
  const params = new URLSearchParams(location.search + '&' + location.hash.replace(/^#/, ''));
  const PT = 96 / 72; // CSS pixels per point at 100 %
  const STEPS = [0.1, 0.25, 0.33, 0.5, 0.67, 0.75, 0.8, 0.9, 1, 1.1, 1.25, 1.5, 1.75, 2, 2.5, 3, 4, 5];
  const KEEP = 3; // pages kept drawn on each side of the shown ones

  // ---- icons (24 x 24, stroked) ----
  const ICONS = {
    menu: 'M4 7h16M4 12h16M4 17h16',
    side: 'M4 5h16v14H4zM9 5v14',
    gear: 'M12 9a3 3 0 1 0 0 6 3 3 0 0 0 0-6zM19 12l2-1-1-3-2 .2-1.3-1.3.2-2-3-1-1 2h-1.8l-1-2-3 1 .2 2L6 7.2 4 7 3 10l2 1v2l-2 1 1 3 2-.2 1.3 1.3-.2 2 3 1 1-2h1.8l1 2 3-1-.2-2 1.3-1.3 2 .2 1-3-2-1z',
    chev: 'M7 10l5 5 5-5',
    minus: 'M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM8 12h8',
    plus: 'M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM8 12h8M12 8v8',
    hand: 'M8 13V6.5a1.5 1.5 0 0 1 3 0V12M11 11V4.5a1.5 1.5 0 0 1 3 0V12M14 11.5V6a1.5 1.5 0 0 1 3 0v8c0 4-2.5 7-6 7-2.5 0-4-1.2-5.5-3.5L3.8 14a1.5 1.5 0 0 1 2.4-1.8L8 14.5',
    select: 'M4 8V4h4M16 4h4v4M20 16v4h-4M8 20H4v-4M10 4h4M10 20h4M4 10v4M20 10v4',
    search: 'M10.5 4a6.5 6.5 0 1 0 0 13 6.5 6.5 0 0 0 0-13zM15.5 15.5L20 20',
    up: 'M6 15l6-6 6 6',
    down: 'M6 9l6 6 6-6',
    left: 'M15 6l-6 6 6 6',
    right: 'M9 6l6 6-6 6',
    close: 'M6 6l12 12M18 6L6 18',
    full: 'M4 9V4h5M15 4h5v5M20 15v5h-5M9 20H4v-5',
    open: 'M3 7V5h7l2 2h9v12H3zM3 9h18',
    download: 'M12 4v11M7 10l5 5 5-5M5 20h14',
    doc: 'M6 3h8l4 4v14H6zM14 3v4h4M9 12h6M9 16h6',
    rotr: 'M20 12a8 8 0 1 1-2.3-5.7M20 4v4h-4',
    rotl: 'M4 12a8 8 0 1 0 2.3-5.7M4 4v4h4',
    info: 'M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM12 11v6M12 7.5v.5',
  };
  const icon = (name) => `<svg class="pv-i" viewBox="0 0 24 24" aria-hidden="true"><path d="${ICONS[name] || ''}"/></svg>`;
  for (const el of document.querySelectorAll('[data-icon]')) el.innerHTML = icon(el.dataset.icon);

  // ---- the worker ----
  const base = new URL('.', document.currentScript ? document.currentScript.src : location.href).href;
  const worker = new Worker(new URL('pdf-worker.js', base));
  let seq = 0;
  const waiting = new Map();
  worker.onmessage = (e) => {
    const w = waiting.get(e.data.id);
    if (!w) return;
    waiting.delete(e.data.id);
    if (e.data.ok) w.ok(e.data); else w.fail(new Error(e.data.error));
  };
  const call = (op, args = {}, transfer = []) => new Promise((ok, fail) => {
    const id = ++seq;
    waiting.set(id, { ok, fail });
    worker.postMessage({ id, op, ...args }, transfer);
  });
  const ready = call('init', { base });

  // ---- state ----
  const S = {
    info: null, name: '', bytes: null, loadMs: 0,
    zoom: 1, fit: 'width', quarter: 0, cur: 0, tool: 'select',
    pages: [], tops: [],
    query: '', hits: [], now: null, pageHits: [], searchGen: 0,
    gen: 0, busy: false, renders: 0,
  };
  window.rangerPdf = { S, open: (data, name) => openBytes(data, name), goto: (i) => goto(i), setZoom: (z) => setZoom(z), rotate: (d) => rotate(d), find: (q) => runSearch(q) };

  const pagesEl = $('pv-pages');
  const pv = $('pv');
  if (params.get('embed') === '1') pv.classList.add('embed');
  if (params.get('theme')) document.documentElement.dataset.theme = params.get('theme');

  // ---- sizes ----
  const turned = () => S.quarter % 2 === 1;
  const sizeOf = (i) => {
    const [w, h] = S.info.sizes[i] || [612, 792];
    return turned() ? [h, w] : [w, h];
  };
  const scale = () => S.zoom * PT; // CSS px per point

  function fitZoom() {
    if (!S.info || !S.fit) return;
    const availW = pagesEl.clientWidth - 42;
    const availH = pagesEl.clientHeight - 56;
    // as pdf.js: the page shown now decides
    const [w, h] = sizeOf(S.cur);
    let z = availW / (w * PT);
    if (S.fit === 'page') z = Math.min(z, availH / (h * PT));
    S.zoom = Math.max(0.1, Math.min(5, z));
  }

  // ---- pages ----
  function buildPages() {
    pagesEl.querySelectorAll('.pv-page, .pv-msg').forEach((e) => e.remove());
    S.pages = [];
    for (let i = 0; i < S.info.pages; i++) {
      const el = document.createElement('div');
      el.className = 'pv-page';
      el.dataset.page = String(i + 1);
      el.innerHTML = '<div class="pv-sheet"><canvas></canvas><div class="pv-over"><div class="pv-hits"></div><div class="pv-text"></div></div></div>' +
        `<div class="pv-num">${i + 1}</div>`;
      pagesEl.appendChild(el);
      const sheet = el.firstChild;
      S.pages.push({
        el, sheet, canvas: sheet.querySelector('canvas'), over: sheet.querySelector('.pv-over'),
        hitsEl: sheet.querySelector('.pv-hits'), textEl: sheet.querySelector('.pv-text'),
        drawn: '', text: null,
      });
    }
    layout();
  }

  function layout() {
    if (!S.info) return;
    const s = scale();
    for (let i = 0; i < S.pages.length; i++) {
      const p = S.pages[i];
      const [w, h] = sizeOf(i);
      const cw = Math.round(w * s), ch = Math.round(h * s);
      p.el.style.width = cw + 2 + 'px';
      p.sheet.style.width = cw + 2 + 'px';
      p.sheet.style.height = ch + 2 + 'px';
      p.canvas.style.width = cw + 'px';
      p.canvas.style.height = ch + 'px';
      const [bw, bh] = S.info.sizes[i];
      const q = S.quarter;
      const tx = q === 1 ? bh * s : q === 2 ? bw * s : 0;
      const ty = q === 2 ? bh * s : q === 3 ? bw * s : 0;
      p.over.style.width = bw + 'px';
      p.over.style.height = bh + 'px';
      p.over.style.transform = `translate(${tx}px, ${ty}px) rotate(${q * 90}deg) scale(${s})`;
    }
    S.tops = S.pages.map((p) => p.el.offsetTop);
    $('pv-zoom-val').textContent = Math.round(S.zoom * 100) + ' %';
    schedule();
  }

  function shown() {
    const top = pagesEl.scrollTop, bottom = top + pagesEl.clientHeight;
    let first = -1, last = -1;
    for (let i = 0; i < S.pages.length; i++) {
      const t = S.tops[i], b = t + S.pages[i].el.offsetHeight;
      if (b >= top && t <= bottom) { if (first < 0) first = i; last = i; }
      if (t > bottom) break;
    }
    return [Math.max(first, 0), Math.max(last, 0)];
  }

  function updateCurrent() {
    if (!S.info || !S.pages.length) return;
    const y = pagesEl.scrollTop + pagesEl.clientHeight * 0.3;
    let cur = 0;
    for (let i = 0; i < S.tops.length; i++) if (S.tops[i] <= y) cur = i; else break;
    if (pagesEl.scrollTop + pagesEl.clientHeight >= pagesEl.scrollHeight - 2) {
      const [, last] = shown();
      if (S.tops[last] > pagesEl.scrollTop) cur = Math.max(cur, last);
    }
    if (cur !== S.cur) {
      S.cur = cur;
      markThumb();
    }
    if (document.activeElement !== $('pv-page')) $('pv-page').value = String(S.cur + 1);
  }

  // one render at a time; pages on screen first, then their neighbours,
  // then sidebar thumbnails
  function schedule() {
    if (S.busy || !S.info) return;
    const [first, last] = shown();
    const s = scale() * (window.devicePixelRatio || 1);
    const key = (i) => `${s.toFixed(4)}/${S.quarter}`;
    const order = [];
    for (let i = first; i <= last; i++) order.push(i);
    for (let d = 1; d <= KEEP; d++) { order.push(last + d); order.push(first - d); }
    let next = -1;
    for (const i of order) {
      if (i < 0 || i >= S.pages.length) continue;
      if (S.pages[i].drawn !== key(i)) { next = i; break; }
    }
    // drop pages far from view
    for (let i = 0; i < S.pages.length; i++) {
      if ((i < first - KEEP || i > last + KEEP) && S.pages[i].drawn) {
        const p = S.pages[i];
        p.canvas.width = 0; p.canvas.height = 0; p.drawn = '';
        p.textEl.textContent = ''; p.text = null;
      }
    }
    if (next >= 0) { drawPage(next, s, key(next)); return; }
    const t = nextThumb();
    if (t >= 0) drawThumb(t);
  }

  async function drawPage(i, s, key) {
    S.busy = true;
    const gen = S.gen;
    try {
      const r = await call('render', { page: i, scale: s, quarter: S.quarter });
      if (gen !== S.gen) return;
      const p = S.pages[i];
      if (r.w > 0) {
        p.canvas.width = r.w; p.canvas.height = r.h;
        p.canvas.getContext('2d').putImageData(new ImageData(new Uint8ClampedArray(r.px), r.w, r.h), 0, 0);
      }
      p.drawn = key;
      S.renders++;
      if (S.tool === 'select' && !p.text) await loadText(i, gen);
      drawHits(i);
    } catch (e) {
      S.pages[i].drawn = 'error';
      console.error('page', i + 1, e);
    } finally {
      S.busy = false;
      schedule();
    }
  }

  // the text layer: characters joined into runs along a line, each run
  // stretched to the width the reader measured
  const measure = document.createElement('canvas').getContext('2d');
  async function loadText(i, gen) {
    const r = await call('text', { page: i });
    if (gen !== S.gen) return;
    const p = S.pages[i];
    const runs = [];
    for (const [x, y, w, h, dir, t] of r.items) {
      const last = runs[runs.length - 1];
      if (last && dir === 0 && last.dir === 0 && Math.abs(last.y - y) < 0.5 && Math.abs(last.h - h) < 0.5 &&
          x >= last.x + last.w - h * 0.2 && x <= last.x + last.w + h * 0.35) {
        if (x > last.x + last.w + h * 0.15 && t !== ' ' && !last.t.endsWith(' ')) last.t += ' ';
        last.t += t;
        last.w = x + w - last.x;
      } else {
        runs.push({ x, y, w, h, dir, t });
      }
    }
    const frag = document.createDocumentFragment();
    for (const run of runs) {
      if (!run.t.trim() || run.h <= 0) continue;
      const span = document.createElement('span');
      span.textContent = run.t;
      measure.font = `${run.h}px sans-serif`;
      const natural = measure.measureText(run.t).width || 1;
      span.style.left = run.x + 'px';
      span.style.top = run.y + 'px';
      span.style.fontSize = run.h + 'px';
      span.style.transform = `scaleX(${run.w / natural})${run.dir ? ` rotate(${run.dir}deg)` : ''}`;
      frag.appendChild(span);
      frag.appendChild(document.createTextNode('\n'));
    }
    p.textEl.textContent = '';
    p.textEl.appendChild(frag);
    p.text = runs;
  }

  // ---- opening ----
  function message(text, err) {
    pagesEl.querySelectorAll('.pv-page, .pv-msg').forEach((e) => e.remove());
    $('pv-empty').hidden = true;
    const m = document.createElement('div');
    m.className = 'pv-msg' + (err ? ' err' : '');
    m.textContent = text;
    pagesEl.appendChild(m);
  }

  async function openBytes(data, name, password = '') {
    await ready;
    const bytes = data instanceof ArrayBuffer ? data : data.buffer.slice(data.byteOffset, data.byteOffset + data.byteLength);
    S.gen++;
    S.bytes = bytes.slice(0);
    S.name = name || 'document.pdf';
    $('pv-file').textContent = S.name;
    document.title = S.name + ' - PDF viewer';
    $('pv-empty').hidden = true;
    message('Opening ' + S.name + '…');
    const t0 = performance.now();
    const r = await call('open', { data: bytes, password }, [bytes]);
    S.loadMs = performance.now() - t0;
    const info = r.info;
    const reply = (extra) => { if (parent !== window) parent.postMessage({ type: 'rangerpdf:opened', pages: info.pages, ...extra }, '*'); };
    if (info.pages === 0) {
      S.info = null;
      if (/password/i.test(info.error)) {
        const pw = await ask('Password required', `<p>${esc(S.name)} is protected. Enter its password.</p><input type="password" id="pv-pw" autocomplete="off">`, '#pv-pw');
        if (pw !== null) return openBytes(S.bytes, S.name, pw);
        message('The file is protected by a password.', true);
      } else {
        message('The file could not be shown' + (info.error ? ': ' + info.error : '.'), true);
      }
      reply({ error: info.error || 'no pages' });
      return;
    }
    S.info = info;
    S.quarter = 0; S.cur = 0;
    clearSearch();
    $('pv-download-btn').disabled = false;
    $('pv-pages-n').textContent = String(info.pages);
    const z = params.get('zoom');
    if (z && /^\d+$/.test(z)) { S.fit = null; S.zoom = Math.max(0.1, Math.min(5, +z / 100)); } else S.fit = z === 'page' ? 'page' : 'width';
    fitZoom();
    buildPages();
    buildThumbs();
    loadOutline();
    pagesEl.scrollTop = 0;
    const pg = +params.get('page');
    if (pg > 1) goto(pg - 1);
    updateCurrent();
    reply({ error: '' });
    if (params.get('q') && !S.query) { openSearch(); $('pv-q').value = params.get('q'); runSearch(params.get('q')); }
  }

  async function openUrl(url, name) {
    message('Loading…');
    try {
      const r = await fetch(url, { credentials: 'same-origin' });
      if (!r.ok) throw new Error(r.status + ' ' + r.statusText);
      const n = name || decodeURIComponent(new URL(url, location.href).pathname.split('/').pop() || 'document.pdf');
      await openBytes(await r.arrayBuffer(), n);
    } catch (e) {
      message('The file could not be loaded: ' + e.message, true);
    }
  }

  // ---- navigation and zoom ----
  function goto(i, smooth) {
    if (!S.info) return;
    i = Math.max(0, Math.min(S.info.pages - 1, i));
    pagesEl.scrollTo({ top: S.tops[i] - 12, behavior: smooth ? 'smooth' : 'auto' });
    S.cur = i;
    $('pv-page').value = String(i + 1);
    markThumb();
  }

  function setZoom(z, anchor) {
    if (!S.info) return;
    if (z === 'width' || z === 'page') { S.fit = z; fitZoom(); } else { S.fit = null; S.zoom = Math.max(0.1, Math.min(5, z)); }
    keepPlace(() => layout(), anchor);
  }

  // re-lays out keeping the point under `anchor` (or the top of the view) in place
  function keepPlace(fn, anchor) {
    const i = S.cur;
    const ay = anchor ? anchor.y : 0, ax = anchor ? anchor.x : 0;
    const relY = (pagesEl.scrollTop + ay - (S.tops[i] || 0)) / Math.max(1, S.pages[i] ? S.pages[i].el.offsetHeight : 1);
    const relX = (pagesEl.scrollLeft + ax) / Math.max(1, pagesEl.scrollWidth);
    fn();
    if (!S.pages[i]) return;
    pagesEl.scrollTop = S.tops[i] + relY * S.pages[i].el.offsetHeight - ay;
    pagesEl.scrollLeft = relX * pagesEl.scrollWidth - ax;
    updateCurrent();
  }

  function zoomStep(dir, anchor) {
    const z = S.zoom;
    let n = dir > 0 ? STEPS.find((s) => s > z + 0.001) : [...STEPS].reverse().find((s) => s < z - 0.001);
    if (n === undefined) n = dir > 0 ? STEPS[STEPS.length - 1] : STEPS[0];
    setZoom(n, anchor);
  }

  function rotate(dir) {
    if (!S.info) return;
    S.quarter = (S.quarter + dir + 4) % 4;
    S.gen++;
    for (const p of S.pages) { p.drawn = ''; p.text = null; p.textEl.textContent = ''; }
    if (S.fit) fitZoom();
    const i = S.cur;
    layout();
    buildThumbs();
    goto(i);
  }

  function setTool(t) {
    S.tool = t;
    $('pv-pan').setAttribute('aria-pressed', String(t === 'pan'));
    $('pv-select').setAttribute('aria-pressed', String(t === 'select'));
    pagesEl.classList.toggle('pan', t === 'pan');
    if (t === 'select') {
      const gen = S.gen;
      S.pages.forEach((p, i) => { if (p.drawn && !p.text) loadText(i, gen); });
    }
  }

  // ---- search ----
  function openSearch() {
    $('pv-search').hidden = false;
    $('pv-find-btn').setAttribute('aria-pressed', 'true');
    $('pv-q').focus();
    $('pv-q').select();
  }
  function clearSearch() {
    S.searchGen++;
    S.query = ''; S.hits = []; S.now = null; S.pageHits = [];
    $('pv-count').textContent = '';
    for (let i = 0; i < S.pages.length; i++) drawHits(i);
  }
  function closeSearch() {
    $('pv-search').hidden = true;
    $('pv-find-btn').setAttribute('aria-pressed', 'false');
    clearSearch();
    pagesEl.focus();
  }

  async function runSearch(q) {
    clearSearch();
    q = (q || '').trim();
    if (!q || !S.info) return;
    const gen = ++S.searchGen;
    S.query = q;
    $('pv-count').textContent = '…';
    // from the current page on, wrapping round
    const n = S.info.pages;
    const found = new Array(n);
    for (let k = 0; k < n; k++) {
      const i = (S.cur + k) % n;
      const r = await call('find', { page: i, query: q });
      if (gen !== S.searchGen) return;
      found[i] = r.boxes;
      S.pageHits[i] = r.boxes;
      drawHits(i);
      S.hits = flat(found);
      if (!S.now && r.boxes.length) {
        S.now = { page: i, k: 0 };
        showHit();
      }
      showCount(k + 1 < n);
    }
    showCount(false);
  }
  const hitIndex = () => S.now ? S.hits.findIndex((h) => h.page === S.now.page && h.k === S.now.k) : -1;
  function showCount(more) {
    $('pv-count').textContent = `${hitIndex() + 1}/${S.hits.length}${more ? '+' : ''}`;
  }
  function flat(found) {
    const out = [];
    found.forEach((boxes, page) => (boxes || []).forEach((b, k) => out.push({ page, k, box: b })));
    return out;
  }

  function drawHits(i) {
    const p = S.pages[i];
    if (!p) return;
    p.hitsEl.textContent = '';
    const boxes = S.pageHits[i] || [];
    const now = S.now;
    boxes.forEach(([x, y, w, h], k) => {
      const d = document.createElement('div');
      d.className = 'pv-hit' + (now && now.page === i && now.k === k ? ' now' : '');
      d.style.left = x - 1 + 'px'; d.style.top = y - 1 + 'px';
      d.style.width = w + 2 + 'px'; d.style.height = h + 2 + 'px';
      p.hitsEl.appendChild(d);
    });
  }

  function showHit() {
    const h = S.hits[hitIndex()];
    if (!h) return;
    showCount(false);
    for (let i = 0; i < S.pages.length; i++) if (S.pageHits[i] && S.pageHits[i].length) drawHits(i);
    // the box on the turned page, in CSS px from the page's corner
    const [bw, bh] = S.info.sizes[h.page];
    let [x, y, w, hh] = h.box;
    const q = S.quarter;
    if (q === 1) [x, y, w, hh] = [bh - (y + hh), x, hh, w];
    else if (q === 2) [x, y] = [bw - (x + w), bh - (y + hh)];
    else if (q === 3) [x, y, w, hh] = [y, bw - (x + w), hh, w];
    const s = scale();
    const top = S.tops[h.page] + 1 + y * s, left = S.pages[h.page].el.offsetLeft + 1 + x * s;
    if (top < pagesEl.scrollTop + 20 || top + hh * s > pagesEl.scrollTop + pagesEl.clientHeight - 20) {
      pagesEl.scrollTop = top - pagesEl.clientHeight / 3;
    }
    if (left < pagesEl.scrollLeft || left + w * s > pagesEl.scrollLeft + pagesEl.clientWidth) {
      pagesEl.scrollLeft = left - pagesEl.clientWidth / 3;
    }
    updateCurrent();
  }
  function stepHit(d) {
    if (!S.hits.length) { if ($('pv-q').value.trim() !== S.query) runSearch($('pv-q').value); return; }
    const h = S.hits[(hitIndex() + d + S.hits.length) % S.hits.length];
    S.now = { page: h.page, k: h.k };
    showHit();
  }

  // ---- sidebar ----
  const THUMB_W = 120;
  let thumbSeen = new Set();
  const thumbIo = new IntersectionObserver((es) => {
    for (const e of es) {
      const i = +e.target.dataset.i;
      if (e.isIntersecting) thumbSeen.add(i); else thumbSeen.delete(i);
    }
    schedule();
  }, { root: $('pv-thumbs'), rootMargin: '200px' });

  function buildThumbs() {
    const box = $('pv-thumbs');
    thumbIo.disconnect();
    thumbSeen = new Set();
    box.textContent = '';
    if (!S.info) return;
    for (let i = 0; i < S.info.pages; i++) {
      const [w, h] = sizeOf(i);
      const b = document.createElement('button');
      b.className = 'pv-thumb';
      b.dataset.i = String(i);
      b.setAttribute('role', 'listitem');
      b.setAttribute('aria-label', 'Page ' + (i + 1));
      const c = document.createElement('canvas');
      c.style.width = THUMB_W + 'px';
      c.style.height = Math.round(THUMB_W * h / w) + 'px';
      c.width = 0;
      b.append(c, String(i + 1));
      b.onclick = () => goto(i, true);
      box.appendChild(b);
      thumbIo.observe(b);
    }
    markThumb();
  }
  function nextThumb() {
    if ($('pv-side').hidden || !$('pv-outline').hidden) return -1;
    for (const i of [...thumbSeen].sort((a, b) => a - b)) {
      const c = $('pv-thumbs').children[i];
      if (c && !c.dataset.drawn) return i;
    }
    return -1;
  }
  async function drawThumb(i) {
    S.busy = true;
    const gen = S.gen;
    try {
      const [w] = sizeOf(i);
      const s = THUMB_W / w * (window.devicePixelRatio || 1);
      const r = await call('render', { page: i, scale: s, quarter: S.quarter });
      if (gen !== S.gen) return;
      const b = $('pv-thumbs').children[i];
      const c = b.querySelector('canvas');
      if (r.w > 0) {
        c.width = r.w; c.height = r.h;
        c.getContext('2d').putImageData(new ImageData(new Uint8ClampedArray(r.px), r.w, r.h), 0, 0);
      }
      b.dataset.drawn = '1';
    } catch (e) {
      console.error('thumbnail', i + 1, e);
    } finally {
      S.busy = false;
      schedule();
    }
  }
  function markThumb() {
    const box = $('pv-thumbs');
    for (const b of box.querySelectorAll('.pv-thumb.now')) b.classList.remove('now');
    const b = box.children[S.cur];
    if (!b) return;
    b.classList.add('now');
    if (!$('pv-side').hidden && (b.offsetTop < box.scrollTop || b.offsetTop + b.offsetHeight > box.scrollTop + box.clientHeight)) {
      box.scrollTop = b.offsetTop - 20;
    }
  }

  async function loadOutline() {
    const box = $('pv-outline');
    box.textContent = '';
    const r = await call('outline');
    if (!r.items.length) {
      box.innerHTML = '<div class="pv-bm-none">No bookmarks</div>';
      return;
    }
    for (const [level, page, title] of r.items) {
      const b = document.createElement('button');
      b.className = 'pv-bm';
      b.setAttribute('role', 'treeitem');
      b.setAttribute('aria-level', String(level));
      b.style.paddingLeft = 12 + (level - 1) * 16 + 'px';
      b.textContent = title || '(untitled)';
      if (page >= 0) b.onclick = () => goto(page, true); else b.disabled = true;
      box.appendChild(b);
    }
  }

  function sideTab(which) {
    $('pv-tab-thumbs').setAttribute('aria-selected', String(which === 'thumbs'));
    $('pv-tab-outline').setAttribute('aria-selected', String(which === 'outline'));
    $('pv-thumbs').hidden = which !== 'thumbs';
    $('pv-outline').hidden = which !== 'outline';
    schedule();
  }
  function toggleSide(open) {
    const side = $('pv-side');
    side.hidden = open === undefined ? !side.hidden : !open;
    $('pv-side-btn').setAttribute('aria-pressed', String(!side.hidden));
    if (S.fit) setZoom(S.fit); else layout();
    markThumb();
  }

  // ---- menus and dialogs ----
  const pop = $('pv-pop');
  let popFor = null;
  function menu(btn, items) {
    if (popFor === btn) return closeMenu();
    closeMenu();
    pop.textContent = '';
    for (const it of items) {
      if (it === '-') { pop.appendChild(document.createElement('hr')); continue; }
      const b = document.createElement('button');
      b.setAttribute('role', it.checked === undefined ? 'menuitem' : 'menuitemradio');
      if (it.checked !== undefined) b.setAttribute('aria-checked', String(!!it.checked));
      b.innerHTML = (it.icon ? icon(it.icon) : '') + `<span>${esc(it.label)}</span>`;
      b.disabled = !!it.disabled;
      b.onclick = () => { closeMenu(); it.run(); };
      pop.appendChild(b);
    }
    const r = btn.getBoundingClientRect();
    pop.hidden = false;
    pop.style.left = Math.max(8, Math.min(r.left, innerWidth - pop.offsetWidth - 8)) + 'px';
    pop.style.top = r.bottom + 4 + 'px';
    btn.classList.add('open');
    popFor = btn;
    const f = pop.querySelector('button:not(:disabled)');
    if (f) f.focus();
  }
  function closeMenu() {
    pop.hidden = true;
    if (popFor) popFor.classList.remove('open');
    popFor = null;
  }
  pop.addEventListener('keydown', (e) => {
    const bs = [...pop.querySelectorAll('button:not(:disabled)')];
    const k = bs.indexOf(document.activeElement);
    if (e.key === 'ArrowDown') { bs[(k + 1) % bs.length].focus(); e.preventDefault(); }
    if (e.key === 'ArrowUp') { bs[(k - 1 + bs.length) % bs.length].focus(); e.preventDefault(); }
    if (e.key === 'Escape') { const b = popFor; closeMenu(); if (b) b.focus(); }
  });
  document.addEventListener('pointerdown', (e) => {
    if (popFor && !pop.contains(e.target) && !popFor.contains(e.target)) closeMenu();
  });

  const esc = (s) => String(s).replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));

  // a modal with OK / Cancel; resolves with the focused input's value (or
  // '' without one), null when cancelled
  function ask(title, html, focus, okOnly) {
    return new Promise((resolve) => {
      $('pv-dialog-title').textContent = title;
      $('pv-dialog-body').innerHTML = html;
      $('pv-dialog-cancel').hidden = !!okOnly;
      $('pv-dialog').hidden = false;
      const inp = focus ? $('pv-dialog-body').querySelector(focus) : null;
      (inp || $('pv-dialog-ok')).focus();
      const done = (v) => {
        $('pv-dialog').hidden = true;
        $('pv-dialog-form').onsubmit = null;
        $('pv-dialog-cancel').onclick = null;
        resolve(v);
      };
      $('pv-dialog-form').onsubmit = (e) => { e.preventDefault(); done(inp ? inp.value : ''); };
      $('pv-dialog-cancel').onclick = () => done(null);
      $('pv-dialog').onkeydown = (e) => { if (e.key === 'Escape') done(null); };
    });
  }

  function properties() {
    if (!S.info) return;
    const [w, h] = S.info.sizes[0];
    const mm = (pt) => Math.round(pt / 72 * 25.4);
    const kb = S.bytes ? (S.bytes.byteLength / 1024).toFixed(1) + ' kB' : '';
    ask('Document properties', `<dl>
      <dt>File</dt><dd>${esc(S.name)}</dd>
      <dt>Title</dt><dd>${esc(S.info.title || '—')}</dd>
      <dt>Pages</dt><dd>${S.info.pages}</dd>
      <dt>Page size</dt><dd>${mm(w)} × ${mm(h)} mm</dd>
      <dt>File size</dt><dd>${kb}</dd>
      <dt>Opened in</dt><dd>${Math.round(S.loadMs)} ms</dd></dl>`, null, true);
  }

  function download() {
    if (!S.bytes) return;
    const a = document.createElement('a');
    a.href = URL.createObjectURL(new Blob([S.bytes], { type: 'application/pdf' }));
    a.download = S.name;
    document.body.appendChild(a);
    a.click();
    setTimeout(() => { URL.revokeObjectURL(a.href); a.remove(); }, 1000);
  }
  const pickFile = () => $('pv-file-input').click();

  // ---- wiring ----
  $('pv-menu-btn').onclick = (e) => menu(e.currentTarget, [
    { label: 'Open file…', icon: 'open', run: pickFile },
    { label: 'Download', icon: 'download', run: download, disabled: !S.bytes },
    '-',
    { label: 'Document properties', icon: 'info', run: properties, disabled: !S.info },
  ]);
  $('pv-view-btn').onclick = (e) => menu(e.currentTarget, [
    { label: 'Rotate clockwise', icon: 'rotr', run: () => rotate(1), disabled: !S.info },
    { label: 'Rotate counterclockwise', icon: 'rotl', run: () => rotate(-1), disabled: !S.info },
    '-',
    { label: 'Page fit', checked: S.fit === 'page', run: () => setZoom('page') },
    { label: 'Page width', checked: S.fit === 'width', run: () => setZoom('width') },
    { label: 'Actual size', checked: !S.fit && Math.abs(S.zoom - 1) < 0.001, run: () => setZoom(1) },
  ]);
  $('pv-zoom-btn').onclick = (e) => menu(e.currentTarget, [
    { label: 'Page fit', checked: S.fit === 'page', run: () => setZoom('page') },
    { label: 'Page width', checked: S.fit === 'width', run: () => setZoom('width') },
    '-',
    ...[0.5, 0.75, 1, 1.25, 1.5, 2, 3, 4].map((z) => ({ label: z * 100 + ' %', checked: !S.fit && Math.abs(S.zoom - z) < 0.001, run: () => setZoom(z) })),
  ]);
  $('pv-zoom-in').onclick = () => zoomStep(1);
  $('pv-zoom-out').onclick = () => zoomStep(-1);
  $('pv-side-btn').onclick = () => toggleSide();
  $('pv-tab-thumbs').onclick = () => sideTab('thumbs');
  $('pv-tab-outline').onclick = () => sideTab('outline');
  $('pv-pan').onclick = () => setTool('pan');
  $('pv-select').onclick = () => setTool('select');
  $('pv-find-btn').onclick = () => ($('pv-search').hidden ? openSearch() : closeSearch());
  $('pv-find-close').onclick = closeSearch;
  $('pv-next').onclick = () => stepHit(1);
  $('pv-prev').onclick = () => stepHit(-1);
  let qTimer = 0;
  $('pv-q').addEventListener('input', () => {
    clearTimeout(qTimer);
    qTimer = setTimeout(() => runSearch($('pv-q').value), 300);
  });
  $('pv-q').addEventListener('keydown', (e) => {
    if (e.key === 'Enter') {
      e.preventDefault();
      clearTimeout(qTimer);
      if ($('pv-q').value.trim() !== S.query) runSearch($('pv-q').value); else stepHit(e.shiftKey ? -1 : 1);
    }
    if (e.key === 'Escape') closeSearch();
  });
  $('pv-page-prev').onclick = () => goto(S.cur - 1);
  $('pv-page-next').onclick = () => goto(S.cur + 1);
  $('pv-page').addEventListener('keydown', (e) => {
    if (e.key === 'Enter') { const n = parseInt($('pv-page').value, 10); if (n > 0) goto(n - 1); pagesEl.focus(); }
  });
  $('pv-page').addEventListener('blur', () => { $('pv-page').value = String(S.cur + 1); });
  $('pv-open-btn').onclick = pickFile;
  $('pv-open-big').onclick = pickFile;
  $('pv-download-btn').onclick = download;
  $('pv-sample').onclick = (e) => { e.preventDefault(); openUrl(new URL('sample.pdf', base).href, 'sample.pdf'); };
  $('pv-full').onclick = () => {
    if (document.fullscreenElement) document.exitFullscreen(); else pv.requestFullscreen && pv.requestFullscreen();
  };
  $('pv-file-input').onchange = async (e) => {
    const f = e.target.files[0];
    if (f) openBytes(await f.arrayBuffer(), f.name);
    e.target.value = '';
  };

  pagesEl.addEventListener('scroll', () => { updateCurrent(); schedule(); }, { passive: true });
  let resizeTimer = 0;
  new ResizeObserver(() => {
    clearTimeout(resizeTimer);
    resizeTimer = setTimeout(() => { if (S.fit) setZoom(S.fit); else schedule(); }, 80);
  }).observe(pagesEl);

  pagesEl.addEventListener('wheel', (e) => {
    if (!(e.ctrlKey || e.metaKey) || !S.info) return;
    e.preventDefault();
    const r = pagesEl.getBoundingClientRect();
    zoomStep(e.deltaY < 0 ? 1 : -1, { x: e.clientX - r.left, y: e.clientY - r.top });
  }, { passive: false });

  // pan tool: drag the pages
  let drag = null;
  pagesEl.addEventListener('pointerdown', (e) => {
    if (S.tool !== 'pan' || e.button !== 0) return;
    drag = { x: e.clientX, y: e.clientY, l: pagesEl.scrollLeft, t: pagesEl.scrollTop };
    pagesEl.setPointerCapture(e.pointerId);
    pagesEl.classList.add('dragging');
  });
  pagesEl.addEventListener('pointermove', (e) => {
    if (!drag) return;
    pagesEl.scrollLeft = drag.l - (e.clientX - drag.x);
    pagesEl.scrollTop = drag.t - (e.clientY - drag.y);
  });
  const endDrag = () => { drag = null; pagesEl.classList.remove('dragging'); };
  pagesEl.addEventListener('pointerup', endDrag);
  pagesEl.addEventListener('pointercancel', endDrag);

  // drop a file
  pagesEl.addEventListener('dragover', (e) => { e.preventDefault(); pagesEl.classList.add('drag'); });
  pagesEl.addEventListener('dragleave', () => pagesEl.classList.remove('drag'));
  pagesEl.addEventListener('drop', async (e) => {
    e.preventDefault();
    pagesEl.classList.remove('drag');
    const f = e.dataTransfer.files[0];
    if (f) openBytes(await f.arrayBuffer(), f.name);
  });

  document.addEventListener('keydown', (e) => {
    const mod = e.ctrlKey || e.metaKey;
    if (mod && e.key.toLowerCase() === 'f') { e.preventDefault(); openSearch(); return; }
    if (e.key === 'F3') { e.preventDefault(); stepHit(e.shiftKey ? -1 : 1); return; }
    if (mod && (e.key === '+' || e.key === '=')) { e.preventDefault(); zoomStep(1); return; }
    if (mod && e.key === '-') { e.preventDefault(); zoomStep(-1); return; }
    if (mod && e.key === '0') { e.preventDefault(); setZoom('width'); return; }
    const t = e.target;
    if (t && (t.tagName === 'INPUT' || !$('pv-dialog').hidden || !pop.hidden)) return;
    if (e.key === 'PageDown' || (e.key === 'ArrowRight' && pagesEl.scrollWidth <= pagesEl.clientWidth)) { e.preventDefault(); goto(S.cur + 1); }
    else if (e.key === 'PageUp' || (e.key === 'ArrowLeft' && pagesEl.scrollWidth <= pagesEl.clientWidth)) { e.preventDefault(); goto(S.cur - 1); }
    else if (e.key === 'Home' && !mod) { e.preventDefault(); goto(0); }
    else if (e.key === 'End' && !mod) { e.preventDefault(); goto(S.info ? S.info.pages - 1 : 0); }
  });

  // a host page hands the file over
  window.addEventListener('message', (e) => {
    const m = e.data;
    if (m && m.type === 'rangerpdf:open' && m.data) openBytes(m.data, m.name);
  });

  ready.then(() => {
    if (parent !== window) parent.postMessage({ type: 'rangerpdf:ready' }, '*');
  }, (e) => message('The viewer could not start: ' + e.message, true));
  const file = params.get('file');
  if (file) openUrl(file, params.get('name'));
})();
