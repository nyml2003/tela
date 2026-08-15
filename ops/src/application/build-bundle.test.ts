import { test } from 'node:test';
import assert from 'node:assert/strict';
import type { FsPort, ProcessPort, Reporter } from '../domain/ports.ts';
import { resolveWorkspace } from '../domain/workspace.ts';
import { runBuildBundle } from './build-bundle.ts';

class TestReporter implements Reporter {
  readonly messages: string[] = [];
  section(message: string): void { this.messages.push(message); }
  ok(message: string): void { this.messages.push(message); }
  fail(message: string): void { this.messages.push(message); }
  info(message: string): void { this.messages.push(message); }
  warn(message: string): void { this.messages.push(message); }
  table(): void {}
}

test('bundle 在索引发布前先原子替换 archive', async () => {
  const calls: string[] = [];
  const wasmBuilds: unknown[][] = [];
  const fs: FsPort = {
    exists: async () => false,
    ensureDir: async (path) => { calls.push(`dir:${path}`); },
    resetDir: async () => undefined,
    copyFile: async () => undefined,
    setMode: async () => undefined,
    rename: async (from, to) => { calls.push(`rename:${from}->${to}`); },
    statSize: async () => 1536,
    touch: async () => undefined,
  };
  const process: ProcessPort = {
    run: async (cmd, args) => {
      calls.push(`${cmd}:${args.join(' ')}`);
      return { code: 0, stdout: '', stderr: '' };
    },
  };
  const cargo = {
    buildWasm: async (...args: unknown[]) => {
      wasmBuilds.push(args);
      return { id: 'build' as const, passed: true, durationMs: 1 };
    },
  };
  const result = await runBuildBundle({
    cargo: cargo as never,
    process,
    fs,
    reporter: new TestReporter(),
    workspace: resolveWorkspace('/repo'),
  });

  assert.deepEqual(result, { ok: true });
  assert.deepEqual(calls.slice(-3), [
    'cargo:run --quiet -p tela-native-sdk-runtime --bin tela-sdk-verify -- /repo/dist/tela-dev/tela-demo.tela.tmp',
    'rename:/repo/dist/tela-dev/tela-demo.tela.tmp->/repo/dist/tela-dev/tela-demo.tela',
    'rename:/repo/dist/tela-dev/latest.json.tmp->/repo/dist/tela-dev/latest.json',
  ]);
  assert.deepEqual(wasmBuilds, [['tela-demo', 'release', ['app-wasm']]]);
});

test('guest 初始化校验失败时不发布临时 archive 或索引', async () => {
  const calls: string[] = [];
  const reporter = new TestReporter();
  const fs: FsPort = {
    exists: async () => false,
    ensureDir: async (path) => { calls.push(`dir:${path}`); },
    resetDir: async () => undefined,
    copyFile: async () => undefined,
    setMode: async () => undefined,
    rename: async (from, to) => { calls.push(`rename:${from}->${to}`); },
    statSize: async () => 1536,
    touch: async () => undefined,
  };
  let invocation = 0;
  const process: ProcessPort = {
    run: async (cmd, args) => {
      invocation += 1;
      calls.push(`${cmd}:${args.join(' ')}`);
      return invocation === 1
        ? { code: 0, stdout: '', stderr: '' }
        : { code: 1, stdout: '', stderr: 'wasm trap: all fuel consumed' };
    },
  };
  const cargo = {
    buildWasm: async () => ({ id: 'build' as const, passed: true, durationMs: 1 }),
  };

  const result = await runBuildBundle({
    cargo: cargo as never,
    process,
    fs,
    reporter,
    workspace: resolveWorkspace('/repo'),
  });

  assert.deepEqual(result, { ok: false });
  assert.equal(calls.some((call) => call.startsWith('rename:')), false);
  assert.equal(reporter.messages.some((message) => message.includes('初始化校验失败')), true);
});
