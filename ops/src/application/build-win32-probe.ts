// 应用层：交叉构建并发布最小 Win32 surface probe。
import type { FsPort, Reporter } from '../domain/ports.ts';
import { WIN32_PROBE_CRATE, type BuildProfile, type WorkspacePaths } from '../domain/workspace.ts';
import type { CargoPort } from '../infrastructure/cargo.ts';

export interface BuildWin32ProbeDeps {
  cargo: CargoPort;
  fs: FsPort;
  reporter: Reporter;
  workspace: WorkspacePaths;
}

export interface BuildWin32ProbeResult {
  ok: boolean;
}

/** 编译最小静态诊断产品并复制到 `dist/win32-probe/`。 */
export async function runBuildWin32Probe(
  deps: BuildWin32ProbeDeps,
  profile: BuildProfile,
): Promise<BuildWin32ProbeResult> {
  const { cargo, fs, reporter, workspace } = deps;
  reporter.section(`构建 Win32 surface probe（${profile}）`);
  const build = await cargo.buildWin32(WIN32_PROBE_CRATE, profile);
  if (!build.passed) {
    reporter.fail('Win32 surface probe GNU 交叉构建失败');
    if (build.detail) reporter.info(build.detail);
    return { ok: false };
  }
  await fs.ensureDir(workspace.win32ProbeDistDir());
  await fs.copyFile(
    workspace.win32ProbeArtifactPath(profile),
    workspace.win32ProbeDistPath(),
  );
  const bytes = await fs.statSize(workspace.win32ProbeDistPath());
  reporter.ok(`发布 Win32 surface probe → ${workspace.win32ProbeDistPath()}`);
  reporter.info(`尺寸 ${((bytes ?? 0) / 1024 / 1024).toFixed(1)}MB；静态链接，无 bundle/WASM`);
  return { ok: true };
}
