import { test } from 'node:test';
import assert from 'node:assert/strict';
import type { FsPort, Reporter, ServerPort } from '../domain/ports.ts';
import { resolveWorkspace } from '../domain/workspace.ts';
import { runAndroidServe } from './serve-android.ts';

class TestReporter implements Reporter {
  readonly messages: string[] = [];
  section(message: string): void { this.messages.push(message); }
  ok(message: string): void { this.messages.push(message); }
  fail(message: string): void { this.messages.push(message); }
  info(message: string): void { this.messages.push(message); }
  warn(message: string): void { this.messages.push(message); }
  table(): void {}
}

function filesystem(paths: readonly string[]): FsPort {
  return {
    exists: async (path) => paths.includes(path),
    ensureDir: async () => undefined,
    resetDir: async () => undefined,
    copyFile: async () => undefined,
    setMode: async () => undefined,
    rename: async () => undefined,
    statSize: async () => null,
    touch: async () => undefined,
  };
}

test('Android serve requires the verified mobile index and archive', async () => {
  const reporter = new TestReporter();
  let called = false;
  const server: ServerPort = {
    serve: async () => { throw new Error('must not serve'); },
    serveExact: async () => { called = true; throw new Error('must not serve'); },
  };

  const result = await runAndroidServe({
    fs: filesystem([]),
    server,
    reporter,
    workspace: resolveWorkspace('/repo'),
  });

  assert.equal(result, undefined);
  assert.equal(called, false);
  assert.ok(reporter.messages.some((message) => message.includes('缺少 Android mobile bundle')));
});

test('Android serve uses exact localhost:8000 without generic port fallback', async () => {
  const workspace = resolveWorkspace('/repo');
  const calls: unknown[][] = [];
  const server: ServerPort = {
    serve: async () => { throw new Error('must not use generic serve'); },
    serveExact: async (...args) => {
      calls.push(args);
      return { port: 8000, close: async () => undefined };
    },
  };

  const result = await runAndroidServe({
    fs: filesystem([workspace.bundle('mobile').indexPath(), workspace.bundle('mobile').archivePath()]),
    server,
    reporter: new TestReporter(),
    workspace,
  });

  assert.equal(result?.port, 8000);
  assert.equal(calls.length, 1);
  const [root, host, port, log] = calls[0]!;
  assert.equal(root, workspace.distDir);
  assert.equal(host, '127.0.0.1');
  assert.equal(port, 8000);
  assert.equal(typeof log, 'function');
});
