// 领域层：验证门模型（check 命令的四道门 + 冒烟门），无 I/O。

export type GateId = 'fmt' | 'clippy' | 'test' | 'arch' | 'smoke' | 'build';

export interface GateSpec {
  id: GateId;
  label: string;
}

/** check 命令的验证门序列（顺序即执行顺序）。 */
export const CHECK_GATES: readonly GateSpec[] = [
  { id: 'fmt', label: 'fmt --check' },
  { id: 'clippy', label: 'clippy --all-targets -D warnings' },
  { id: 'test', label: 'cargo test' },
  { id: 'arch', label: '依赖方向检查' },
];

export interface GateResult {
  id: GateId;
  passed: boolean;
  durationMs: number;
  /** 失败时的命令输出摘要。 */
  detail?: string;
}
