// 应用层：验证已发布的统一 `.tela` guest，而不是旧的浏览器 CPU wasm。

import type { FsPort, ProcessPort, Reporter } from '../domain/ports.ts';
import type { BundleChannel, WorkspacePaths } from '../domain/workspace.ts';

export interface VerifyBundleDeps {
  fs: FsPort;
  process: ProcessPort;
  reporter: Reporter;
  workspace: WorkspacePaths;
}

export interface VerifyBundleResult {
  ok: boolean;
  detail?: string;
}

/** Runs the same archive/ABI/guest-start gate against the already published bundle. */
export async function runVerifyBundle(
  deps: VerifyBundleDeps,
  channel: BundleChannel = 'desktop',
): Promise<VerifyBundleResult> {
  const { fs, process, reporter, workspace } = deps;
  const bundle = workspace.bundle(channel);
  const archive = bundle.archivePath();
  reporter.section(`验证 ${bundle.label} bundle`);
  if (!(await fs.exists(archive))) {
    const suffix = channel === 'mobile' ? ' mobile' : '';
    reporter.fail(`缺少 ${archive}，请先运行: ops build bundle${suffix}`);
    return { ok: false, detail: 'bundle 未构建' };
  }
  const result = await process.run(
    'cargo',
    ['run', '--quiet', '-p', 'tela-guest-runtime', '--bin', 'tela-guest-verify', '--', archive],
    { cwd: workspace.root },
  );
  if (result.code !== 0) {
    const detail = (result.stderr || result.stdout).slice(-4000);
    reporter.fail('bundle guest 验证失败');
    reporter.info(detail);
    return { ok: false, detail };
  }
  reporter.ok(`bundle guest 验证通过（${(result.stderr || result.stdout).trim()}）`);
  return { ok: true };
}
