// 应用层：core 产品闭包只检查 Kernel 与 UI foundation，不启动窗口或引入视觉资源。

import type { Reporter } from '../domain/ports.ts';
import type { WorkspacePaths } from '../domain/workspace.ts';
import type { CargoPort } from '../infrastructure/cargo.ts';

export interface BuildCoreDeps {
  cargo: CargoPort;
  reporter: Reporter;
  workspace: WorkspacePaths;
}

export interface BuildCoreResult {
  ok: boolean;
}

/** Runs the explicit pure-Rust core product closure. */
export async function runBuildCore(deps: BuildCoreDeps): Promise<BuildCoreResult> {
  const { cargo, reporter, workspace } = deps;
  const product = workspace.product('core');
  reporter.section('构建 core 产品闭包（Kernel + UI foundation）');
  const result = await cargo.checkPackages(product.packages);
  if (!result.passed) {
    reporter.fail('core 产品闭包构建失败');
    if (result.detail) reporter.info(result.detail);
    return { ok: false };
  }
  reporter.ok(`cargo check ${product.packages.map((name) => `-p ${name}`).join(' ')}`);
  return { ok: true };
}
