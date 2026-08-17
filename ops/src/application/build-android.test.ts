import { test } from 'node:test';
import assert from 'node:assert/strict';
import type { FsPort, Reporter } from '../domain/ports.ts';
import { resolveWorkspace } from '../domain/workspace.ts';
import { runBuildAndroid } from './build-android.ts';

class TestReporter implements Reporter {
  readonly messages: string[] = [];
  section(message: string): void { this.messages.push(message); }
  ok(message: string): void { this.messages.push(message); }
  fail(message: string): void { this.messages.push(message); }
  info(message: string): void { this.messages.push(message); }
  warn(message: string): void { this.messages.push(message); }
  table(): void {}
}

function filesystem(calls: string[]): FsPort {
  return {
    exists: async () => true,
    ensureDir: async (path) => { calls.push(`dir:${path}`); },
    resetDir: async (path) => { calls.push(`reset:${path}`); },
    copyFile: async (from, to) => { calls.push(`copy:${from}->${to}`); },
    setMode: async () => undefined,
    rename: async (from, to) => { calls.push(`rename:${from}->${to}`); },
    statSize: async () => 2_097_152,
    touch: async () => undefined,
  };
}

test('Android build publishes an ARM64 host APK and configures the strict localhost mobile index', async () => {
  const calls: string[] = [];
  const wasmBuilds: unknown[][] = [];
  const result = await runBuildAndroid({
    cargo: {
      buildWasm: async (...args: unknown[]) => {
        wasmBuilds.push(args);
        return { id: 'build' as const, passed: true, durationMs: 1 };
      },
    } as never,
    process: {
      run: async (command, args, options) => {
        calls.push(`${command}:${args.join(' ')}@${options?.cwd ?? ''}`);
        return { code: 0, stdout: '', stderr: '' };
      },
    },
    fs: filesystem(calls),
    reporter: new TestReporter(),
    workspace: resolveWorkspace('/repo'),
  });

  assert.deepEqual(result, { ok: true });
  assert.deepEqual(wasmBuilds, [['tela-product-mobile-guest', 'release', []]]);
  assert.ok(calls.includes('reset:/repo/products/android/app/src/main/jniLibs'));
  assert.ok(calls.some((call) => call === 'tela-android-cargo:build --target aarch64-linux-android --release -p tela-target-android@/repo'));
  assert.ok(calls.includes('dir:/repo/products/android/app/src/main/jniLibs/arm64-v8a'));
  assert.ok(calls.includes('copy:/repo/target/aarch64-linux-android/release/libmain.so->/repo/products/android/app/src/main/jniLibs/arm64-v8a/libmain.so'));
  assert.ok(calls.some((call) => call === 'tela-android-gradle:--no-daemon :app:assembleDebug -PtelaBundleIndex=http://127.0.0.1:8000/tela-mobile/latest.json@/repo/products/android'));
  assert.ok(calls.includes('copy:/repo/products/android/app/build/outputs/apk/debug/app-debug.apk->/repo/dist/android/tela-mobile-debug.apk'));
});

test('native build failure stops Gradle and does not publish an APK', async () => {
  const calls: string[] = [];
  const result = await runBuildAndroid({
    cargo: { buildWasm: async () => ({ id: 'build' as const, passed: true, durationMs: 1 }) } as never,
    process: {
      run: async (command, args, options) => {
        calls.push(`${command}:${args.join(' ')}@${options?.cwd ?? ''}`);
        const native = command === 'tela-android-cargo';
        return native
          ? { code: 1, stdout: '', stderr: 'missing Android NDK' }
          : { code: 0, stdout: '', stderr: '' };
      },
    },
    fs: filesystem(calls),
    reporter: new TestReporter(),
    workspace: resolveWorkspace('/repo'),
  });

  assert.deepEqual(result, { ok: false });
  assert.equal(calls.some((call) => call.startsWith('gradle:')), false);
  assert.equal(calls.some((call) => call.startsWith('copy:')), false);
});
