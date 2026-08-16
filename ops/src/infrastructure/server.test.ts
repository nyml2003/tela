// infrastructure/server 集成测试：端口占用自动递增（EADDRINUSE 重试）。
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { mkdir, mkdtemp, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import type { ServeResult } from '../domain/ports.ts';
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
  // 占位服务器先 listen(0) 再取端口：消除 freePort 释放端口到重新占用之间的竞态窗口，
  // base 保证被 blocker 占住，serve 必然触发 EADDRINUSE 跳转。blocker 必须与 serve 使用
  // 相同的监听地址（0.0.0.0）：macOS/BSD 允许先绑 127.0.0.1 再绑 0.0.0.0 同端口不冲突，
  // 而 Linux 会报 EADDRINUSE——用 0.0.0.0 两侧一致，跨平台行为稳定。
  const blocker = createServer();
  await new Promise<void>((resolve) => blocker.listen(0, '0.0.0.0', () => resolve()));
  const base = (blocker.address() as { port: number }).port;

  const root = await mkdtemp(join(tmpdir(), 'ops-serve-'));
  await writeFile(join(root, 'index.html'), '<h1>hi</h1>');
  const logs: string[] = [];
  const serverPort = new HttpServerPort();
  let result: ServeResult | undefined;
  try {
    result = await serverPort.serve(root, base, (msg) => logs.push(msg));
    if (result === undefined) {
      throw new Error('serve 应返回结果');
    }
    const servedPort = result.port;
    assert.ok(servedPort > base, '应跳过被占端口');
    assert.ok(logs.some((l) => l.includes(`端口 ${base} 被占用`)), '应提示占用跳转');
    assert.ok(logs.some((l) => l.includes(`:${servedPort}/`)), '应报告实际端口');
    // 实际可访问。
    const res = await fetch(`http://127.0.0.1:${servedPort}/`);
    assert.equal(res.status, 200);
    assert.match(await res.text(), /<h1>hi<\/h1>/);
  } finally {
    // 断言失败时也必须关掉 serve 的服务器，否则监听泄漏会让测试进程挂起。
    await result?.close();
    await new Promise<void>((resolve) => blocker.close(() => resolve()));
    await rm(root, { recursive: true, force: true });
  }
});

test('首选端口空闲时直接使用（不跳转）', async () => {
  const base = await freePort();
  const root = await mkdtemp(join(tmpdir(), 'ops-serve-'));
  await writeFile(join(root, 'index.html'), 'ok');
  const serverPort = new HttpServerPort();
  let result: ServeResult | undefined;
  try {
    result = await serverPort.serve(root, base, () => {});
    assert.equal(result.port, base);
  } finally {
    await result?.close();
    await rm(root, { recursive: true, force: true });
  }
});

test('精确监听固定回环端口，端口冲突时不递增', async () => {
  const base = await freePort();
  const root = await mkdtemp(join(tmpdir(), 'ops-serve-exact-'));
  await writeFile(join(root, 'index.html'), 'ok');
  const serverPort = new HttpServerPort();
  let result: ServeResult | undefined;
  try {
    result = await serverPort.serveExact(root, '127.0.0.1', base, () => {});
    assert.equal(result.port, base);
    const res = await fetch(`http://127.0.0.1:${base}/`);
    assert.equal(res.status, 200);
  } finally {
    await result?.close();
  }

  const blocker = createServer();
  await new Promise<void>((resolve) => blocker.listen(base, '127.0.0.1', () => resolve()));
  try {
    await assert.rejects(
      () => serverPort.serveExact(root, '127.0.0.1', base, () => {}),
      /无法监听 127\.0\.0\.1:/,
    );
  } finally {
    await new Promise<void>((resolve) => blocker.close(() => resolve()));
    await rm(root, { recursive: true, force: true });
  }
});

test('desktop 与 mobile 开发 bundle 路径开放跨 origin 读取', async () => {
  const base = await freePort();
  const root = await mkdtemp(join(tmpdir(), 'ops-serve-cors-'));
  await writeFile(join(root, 'index.html'), 'page');
  await mkdir(join(root, 'tela-dev'));
  await mkdir(join(root, 'tela-mobile'));
  await writeFile(join(root, 'tela-dev', 'latest.json'), '{}');
  await writeFile(join(root, 'tela-mobile', 'latest.json'), '{}');
  const serverPort = new HttpServerPort();
  let result: ServeResult | undefined;
  try {
    result = await serverPort.serve(root, base, () => {});
    const bundle = await fetch(`http://127.0.0.1:${result.port}/tela-dev/latest.json`);
    assert.equal(bundle.headers.get('access-control-allow-origin'), '*');
    const mobileBundle = await fetch(`http://127.0.0.1:${result.port}/tela-mobile/latest.json`);
    assert.equal(mobileBundle.headers.get('access-control-allow-origin'), '*');
    const page = await fetch(`http://127.0.0.1:${result.port}/`);
    assert.equal(page.headers.get('access-control-allow-origin'), null);
  } finally {
    await result?.close();
    await rm(root, { recursive: true, force: true });
  }
});
