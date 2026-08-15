// 应用层：构建 macOS 原生开发壳并打成最小 App bundle；应用内容仍启动时从 tela-dev 获取。
import type { FsPort, Reporter } from '../domain/ports.ts';
import type { BuildProfile, WorkspacePaths } from '../domain/workspace.ts';
import type { CargoPort } from '../infrastructure/cargo.ts';

const MACOS_SDK_CRATE = 'tela-macos-sdk';
const EXECUTABLE_MODE = 0o755;

export interface BuildMacosDeps {
  cargo: CargoPort;
  fs: FsPort;
  reporter: Reporter;
  workspace: WorkspacePaths;
}

export interface BuildMacosResult {
  ok: boolean;
}

/**
 * 发布标准 `Tela.app` 目录。
 *
 * App 只包含 AppKit/Metal 壳和 Info.plist；WASM、资源与 latest.json 不复制进 bundle，
 * 保持“进程启动时请求一次开发包”的协议边界。
 */
export async function runBuildMacos(
  deps: BuildMacosDeps,
  profile: BuildProfile,
): Promise<BuildMacosResult> {
  const { cargo, fs, reporter, workspace } = deps;
  reporter.section(`构建 macOS 开发壳（${profile}，aarch64）`);
  const build = await cargo.buildMacos(MACOS_SDK_CRATE, profile);
  if (!build.passed) {
    reporter.fail('macOS Apple Silicon 构建失败');
    if (build.detail) reporter.info(build.detail);
    return { ok: false };
  }

  await fs.resetDir(workspace.macosAppDir());
  await fs.ensureDir(workspace.macosExecutableDir());
  await fs.copyFile(workspace.macosInfoPlistSourcePath(), workspace.macosInfoPlistPath());
  await fs.copyFile(workspace.macosArtifactPath(profile), workspace.macosExecutablePath());
  await fs.setMode(workspace.macosExecutablePath(), EXECUTABLE_MODE);

  const bytes = await fs.statSize(workspace.macosExecutablePath());
  reporter.ok(`发布 macOS App → ${workspace.macosAppDir()}`);
  reporter.info(`壳 ${(bytes ?? 0) / 1024 / 1024 < 1
    ? `${Math.ceil((bytes ?? 0) / 1024)}KB`
    : `${((bytes ?? 0) / 1024 / 1024).toFixed(1)}MB`}；启动时请求 --bundle-index 指向的 /tela-dev/latest.json`);
  return { ok: true };
}
