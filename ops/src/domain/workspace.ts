// 领域层：工作区模型（纯数据 + 路径推导，无 I/O）。
// tela 仓库布局约定：源码位于 crates/、web/、ops/；dist/ 只存放可删除的构建产物。

import { ANDROID_NDK_ABI, ANDROID_RUST_TARGET } from './android.ts';

export type BuildProfile = 'dev' | 'release';

/** A separately published guest channel. Channels do not imply a shared application UI. */
export type BundleChannel = 'desktop' | 'mobile';

/** Paths and build identity of one dynamically delivered guest. */
export interface BundlePaths {
  /** Delivery channel identifier used by the CLI. */
  channel: BundleChannel;
  /** Human-readable target label used in build diagnostics. */
  label: string;
  /** Guest crate compiled into this channel's WASM artifact. */
  guestCrate: string;
  /** Features required for its typed Guest ABI exports. */
  guestFeatures: readonly string[];
  /** WASM artifact generated for the selected profile. */
  guestWasmArtifactPath(profile: BuildProfile): string;
  /** Directory served as this channel's remote bundle root. */
  dir(): string;
  /** Published `.tela` archive. */
  archivePath(): string;
  /** Temporary archive written before the index is atomically replaced. */
  archiveTempPath(): string;
  /** Published development manifest. */
  indexPath(): string;
  /** Temporary manifest written before publication. */
  indexTempPath(): string;
  /** URL stored inside the development manifest, resolved relative to its index. */
  archiveUrl: string;
  /** Optional static assets included in this channel's archive. */
  assetsDir(): string;
}

/** 工作区路径模型：全部路径从仓库根派生，纯函数计算，禁止魔法字符串散落。 */
export interface WorkspacePaths {
  /** 仓库根目录。 */
  root: string;
  /** crates 目录。 */
  cratesDir: string;
  /** 静态发布目录（index.html / wasm / 前端 bundle；始终由构建生成）。 */
  distDir: string;
  /** web 前端源码目录（TypeScript，esbuild 构建到 dist/assets/tela-web）。 */
  webDir: string;
  /** One independently published dynamic guest channel. */
  bundle(channel: BundleChannel): BundlePaths;
  /** 应用 guest wasm 工件目标路径（构建输出）。 */
  appGuestWasmArtifactPath(profile: BuildProfile): string;
  /** 浏览器 WebView SDK wasm 工件目标路径。 */
  webviewSdkArtifactPath(profile: BuildProfile): string;
  /** wasm-bindgen 生成的浏览器 WebView SDK glue。 */
  webviewSdkGluePath(): string;
  /** wasm-bindgen 生成的浏览器 WebView SDK 背景 wasm。 */
  webviewSdkWasmPath(): string;
  /** 开发期平台 SDK 请求的 bundle 目录。 */
  bundleDir(): string;
  /** 开发期平台 SDK 请求的压缩 bundle。 */
  bundleArchivePath(): string;
  /** bundle 生成期间使用的临时压缩包路径。 */
  bundleArchiveTempPath(): string;
  /** SDK 在启动时首先请求的开发索引。 */
  bundleIndexPath(): string;
  /** bundle 生成期间使用的临时索引路径。 */
  bundleIndexTempPath(): string;
  /** 可选的 SDK 静态资源根目录。 */
  bundleAssetsDir(): string;
  /** Win32 开发壳的发布目录。 */
  win32DistDir(): string;
  /** Win32 开发壳发布的可执行文件。 */
  win32DistPath(): string;
  /** Win32 GNU target 的二进制工件位置。 */
  win32ArtifactPath(profile: BuildProfile): string;
  /** macOS App bundle 的根目录。 */
  macosAppDir(): string;
  /** macOS App bundle 的 Contents 目录。 */
  macosContentsDir(): string;
  /** macOS App bundle 的可执行文件目录。 */
  macosExecutableDir(): string;
  /** macOS App bundle 内的 Info.plist 位置。 */
  macosInfoPlistPath(): string;
  /** macOS App bundle 内的原生壳可执行文件位置。 */
  macosExecutablePath(): string;
  /** macOS App 的可编辑 Info.plist 源文件。 */
  macosInfoPlistSourcePath(): string;
  /** Apple Silicon macOS target 的二进制工件位置。 */
  macosArtifactPath(profile: BuildProfile): string;
  /** Android Gradle project root. */
  androidProjectDir(): string;
  /** ARM64 JNI library directory consumed by the Gradle source set. */
  androidJniLibsDir(): string;
  /** ARM64 ABI subdirectory inside the Gradle JNI source set. */
  androidJniAbiDir(): string;
  /** Rust cross-compile output before it is copied into the Gradle source set. */
  androidRustNativeLibraryPath(): string;
  /** Expected ARM64 native library packaged by Gradle. */
  androidNativeLibraryPath(): string;
  /** Gradle debug APK output. */
  androidDebugApkPath(): string;
  /** Final Android release directory under dist/. */
  androidDistDir(): string;
  /** Published debug APK path. */
  androidDistPath(): string;
  /** iPhone Xcode project root. */
  iosProjectDir(): string;
  /** iPhone Xcode project consumed by xcodebuild. */
  iosXcodeProjectPath(): string;
  /** Generated Rust static-library staging directory referenced by Xcode. */
  iosStaticLibraryDir(): string;
  /** Rust ARM64 iPhone static library before it is staged for Xcode. */
  iosRustStaticLibraryPath(profile: BuildProfile): string;
  /** Static library path referenced by the checked-in Xcode project. */
  iosXcodeStaticLibraryPath(): string;
  /** Per-project Xcode DerivedData location, never committed. */
  iosDerivedDataDir(): string;
  /** Device `.app` produced by the selected Xcode configuration. */
  iosAppPath(profile: BuildProfile): string;
}

