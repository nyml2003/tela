// 应用层：交叉构建并发布变速齿轮 Windows x64 静态产品。
import type { FsPort, Reporter } from '../domain/ports.ts';
import { SPEED_GEAR_CRATE, SPEED_GEAR_HOOK_CRATE, type BuildProfile, type WorkspacePaths } from '../domain/workspace.ts';
import type { CargoPort } from '../infrastructure/cargo.ts';

export interface BuildSpeedGearDeps {
  cargo: CargoPort;
  fs: FsPort;
  reporter: Reporter;
  workspace: WorkspacePaths;
}

export interface BuildSpeedGearResult {
  ok: boolean;
}

/** 编译并发布独立的变速齿轮产品；不依赖 desktop guest/bundle。 */
export async function runBuildSpeedGear(
  deps: BuildSpeedGearDeps,
  profile: BuildProfile,
): Promise<BuildSpeedGearResult> {
  const { cargo, fs, reporter, workspace } = deps;
  reporter.section(`构建变速齿轮 Windows x64（${profile}）`);
  const build = await cargo.buildWin32(SPEED_GEAR_CRATE, profile);
  if (!build.passed) {
    reporter.fail('变速齿轮 Windows GNU 交叉构建失败');
    if (build.detail) reporter.info(build.detail);
    return { ok: false };
  }
  await fs.ensureDir(workspace.speedGearDistDir());
  await fs.copyFile(workspace.speedGearArtifactPath(profile), workspace.speedGearDistPath());
  const hook = await cargo.buildWin32(SPEED_GEAR_HOOK_CRATE, profile);
  if (!hook.passed) {
    reporter.fail('变速齿轮目标端 DLL 构建失败');
    if (hook.detail) reporter.info(hook.detail);
    return { ok: false };
  }
  await fs.copyFile(workspace.speedGearHookArtifactPath(profile), workspace.speedGearHookDistPath());
  const bytes = await fs.statSize(workspace.speedGearDistPath());
  reporter.ok(`发布变速齿轮 → ${workspace.speedGearDistPath()}`);
  reporter.info(`尺寸 ${((bytes ?? 0) / 1024 / 1024).toFixed(1)}MB；静态链接，无 bundle/WASM；附带 QPC 目标端 DLL`);
  return { ok: true };
}
