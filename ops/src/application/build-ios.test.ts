import { test } from 'node:test';
import assert from 'node:assert/strict';
import type { FsPort, Reporter } from '../domain/ports.ts';
import { resolveWorkspace } from '../domain/workspace.ts';
import { runBuildIos } from './build-ios.ts';

class TestReporter implements Reporter {
  readonly messages: string[] = [];
  section(message: string): void { this.messages.push(message); }
  ok(message: string): void { this.messages.push(message); }
  fail(message: string): void { this.messages.push(message); }
  info(message: string): void { this.messages.push(message); }
  warn(message: string): void { this.messages.push(message); }
  table(): void {}
}

test('iOS 构建静态链接 mobile app 并以无签名 Xcode 设备目标检查', async () => {
  const calls: string[] = [];
  const fs: FsPort = {
    exists: async (path) => path.endsWith('TelaMobile.app'),
    ensureDir: async (path) => { calls.push(`dir:${path}`); },
    resetDir: async () => undefined,
    copyFile: async (from, to) => { calls.push(`copy:${from}->${to}`); },
    setMode: async () => undefined,
    rename: async () => undefined,
    statSize: async () => null,
    touch: async () => undefined,
  };
  const result = await runBuildIos({
    cargo: { buildIos: async () => ({ id: 'build' as const, passed: true, durationMs: 1 }) } as never,
    process: {
      run: async (command, args, options) => {
        calls.push(`${command}:${args.join(' ')}@${options?.cwd ?? ''}`);
        return { code: 0, stdout: '', stderr: '' };
      },
    },
    fs,
    reporter: new TestReporter(),
    workspace: resolveWorkspace('/repo'),
  }, 'release');

  assert.deepEqual(result, { ok: true });
  assert.ok(calls.includes('dir:/repo/ios/build/rust'));
  assert.ok(calls.includes('copy:/repo/target/aarch64-apple-ios/release/libtela_ios_sdk.a->/repo/ios/build/rust/libtela_ios_sdk.a'));
  assert.ok(calls.some((call) => call === 'tela-ios-xcodebuild:-project /repo/ios/TelaMobile.xcodeproj -target TelaMobile -configuration Release -sdk iphoneos -derivedDataPath /repo/ios/build/DerivedData CODE_SIGNING_ALLOWED=NO build@/repo/ios'));
});

test('Rust 静态库失败时不会调用 Xcode', async () => {
  const calls: string[] = [];
  const result = await runBuildIos({
    cargo: { buildIos: async () => ({ id: 'build' as const, passed: false, durationMs: 1, detail: 'missing target' }) } as never,
    process: { run: async () => { calls.push('xcode'); return { code: 0, stdout: '', stderr: '' }; } },
    fs: {
      exists: async () => false,
      ensureDir: async () => undefined,
      resetDir: async () => undefined,
      copyFile: async () => undefined,
      setMode: async () => undefined,
      rename: async () => undefined,
      statSize: async () => null,
      touch: async () => undefined,
    },
    reporter: new TestReporter(),
    workspace: resolveWorkspace('/repo'),
  }, 'dev');

  assert.deepEqual(result, { ok: false });
  assert.deepEqual(calls, []);
});
