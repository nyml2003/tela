// 领域层：构建产物（wasm 工件）模型，无 I/O。

import type { BuildProfile } from './workspace.ts';

/** 一次构建任务的定义：源（cargo 产物）与目的地（demo 目录）。 */
export interface BuildJob {
  crate: string;
  profile: BuildProfile;
  /** 产物源路径。 */
  sourcePath: string;
  /** 发布目的地（demo/）。 */
  destPath: string;
}

/** 构建完成后工件信息（用于报告）。 */
export interface ArtifactInfo {
  sourcePath: string;
  destPath: string;
  bytes: number;
}
