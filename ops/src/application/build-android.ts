// 应用层：构建 Android GameActivity 开发 APK；mobile guest 保持在单独的远程 bundle 中。

import type { FsPort, ProcessPort, ProcessResult, Reporter } from '../domain/ports.ts';
import type { WorkspacePaths } from '../domain/workspace.ts';
import { runBuildBundle } from './build-bundle.ts';
import type { CargoPort } from '../infrastructure/cargo.ts';

const ANDROID_SDK_CRATE = 'tela-android-sdk';
const ANDROID_NDK_ABI = 'x86_64';

export interface BuildAndroidDeps {
  cargo: CargoPort;
  process: ProcessPort;
  fs: FsPort;
  reporter: Reporter;
  workspace: WorkspacePaths;
}

export interface BuildAndroidOptions {
  /** Exact development manifest URL requested by the app on each launch. */
  bundleIndex: string;
}

export interface BuildAndroidResult {
  ok: boolean;
}

/**
 * Builds the independent mobile guest first, then packages a Vulkan-only x86_64 GameActivity APK.
 *
 * The APK receives only the target host and its remote index URL. It never embeds the guest archive
 * or substitutes a cache when that URL cannot provide a valid current bundle.
 */
export async function runBuildAndroid(
  deps: BuildAndroidDeps,
  options: BuildAndroidOptions,
): Promise<BuildAndroidResult> {
  const { cargo, process, fs, reporter, workspace } = deps;
  const bundleIndex = normalizeBundleIndex(options.bundleIndex);
  if (!bundleIndex) {
    reporter.fail('Android 构建需要 --bundle-index http(s)://<host>:<port>/tela-mobile/latest.json');
    return { ok: false };
  }

  const bundle = await runBuildBundle({ cargo, process, fs, reporter, workspace }, 'mobile');
  if (!bundle.ok) return bundle;

  reporter.section(`构建 Android GameActivity APK（${ANDROID_NDK_ABI} / Vulkan）`);
  try {
    await fs.ensureDir(workspace.androidJniLibsDir());
  } catch (error) {
    reporter.fail('无法准备 Android JNI 输出目录');
    reporter.info(String(error));
    return { ok: false };
  }

  const native = await runExternal(
    process,
    'cargo',
    [
      'ndk', '-t', ANDROID_NDK_ABI, '-o', workspace.androidJniLibsDir(),
      'build', '--release', '-p', ANDROID_SDK_CRATE,
    ],
    workspace.root,
    reporter,
    'Android Rust 原生库构建失败（需要 cargo-ndk、Android NDK 与 x86_64-linux-android target）',
  );
  if (!native || native.code !== 0) return { ok: false };

  const gradle = await runExternal(
    process,
    'gradle',
    ['--no-daemon', ':app:assembleDebug', `-PtelaBundleIndex=${bundleIndex}`],
    workspace.androidProjectDir(),
    reporter,
    'Android Gradle APK 构建失败（需要 JDK 17、Android SDK API 36 与 Gradle）',
  );
  if (!gradle || gradle.code !== 0) return { ok: false };

  try {
    await fs.ensureDir(workspace.androidDistDir());
    await fs.copyFile(workspace.androidDebugApkPath(), workspace.androidDistPath());
  } catch (error) {
    reporter.fail('Android APK 构建完成，但发布到 dist/ 失败');
    reporter.info(String(error));
    return { ok: false };
  }

  const bytes = await fs.statSize(workspace.androidDistPath());
  reporter.ok(`发布 Android APK → ${workspace.androidDistPath()}`);
  reporter.info(`尺寸 ${((bytes ?? 0) / 1024 / 1024).toFixed(1)}MB；启动时严格请求 ${bundleIndex}`);
  return { ok: true };
}

function normalizeBundleIndex(value: string): string | undefined {
  if (!value.trim()) return undefined;
  try {
    const url = new URL(value);
    return url.protocol === 'http:' || url.protocol === 'https:' ? url.toString() : undefined;
  } catch {
    return undefined;
  }
}

async function runExternal(
  process: ProcessPort,
  command: string,
  args: string[],
  cwd: string,
  reporter: Reporter,
  failure: string,
): Promise<ProcessResult | undefined> {
  try {
    const result = await process.run(command, args, { cwd });
    if (result.code !== 0) {
      reporter.fail(`${failure}（exit=${result.code}）`);
      reporter.info((result.stderr || result.stdout).slice(-4000));
    }
    return result;
  } catch (error) {
    reporter.fail(failure);
    reporter.info(String(error));
    return undefined;
  }
}
