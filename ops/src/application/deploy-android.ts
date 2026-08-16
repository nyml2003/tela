// 应用层：把 WSL 构建出的 ARM64 APK 通过 Windows ADB 安装到单台真机，并建立固定 reverse 通道。

import {
  ANDROID_DEBUG_COMPONENT,
  ANDROID_DEV_PORT,
  ANDROID_NDK_ABI,
} from '../domain/android.ts';
import type { AndroidDevice, AndroidDevicePort, FsPort, Reporter } from '../domain/ports.ts';
import type { WorkspacePaths } from '../domain/workspace.ts';

export interface DeployAndroidDeps {
  adb: AndroidDevicePort;
  fs: FsPort;
  reporter: Reporter;
  workspace: WorkspacePaths;
}

export interface DeployAndroidOptions {
  serial?: string;
}

export async function runDeployAndroid(
  deps: DeployAndroidDeps,
  options: DeployAndroidOptions = {},
): Promise<boolean> {
  const { adb, fs, reporter, workspace } = deps;
  const apkPath = workspace.androidDistPath();
  reporter.section('部署 Android ARM64 真机');

  if (!(await fs.exists(apkPath))) {
    reporter.fail(`缺少 Android APK: ${apkPath}`);
    reporter.info('请先运行: nix develop .#android --command ops build android');
    return false;
  }

  let devices: readonly AndroidDevice[];
  try {
    devices = await adb.listDevices();
  } catch (error) {
    reporter.fail('无法通过 Windows adb.exe 枚举 Android 设备');
    reporter.info(String(error));
    return false;
  }

  const selected = selectDevice(devices, options.serial, reporter);
  if (!selected) return false;

  let abiList: readonly string[];
  try {
    abiList = await adb.abiList(selected.serial);
  } catch (error) {
    reporter.fail(`无法读取设备 ${selected.serial} 的 ABI`);
    reporter.info(String(error));
    return false;
  }
  if (!abiList.includes(ANDROID_NDK_ABI)) {
    reporter.fail(`设备 ${selected.serial} 不支持 ${ANDROID_NDK_ABI}（报告: ${abiList.join(', ') || '无'}）`);
    return false;
  }

  const steps: readonly [string, () => Promise<void>][] = [
    ['建立 adb reverse', () => adb.reverse(selected.serial, ANDROID_DEV_PORT, ANDROID_DEV_PORT)],
    ['安装 debug APK', () => adb.install(selected.serial, apkPath)],
    ['启动 TelaActivity', () => adb.launch(selected.serial, ANDROID_DEBUG_COMPONENT)],
  ];
  for (const [label, action] of steps) {
    try {
      await action();
    } catch (error) {
      reporter.fail(`${label}失败；后续步骤已停止`);
      reporter.info(String(error));
      return false;
    }
  }

  reporter.ok(`已部署到 ${selected.serial}（${ANDROID_NDK_ABI}）`);
  reporter.info(`设备 localhost:${ANDROID_DEV_PORT} 已反向映射到 Windows/WSL 开发服务。`);
  return true;
}

function selectDevice(
  devices: readonly AndroidDevice[],
  requestedSerial: string | undefined,
  reporter: Reporter,
): AndroidDevice | undefined {
  const serial = requestedSerial?.trim() || undefined;
  if (devices.length === 0) {
    reporter.fail('未发现 Android 设备；请在 Windows Android Studio 中确认 USB 调试授权。');
    return undefined;
  }
  if (devices.length > 1 && !serial) {
    reporter.fail('发现多个 Android 设备，请使用 --serial <serial> 明确选择。');
    reporter.table(['serial', 'state'], devices.map((device) => [device.serial, device.state]));
    return undefined;
  }
  const device = serial ? devices.find((candidate) => candidate.serial === serial) : devices[0];
  if (!device) {
    reporter.fail(`未发现 serial 为 ${serial} 的 Android 设备。`);
    reporter.table(['serial', 'state'], devices.map((candidate) => [candidate.serial, candidate.state]));
    return undefined;
  }
  if (device.state !== 'device') {
    reporter.fail(`设备 ${device.serial} 当前状态为 ${device.state}，请先在真机上授权 USB 调试。`);
    return undefined;
  }
  return device;
}
