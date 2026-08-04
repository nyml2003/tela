#!/usr/bin/env node
// tela demo 静态服务器（零依赖，仅 Node 内置模块）。
// wasm 必须按 application/wasm 提供，否则 instantiateStreaming 会失败。
// 用法：node scripts/serve-demo.mjs [port]  （默认 8000）
import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
import { extname, join, normalize } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = join(fileURLToPath(new URL('.', import.meta.url)), '..', 'demo');
const PORT = Number(process.argv[2] ?? process.env.PORT ?? 8000);

const MIME = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.mjs': 'text/javascript; charset=utf-8',
  '.cjs': 'text/javascript; charset=utf-8',
  '.wasm': 'application/wasm',
  '.json': 'application/json; charset=utf-8',
  '.png': 'image/png',
  '.jpg': 'image/jpeg',
  '.svg': 'image/svg+xml',
  '.css': 'text/css; charset=utf-8',
  '.txt': 'text/plain; charset=utf-8',
};

const server = createServer(async (req, res) => {
  try {
    const urlPath = decodeURIComponent(new URL(req.url, 'http://x').pathname);
    let file = normalize(join(ROOT, urlPath));
    if (!file.startsWith(ROOT)) throw Object.assign(new Error('forbidden'), { code: 403 });
    if (req.method !== 'GET' && req.method !== 'HEAD') throw Object.assign(new Error('method'), { code: 405 });

    let content = null;
    try {
      content = await readFile(file);
    } catch {
      if (urlPath === '/' || !extname(urlPath)) file = join(file, 'index.html');
      content = await readFile(file);
    }
    res.writeHead(200, {
      'Content-Type': MIME[extname(file)] ?? 'application/octet-stream',
      'Content-Length': content.length,
      'Cache-Control': 'no-store',
    });
    if (req.method === 'GET') res.end(content);
    else res.end();
    console.log(`${new Date().toISOString()} 200 ${req.method} ${urlPath}`);
  } catch (err) {
    const code = { ENOENT: 404, EACCES: 403, EISDIR: 404 }[err.code] ?? 500;
    res.writeHead(code, { 'Content-Type': 'text/plain; charset=utf-8' });
    res.end(`tela demo server: ${code} ${err.message}`);
    console.log(`${new Date().toISOString()} ${code} ${req.method} ${req.url}`);
  }
});

server.listen(PORT, '127.0.0.1', () => {
  console.log(`tela demo: http://127.0.0.1:${PORT}/ (root: ${ROOT})`);
});
