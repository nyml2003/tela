// 应用层：build frontend 用例——浏览器宿主（web/）esbuild 构建到 dist/assets/tela-web/。
import type { ProcessPort, Reporter } from '../domain/ports.ts';
import type { WorkspacePaths } from '../domain/workspace.ts';

export interface BuildFrontendDeps {
  process: ProcessPort;
  reporter: Reporter;
  workspace: WorkspacePaths;
}

export interface BuildFrontendResult {
  ok: boolean;
  durationMs: number;
}

/** 构建浏览器宿主：pnpm --dir web build（build.mjs：页面模板 + bundle → dist/）。 */
export async function runBuildFrontend(
  deps: BuildFrontendDeps,
): Promise<BuildFrontendResult> {
  const { process, reporter, workspace } = deps;
  const t0 = Date.now();
  reporter.section('构建前端（web/ → dist/assets/tela-web）');
  // 直接 node build.mjs（绕开 pnpm 11 的 verify-deps-before-run 前置检查；
  // 依赖由 pnpm install 管理，构建本身只需 node + node_modules）。
  const result = await process.run('node', ['build.mjs'], { cwd: workspace.webDir });
  const durationMs = Date.now() - t0;
  if (result.code !== 0) {
    reporter.fail(`前端构建失败（exit=${result.code}）`);
    if (result.stdout) reporter.info(result.stdout);
    if (result.stderr) reporter.info(result.stderr);
    return { ok: false, durationMs };
  }
  reporter.ok(`esbuild 构建完成 → dist/assets/tela-web/ (${durationMs.toFixed(0)}ms)`);
  return { ok: true, durationMs };
}
