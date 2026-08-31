// 应用层：构建静态链接的 WebView Probe Web 产品；单一 Wasm 同时包含应用、Target 与 renderer。

import type { FsPort, ProcessPort, Reporter } from '../domain/ports.ts';
import type { WorkspacePaths } from '../domain/workspace.ts';
import { WEBVIEW_PROBE_PRODUCT_CRATE } from '../domain/workspace.ts';
import type { CargoPort } from '../infrastructure/cargo.ts';

export interface BuildWebviewProbeDeps {
  cargo: CargoPort;
  process: ProcessPort;
  fs: FsPort;
  reporter: Reporter;
  workspace: WorkspacePaths;
}

export interface BuildWebviewProbeResult {
  ok: boolean;
}

/** Builds and emits the release single-Wasm WebView probe product at the static server root. */
export async function runBuildWebviewProbe(
  deps: BuildWebviewProbeDeps,
): Promise<BuildWebviewProbeResult> {
  const { cargo, process, fs, reporter, workspace } = deps;
  const profile = 'release' as const;
  reporter.section('构建 Tela WebView Probe（release，单 Wasm 静态链接）');
  await fs.ensureDir(workspace.distDir);
  const build = await cargo.buildWasm(WEBVIEW_PROBE_PRODUCT_CRATE, profile);
  if (!build.passed) {
    reporter.fail('WebView Probe Wasm 构建失败');
    if (build.detail) reporter.info(build.detail);
    return { ok: false };
  }
  reporter.ok(
    `cargo build --target wasm32-unknown-unknown -p ${WEBVIEW_PROBE_PRODUCT_CRATE} --release (${build.durationMs.toFixed(0)}ms)`,
  );

  const generated = await process.run(
    'wasm-bindgen',
    [
      '--target', 'web',
      '--out-dir', workspace.distDir,
      '--out-name', 'tela_webview_probe',
      workspace.webviewProbeArtifactPath(profile),
    ],
    { cwd: workspace.root },
  );
  if (generated.code !== 0) {
    reporter.fail('WebView Probe wasm-bindgen glue 生成失败（需与 Cargo.lock 匹配的 wasm-bindgen-cli）');
    reporter.info((generated.stderr || generated.stdout).slice(-2000));
    return { ok: false };
  }
  reporter.ok(
    `wasm-bindgen glue → ${workspace.webviewProbeGluePath()} + tela_webview_probe_bg.wasm`,
  );
  return { ok: true };
}
