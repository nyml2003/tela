import { test } from 'node:test';
import assert from 'node:assert/strict';
import type { FsPort, Reporter } from '../domain/ports.ts';
import { resolveWorkspace } from '../domain/workspace.ts';
import { runBuildMacos } from './build-macos.ts';

class TestReporter implements Reporter {
  readonly messages: string[] = [];
  section(message: string): void { this.messages.push(message); }
  ok(message: string): void { this.messages.push(message); }
  fail(message: string): void { this.messages.push(message); }
  info(message: string): void { this.messages.push(message); }
  warn(message: string): void { this.messages.push(message); }
  table(): void {}
}

test('macOS App 只发布本地壳与 Info.plist，不嵌入远程 bundle', async () => {
  const calls: string[] = [];
  const fs: FsPort = {
    exists: async () => false,
    ensureDir: async (path) => { calls.push(`dir:${path}`); },
    resetDir: async (path) => { calls.push(`reset:${path}`); },
    copyFile: async (from, to) => { calls.push(`copy:${from}->${to}`); },
    setMode: async (path, mode) => { calls.push(`mode:${path}:${mode.toString(8)}`); },
    rename: async () => undefined,
    statSize: async () => 2048,
    touch: async () => undefined,
  };
  const cargo = {
    buildMacos: async () => ({ id: 'build' as const, passed: true, durationMs: 1 }),
  };

  const result = await runBuildMacos({
    cargo: cargo as never,
    fs,
    reporter: new TestReporter(),
    workspace: resolveWorkspace('/repo'),
  }, 'release');

  assert.deepEqual(result, { ok: true });
  assert.deepEqual(calls, [
    'reset:/repo/dist/macos/Tela.app',
    'dir:/repo/dist/macos/Tela.app/Contents/MacOS',
    'copy:/repo/crates/tela-macos-sdk/resources/Info.plist->/repo/dist/macos/Tela.app/Contents/Info.plist',
    'copy:/repo/target/aarch64-apple-darwin/release/tela-macos-sdk->/repo/dist/macos/Tela.app/Contents/MacOS/tela-macos-sdk',
    'mode:/repo/dist/macos/Tela.app/Contents/MacOS/tela-macos-sdk:755',
  ]);
});
