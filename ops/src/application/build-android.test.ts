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
    exists: async () => false,
    ensureDir: async (path) => { calls.push(`dir:${path}`); },
    resetDir: async () => undefined,
    copyFile: async (from, to) => { calls.push(`copy:${from}->${to}`); },
    setMode: async () => undefined,
    rename: async (from, to) => { calls.push(`rename:${from}->${to}`); },
    statSize: async () => 2_097_152,
    touch: async () => undefined,
  };
}

test('Android build publishes only a host APK and configures the strict mobile index URL', async () => {
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
  }, { bundleIndex: 'http://192.168.1.5:8000/tela-mobile/latest.json' });

  assert.deepEqual(result, { ok: true });
  assert.deepEqual(wasmBuilds, [['tela-mobile-demo', 'release', ['app-wasm']]]);
  assert.ok(calls.some((call) => call === 'cargo:ndk -t x86_64 -o /repo/android/app/src/main/jniLibs build --release -p tela-android-sdk@/repo'));
  assert.ok(calls.some((call) => call === 'gradle:--no-daemon :app:assembleDebug -PtelaBundleIndex=http://192.168.1.5:8000/tela-mobile/latest.json@/repo/android'));
  assert.ok(calls.includes('copy:/repo/android/app/build/outputs/apk/debug/app-debug.apk->/repo/dist/android/tela-mobile-debug.apk'));
});

test('Android build rejects a non-network bundle index before emitting artifacts', async () => {
  const calls: string[] = [];
  const reporter = new TestReporter();
  const result = await runBuildAndroid({
    cargo: { buildWasm: async () => { throw new Error('must not build'); } } as never,
    process: { run: async () => { calls.push('process'); return { code: 0, stdout: '', stderr: '' }; } },
    fs: filesystem(calls),
    reporter,
    workspace: resolveWorkspace('/repo'),
  }, { bundleIndex: 'file:///tmp/tela-mobile/latest.json' });

  assert.deepEqual(result, { ok: false });
  assert.deepEqual(calls, []);
  assert.ok(reporter.messages.some((message) => message.includes('--bundle-index')));
});

test('native build failure stops Gradle and does not publish an APK', async () => {
  const calls: string[] = [];
  const result = await runBuildAndroid({
    cargo: { buildWasm: async () => ({ id: 'build' as const, passed: true, durationMs: 1 }) } as never,
    process: {
      run: async (command, args, options) => {
        calls.push(`${command}:${args.join(' ')}@${options?.cwd ?? ''}`);
        const native = command === 'cargo' && args[0] === 'ndk';
        return native
          ? { code: 1, stdout: '', stderr: 'missing Android NDK' }
          : { code: 0, stdout: '', stderr: '' };
      },
    },
    fs: filesystem(calls),
    reporter: new TestReporter(),
    workspace: resolveWorkspace('/repo'),
  }, { bundleIndex: 'https://dev.example.test/tela-mobile/latest.json' });

  assert.deepEqual(result, { ok: false });
  assert.equal(calls.some((call) => call.startsWith('gradle:')), false);
  assert.equal(calls.some((call) => call.startsWith('copy:')), false);
});
