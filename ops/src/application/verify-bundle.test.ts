// application/verify-bundle：对已发布统一应用包复用 native guest 验证器。
import { test } from 'node:test';
import assert from 'node:assert/strict';
import type { FsPort, Reporter } from '../domain/ports.ts';
import { resolveWorkspace } from '../domain/workspace.ts';
import { runVerifyBundle } from './verify-bundle.ts';

class TestReporter implements Reporter {
  readonly messages: string[] = [];
  section(message: string): void { this.messages.push(message); }
  ok(message: string): void { this.messages.push(message); }
  fail(message: string): void { this.messages.push(message); }
  info(message: string): void { this.messages.push(message); }
  warn(message: string): void { this.messages.push(message); }
  table(): void {}
}

function filesystem(exists: boolean): FsPort {
  return {
    exists: async () => exists,
    ensureDir: async () => undefined,
    resetDir: async () => undefined,
    copyFile: async () => undefined,
    setMode: async () => undefined,
    rename: async () => undefined,
    statSize: async () => null,
    touch: async () => undefined,
  };
}

test('对发布 archive 调用 SDK 验证器', async () => {
  const calls: string[] = [];
  const result = await runVerifyBundle({
    fs: filesystem(true),
    process: {
      run: async (command, args) => {
        calls.push(`${command}:${args.join(' ')}`);
        return { code: 0, stdout: 'verified', stderr: '' };
      },
    },
    reporter: new TestReporter(),
    workspace: resolveWorkspace('/repo'),
  });
  assert.deepEqual(result, { ok: true });
  assert.deepEqual(calls, [
    'cargo:run --quiet -p tela-guest-runtime --bin tela-guest-verify -- /repo/dist/tela-dev/tela-desktop-guest.tela',
  ]);
});

test('mobile archive uses the same neutral verifier without selecting the desktop path', async () => {
  const calls: string[] = [];
  const result = await runVerifyBundle({
    fs: filesystem(true),
    process: {
      run: async (command, args) => {
        calls.push(`${command}:${args.join(' ')}`);
        return { code: 0, stdout: 'verified', stderr: '' };
      },
    },
    reporter: new TestReporter(),
    workspace: resolveWorkspace('/repo'),
  }, 'mobile');
  assert.equal(result.ok, true);
  assert.deepEqual(calls, [
    'cargo:run --quiet -p tela-guest-runtime --bin tela-guest-verify -- /repo/dist/tela-mobile/tela-mobile-guest.tela',
  ]);
});

test('缺少 bundle 时不启动 cargo', async () => {
  const result = await runVerifyBundle({
    fs: filesystem(false),
    process: { run: async () => { throw new Error('不应运行 cargo'); } },
    reporter: new TestReporter(),
    workspace: resolveWorkspace('/repo'),
  });
  assert.equal(result.ok, false);
  assert.match(result.detail ?? '', /未构建/);
});
