// SPDX-License-Identifier: AGPL-3.0-or-later
//
// A headless Chromium driven over the DevTools protocol with Node's own
// WebSocket: no Playwright, no Puppeteer. Enough for the web UI's checks:
// open a page, press keys, read the accessibility tree the browser built,
// evaluate a script, take a screenshot.
//
//   const b = await launch();            // CHROME=/path/to/chrome to choose
//   const p = await b.page("http://127.0.0.1:8788/");
//   await p.key("Tab"); const ax = await p.axTree(); await b.close();

import { spawn } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

const CANDIDATES = [
  process.env.CHROME,
  "/opt/pw-browsers/chromium-1243/chrome-linux64/chrome",
  "/usr/bin/chromium",
  "/usr/bin/chromium-browser",
  "/usr/bin/google-chrome",
  "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
];

export function findChrome() {
  for (const c of CANDIDATES) if (c && fs.existsSync(c)) return c;
  const pw = "/opt/pw-browsers";
  if (fs.existsSync(pw)) {
    for (const d of fs.readdirSync(pw)) {
      const p = path.join(pw, d, "chrome-linux64", "chrome");
      if (d.startsWith("chromium-") && fs.existsSync(p)) return p;
    }
  }
  return null;
}

const sleep = (ms) => new Promise((ok) => setTimeout(ok, ms));

// keys as the page sees them: key, code, keyCode
const KEYS = {
  Tab: ["Tab", 9], Enter: ["Enter", 13], Escape: ["Escape", 27], " ": ["Space", 32],
  ArrowUp: ["ArrowUp", 38], ArrowDown: ["ArrowDown", 40], ArrowLeft: ["ArrowLeft", 37], ArrowRight: ["ArrowRight", 39],
  Home: ["Home", 36], End: ["End", 35], PageUp: ["PageUp", 33], PageDown: ["PageDown", 34], F6: ["F6", 117], Backspace: ["Backspace", 8],
};

