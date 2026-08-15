// 应用层：构建供原生开发 SDK 消费的单文件 WASM bundle。
import type { FsPort, ProcessPort, Reporter } from '../domain/ports.ts';
import type { WorkspacePaths } from '../domain/workspace.ts';
import { DEMO_CRATE } from '../domain/workspace.ts';
import type { CargoPort } from '../infrastructure/cargo.ts';

export interface BuildBundleDeps {
  cargo: CargoPort;
  process: ProcessPort;
  fs: FsPort;
  reporter: Reporter;
  workspace: WorkspacePaths;
}

export interface BuildBundleResult {
  ok: boolean;
}

/**
 * 生成 `dist/tela-dev/tela-demo.tela` 与 `latest.json`。
 *
 * 压缩包先写临时路径，索引最后原子替换；原生 SDK 因此只会看见完整 archive。
 */
export async function runBuildBundle(
  deps: BuildBundleDeps,
): Promise<BuildBundleResult> {
  const { cargo, process, fs, reporter, workspace } = deps;
  // Wasmtime executes this artifact directly. A debug WASM guest makes ordinary text/layout work
  // consume hundreds of millions of fuel, so the development package is always optimized.
  const profile = 'release' as const;
  reporter.section(`构建平台 SDK bundle（${profile} WASM）`);
  await fs.ensureDir(workspace.bundleDir());

  const build = await cargo.buildWasm(DEMO_CRATE, profile, ['app-wasm']);
  if (!build.passed) {
    reporter.fail('应用 guest WASM 构建失败');
    if (build.detail) reporter.info(build.detail);
    return { ok: false };
  }

  const result = await process.run(
    'cargo',
    [
      'run', '--quiet', '-p', 'tela-bundle', '--bin', 'tela-bundle', '--',
      workspace.appGuestWasmArtifactPath(profile),
      workspace.bundleArchiveTempPath(),
      workspace.bundleIndexTempPath(),
      '/tela-dev/tela-demo.tela',
      workspace.bundleAssetsDir(),
    ],
    { cwd: workspace.root },
  );
  if (result.code !== 0) {
    reporter.fail(`bundle 打包失败（exit=${result.code}）`);
    reporter.info((result.stderr || result.stdout).slice(-2000));
    return { ok: false };
  }

  const guestVerification = await process.run(
    'cargo',
    [
      'run', '--quiet', '-p', 'tela-native-sdk-runtime', '--bin', 'tela-sdk-verify', '--',
      workspace.bundleArchiveTempPath(),
    ],
    { cwd: workspace.root },
  );
  if (guestVerification.code !== 0) {
    reporter.fail(`bundle guest 初始化校验失败（exit=${guestVerification.code}）`);
    reporter.info((guestVerification.stderr || guestVerification.stdout).slice(-4000));
    return { ok: false };
  }
  reporter.ok('bundle guest 初始化与 viewport 校验通过');

  await fs.rename(workspace.bundleArchiveTempPath(), workspace.bundleArchivePath());
  await fs.rename(workspace.bundleIndexTempPath(), workspace.bundleIndexPath());
  const bytes = await fs.statSize(workspace.bundleArchivePath());
  reporter.ok(`发布 bundle → ${workspace.bundleArchivePath()}`);
  reporter.info(`索引 → ${workspace.bundleIndexPath()}；压缩包 ${(bytes ?? 0) / 1024 / 1024 < 1
    ? `${Math.ceil((bytes ?? 0) / 1024)}KB`
    : `${((bytes ?? 0) / 1024 / 1024).toFixed(1)}MB`}`);
  return { ok: true };
}
