import { test } from 'node:test';
import assert from 'node:assert/strict';
import type { FsPort, Reporter } from '../domain/ports.ts';
import { resolveWorkspace } from '../domain/workspace.ts';
import { runDeployIos } from './deploy-ios.ts';

class TestReporter implements Reporter {
  readonly messages: string[] = [];
  section(message: string): void { this.messages.push(message); }
  ok(message: string): void { this.messages.push(message); }
  fail(message: string): void { this.messages.push(message); }
  info(message: string): void { this.messages.push(message); }
  warn(message: string): void { this.messages.push(message); }
  table(): void {}
}

const filesystem = (): FsPort => ({
  exists: async () => true,
  ensureDir: async () => undefined,
  resetDir: async () => undefined,
  copyFile: async () => undefined,
  setMode: async () => undefined,
  rename: async () => undefined,
  statSize: async () => null,
  touch: async () => undefined,
});

test('iOS 部署使用 Xcode 签名后通过 devicectl 安装和启动', async () => {
  const calls: string[] = [];
  const ok = await runDeployIos({
    process: {
      run: async (command, args, options) => {
        calls.push(`${command}:${args.join(' ')}@${options?.cwd ?? ''}`);
        return { code: 0, stdout: '', stderr: '' };
      },
    },
    fs: filesystem(),
    reporter: new TestReporter(),
    workspace: resolveWorkspace('/repo'),
  }, { deviceId: '00008110-TEST' });

  assert.equal(ok, true);
  assert.equal(calls.length, 3);
  assert.ok(calls[0]!.includes('-allowProvisioningUpdates build'));
  assert.equal(calls[1], 'tela-ios-xcrun:devicectl device install app --device 00008110-TEST /repo/products/ios/build/DerivedData/Build/Products/Debug-iphoneos/TelaMobile.app@/repo/products/ios');
  assert.equal(calls[2], 'tela-ios-xcrun:devicectl device process launch --device 00008110-TEST dev.tela.mobile@/repo/products/ios');
});

test('iOS 部署拒绝空 UDID，且不运行外部命令', async () => {
  const calls: string[] = [];
  const ok = await runDeployIos({
    process: { run: async () => { calls.push('run'); return { code: 0, stdout: '', stderr: '' }; } },
    fs: filesystem(),
    reporter: new TestReporter(),
    workspace: resolveWorkspace('/repo'),
  }, { deviceId: ' ' });

  assert.equal(ok, false);
  assert.deepEqual(calls, []);
});
