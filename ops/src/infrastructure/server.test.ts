// infrastructure/server 集成测试：端口占用自动递增（EADDRINUSE 重试）。
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { mkdtemp, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { HttpServerPort } from './server.ts';

/** 找一个未占用的端口（用于占位服务器，避免撞上 CI 并行端口）。 */
async function freePort(): Promise<number> {
  const srv = createServer();
  await new Promise<void>((resolve) => srv.listen(0, '127.0.0.1', () => resolve()));
  const port = (srv.address() as { port: number }).port;
  await new Promise<void>((resolve) => srv.close(() => resolve()));
  return port;
}

test('首选端口被占用时自动使用下一个端口', async () => {
  const base = await freePort();
  // 占位服务器：占住 base 端口。
  const blocker = createServer();
  await new Promise<void>((resolve) => blocker.listen(base, '127.0.0.1', () => resolve()));

  const root = await mkdtemp(join(tmpdir(), 'ops-serve-'));
  await writeFile(join(root, 'index.html'), '<h1>hi</h1>');
  const logs: string[] = [];
  const serverPort = new HttpServerPort();
  try {
    const result = await serverPort.serve(root, base, (msg) => logs.push(msg));
    assert.equal(result.port, base + 1, '应跳到 base+1');
    assert.ok(logs.some((l) => l.includes(`端口 ${base} 被占用`)), '应提示占用跳转');
    assert.ok(logs.some((l) => l.includes(`:${base + 1}/`)), '应报告实际端口');
    // 实际可访问。
    const res = await fetch(`http://127.0.0.1:${result.port}/`);
    assert.equal(res.status, 200);
    assert.match(await res.text(), /<h1>hi<\/h1>/);
    await result.close();
  } finally {
    await new Promise<void>((resolve) => blocker.close(() => resolve()));
    await rm(root, { recursive: true, force: true });
  }
});

test('首选端口空闲时直接使用（不跳转）', async () => {
  const base = await freePort();
  const root = await mkdtemp(join(tmpdir(), 'ops-serve-'));
  await writeFile(join(root, 'index.html'), 'ok');
  const serverPort = new HttpServerPort();
  try {
    const result = await serverPort.serve(root, base, () => {});
    assert.equal(result.port, base);
    await result.close();
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});
