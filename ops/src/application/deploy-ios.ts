// 应用层：使用 Xcode 已配置的 Apple Development 签名安装并启动 iPhone 调试 App。

import type { FsPort, ProcessPort, Reporter } from '../domain/ports.ts';
import type { WorkspacePaths } from '../domain/workspace.ts';

const IOS_XCODE_TARGET = 'TelaMobile';
const IOS_BUNDLE_IDENTIFIER = 'dev.tela.mobile';

export interface DeployIosDeps {
  process: ProcessPort;
  fs: FsPort;
  reporter: Reporter;
  workspace: WorkspacePaths;
}

export interface DeployIosOptions {
  deviceId: string;
}

/** Builds with the Team selected in Xcode, then installs and launches the resulting iPhone App. */
export async function runDeployIos(
  deps: DeployIosDeps,
  options: DeployIosOptions,
): Promise<boolean> {
  const { process, fs, reporter, workspace } = deps;
  const deviceId = options.deviceId.trim();
  if (!deviceId) {
    reporter.fail('iOS 部署需要明确的 UDID：ops ios deploy --device <UDID>');
    return false;
  }
  if (!(await fs.exists(workspace.iosXcodeStaticLibraryPath()))) {
    reporter.fail(`缺少 iOS Rust 静态库: ${workspace.iosXcodeStaticLibraryPath()}`);
    reporter.info('请先运行: nix develop .#ios --command ops build ios');
    return false;
  }

  reporter.section(`签名、安装并启动 iPhone App（${deviceId}）`);
  const build = await process.run(
    'tela-ios-xcodebuild',
    [
      '-project', workspace.iosXcodeProjectPath(),
      '-scheme', IOS_XCODE_TARGET,
      '-configuration', 'Debug',
      '-sdk', 'iphoneos',
      // A concrete device destination lets Xcode register this UDID in the Team before
      // generating the provisioning profile; generic/platform=iOS yields "no devices".
      '-destination', `platform=iOS,id=${deviceId}`,
      '-derivedDataPath', workspace.iosDerivedDataDir(),
      '-allowProvisioningUpdates',
      'build',
    ],
    { cwd: workspace.iosProjectDir() },
  );
  if (build.code !== 0) {
    reporter.fail('签名 iPhone App 构建失败');
    reporter.info((build.stderr || build.stdout).trim().slice(0, 2000));
    reporter.info('在 Xcode 打开 products/ios/TelaMobile.xcodeproj，为 TelaMobile 选择可用的 Apple Development Team。');
    return false;
  }

  const appPath = workspace.iosAppPath('dev');
  if (!(await fs.exists(appPath))) {
    reporter.fail(`签名构建未产出预期 App: ${appPath}`);
    return false;
  }
  const install = await process.run(
    'tela-ios-xcrun',
    ['devicectl', 'device', 'install', 'app', '--device', deviceId, appPath],
    { cwd: workspace.iosProjectDir() },
  );
  if (install.code !== 0) {
    reporter.fail('iPhone App 安装失败');
    reporter.info((install.stderr || install.stdout).trim().slice(0, 2000));
    return false;
  }
  const launch = await process.run(
    'tela-ios-xcrun',
    ['devicectl', 'device', 'process', 'launch', '--device', deviceId, IOS_BUNDLE_IDENTIFIER],
    { cwd: workspace.iosProjectDir() },
  );
  if (launch.code !== 0) {
    reporter.fail('iPhone App 已安装，但启动失败');
    reporter.info((launch.stderr || launch.stdout).trim().slice(0, 2000));
    return false;
  }
  reporter.ok(`已在 ${deviceId} 安装并启动 ${IOS_BUNDLE_IDENTIFIER}`);
  return true;
}
