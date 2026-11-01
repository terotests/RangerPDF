// Serves viewer/dist (or the folder given) on http://127.0.0.1:8790/ for
// trying the viewer. usage: node viewer/serve.mjs [dir] [--port=N]
import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const args = process.argv.slice(2);
const port = +(args.find((a) => a.startsWith('--port=')) || '--port=8790').slice(7);
const dir = path.resolve(args.find((a) => !a.startsWith('--')) || path.join(path.dirname(fileURLToPath(import.meta.url)), 'dist'));
const TYPES = { '.html': 'text/html; charset=utf-8', '.js': 'text/javascript', '.css': 'text/css', '.wasm': 'application/wasm', '.pdf': 'application/pdf', '.ttf': 'font/ttf' };

export function serve(root, p) {
  const server = http.createServer((req, res) => {
    const u = decodeURIComponent(new URL(req.url, 'http://x').pathname);
    let f = path.join(root, path.normalize(u).replace(/^(\.\.[/\\])+/, ''));
    if (!f.startsWith(root)) { res.writeHead(403); res.end(); return; }
    if (fs.existsSync(f) && fs.statSync(f).isDirectory()) f = path.join(f, 'index.html');
    fs.readFile(f, (err, data) => {
      if (err) { res.writeHead(404); res.end('not found'); return; }
      res.writeHead(200, { 'content-type': TYPES[path.extname(f)] || 'application/octet-stream' });
      res.end(data);
    });
  });
  return new Promise((ok) => server.listen(p, '127.0.0.1', () => ok(server)));
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  await serve(dir, port);
  console.log(`viewer: http://127.0.0.1:${port}/  (${dir})`);
}
