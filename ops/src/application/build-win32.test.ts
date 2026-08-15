import { test } from 'node:test';
import assert from 'node:assert/strict';
import type { FsPort, Reporter } from '../domain/ports.ts';
import { resolveWorkspace } from '../domain/workspace.ts';
import { runBuildWin32 } from './build-win32.ts';

class TestReporter implements Reporter {
  readonly messages: string[] = [];
  section(message: string): void { this.messages.push(message); }
  ok(message: string): void { this.messages.push(message); }
  fail(message: string): void { this.messages.push(message); }
  info(message: string): void { this.messages.push(message); }
  warn(message: string): void { this.messages.push(message); }
  table(): void {}
}

test('Win32 壳只复制原生二进制，应用内容保持在远程 bundle', async () => {
  const copied: string[] = [];
  const fs: FsPort = {
    exists: async () => false,
    ensureDir: async () => undefined,
    resetDir: async () => undefined,
    copyFile: async (from, to) => { copied.push(`${from}->${to}`); },
    rename: async () => undefined,
    statSize: async () => 1024,
    touch: async () => undefined,
  };
  const cargo = {
    buildWin32: async () => ({ id: 'build' as const, passed: true, durationMs: 1 }),
  };
  const result = await runBuildWin32({
    cargo: cargo as never,
    fs,
    reporter: new TestReporter(),
    workspace: resolveWorkspace('/repo'),
  }, 'dev');

  assert.deepEqual(result, { ok: true });
  assert.deepEqual(copied, [
    '/repo/target/x86_64-pc-windows-gnu/debug/tela-win32-sdk.exe->/repo/dist/win32/tela-win32-sdk.exe',
  ]);
});
