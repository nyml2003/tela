import { test } from 'node:test';
import assert from 'node:assert/strict';
import type { FsPort, ProcessPort, ProcessResult } from '../domain/ports.ts';
import { WindowsAdbPort } from './windows-adb.ts';

const WINDOWS_CMD = '/mnt/c/Windows/System32/cmd.exe';

class TestProcess implements ProcessPort {
  readonly calls: string[] = [];
  results = new Map<string, ProcessResult>();

  async run(command: string, args: string[]): Promise<ProcessResult> {
    const key = `${command}:${args.join(' ')}`;
    this.calls.push(key);
    return this.results.get(key) ?? { code: 0, stdout: '', stderr: '' };
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

test('uses configured Windows ADB path and parses device/ABI output', async () => {
  const process = new TestProcess();
  const adbPath = '/mnt/c/Users/nyml/AppData/Local/Android/Sdk/platform-tools/adb.exe';
  const localAppData = 'C:\\Users\\nyml\\AppData\\Local';
  const stagedWindowsApk = `${localAppData}\\Temp\\tela-android\\tela-mobile-debug.apk`;
  const stagedWslApk = '/mnt/c/Users/nyml/AppData/Local/Temp/tela-android/tela-mobile-debug.apk';
  process.results.set(`${adbPath}:devices -l`, {
    code: 0,
    stdout: 'List of devices attached\nphone device product:demo model:Demo\nlocked unauthorized\n',
    stderr: '',
  });
  process.results.set(`${adbPath}:-s phone shell getprop ro.product.cpu.abilist`, {
    code: 0,
    stdout: 'arm64-v8a,armeabi-v7a\n',
    stderr: '',
  });
  process.results.set(`${WINDOWS_CMD}:/D /S /C echo %LOCALAPPDATA%`, {
    code: 0,
    stdout: `${localAppData}\r\n`,
    stderr: '',
  });
  process.results.set(`wslpath:-u ${stagedWindowsApk}`, { code: 0, stdout: `${stagedWslApk}\n`, stderr: '' });
  const adb = new WindowsAdbPort(process, filesystem(), {
    environment: { TELA_WINDOWS_ADB: adbPath },
  });

  assert.deepEqual(await adb.listDevices(), [
    { serial: 'phone', state: 'device' },
    { serial: 'locked', state: 'unauthorized' },
  ]);
  assert.deepEqual(await adb.abiList('phone'), ['arm64-v8a', 'armeabi-v7a']);
  await adb.reverse('phone', 8000, 8000);
  await adb.install('phone', '/repo/dist/android/tela-mobile-debug.apk');
  await adb.launch('phone', 'dev.tela.mobile.dev/dev.tela.mobile.TelaActivity');

  assert.deepEqual(process.calls, [
    `${adbPath}:devices -l`,
    `${adbPath}:-s phone shell getprop ro.product.cpu.abilist`,
    `${adbPath}:-s phone reverse tcp:8000 tcp:8000`,
    `${WINDOWS_CMD}:/D /S /C echo %LOCALAPPDATA%`,
    `wslpath:-u ${stagedWindowsApk}`,
    `${adbPath}:-s phone install -r ${stagedWindowsApk}`,
    `${adbPath}:-s phone shell am start -S -n dev.tela.mobile.dev/dev.tela.mobile.TelaActivity`,
  ]);
});

test('discovers the Android Studio SDK default path through Windows LOCALAPPDATA', async () => {
  const process = new TestProcess();
  const windowsPath = 'C:\\Users\\nyml\\AppData\\Local\\Android\\Sdk\\platform-tools\\adb.exe';
  const adbPath = '/mnt/c/Users/nyml/AppData/Local/Android/Sdk/platform-tools/adb.exe';
  process.results.set(`${WINDOWS_CMD}:/D /S /C echo %LOCALAPPDATA%`, {
    code: 0,
    stdout: 'C:\\Users\\nyml\\AppData\\Local\r\n',
    stderr: '',
  });
  process.results.set(`wslpath:-u ${windowsPath}`, { code: 0, stdout: `${adbPath}\n`, stderr: '' });
  process.results.set(`${adbPath}:devices -l`, { code: 0, stdout: 'List of devices attached\n', stderr: '' });
  const adb = new WindowsAdbPort(process, filesystem());

  assert.deepEqual(await adb.listDevices(), []);
  assert.deepEqual(process.calls, [
    `${WINDOWS_CMD}:/D /S /C echo %LOCALAPPDATA%`,
    `wslpath:-u ${windowsPath}`,
    `${adbPath}:devices -l`,
  ]);
});

test('surfaces a failed Windows ADB command with its diagnostic', async () => {
  const process = new TestProcess();
  const adbPath = '/mnt/c/sdk/adb.exe';
  process.results.set(`${adbPath}:devices -l`, { code: 1, stdout: '', stderr: 'no permissions' });
  const adb = new WindowsAdbPort(process, filesystem(), { environment: { TELA_WINDOWS_ADB: adbPath } });

  await assert.rejects(() => adb.listDevices(), /no permissions/);
});
