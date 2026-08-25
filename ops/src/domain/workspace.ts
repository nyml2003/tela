// 领域层：工作区与产品闭包模型（纯数据 + 路径推导，无 I/O）。
//
// `products/<id>` 是交付编排和原生输入的物理根；Rust crate 的目录层级不等于产品依赖
// 层级。这里集中声明产品选择，避免构建命令从默认环境或旧 demo crate 名推断目标。

import { ANDROID_NDK_ABI, ANDROID_RUST_TARGET } from './android.ts';

export type BuildProfile = 'dev' | 'release';

/** 独立发布的动态 guest 通道；通道不意味着共享业务视图。 */
export type BundleChannel = 'desktop' | 'mobile';

/** 六个显式产品闭包。`core` 是纯 Rust library 闭包，不是虚构的 GUI Target。 */
export type ProductId = 'core' | 'webview' | 'android' | 'ios' | 'win32' | 'win32-editor' | 'speed-gear' | 'macos';

export type DeliveryRoute = 'none' | 'dynamic-bundle' | 'static-link';

/** 一项产品所选择的完整责任链；只用于构建与治理，不可被应用反向依赖。 */
export interface ProductSpec {
  id: ProductId;
  /** `products/<id>` 物理根，可能只包含由 ops 管理的编排而无 Rust package。 */
  root: string;
  /** 选中的 application 或 product guest package；core 没有 UI application。 */
  application?: string;
  delivery: DeliveryRoute;
  /** 最终产品选中的 renderer；core 不带 renderer。 */
  renderer?: string;
  /** 最终产品选中的 Target Runtime；core 不带 Target。 */
  target?: string;
  /** 能代表该产品闭包的 Cargo package 根，用于显式 build/check 入口。 */
  packages: readonly string[];
}

/** 动态交付 guest 的发布路径与编译身份。 */
export interface BundlePaths {
  channel: BundleChannel;
  label: string;
  /** 由产品装配生成 ABI export 的 WASM crate，而不是业务 app crate。 */
  guestCrate: string;
  /** Guest root 自己拥有 ABI export，不需要给 application 打开 feature。 */
  guestFeatures: readonly string[];
  guestWasmArtifactPath(profile: BuildProfile): string;
  dir(): string;
  archivePath(): string;
  archiveTempPath(): string;
  indexPath(): string;
  indexTempPath(): string;
  archiveUrl: string;
  assetsDir(): string;
}

/** 工作区路径模型：全部路径从仓库根派生，纯函数计算，禁止魔法字符串散落。 */
export interface WorkspacePaths {
  root: string;
  cratesDir: string;
  productsDir: string;
  distDir: string;
  /** 浏览器产品源码根（TypeScript，构建到 dist/assets/tela-web）。 */
  webviewProductDir: string;
  product(id: ProductId): ProductSpec;
  bundle(channel: BundleChannel): BundlePaths;
  /** 浏览器 Target host 的 WASM 工件。 */
  webviewTargetArtifactPath(profile: BuildProfile): string;
  /** wasm-bindgen 生成的浏览器 host glue。 */
  webviewHostGluePath(): string;
  /** wasm-bindgen 生成的浏览器 host WASM。 */
  webviewHostWasmPath(): string;
  win32DistDir(): string;
  win32DistPath(): string;
  win32ArtifactPath(profile: BuildProfile): string;
  win32EditorDistDir(): string;
  win32EditorDistPath(): string;
  win32EditorArtifactPath(profile: BuildProfile): string;
  speedGearDistDir(): string;
  speedGearDistPath(): string;
  speedGearArtifactPath(profile: BuildProfile): string;
  speedGearHookDistPath(): string;
  speedGearHookArtifactPath(profile: BuildProfile): string;
  macosAppDir(): string;
  macosContentsDir(): string;
  macosExecutableDir(): string;
  macosInfoPlistPath(): string;
  macosExecutablePath(): string;
  macosInfoPlistSourcePath(): string;
  macosArtifactPath(profile: BuildProfile): string;
  androidProjectDir(): string;
  androidJniLibsDir(): string;
  androidJniAbiDir(): string;
  androidRustNativeLibraryPath(): string;
  androidNativeLibraryPath(): string;
  androidDebugApkPath(): string;
  androidDistDir(): string;
  androidDistPath(): string;
  iosProjectDir(): string;
  iosXcodeProjectPath(): string;
  iosStaticLibraryDir(): string;
  iosRustStaticLibraryPath(profile: BuildProfile): string;
  iosXcodeStaticLibraryPath(): string;
  iosDerivedDataDir(): string;
  iosAppPath(profile: BuildProfile): string;
}

