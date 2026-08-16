// 应用层：构建 iPhone ARM64 静态库，并以无签名方式编译最小 UIKit App 供本机检查。

import type { FsPort, ProcessPort, Reporter } from '../domain/ports.ts';
import type { BuildProfile, WorkspacePaths } from '../domain/workspace.ts';
import type { CargoPort } from '../infrastructure/cargo.ts';

const IOS_SDK_CRATE = 'tela-ios-sdk';
const IOS_XCODE_TARGET = 'TelaMobile';

export interface BuildIosDeps {
  cargo: CargoPort;
  process: ProcessPort;
  fs: FsPort;
  reporter: Reporter;
  workspace: WorkspacePaths;
}

export interface BuildIosResult {
  ok: boolean;
}

/** Builds an unsigned iPhone device app without relying on a signing identity or connected phone. */
export async function runBuildIos(
  deps: BuildIosDeps,
  profile: BuildProfile,
): Promise<BuildIosResult> {
  const { cargo, process, fs, reporter, workspace } = deps;
  const configuration = profile === 'release' ? 'Release' : 'Debug';

  reporter.section(`构建 iPhone UIKit/Metal 开发壳（${configuration}，arm64）`);
  const rust = await cargo.buildIos(IOS_SDK_CRATE, profile);
  if (!rust.passed) {
    reporter.fail('iPhone ARM64 Rust 静态库构建失败');
    if (rust.detail) reporter.info(rust.detail);
    return { ok: false };
  }

  try {
    await fs.ensureDir(workspace.iosStaticLibraryDir());
    await fs.copyFile(workspace.iosRustStaticLibraryPath(profile), workspace.iosXcodeStaticLibraryPath());
  } catch (error) {
    reporter.fail('无法为 Xcode 准备 iOS Rust 静态库');
    reporter.info(String(error));
    return { ok: false };
  }

  const build = await process.run(
    'tela-ios-xcodebuild',
    [
      '-project', workspace.iosXcodeProjectPath(),
      '-target', IOS_XCODE_TARGET,
      '-configuration', configuration,
      '-sdk', 'iphoneos',
      '-derivedDataPath', workspace.iosDerivedDataDir(),
      'CODE_SIGNING_ALLOWED=NO',
      'build',
    ],
    { cwd: workspace.iosProjectDir() },
  );
  if (build.code !== 0) {
    reporter.fail('Xcode 无签名 iPhone App 构建失败');
    reporter.info((build.stderr || build.stdout).trim().slice(0, 2000));
    return { ok: false };
  }
  if (!(await fs.exists(workspace.iosAppPath(profile)))) {
    reporter.fail(`Xcode 未产出预期 iPhone App: ${workspace.iosAppPath(profile)}`);
    return { ok: false };
  }

  reporter.ok(`已构建无签名 iPhone App → ${workspace.iosAppPath(profile)}`);
  reporter.info('真机安装需要在 Xcode 为 TelaMobile 配置 Apple Development Team，然后运行 ops ios deploy --device <UDID>。');
  return { ok: true };
}
