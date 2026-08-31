import { test } from 'node:test';
import assert from 'node:assert/strict';
import type { FsPort, ProcessPort, Reporter } from '../domain/ports.ts';
import { resolveWorkspace } from '../domain/workspace.ts';
import { runBuildWebviewProbe } from './build-webview-probe.ts';

class TestReporter implements Reporter {
  readonly messages: string[] = [];
  section(message: string): void { this.messages.push(message); }
  ok(message: string): void { this.messages.push(message); }
  fail(message: string): void { this.messages.push(message); }
  info(message: string): void { this.messages.push(message); }
  warn(message: string): void { this.messages.push(message); }
  table(): void {}
}

test('静态 WebView Probe 产品经 wasm-bindgen 发布为单一模块', async () => {
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
  const result = await runBuildWebviewProbe({
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
  assert.deepEqual(builds, [['tela-product-webview-probe', 'release']]);
  assert.deepEqual(calls, [
    'dir:/repo/dist',
    'wasm-bindgen:--target web --out-dir /repo/dist --out-name tela_webview_probe /repo/target/wasm32-unknown-unknown/release/tela_product_webview_probe.wasm',
  ]);
});
