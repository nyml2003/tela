// 领域层：工作区模型（纯数据 + 路径推导，无 I/O）。
// tela 仓库布局约定：源码位于 crates/、web/、ops/；dist/ 只存放可删除的构建产物。

export type BuildProfile = 'dev' | 'release';

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
  /** 演示 wasm 工件目标路径（构建输出）。 */
  wasmArtifactPath(profile: BuildProfile): string;
  /** 演示 wasm 发布位置（dist 目录内）。 */
  wasmDistPath(): string;
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
}

/** 根据仓库根构造路径模型（纯函数）。 */
export function resolveWorkspace(root: string): WorkspacePaths {
  const cratesDir = `${root}/crates`;
  const distDir = `${root}/dist`;
  const webDir = `${root}/web`;
  return {
    root,
    cratesDir,
    distDir,
    webDir,
    wasmArtifactPath(profile) {
      const dir = profile === 'release' ? 'release' : 'debug';
      return `${root}/target/wasm32-unknown-unknown/${dir}/tela_demo.wasm`;
    },
    wasmDistPath() {
      return `${distDir}/tela_demo.wasm`;
    },
    bundleDir() {
      return `${distDir}/tela-dev`;
    },
    bundleArchivePath() {
      return `${distDir}/tela-dev/tela-demo.tela`;
    },
    bundleArchiveTempPath() {
      return `${distDir}/tela-dev/tela-demo.tela.tmp`;
    },
    bundleIndexPath() {
      return `${distDir}/tela-dev/latest.json`;
    },
    bundleIndexTempPath() {
      return `${distDir}/tela-dev/latest.json.tmp`;
    },
    bundleAssetsDir() {
      return `${root}/assets`;
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
  };
}

/** demo 演示二进制所属 crate。 */
export const DEMO_CRATE = 'tela-demo';
/** 演示页依赖的 CPU wasm 文件名。 */
export const DEMO_WASM = 'tela_demo.wasm';
