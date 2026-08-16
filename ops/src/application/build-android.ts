// 应用层：构建 Android GameActivity 开发 APK；mobile guest 保持在单独的远程 bundle 中。

import type { FsPort, ProcessPort, ProcessResult, Reporter } from '../domain/ports.ts';
import { ANDROID_BUNDLE_INDEX_URL, ANDROID_NDK_ABI, ANDROID_RUST_TARGET } from '../domain/android.ts';
import type { WorkspacePaths } from '../domain/workspace.ts';
import { runBuildBundle } from './build-bundle.ts';
import type { CargoPort } from '../infrastructure/cargo.ts';

const ANDROID_SDK_CRATE = 'tela-android-sdk';

export interface BuildAndroidDeps {
  cargo: CargoPort;
  process: ProcessPort;
  fs: FsPort;
  reporter: Reporter;
  workspace: WorkspacePaths;
}

export interface BuildAndroidResult {
  ok: boolean;
}

/**
 * Builds the independent mobile guest first, then packages a Vulkan-only ARM64 GameActivity APK.
 *
 * The APK always receives the ADB-reverse localhost index. It never embeds the guest archive or
 * substitutes a cache when that URL cannot provide a valid current bundle.
 */
export async function runBuildAndroid(
  deps: BuildAndroidDeps,
): Promise<BuildAndroidResult> {
  const { cargo, process, fs, reporter, workspace } = deps;
  const bundleIndex = ANDROID_BUNDLE_INDEX_URL;

  const bundle = await runBuildBundle({ cargo, process, fs, reporter, workspace }, 'mobile');
  if (!bundle.ok) return bundle;

  reporter.section(`构建 Android GameActivity APK（${ANDROID_NDK_ABI} / Vulkan）`);
  try {
    // jniLibs 只接受本次 ARM64 构建产物；重建可避免旧 x86 library 被 Gradle 一并打包。
    await fs.resetDir(workspace.androidJniLibsDir());
  } catch (error) {
    reporter.fail('无法准备 Android JNI 输出目录');
    reporter.info(String(error));
    return { ok: false };
  }

  const native = await runExternal(
    process,
    'tela-android-cargo',
    [
      'build', '--target', ANDROID_RUST_TARGET, '--release', '-p', ANDROID_SDK_CRATE,
    ],
    workspace.root,
    reporter,
    `Android Rust 原生库构建失败（需要 ${ANDROID_RUST_TARGET} target 与 Windows Android NDK）`,
  );
  if (!native || native.code !== 0) return { ok: false };

  if (!(await fs.exists(workspace.androidRustNativeLibraryPath()))) {
    reporter.fail(`Cargo 未生成预期 ARM64 library: ${workspace.androidRustNativeLibraryPath()}`);
    return { ok: false };
  }
  try {
    await fs.ensureDir(workspace.androidJniAbiDir());
    await fs.copyFile(workspace.androidRustNativeLibraryPath(), workspace.androidNativeLibraryPath());
  } catch (error) {
    reporter.fail('无法把 ARM64 Rust library 发布到 Android JNI source set');
    reporter.info(String(error));
    return { ok: false };
  }

  const gradle = await runExternal(
    process,
    'tela-android-gradle',
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
