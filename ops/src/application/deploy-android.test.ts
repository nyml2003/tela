import { test } from 'node:test';
import assert from 'node:assert/strict';
import type { AndroidDevice, AndroidDevicePort, FsPort, Reporter } from '../domain/ports.ts';
import { resolveWorkspace } from '../domain/workspace.ts';
import { runDeployAndroid } from './deploy-android.ts';

class TestReporter implements Reporter {
  readonly messages: string[] = [];
  readonly tables: (readonly (readonly string[])[])[] = [];
  section(message: string): void { this.messages.push(message); }
  ok(message: string): void { this.messages.push(message); }
  fail(message: string): void { this.messages.push(message); }
  info(message: string): void { this.messages.push(message); }
  warn(message: string): void { this.messages.push(message); }
  table(_headers: readonly string[], rows: readonly (readonly string[])[]): void { this.tables.push(rows); }
}

class TestAdb implements AndroidDevicePort {
  readonly calls: string[] = [];
  devices: readonly AndroidDevice[] = [{ serial: 'phone', state: 'device' }];
  abis: readonly string[] = ['arm64-v8a', 'armeabi-v7a'];
  failAt: string | undefined;

  async listDevices(): Promise<readonly AndroidDevice[]> {
    this.calls.push('devices');
    return this.devices;
  }

  async abiList(serial: string): Promise<readonly string[]> {
    this.calls.push(`abi:${serial}`);
    return this.abis;
  }

  async reverse(serial: string, devicePort: number, hostPort: number): Promise<void> {
    this.calls.push(`reverse:${serial}:${devicePort}:${hostPort}`);
    if (this.failAt === 'reverse') throw new Error('reverse failed');
  }

  async install(serial: string, apkPath: string): Promise<void> {
    this.calls.push(`install:${serial}:${apkPath}`);
    if (this.failAt === 'install') throw new Error('install failed');
  }

  async launch(serial: string, component: string): Promise<void> {
    this.calls.push(`launch:${serial}:${component}`);
    if (this.failAt === 'launch') throw new Error('launch failed');
  }
}

function filesystem(exists = true): FsPort {
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

test('deploy reverses, installs, and launches one authorized ARM64 device in order', async () => {
  const adb = new TestAdb();
  const workspace = resolveWorkspace('/repo');
  const ok = await runDeployAndroid({ adb, fs: filesystem(), reporter: new TestReporter(), workspace });

  assert.equal(ok, true);
  assert.deepEqual(adb.calls, [
    'devices',
    'abi:phone',
    'reverse:phone:8000:8000',
    `install:phone:${workspace.androidDistPath()}`,
    'launch:phone:dev.tela.mobile.dev/dev.tela.mobile.TelaActivity',
  ]);
});

test('deploy requires --serial when more than one device is visible', async () => {
  const adb = new TestAdb();
  adb.devices = [
    { serial: 'phone', state: 'device' },
    { serial: 'tablet', state: 'device' },
  ];
  const reporter = new TestReporter();
  const ok = await runDeployAndroid({ adb, fs: filesystem(), reporter, workspace: resolveWorkspace('/repo') });

  assert.equal(ok, false);
  assert.deepEqual(adb.calls, ['devices']);
  assert.ok(reporter.messages.some((message) => message.includes('--serial')));
});

test('deploy stops before reverse for a non-ARM64 device', async () => {
  const adb = new TestAdb();
  adb.abis = ['x86_64'];
  const ok = await runDeployAndroid({ adb, fs: filesystem(), reporter: new TestReporter(), workspace: resolveWorkspace('/repo') });

  assert.equal(ok, false);
  assert.deepEqual(adb.calls, ['devices', 'abi:phone']);
});

test('deploy stops installation when ADB reverse fails', async () => {
  const adb = new TestAdb();
  adb.failAt = 'reverse';
  const ok = await runDeployAndroid({ adb, fs: filesystem(), reporter: new TestReporter(), workspace: resolveWorkspace('/repo') });

  assert.equal(ok, false);
  assert.deepEqual(adb.calls, ['devices', 'abi:phone', 'reverse:phone:8000:8000']);
});
