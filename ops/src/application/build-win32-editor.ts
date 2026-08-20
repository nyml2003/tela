// 应用层：交叉构建并发布 Win32 静态文本编辑器（无 bundle、无 WASM）。
import type { FsPort, Reporter } from '../domain/ports.ts';
import { WIN32_EDITOR_CRATE, type BuildProfile, type WorkspacePaths } from '../domain/workspace.ts';
import type { CargoPort } from '../infrastructure/cargo.ts';

export interface BuildWin32EditorDeps {
  cargo: CargoPort;
  fs: FsPort;
  reporter: Reporter;
  workspace: WorkspacePaths;
}

export interface BuildWin32EditorResult {
  ok: boolean;
}

/** 编译静态编辑器产品并复制到 `dist/win32-editor/`。 */
export async function runBuildWin32Editor(
  deps: BuildWin32EditorDeps,
  profile: BuildProfile,
): Promise<BuildWin32EditorResult> {
  const { cargo, fs, reporter, workspace } = deps;
  reporter.section(`构建 Win32 静态文本编辑器（${profile}）`);
  const build = await cargo.buildWin32(WIN32_EDITOR_CRATE, profile);
  if (!build.passed) {
    reporter.fail('Win32 GNU 交叉构建失败');
    if (build.detail) reporter.info(build.detail);
    return { ok: false };
  }
  await fs.ensureDir(workspace.win32EditorDistDir());
  await fs.copyFile(
    workspace.win32EditorArtifactPath(profile),
    workspace.win32EditorDistPath(),
  );
  const bytes = await fs.statSize(workspace.win32EditorDistPath());
  reporter.ok(`发布 Win32 编辑器 → ${workspace.win32EditorDistPath()}`);
  reporter.info(`尺寸 ${((bytes ?? 0) / 1024 / 1024).toFixed(1)}MB；静态链接，无 bundle/WASM`);
  return { ok: true };
}