export async function launch({ width = 1366, height = 900, automation = false, args = [] } = {}) {
  const exe = findChrome();
  if (!exe) throw new Error("no Chromium found (set CHROME)");
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "mfd-cdp-"));
  const proc = spawn(exe, ["--headless=new", "--no-sandbox", "--disable-gpu", "--remote-debugging-port=0",
    `--user-data-dir=${dir}`, `--window-size=${width},${height}`, "--force-renderer-accessibility",
    // as chromedriver and Playwright start it: navigator.webdriver is true
    ...(automation ? ["--enable-automation"] : []), ...args, "about:blank"],
  { stdio: ["ignore", "ignore", "pipe"] });
  const wsUrl = await new Promise((ok, fail) => {
    let err = "";
    const t = setTimeout(() => fail(new Error("Chromium did not start: " + err)), 20000);
    proc.stderr.on("data", (d) => {
      err += d;
      const m = /DevTools listening on (ws:\/\/\S+)/.exec(err);
      if (m) { clearTimeout(t); ok(m[1]); }
    });
  });
  const port = new URL(wsUrl).port;
  let target = null;
  for (let i = 0; i < 100 && !target; i++) {
    const list = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
    target = list.find((t) => t.type === "page");
    if (!target) await sleep(100);
  }
  if (!target) throw new Error("Chromium has no page");
  const ws = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((ok, fail) => { ws.onopen = ok; ws.onerror = fail; });
  let seq = 0;
  const waiting = new Map();
  ws.onmessage = (ev) => {
    const m = JSON.parse(ev.data);
    if (m.id && waiting.has(m.id)) {
      const { ok, fail } = waiting.get(m.id);
      waiting.delete(m.id);
      if (m.error) fail(new Error(m.error.message)); else ok(m.result);
    }
  };
  const send = (method, params = {}) => new Promise((ok, fail) => {
    const id = ++seq;
    waiting.set(id, { ok, fail });
    ws.send(JSON.stringify({ id, method, params }));
  });
  await send("Page.enable");
  await send("Runtime.enable");
  await send("Accessibility.enable");
  await send("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: 1, mobile: false });

  const page = {
    send,
    async goto(url) {
      await send("Page.navigate", { url });
      await sleep(500);
    },
    async eval(expr) {
      const r = await send("Runtime.evaluate", { expression: expr, awaitPromise: true, returnByValue: true });
      if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description || r.exceptionDetails.text);
      return r.result.value;
    },
    async waitFor(expr, ms = 15000) {
      const end = Date.now() + ms;
      for (;;) {
        const v = await page.eval(expr).catch(() => false);
        if (v) return v;
        if (Date.now() > end) throw new Error("timed out waiting for " + expr);
        await sleep(100);
      }
    },
    async key(k, { shift = false, ctrl = false } = {}) {
      const [code, keyCode] = KEYS[k] || [k.length === 1 ? "Key" + k.toUpperCase() : k, k.length === 1 ? k.toUpperCase().charCodeAt(0) : 0];
      const modifiers = (shift ? 8 : 0) | (ctrl ? 2 : 0);
      const base = { key: k, code, windowsVirtualKeyCode: keyCode, modifiers };
      const text = k.length === 1 ? k : k === "Enter" ? "\r" : undefined;
      await send("Input.dispatchKeyEvent", { type: text ? "keyDown" : "rawKeyDown", ...base, text });
      await send("Input.dispatchKeyEvent", { type: "keyUp", ...base });
      await sleep(250);
    },
    async type(s) {
      for (const ch of s) await page.key(ch);
    },
    async click(x, y) {
      for (const type of ["mousePressed", "mouseReleased"]) {
        await send("Input.dispatchMouseEvent", { type, x, y, button: "left", clickCount: 1 });
      }
      await sleep(150);
    },
    /** The browser's own accessibility tree, as {role, name, ...} nodes with children. */
    async axTree() {
      const { nodes } = await send("Accessibility.getFullAXTree");
      const byId = new Map(nodes.map((n) => [n.nodeId, n]));
      const conv = (n) => {
        const props = {};
        for (const p of n.properties || []) props[p.name] = p.value?.value;
        return {
          role: n.role?.value, name: n.name?.value || "", ignored: !!n.ignored, props,
          children: (n.childIds || []).map((c) => byId.get(c)).filter(Boolean).map(conv),
        };
      };
      return conv(nodes[0]);
    },
    /** What has the browser's focus: role and accessible name of document.activeElement. */
    async active() {
      return page.eval(`(() => { const e = document.activeElement; return e ? { tag: e.tagName, role: e.getAttribute("role") || e.tagName.toLowerCase(), name: e.getAttribute("aria-label") || e.textContent, id: e.dataset ? e.dataset.a11yId || "" : "" } : null })()`);
    },
    async screenshot(file) {
      const r = await send("Page.captureScreenshot", { format: "png" });
      fs.writeFileSync(file, Buffer.from(r.data, "base64"));
    },
  };
  return {
    page: async (url) => { await page.goto(url); return page; },
    async close() {
      try { ws.close(); } catch { /* gone */ }
      const gone = new Promise((ok) => proc.once("exit", ok));
      proc.kill("SIGKILL");
      await Promise.race([gone, sleep(3000)]);
      // Chromium may still be writing its profile as it dies
      try { fs.rmSync(dir, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 }); } catch { /* left in tmp */ }
    },
  };
}

/** The tree as indented text, ignored nodes left out (their children kept). */
export function axDump(n, depth = 0, out = []) {
  const skip = n.ignored || n.role === "none" || n.role === "generic" && !n.name;
  if (!skip) {
    const st = Object.entries(n.props).filter(([k, v]) => v !== false && v !== undefined && !["readonly"].includes(k) || k === "focused" && v)
      .map(([k, v]) => v === true ? k : `${k}=${v}`).join(" ");
    out.push(`${"  ".repeat(depth)}${n.role}${n.name ? ` "${n.name}"` : ""}${st ? ` [${st}]` : ""}`);
  }
  for (const c of n.children) axDump(c, skip ? depth : depth + 1, out);
  return out;
}
