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
  };
}

/** demo 演示二进制所属 crate。 */
export const DEMO_CRATE = 'tela-demo';
/** 演示页依赖的 CPU wasm 文件名。 */
export const DEMO_WASM = 'tela_demo.wasm';
