// 基础设施层：在 WSL 中调用 Windows Android Studio SDK 的 adb.exe。

import type { AndroidDevice, AndroidDevicePort, FsPort, ProcessPort, ProcessResult } from '../domain/ports.ts';

const DEFAULT_WINDOWS_CMD = '/mnt/c/Windows/System32/cmd.exe';

export interface WindowsAdbOptions {
  environment?: NodeJS.ProcessEnv;
}

export class WindowsAdbPort implements AndroidDevicePort {
  private executablePromise: Promise<string> | undefined;
  private localAppDataPromise: Promise<string> | undefined;
  private readonly processPort: ProcessPort;
  private readonly fs: FsPort;
  private readonly environment: NodeJS.ProcessEnv;

  constructor(
    processPort: ProcessPort,
    fs: FsPort,
    options: WindowsAdbOptions = {},
  ) {
    this.processPort = processPort;
    this.fs = fs;
    this.environment = options.environment ?? process.env;
  }

  async listDevices(): Promise<readonly AndroidDevice[]> {
    const result = await this.run(['devices', '-l']);
    return result.stdout
      .split(/\r?\n/)
      .map((line) => line.trim())
      .filter((line) => line.length > 0 && !line.startsWith('List of devices attached'))
      .map((line) => {
        const [serial, state] = line.split(/\s+/, 3);
        return serial && state ? { serial, state } : undefined;
      })
      .filter((device): device is AndroidDevice => device !== undefined);
  }

  async abiList(serial: string): Promise<readonly string[]> {
    const list = splitAbiList((await this.run(['-s', serial, 'shell', 'getprop', 'ro.product.cpu.abilist'])).stdout);
    if (list.length > 0) return list;
    return splitAbiList((await this.run(['-s', serial, 'shell', 'getprop', 'ro.product.cpu.abi'])).stdout);
  }

  async reverse(serial: string, devicePort: number, hostPort: number): Promise<void> {
    await this.run(['-s', serial, 'reverse', `tcp:${devicePort}`, `tcp:${hostPort}`]);
  }

  async install(serial: string, apkPath: string): Promise<void> {
    const stagedApk = await this.stageApkForWindowsAdb(apkPath);
    await this.run(['-s', serial, 'install', '-r', stagedApk]);
  }

  async launch(serial: string, component: string): Promise<void> {
    await this.run(['-s', serial, 'shell', 'am', 'start', '-S', '-n', component]);
  }

  private async run(args: string[]): Promise<ProcessResult> {
    const executable = await this.executable();
    const result = await this.processPort.run(executable, args);
    if (result.code !== 0) {
      const detail = (result.stderr || result.stdout).trim();
      throw new Error(`Windows adb.exe 失败（${args.join(' ')}，exit=${result.code}）${detail ? `: ${detail}` : ''}`);
    }
    return result;
  }

  private executable(): Promise<string> {
    if (!this.executablePromise) this.executablePromise = this.resolveExecutable();
    return this.executablePromise;
  }

  private async resolveExecutable(): Promise<string> {
    const configured = this.environment.TELA_WINDOWS_ADB?.trim();
    const candidate = configured || await this.defaultWindowsAdbPath();
    const wslPath = await this.toWslPath(candidate);
    if (!(await this.fs.exists(wslPath))) {
      throw new Error(
        `找不到 Windows adb.exe: ${wslPath}。请在 Android Studio 安装 Platform Tools，或设置 TELA_WINDOWS_ADB。`,
      );
    }
    return wslPath;
  }

  private async defaultWindowsAdbPath(): Promise<string> {
    return `${await this.windowsLocalAppData()}\\Android\\Sdk\\platform-tools\\adb.exe`;
  }

  private windowsLocalAppData(): Promise<string> {
    if (!this.localAppDataPromise) this.localAppDataPromise = this.resolveWindowsLocalAppData();
    return this.localAppDataPromise;
  }

  private async resolveWindowsLocalAppData(): Promise<string> {
    // WSL may intentionally omit Windows directories from PATH; invoke the mounted command prompt directly.
    const command = this.environment.TELA_WINDOWS_CMD?.trim() || DEFAULT_WINDOWS_CMD;
    const result = await this.processPort.run(command, ['/D', '/S', '/C', 'echo %LOCALAPPDATA%']);
    const localAppData = result.stdout.trim();
    if (result.code !== 0 || !localAppData || localAppData === '%LOCALAPPDATA%') {
      throw new Error('无法读取 Windows LOCALAPPDATA；请设置 TELA_WINDOWS_ADB 为 adb.exe 的 WSL 路径。');
    }
    return localAppData;
  }

  private async stageApkForWindowsAdb(apkPath: string): Promise<string> {
    const filename = apkPath.split(/[\\/]/).at(-1);
    if (!filename || filename === '.' || filename === '..') {
      throw new Error(`无法从 APK 路径推导 Windows 临时文件名: ${apkPath}`);
    }
    const windowsPath = `${await this.windowsLocalAppData()}\\Temp\\tela-android\\${filename}`;
    const wslPath = await this.toWslPath(windowsPath);
    const parent = wslPath.slice(0, wslPath.lastIndexOf('/'));
    if (!parent) {
      throw new Error(`无法从 Windows 临时 APK 路径推导目录: ${wslPath}`);
    }
    await this.fs.ensureDir(parent);
    await this.fs.copyFile(apkPath, wslPath);
    return windowsPath;
  }

  private async toWslPath(path: string): Promise<string> {
    if (path.startsWith('/')) return path;
    const result = await this.processPort.run('wslpath', ['-u', path]);
    const converted = result.stdout.trim();
    if (result.code !== 0 || !converted) {
      throw new Error(`无法把 Windows adb.exe 路径转换为 WSL 路径: ${path}`);
    }
    return converted;
  }
}

function splitAbiList(output: string): string[] {
  return output.trim().split(/[\s,]+/).filter((abi) => abi.length > 0);
}