export const CORE_PRODUCT_PACKAGES: readonly string[] = [
  'tela-contract',
  'tela-core',
  'tela-ui-foundation',
];
export const DESKTOP_GUEST_CRATE = 'tela-product-desktop-guest';
export const MOBILE_GUEST_CRATE = 'tela-product-mobile-guest';
export const WEBVIEW_TARGET_CRATE = 'tela-target-webview';
export const ANDROID_TARGET_CRATE = 'tela-target-android';
export const IOS_PRODUCT_CRATE = 'tela-product-ios';
export const WIN32_TARGET_CRATE = 'tela-target-win32';
export const WIN32_EDITOR_CRATE = 'tela-product-win32-editor';
export const SPEED_GEAR_CRATE = 'tela-product-speed-gear';
export const SPEED_GEAR_HOOK_CRATE = 'tela-speed-gear-hook';
export const MACOS_TARGET_CRATE = 'tela-target-macos';

/** 根据仓库根构造路径和产品闭包模型。 */
export function resolveWorkspace(root: string): WorkspacePaths {
  const cratesDir = `${root}/crates`;
  const productsDir = `${root}/products`;
  const distDir = `${root}/dist`;
  const productRoot = (id: ProductId): string => `${productsDir}/${id}`;
  const webviewProductDir = productRoot('webview');

  const products: Record<ProductId, ProductSpec> = {
    core: {
      id: 'core',
      root: productRoot('core'),
      delivery: 'none',
      packages: CORE_PRODUCT_PACKAGES,
    },
    webview: {
      id: 'webview',
      root: webviewProductDir,
      application: DESKTOP_GUEST_CRATE,
      delivery: 'dynamic-bundle',
      renderer: 'tela-render-wgpu',
      target: WEBVIEW_TARGET_CRATE,
      packages: [DESKTOP_GUEST_CRATE, WEBVIEW_TARGET_CRATE],
    },
    android: {
      id: 'android',
      root: productRoot('android'),
      application: MOBILE_GUEST_CRATE,
      delivery: 'dynamic-bundle',
      renderer: 'tela-render-wgpu',
      target: ANDROID_TARGET_CRATE,
      packages: [MOBILE_GUEST_CRATE, ANDROID_TARGET_CRATE],
    },
    ios: {
      id: 'ios',
      root: productRoot('ios'),
      application: IOS_PRODUCT_CRATE,
      delivery: 'static-link',
      renderer: 'tela-render-wgpu',
      target: 'tela-target-ios',
      packages: [IOS_PRODUCT_CRATE],
    },
    win32: {
      id: 'win32',
      root: productRoot('win32'),
      application: DESKTOP_GUEST_CRATE,
      delivery: 'dynamic-bundle',
      renderer: 'tela-render-wgpu',
      target: WIN32_TARGET_CRATE,
      packages: [DESKTOP_GUEST_CRATE, WIN32_TARGET_CRATE],
    },
    'win32-editor': {
      id: 'win32-editor',
      root: productRoot('win32-editor'),
      application: 'tela-win32-editor',
      delivery: 'static-link',
      target: WIN32_TARGET_CRATE,
      packages: [WIN32_EDITOR_CRATE],
    },
    'speed-gear': {
      id: 'speed-gear',
      root: productRoot('speed-gear'),
      application: 'tela-speed-gear',
      delivery: 'static-link',
      target: WIN32_TARGET_CRATE,
      packages: [SPEED_GEAR_CRATE],
    },
    macos: {
      id: 'macos',
      root: productRoot('macos'),
      application: DESKTOP_GUEST_CRATE,
      delivery: 'dynamic-bundle',
      renderer: 'tela-render-wgpu',
      target: MACOS_TARGET_CRATE,
      packages: [DESKTOP_GUEST_CRATE, MACOS_TARGET_CRATE],
    },
  };

  const bundle = (channel: BundleChannel): BundlePaths => {
    const mobile = channel === 'mobile';
    const directory = mobile ? `${distDir}/tela-mobile` : `${distDir}/tela-dev`;
    const archiveName = mobile ? 'tela-mobile-guest.tela' : 'tela-desktop-guest.tela';
    const guestName = mobile ? 'tela_product_mobile_guest' : 'tela_product_desktop_guest';
    return {
      channel,
      label: mobile ? 'Android mobile product guest' : 'desktop product guest',
      guestCrate: mobile ? MOBILE_GUEST_CRATE : DESKTOP_GUEST_CRATE,
      guestFeatures: [],
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
    productsDir,
    distDir,
    webviewProductDir,
    product(id) {
      return products[id];
    },
    bundle,
    webviewTargetArtifactPath(profile) {
      const dir = profile === 'release' ? 'release' : 'debug';
      return `${root}/target/wasm32-unknown-unknown/${dir}/tela_target_webview.wasm`;
    },
    webviewHostGluePath() {
      return `${distDir}/tela_webview_host.js`;
    },
    webviewHostWasmPath() {
      return `${distDir}/tela_webview_host_bg.wasm`;
    },
    win32DistDir() {
      return `${distDir}/win32`;
    },
    win32DistPath() {
      return `${distDir}/win32/tela-win32-host.exe`;
    },
    win32ArtifactPath(profile) {
      const dir = profile === 'release' ? 'release' : 'debug';
      return `${root}/target/x86_64-pc-windows-gnu/${dir}/tela-win32-host.exe`;
    },
    win32EditorDistDir() {
      return `${distDir}/win32-editor`;
    },
    win32EditorDistPath() {
      return `${distDir}/win32-editor/tela-win32-editor-host.exe`;
    },
    win32EditorArtifactPath(profile) {
      const dir = profile === 'release' ? 'release' : 'debug';
      return `${root}/target/x86_64-pc-windows-gnu/${dir}/tela-win32-editor-host.exe`;
    },
    speedGearDistDir() {
      return `${distDir}/speed-gear`;
    },
    speedGearDistPath() {
      return `${distDir}/speed-gear/tela-speed-gear-host.exe`;
    },
    speedGearArtifactPath(profile) {
      const dir = profile === 'release' ? 'release' : 'debug';
      return `${root}/target/x86_64-pc-windows-gnu/${dir}/tela-speed-gear-host.exe`;
    },
    speedGearHookDistPath() {
      return `${distDir}/speed-gear/tela-speed-gear-hook.dll`;
    },
    speedGearHookArtifactPath(profile) {
      const dir = profile === 'release' ? 'release' : 'debug';
      return `${root}/target/x86_64-pc-windows-gnu/${dir}/tela_speed_gear_hook.dll`;
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
      return `${distDir}/macos/Tela.app/Contents/MacOS/tela-macos-host`;
    },
    macosInfoPlistSourcePath() {
      return `${productRoot('macos')}/resources/Info.plist`;
    },
    macosArtifactPath(profile) {
      const dir = profile === 'release' ? 'release' : 'debug';
      return `${root}/target/aarch64-apple-darwin/${dir}/tela-macos-host`;
    },
    androidProjectDir() {
      return productRoot('android');
    },
    androidJniLibsDir() {
      return `${productRoot('android')}/app/src/main/jniLibs`;
    },
    androidJniAbiDir() {
      return `${productRoot('android')}/app/src/main/jniLibs/${ANDROID_NDK_ABI}`;
    },
    androidRustNativeLibraryPath() {
      return `${root}/target/${ANDROID_RUST_TARGET}/release/libmain.so`;
    },
    androidNativeLibraryPath() {
      return `${productRoot('android')}/app/src/main/jniLibs/${ANDROID_NDK_ABI}/libmain.so`;
    },
    androidDebugApkPath() {
      return `${productRoot('android')}/app/build/outputs/apk/debug/app-debug.apk`;
    },
    androidDistDir() {
      return `${distDir}/android`;
    },
    androidDistPath() {
      return `${distDir}/android/tela-mobile-debug.apk`;
    },
    iosProjectDir() {
      return productRoot('ios');
    },
    iosXcodeProjectPath() {
      return `${productRoot('ios')}/TelaMobile.xcodeproj`;
    },
    iosStaticLibraryDir() {
      return `${productRoot('ios')}/build/rust`;
    },
    iosRustStaticLibraryPath(profile) {
      const dir = profile === 'release' ? 'release' : 'debug';
      return `${root}/target/aarch64-apple-ios/${dir}/libtela_product_ios.a`;
    },
    iosXcodeStaticLibraryPath() {
      return `${productRoot('ios')}/build/rust/libtela_product_ios.a`;
    },
    iosDerivedDataDir() {
      return `${productRoot('ios')}/build/DerivedData`;
    },
    iosAppPath(profile) {
      const configuration = profile === 'release' ? 'Release' : 'Debug';
      return `${productRoot('ios')}/build/DerivedData/Build/Products/${configuration}-iphoneos/TelaMobile.app`;
    },
  };
}
