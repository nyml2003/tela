// 基础设施层：静态服务器适配器（Node 内置 http，移植自 scripts/serve-demo.mjs）。
// 端口占用自动递增（EADDRINUSE → 下一个端口，最多 MAX_PORT_ATTEMPTS 次）。
// WebGPU 环境验证端点：POST /api/telemetry 接收 rawgpu.html 的离屏测试结果。
import { createServer, type Server } from 'node:http';
import { readFile, stat } from 'node:fs/promises';
import { extname, join, normalize } from 'node:path';
import type { ServeResult, ServerPort } from '../domain/ports.ts';
import type { TelemetryStore } from './telemetry-store.ts';

const MIME: Record<string, string> = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.mjs': 'text/javascript; charset=utf-8',
  '.cjs': 'text/javascript; charset=utf-8',
  '.wasm': 'application/wasm',
  '.tela': 'application/vnd.tela.bundle+zip',
  '.json': 'application/json; charset=utf-8',
  '.png': 'image/png',
  '.jpg': 'image/jpeg',
  '.svg': 'image/svg+xml',
  '.css': 'text/css; charset=utf-8',
  '.txt': 'text/plain; charset=utf-8',
};

/** EADDRINUSE 重试上限（端口被占满时的兜底，避免无限循环）。 */
export const MAX_PORT_ATTEMPTS = 20;

export class HttpServerPort implements ServerPort {
  async serve(
    root: string,
    preferredPort: number,
    log: (msg: string) => void,
    telemetry?: TelemetryStore,
  ): Promise<ServeResult> {
    const server: Server = createServer(async (req, res) => {
      try {
        const urlPath = decodeURIComponent(new URL(req.url ?? '/', 'http://x').pathname);

        // ---- WebGPU 环境验证端点 ----
        if (urlPath === '/api/telemetry' && req.method === 'POST' && telemetry) {
          let body = '';
          req.on('data', (d: Buffer) => {
            body += d;
          });
          req.on('end', () => {
            try {
              const events = JSON.parse(body);
              const list = Array.isArray(events) ? events : [events];
              const consumed = telemetry.push(list);
              res.writeHead(200, { 'Content-Type': 'application/json' });
              res.end(JSON.stringify({ ok: true, consumed, count: list.length }));
              log(`200 POST /api/telemetry（${list.length} 事件${consumed ? '' : '，无 CLI 订阅'}）`);
            } catch (err) {
              res.writeHead(400, { 'Content-Type': 'application/json' });
              res.end(JSON.stringify({ ok: false, error: String(err) }));
              log(`400 POST /api/telemetry ${String(err)}`);
            }
          });
          return;
        }
        if (urlPath === '/api/events' && telemetry) {
          telemetry.registerSse(res);
          log('200 GET /api/events（验证客户端已连接）');
          return;
        }

        // ---- 静态文件 ----
        // 防目录穿越：normalize 后必须仍在 root 内。
        const file = normalize(join(root, urlPath));
        if (!file.startsWith(root)) {
          res.writeHead(403).end('Forbidden');
          return;
        }
        const isDir = urlPath.endsWith('/') || (await this.isDir(file));
        const target = isDir ? join(file, 'index.html') : file;
        const data = await readFile(target);
        const type = MIME[extname(target)] ?? 'application/octet-stream';
        // 开发服务器禁缓存，避免页面或 wasm/glue 静默使用旧版本。
        const headers: Record<string, string> = {
          'Content-Type': type,
          'Cache-Control': 'no-store',
        };
        // 浏览器页面、Win32 和 macOS 开发壳可以从不同 origin 请求同一开发 bundle。
        // 只给 bundle 路径开放读取权限，普通静态页面和调试 API 不被一并暴露。
        if (isBundlePath(urlPath)) {
          headers['Access-Control-Allow-Origin'] = '*';
        }
        res.writeHead(200, headers);
        res.end(data);
        log(`200 ${req.method} ${urlPath}`);
      } catch (err) {
        if ((err as NodeJS.ErrnoException).code === 'ENOENT') {
          const headers = urlPathForCors(req.url) ? { 'Access-Control-Allow-Origin': '*' } : {};
          res.writeHead(404, headers).end('Not Found');
          log(`404 ${req.method} ${req.url ?? '/'}`);
        } else {
          res.writeHead(500).end('Internal Error');
          log(`500 ${req.method} ${req.url ?? '/'} ${String(err)}`);
        }
      }
    });

    // 端口占用自动递增：EADDRINUSE → +1 重试（listen 失败后可再次 listen）。
    let port = preferredPort;
    let error: unknown;
    for (let attempt = 0; attempt < MAX_PORT_ATTEMPTS; attempt++) {
      try {
        await new Promise<void>((resolve, reject) => {
          server.once('error', reject);
          server.listen(port, '0.0.0.0', () => resolve());
        });
        if (port !== preferredPort) {
          log(`端口 ${preferredPort} 被占用，自动使用 ${port}`);
        }
        log(`tela WebView: http://127.0.0.1:${port}/`);
        log(`  root: ${root}`);
        return {
          port,
          close: () =>
            new Promise<void>((resolve, reject) => {
              server.close((err) => (err ? reject(err) : resolve()));
            }),
        };
      } catch (err) {
        error = err;
        if ((err as NodeJS.ErrnoException).code === 'EADDRINUSE' && port < 65535) {
          port += 1;
          continue;
        }
        throw err;
      }
    }
    throw new Error(`端口 ${preferredPort}~${port} 均被占用（最后错误: ${String(error)}）`);
  }

  private async isDir(p: string): Promise<boolean> {
    try {
      const s = await stat(p);
      return s.isDirectory();
    } catch {
      return false;
    }
  }
}

function urlPathForCors(requestUrl: string | undefined): boolean {
  try {
    return isBundlePath(new URL(requestUrl ?? '/', 'http://x').pathname);
  } catch {
    return false;
  }
}

function isBundlePath(pathname: string): boolean {
  return pathname.startsWith('/tela-dev/') || pathname.startsWith('/tela-mobile/');
}