/** 根据仓库根构造路径模型（纯函数）。 */
export function resolveWorkspace(root: string): WorkspacePaths {
  const cratesDir = `${root}/crates`;
  const distDir = `${root}/dist`;
  const webDir = `${root}/web`;
  const bundle = (channel: BundleChannel): BundlePaths => {
    const mobile = channel === 'mobile';
    const directory = mobile ? `${distDir}/tela-mobile` : `${distDir}/tela-dev`;
    const archiveName = mobile ? 'tela-mobile-demo.tela' : 'tela-demo.tela';
    const guestName = mobile ? 'tela_mobile_demo' : 'tela_demo';
    return {
      channel,
      label: mobile ? 'Android mobile' : 'desktop platform SDK',
      guestCrate: mobile ? MOBILE_DEMO_CRATE : DEMO_CRATE,
      guestFeatures: ['app-wasm'],
      guestWasmArtifactPath(profile) {
        const profileDir = profile === 'release' ? 'release' : 'debug';
        return `${root}/target/wasm32-unknown-unknown/${profileDir}/${guestName}.wasm`;
      },
      dir() {
        return directory;
      },
      archivePath() {
        return `${directory}/${archiveName}`;
      },
      archiveTempPath() {
        return `${directory}/${archiveName}.tmp`;
      },
      indexPath() {
        return `${directory}/latest.json`;
      },
      indexTempPath() {
        return `${directory}/latest.json.tmp`;
      },
      archiveUrl: `/${mobile ? 'tela-mobile' : 'tela-dev'}/${archiveName}`,
      assetsDir() {
        return mobile ? `${root}/assets/mobile` : `${root}/assets`;
      },
    };
  };
  return {
    root,
    cratesDir,
    distDir,
    webDir,
    bundle,
    appGuestWasmArtifactPath(profile) {
      return bundle('desktop').guestWasmArtifactPath(profile);
    },
    webviewSdkArtifactPath(profile) {
      const dir = profile === 'release' ? 'release' : 'debug';
      return `${root}/target/wasm32-unknown-unknown/${dir}/tela_webview_sdk.wasm`;
    },
    webviewSdkGluePath() {
      return `${distDir}/tela_webview_sdk.js`;
    },
    webviewSdkWasmPath() {
      return `${distDir}/tela_webview_sdk_bg.wasm`;
    },
    bundleDir() {
      return bundle('desktop').dir();
    },
    bundleArchivePath() {
      return bundle('desktop').archivePath();
    },
    bundleArchiveTempPath() {
      return bundle('desktop').archiveTempPath();
    },
    bundleIndexPath() {
      return bundle('desktop').indexPath();
    },
    bundleIndexTempPath() {
      return bundle('desktop').indexTempPath();
    },
    bundleAssetsDir() {
      return bundle('desktop').assetsDir();
    },
    win32DistDir() {
      return `${distDir}/win32`;
    },
    win32DistPath() {
      return `${distDir}/win32/tela-win32-sdk.exe`;
    },
    win32ArtifactPath(profile) {
      const dir = profile === 'release' ? 'release' : 'debug';
      return `${root}/target/x86_64-pc-windows-gnu/${dir}/tela-win32-sdk.exe`;
    },
    macosAppDir() {
      return `${distDir}/macos/Tela.app`;
    },
    macosContentsDir() {
      return `${distDir}/macos/Tela.app/Contents`;
    },
    macosExecutableDir() {
      return `${distDir}/macos/Tela.app/Contents/MacOS`;
    },
    macosInfoPlistPath() {
      return `${distDir}/macos/Tela.app/Contents/Info.plist`;
    },
    macosExecutablePath() {
      return `${distDir}/macos/Tela.app/Contents/MacOS/tela-macos-sdk`;
    },
    macosInfoPlistSourcePath() {
      return `${cratesDir}/tela-macos-sdk/resources/Info.plist`;
    },
    macosArtifactPath(profile) {
      const dir = profile === 'release' ? 'release' : 'debug';
      return `${root}/target/aarch64-apple-darwin/${dir}/tela-macos-sdk`;
    },
    androidProjectDir() {
      return `${root}/android`;
    },
    androidJniLibsDir() {
      return `${root}/android/app/src/main/jniLibs`;
    },
    androidJniAbiDir() {
      return `${root}/android/app/src/main/jniLibs/${ANDROID_NDK_ABI}`;
    },
    androidRustNativeLibraryPath() {
      return `${root}/target/${ANDROID_RUST_TARGET}/release/libmain.so`;
    },
    androidNativeLibraryPath() {
      return `${root}/android/app/src/main/jniLibs/${ANDROID_NDK_ABI}/libmain.so`;
    },
    androidDebugApkPath() {
      return `${root}/android/app/build/outputs/apk/debug/app-debug.apk`;
    },
    androidDistDir() {
      return `${distDir}/android`;
    },
    androidDistPath() {
      return `${distDir}/android/tela-mobile-debug.apk`;
    },
    iosProjectDir() {
      return `${root}/ios`;
    },
    iosXcodeProjectPath() {
      return `${root}/ios/TelaMobile.xcodeproj`;
    },
    iosStaticLibraryDir() {
      return `${root}/ios/build/rust`;
    },
    iosRustStaticLibraryPath(profile) {
      const dir = profile === 'release' ? 'release' : 'debug';
      return `${root}/target/aarch64-apple-ios/${dir}/libtela_ios_sdk.a`;
    },
    iosXcodeStaticLibraryPath() {
      return `${root}/ios/build/rust/libtela_ios_sdk.a`;
    },
    iosDerivedDataDir() {
      return `${root}/ios/build/DerivedData`;
    },
    iosAppPath(profile) {
      const configuration = profile === 'release' ? 'Release' : 'Debug';
      return `${root}/ios/build/DerivedData/Build/Products/${configuration}-iphoneos/TelaMobile.app`;
    },
  };
}

/** demo 演示二进制所属 crate。 */
export const DEMO_CRATE = 'tela-demo';
/** First-party mobile Guest crate; it deliberately has an independent domain and presentation. */
export const MOBILE_DEMO_CRATE = 'tela-mobile-demo';
/** 浏览器 WebView 壳所属 crate。 */
export const WEBVIEW_SDK_CRATE = 'tela-webview-sdk';
