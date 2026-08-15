// application/verify-demo：发布工件存在性与 wasm 冒烟端口编排。
import { test } from 'node:test';
import assert from 'node:assert/strict';
import type { FsPort, Reporter, WasmSmokePort } from '../domain/ports.ts';
import { resolveWorkspace } from '../domain/workspace.ts';
import { runVerifyDemo } from './verify-demo.ts';

class TestReporter implements Reporter {
  readonly messages: string[] = [];

  section(message: string): void { this.messages.push(message); }
  ok(message: string): void { this.messages.push(message); }
  fail(message: string): void { this.messages.push(message); }
  info(message: string): void { this.messages.push(message); }
  warn(message: string): void { this.messages.push(message); }
  table(headers: readonly string[], rows: readonly (readonly string[])[]): void {
    this.messages.push(`${headers.join(',')}:${rows.length}`);
  }
}

function filesystem(exists: boolean): FsPort {
  return {
    exists: async () => exists,
    ensureDir: async () => undefined,
    resetDir: async () => undefined,
    copyFile: async () => undefined,
    rename: async () => undefined,
    statSize: async () => null,
    touch: async () => undefined,
  };
}

test('已发布 wasm 时委托 smoke 端口并保留 dist 路径', async () => {
  const calls: string[] = [];
  const smoke: WasmSmokePort = {
    verify: async (path) => {
      calls.push(path);
      return { ok: true, detail: '1366x768' };
    },
  };
  const reporter = new TestReporter();
  const result = await runVerifyDemo({
    fs: filesystem(true),
    smoke,
    reporter,
    workspace: resolveWorkspace('/repo'),
  });

  assert.deepEqual(result, { ok: true });
  assert.deepEqual(calls, ['/repo/dist/tela_demo.wasm']);
  assert.ok(reporter.messages.some((message) => message.includes('发布 wasm 冒烟通过')));
});

test('缺少发布 wasm 时不调用 smoke 端口', async () => {
  const reporter = new TestReporter();
  const smoke: WasmSmokePort = {
    verify: async () => {
      throw new Error('不应调用 smoke');
    },
  };
  const result = await runVerifyDemo({
    fs: filesystem(false),
    smoke,
    reporter,
    workspace: resolveWorkspace('/repo'),
  });

  assert.equal(result.ok, false);
  assert.match(result.detail ?? '', /未构建/);
  assert.ok(reporter.messages.some((message) => message.includes('ops build demo')));
});
