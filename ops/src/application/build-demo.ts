// 应用层：build demo 用例——构建演示 wasm 并发布到 dist/。
import type { FsPort, ProcessPort, Reporter } from '../domain/ports.ts';
import type { BuildProfile, WorkspacePaths } from '../domain/workspace.ts';
import { DEMO_CRATE } from '../domain/workspace.ts';
import type { ArtifactInfo } from '../domain/artifact.ts';
import type { CargoPort } from '../infrastructure/cargo.ts';

export interface BuildDemoDeps {
  cargo: CargoPort;
  process: ProcessPort;
  fs: FsPort;
  reporter: Reporter;
  workspace: WorkspacePaths;
}

export interface BuildDemoResult {
  ok: boolean;
  artifact?: ArtifactInfo;
  gpuGlue?: string;
}

export interface BuildDemoOptions {
  profile: BuildProfile;
  /** GPU 后端（WebGPU）：webgpu feature + wasm-bindgen 生成 glue（tela_demo_gpu.js）。 */
  gpu: boolean;
}

/** 构建 + 发布 wasm 工件（替代手工三步：cargo build → cp → 报告）。 */
export async function runBuildDemo(
  deps: BuildDemoDeps,
  opts: BuildDemoOptions,
): Promise<BuildDemoResult> {
  const { cargo, process, fs, reporter, workspace } = deps;
  // wasm-bindgen 的 WebGPU glue 依赖优化后的 wasm，故 GPU 分支始终发布 release 工件。
  const effectiveProfile: BuildProfile = opts.gpu ? 'release' : opts.profile;
  const label = effectiveProfile === 'release' ? 'release' : 'dev';

  reporter.section(`构建演示 wasm（${label}${opts.gpu ? ' + WebGPU' : ''}）`);
  await fs.ensureDir(workspace.distDir);
  const features = opts.gpu ? ['webgpu'] : [];
  // touch build.rs：cargo 检测到 build.rs mtime 变化 → 重跑 → TELA_BUILD_TS 刷新
  //（构建时间戳每次更新，页面版本号可信；见 crates/tela-demo/build.rs）。
  try {
    const buildRs = `${workspace.cratesDir}/${DEMO_CRATE}/build.rs`;
    await fs.touch(buildRs);
  } catch {
    // build.rs 不存在时忽略（理论不会发生）。
  }
  // GPU 后端强制 release：wasm-bindgen CLI 需要优化后的 wasm（debug 缺 externref intrinsics）。
  const build = await cargo.buildWasm(DEMO_CRATE, effectiveProfile, features);
  if (!build.passed) {
    reporter.fail(`cargo build 失败`);
    if (build.detail) reporter.info(build.detail);
    return { ok: false };
  }
  reporter.ok(
    `cargo build --target wasm32-unknown-unknown -p ${DEMO_CRATE}${features.length ? ` --features ${features.join(',')}` : ''} (${build.durationMs.toFixed(0)}ms)`,
  );

  const source = workspace.wasmArtifactPath(effectiveProfile);
  if (opts.gpu) {
    // wasm-bindgen glue：输出 dist/tela_demo_gpu.js + _bg.wasm + .d.ts。
    const wb = await process.run(
      'wasm-bindgen',
      [
        '--target', 'web',
        '--out-dir', workspace.distDir,
        '--out-name', 'tela_demo_gpu',
        source,
      ],
      { cwd: workspace.root },
    );
    if (wb.code !== 0) {
      reporter.fail('wasm-bindgen glue 生成失败（需 wasm-bindgen-cli 与 Cargo.lock 同版本）');
      reporter.info((wb.stderr || wb.stdout).slice(-2000));
      return { ok: false };
    }
    reporter.ok(`wasm-bindgen glue → ${workspace.distDir}/tela_demo_gpu.js + tela_demo_gpu_bg.wasm`);
    return { ok: true, gpuGlue: `${workspace.distDir}/tela_demo_gpu.js` };
  }

  const dest = workspace.wasmDistPath();
  await fs.copyFile(source, dest);
  const bytes = await fs.statSize(dest);
  reporter.ok(`发布工件 → ${dest}`);
  reporter.info(`尺寸 ${((bytes ?? 0) / 1024 / 1024).toFixed(1)}MB（debug 构建较大属正常）`);
  return { ok: true, artifact: { sourcePath: source, destPath: dest, bytes: bytes ?? 0 } };
}
