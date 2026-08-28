// 应用层：交叉构建并发布 Win32 静态 Agent workbench。
import type { FsPort, Reporter } from '../domain/ports.ts';
import { WIN32_AGENT_CRATE, type BuildProfile, type WorkspacePaths } from '../domain/workspace.ts';
import type { CargoPort } from '../infrastructure/cargo.ts';

export interface BuildWin32AgentDeps {
  cargo: CargoPort;
  fs: FsPort;
  reporter: Reporter;
  workspace: WorkspacePaths;
}

export interface BuildWin32AgentResult {
  ok: boolean;
}

/** 编译静态 Agent 产品并复制到 `dist/win32-agent/`。 */
export async function runBuildWin32Agent(
  deps: BuildWin32AgentDeps,
  profile: BuildProfile,
): Promise<BuildWin32AgentResult> {
  const { cargo, fs, reporter, workspace } = deps;
  reporter.section(`构建 Win32 静态 Agent（${profile}）`);
  const build = await cargo.buildWin32(WIN32_AGENT_CRATE, profile);
  if (!build.passed) {
    reporter.fail('Win32 Agent GNU 交叉构建失败');
    if (build.detail) reporter.info(build.detail);
    return { ok: false };
  }
  await fs.ensureDir(workspace.win32AgentDistDir());
  await fs.copyFile(
    workspace.win32AgentArtifactPath(profile),
    workspace.win32AgentDistPath(),
  );
  const bytes = await fs.statSize(workspace.win32AgentDistPath());
  reporter.ok(`发布 Win32 Agent → ${workspace.win32AgentDistPath()}`);
  reporter.info(`尺寸 ${((bytes ?? 0) / 1024 / 1024).toFixed(1)}MB；静态链接，无 bundle/WASM`);
  return { ok: true };
}
