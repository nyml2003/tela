// application/build-webview：WGPU 壳与 wasm-bindgen glue 的构建编排。
import { test } from 'node:test';
import assert from 'node:assert/strict';
import type { FsPort, ProcessPort, Reporter } from '../domain/ports.ts';
import { resolveWorkspace } from '../domain/workspace.ts';
import { runBuildWebview } from './build-webview.ts';

class TestReporter implements Reporter {
  readonly messages: string[] = [];
  section(message: string): void { this.messages.push(message); }
  ok(message: string): void { this.messages.push(message); }
  fail(message: string): void { this.messages.push(message); }
  info(message: string): void { this.messages.push(message); }
  warn(message: string): void { this.messages.push(message); }
  table(): void {}
}

test('release WGPU 壳经 wasm-bindgen 发布到静态根', async () => {
  const calls: string[] = [];
  const builds: unknown[][] = [];
  const fs: FsPort = {
    exists: async () => false,
    ensureDir: async (path) => { calls.push(`dir:${path}`); },
    resetDir: async () => undefined,
    copyFile: async () => undefined,
    setMode: async () => undefined,
    rename: async () => undefined,
    statSize: async () => null,
    touch: async () => undefined,
  };
  const process: ProcessPort = {
    run: async (command, args) => {
      calls.push(`${command}:${args.join(' ')}`);
      return { code: 0, stdout: '', stderr: '' };
    },
  };
  const result = await runBuildWebview({
    cargo: {
      buildWasm: async (...args: unknown[]) => {
        builds.push(args);
        return { id: 'build' as const, passed: true, durationMs: 1 };
      },
    } as never,
    process,
    fs,
    reporter: new TestReporter(),
    workspace: resolveWorkspace('/repo'),
  });

  assert.deepEqual(result, { ok: true });
  assert.deepEqual(builds, [['tela-webview-sdk', 'release']]);
  assert.deepEqual(calls, [
    'dir:/repo/dist',
    'wasm-bindgen:--target web --out-dir /repo/dist --out-name tela_webview_sdk /repo/target/wasm32-unknown-unknown/release/tela_webview_sdk.wasm',
  ]);
});
