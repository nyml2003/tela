// 应用层：构建静态链接的 Agent Web 产品；单一 Wasm 同时包含应用、Target 与 renderer。

import type { FsPort, ProcessPort, Reporter } from '../domain/ports.ts';
import type { WorkspacePaths } from '../domain/workspace.ts';
import { AGENT_DEMO_PRODUCT_CRATE } from '../domain/workspace.ts';
import type { CargoPort } from '../infrastructure/cargo.ts';

export interface BuildAgentDemoDeps {
  cargo: CargoPort;
  process: ProcessPort;
  fs: FsPort;
  reporter: Reporter;
  workspace: WorkspacePaths;
}

export interface BuildAgentDemoResult {
  ok: boolean;
}

/** Builds and emits the release single-Wasm Agent product at the static server root. */
export async function runBuildAgentDemo(
  deps: BuildAgentDemoDeps,
): Promise<BuildAgentDemoResult> {
  const { cargo, process, fs, reporter, workspace } = deps;
  const profile = 'release' as const;
  reporter.section('构建 Tela Agent Demo（release，单 Wasm 静态链接）');
  await fs.ensureDir(workspace.distDir);
  const build = await cargo.buildWasm(AGENT_DEMO_PRODUCT_CRATE, profile);
  if (!build.passed) {
    reporter.fail('Agent Demo Wasm 构建失败');
    if (build.detail) reporter.info(build.detail);
    return { ok: false };
  }
  reporter.ok(
    `cargo build --target wasm32-unknown-unknown -p ${AGENT_DEMO_PRODUCT_CRATE} --release (${build.durationMs.toFixed(0)}ms)`,
  );

  const generated = await process.run(
    'wasm-bindgen',
    [
      '--target', 'web',
      '--out-dir', workspace.distDir,
      '--out-name', 'tela_agent_demo',
      workspace.agentDemoArtifactPath(profile),
    ],
    { cwd: workspace.root },
  );
  if (generated.code !== 0) {
    reporter.fail('Agent Demo wasm-bindgen glue 生成失败（需与 Cargo.lock 匹配的 wasm-bindgen-cli）');
    reporter.info((generated.stderr || generated.stdout).slice(-2000));
    return { ok: false };
  }
  reporter.ok(
    `wasm-bindgen glue → ${workspace.agentDemoGluePath()} + tela_agent_demo_bg.wasm`,
  );
  return { ok: true };
}
