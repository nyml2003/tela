// 应用层：构建浏览器 WebView Target host。应用 guest 由产品流程单独构建并经索引加载。

import type { FsPort, ProcessPort, Reporter } from '../domain/ports.ts';
import type { WorkspacePaths } from '../domain/workspace.ts';
import { WEBVIEW_TARGET_CRATE } from '../domain/workspace.ts';
import type { CargoPort } from '../infrastructure/cargo.ts';

export interface BuildWebviewDeps {
  cargo: CargoPort;
  process: ProcessPort;
  fs: FsPort;
  reporter: Reporter;
  workspace: WorkspacePaths;
}

export interface BuildWebviewResult {
  ok: boolean;
}

/** Builds the release WGPU Target host and emits wasm-bindgen glue at the static server root. */
export async function runBuildWebview(
  deps: BuildWebviewDeps,
): Promise<BuildWebviewResult> {
  const { cargo, process, fs, reporter, workspace } = deps;
  const profile = 'release' as const;
  reporter.section('构建浏览器 WebView Target host（release WGPU）');
  await fs.ensureDir(workspace.distDir);
  const build = await cargo.buildWasm(WEBVIEW_TARGET_CRATE, profile);
  if (!build.passed) {
    reporter.fail('WebView Target host wasm 构建失败');
    if (build.detail) reporter.info(build.detail);
    return { ok: false };
  }
  reporter.ok(`cargo build --target wasm32-unknown-unknown -p ${WEBVIEW_TARGET_CRATE} --release (${build.durationMs.toFixed(0)}ms)`);

  const generated = await process.run(
    'wasm-bindgen',
    [
      '--target', 'web',
      '--out-dir', workspace.distDir,
      '--out-name', 'tela_webview_host',
      workspace.webviewTargetArtifactPath(profile),
    ],
    { cwd: workspace.root },
  );
  if (generated.code !== 0) {
    reporter.fail('WebView Target host wasm-bindgen glue 生成失败（需与 Cargo.lock 匹配的 wasm-bindgen-cli）');
    reporter.info((generated.stderr || generated.stdout).slice(-2000));
    return { ok: false };
  }
  reporter.ok(`wasm-bindgen glue → ${workspace.webviewHostGluePath()} + tela_webview_host_bg.wasm`);
  return { ok: true };
}
