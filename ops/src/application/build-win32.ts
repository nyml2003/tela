// 应用层：交叉构建并发布 Win32 开发壳；应用内容仍在 tela-dev bundle 中按启动时加载。
import type { FsPort, Reporter } from '../domain/ports.ts';
import { WIN32_TARGET_CRATE, type BuildProfile, type WorkspacePaths } from '../domain/workspace.ts';
import type { CargoPort } from '../infrastructure/cargo.ts';

export interface BuildWin32Deps {
  cargo: CargoPort;
  fs: FsPort;
  reporter: Reporter;
  workspace: WorkspacePaths;
}

export interface BuildWin32Result {
  ok: boolean;
}

/** 编译 Rust Win32 壳并复制到唯一可删除的 `dist/win32/` 发布位置。 */
export async function runBuildWin32(
  deps: BuildWin32Deps,
  profile: BuildProfile,
): Promise<BuildWin32Result> {
  const { cargo, fs, reporter, workspace } = deps;
  reporter.section(`构建 Win32 开发壳（${profile}）`);
  const build = await cargo.buildWin32(WIN32_TARGET_CRATE, profile);
  if (!build.passed) {
    reporter.fail('Win32 GNU 交叉构建失败');
    if (build.detail) reporter.info(build.detail);
    return { ok: false };
  }
  await fs.ensureDir(workspace.win32DistDir());
  await fs.copyFile(workspace.win32ArtifactPath(profile), workspace.win32DistPath());
  const bytes = await fs.statSize(workspace.win32DistPath());
  reporter.ok(`发布 Win32 壳 → ${workspace.win32DistPath()}`);
  reporter.info(`尺寸 ${((bytes ?? 0) / 1024 / 1024).toFixed(1)}MB；启动时请求 /tela-dev/latest.json`);
  return { ok: true };
}
