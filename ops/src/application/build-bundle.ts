// 应用层：构建供原生开发 SDK 消费的单文件 WASM bundle。
import type { FsPort, ProcessPort, Reporter } from '../domain/ports.ts';
import type { BundleChannel, WorkspacePaths } from '../domain/workspace.ts';
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
  channel: BundleChannel = 'desktop',
): Promise<BuildBundleResult> {
  const { cargo, process, fs, reporter, workspace } = deps;
  const bundle = workspace.bundle(channel);
  // Wasmtime executes this artifact directly. A debug WASM guest makes ordinary text/layout work
  // consume hundreds of millions of fuel, so the development package is always optimized.
  const profile = 'release' as const;
  reporter.section(`构建 ${bundle.label} bundle（${profile} WASM）`);
  await fs.ensureDir(bundle.dir());

  const build = await cargo.buildWasm(bundle.guestCrate, profile, bundle.guestFeatures);
  if (!build.passed) {
    reporter.fail('应用 guest WASM 构建失败');
    if (build.detail) reporter.info(build.detail);
    return { ok: false };
  }

  const result = await process.run(
    'cargo',
    [
      'run', '--quiet', '-p', 'tela-bundle', '--bin', 'tela-bundle', '--',
      bundle.guestWasmArtifactPath(profile),
      bundle.archiveTempPath(),
      bundle.indexTempPath(),
      bundle.archiveUrl,
      bundle.assetsDir(),
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
      'run', '--quiet', '-p', 'tela-guest-runtime', '--bin', 'tela-guest-verify', '--',
      bundle.archiveTempPath(),
    ],
    { cwd: workspace.root },
  );
  if (guestVerification.code !== 0) {
    reporter.fail(`bundle guest 初始化校验失败（exit=${guestVerification.code}）`);
    reporter.info((guestVerification.stderr || guestVerification.stdout).slice(-4000));
    return { ok: false };
  }
  reporter.ok('bundle guest 初始化与 viewport 校验通过');

  await fs.rename(bundle.archiveTempPath(), bundle.archivePath());
  await fs.rename(bundle.indexTempPath(), bundle.indexPath());
  const bytes = await fs.statSize(bundle.archivePath());
  reporter.ok(`发布 ${bundle.channel} bundle → ${bundle.archivePath()}`);
  reporter.info(`索引 → ${bundle.indexPath()}；压缩包 ${(bytes ?? 0) / 1024 / 1024 < 1
    ? `${Math.ceil((bytes ?? 0) / 1024)}KB`
    : `${((bytes ?? 0) / 1024 / 1024).toFixed(1)}MB`}`);
  return { ok: true };
}
