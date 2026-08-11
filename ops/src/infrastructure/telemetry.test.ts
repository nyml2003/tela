// 遥测链路集成测试：TelemetryStore + HttpServerPort 的 /api 端点。
// 模拟浏览器：POST 上报 → 断言 store 收到；SSE 连接 → 断言命令下发。
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { TelemetryStore } from './telemetry-store.ts';
import { HttpServerPort } from './server.ts';
import { probeVerdict, probeVerdictText } from '../domain/telemetry.ts';

async function freePort(): Promise<number> {
  const { createServer } = await import('node:http');
  const srv = createServer();
  await new Promise<void>((resolve) => srv.listen(0, '127.0.0.1', () => resolve()));
  const port = (srv.address() as { port: number }).port;
  await new Promise<void>((resolve) => srv.close(() => resolve()));
  return port;
}

test('POST /api/telemetry 上报事件进入 store', async () => {
  const port = await freePort();
  const root = await mkdtemp(join(tmpdir(), 'ops-tel-'));
  await writeFile(join(root, 'index.html'), 'ok');
  const store = new TelemetryStore();
  const server = new HttpServerPort();
  const logs: string[] = [];
  const result = await server.serve(root, port, (m) => logs.push(m), store);
  try {
    const res = await fetch(`http://127.0.0.1:${result.port}/api/telemetry`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify([
        { type: 'console-error', ts: 1, message: 'boom' },
        { type: 'backend', ts: 2, backend: 'WebGPU' },
      ]),
    });
    assert.equal(res.status, 200);
    const body = (await res.json()) as { count: number };
    assert.equal(body.count, 2);
    assert.equal(store.snapshot().length, 2);
    assert.equal(store.snapshot()[0]!.type, 'console-error');
  } finally {
    await result.close();
    store.dispose();
    await rm(root, { recursive: true, force: true });
  }
});

test('SSE 连接后 broadcast 命令可达（模拟 CLI→浏览器）', async () => {
  const port = await freePort();
  const root = await mkdtemp(join(tmpdir(), 'ops-tel-'));
  await writeFile(join(root, 'index.html'), 'ok');
  const store = new TelemetryStore();
  const server = new HttpServerPort();
  const result = await server.serve(root, port, () => {}, store);
  try {
    // 模拟浏览器 EventSource：http GET /api/events 保持连接。
    const ac = new AbortController();
    const ssePromise = (async () => {
      const res = await fetch(`http://127.0.0.1:${result.port}/api/events`, {
        signal: ac.signal,
      });
      const reader = res.body!.getReader();
      const decoder = new TextDecoder();
      let buf = '';
      while (true) {
        const { done, value } = await reader.read();
        if (done) break;
        buf += decoder.decode(value, { stream: true });
        if (buf.includes('set-pressure')) break;
      }
      return buf;
    })();
    // 等 SSE 连接注册。
    await new Promise((r) => setTimeout(r, 200));
    assert.equal(store.connectionCount(), 1, 'SSE 连接应已注册');
    store.broadcast({ type: 'set-pressure', level: 2 });
    const received = await ssePromise;
    assert.match(received, /set-pressure/);
    assert.match(received, /"level":2/);
    ac.abort();
  } finally {
    await result.close();
    store.dispose();
    await rm(root, { recursive: true, force: true });
  }
});

test('探针三态判定', () => {
  assert.equal(probeVerdict([200, 0, 0]), 'ok');
  assert.equal(probeVerdict([0, 200, 0]), 'draw-empty');
  assert.equal(probeVerdict([0, 0, 0]), 'pass-not-run');
  assert.equal(probeVerdict([100, 100, 100]), 'unknown');
  assert.match(probeVerdictText('ok'), /正常/);
});
