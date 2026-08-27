// 应用层：构建 CC Remote 的两个本机服务二进制（中继与桌面 agent）并发布到 dist/。

import type { FsPort, Reporter } from '../domain/ports.ts';
import { CC_AGENT_CRATE, CC_RELAY_CRATE, type BuildProfile, type WorkspacePaths } from '../domain/workspace.ts';
import type { CargoPort } from '../infrastructure/cargo.ts';

export interface BuildCcServiceDeps {
  cargo: CargoPort;
  fs: FsPort;
  reporter: Reporter;
  workspace: WorkspacePaths;
}

export interface BuildCcServiceResult {
  ok: boolean;
}

/** 构建并发布中继二进制；产物部署到 2c2G 服务器，systemd 常驻。 */
export async function runBuildRelay(
  deps: BuildCcServiceDeps,
  profile: BuildProfile = 'release',
): Promise<BuildCcServiceResult> {
  const { workspace } = deps;
  return buildService(deps, {
    crate: CC_RELAY_CRATE,
    label: '中继',
    distDir: workspace.ccRelayDistDir(),
    distPath: workspace.ccRelayDistPath(),
    artifactPath: workspace.ccRelayArtifactPath(profile),
    runHint: '服务器运行: CC_RELAY_TOKEN=<token> ./tela-cc-relay（常驻内存仅数 MB）',
  }, profile);
}

/** 构建并发布桌面 agent 二进制；与 claude CLI 同机（WSL2）运行。 */
export async function runBuildAgent(
  deps: BuildCcServiceDeps,
  profile: BuildProfile = 'release',
): Promise<BuildCcServiceResult> {
  const { workspace } = deps;
  return buildService(deps, {
    crate: CC_AGENT_CRATE,
    label: '桌面 agent',
    distDir: workspace.ccAgentDistDir(),
    distPath: workspace.ccAgentDistPath(),
    artifactPath: workspace.ccAgentArtifactPath(profile),
    runHint: 'WSL2 运行: CC_AGENT_RELAY_ADDR=<host:8789> CC_AGENT_TOKEN=<token> CC_AGENT_CWDS=<dir> ./tela-cc-agent [--fake]',
  }, profile);
}

interface ServiceLayout {
  crate: string;
  label: string;
  distDir: string;
  distPath: string;
  artifactPath: string;
  /** 发布后的运行提示。 */
  runHint: string;
}

async function buildService(
  deps: BuildCcServiceDeps,
  layout: ServiceLayout,
  profile: BuildProfile,
): Promise<BuildCcServiceResult> {
  const { cargo, fs, reporter } = deps;
  reporter.section(`构建 CC Remote ${layout.label}（${profile}）`);
  const build = await cargo.buildHost(layout.crate, profile);
  if (!build.passed) {
    reporter.fail(`CC Remote ${layout.label}构建失败`);
    if (build.detail) reporter.info(build.detail);
    return { ok: false };
  }
  try {
    await fs.ensureDir(layout.distDir);
    await fs.copyFile(layout.artifactPath, layout.distPath);
  } catch (error) {
    reporter.fail(`CC Remote ${layout.label}发布到 dist/ 失败`);
    reporter.info(String(error));
    return { ok: false };
  }
  const bytes = await fs.statSize(layout.distPath);
  reporter.ok(`发布 ${layout.label} → ${layout.distPath}`);
  reporter.info(`尺寸 ${((bytes ?? 0) / 1024 / 1024).toFixed(1)}MB；${layout.runHint}`);
  return { ok: true };
}
