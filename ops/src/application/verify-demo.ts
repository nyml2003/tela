// 应用层：verify demo 用例——验证 dist/ 内发布的 CPU wasm。
// 确保 wasm 已发布后，跑通最小共享场景的 CPU 渲染闭环。
import type { FsPort, Reporter, WasmSmokePort } from '../domain/ports.ts';
import type { WorkspacePaths } from '../domain/workspace.ts';

export interface VerifyDemoDeps {
  fs: FsPort;
  smoke: WasmSmokePort;
  reporter: Reporter;
  workspace: WorkspacePaths;
}

export interface VerifyDemoResult {
  ok: boolean;
  detail?: string;
}

/** 冒烟验证：Node 加载 dist wasm，验证完整客户端首帧。 */
export async function runVerifyDemo(deps: VerifyDemoDeps): Promise<VerifyDemoResult> {
  const { fs, smoke, reporter, workspace } = deps;
  const wasm = workspace.wasmDistPath();

  reporter.section('冒烟验证（verify demo）');
  if (!(await fs.exists(wasm))) {
    reporter.fail(`缺少 ${wasm}，请先运行: ops build demo`);
    return { ok: false, detail: 'wasm 未构建' };
  }

  const result = await smoke.verify(wasm);
  if (result.ok) {
    reporter.ok(`发布 wasm 冒烟通过（${result.detail}）`);
    return { ok: true };
  }
  reporter.fail('发布 wasm 冒烟失败');
  reporter.info(result.detail);
  return { ok: false, detail: result.detail };
}
