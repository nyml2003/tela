// 应用层：verify demo 用例——冒烟测试（node demo/smoke.cjs）。
// 确保 wasm 已发布（未发布可先构建），跑通最小共享场景的 CPU 渲染闭环。
import type { FsPort, ProcessPort, Reporter } from '../domain/ports.ts';
import type { WorkspacePaths } from '../domain/workspace.ts';

export interface VerifyDemoDeps {
  process: ProcessPort;
  fs: FsPort;
  reporter: Reporter;
  workspace: WorkspacePaths;
}

export interface VerifyDemoResult {
  ok: boolean;
  detail?: string;
}

/** 冒烟验证：Node 加载 demo wasm，走指针/键盘/模态/滚动闭环。 */
export async function runVerifyDemo(
  deps: VerifyDemoDeps,
  opts: { autoBuild: boolean },
): Promise<VerifyDemoResult> {
  const { process, fs, reporter, workspace } = deps;
  const wasm = workspace.wasmDemoPath();

  reporter.section('冒烟验证（verify demo）');
  if (!(await fs.exists(wasm))) {
    reporter.fail(`缺少 ${wasm}，请先运行: ops build demo`);
    if (opts.autoBuild) {
      reporter.info('--build 已指定，先构建…');
      // 依赖注入避免循环：由 cli 层决定是否先构建（此处仅提示）。
    }
    return { ok: false, detail: 'wasm 未构建' };
  }

  const t0 = performance.now();
  const res = await process.run('node', ['demo/smoke.cjs'], { cwd: workspace.root });
  const durationMs = performance.now() - t0;
  if (res.code === 0) {
    reporter.ok(`demo/smoke.cjs 通过 (${durationMs.toFixed(0)}ms)`);
    return { ok: true };
  }
  reporter.fail('demo/smoke.cjs 失败');
  reporter.info(res.stdout.slice(-2000));
  reporter.info(res.stderr.slice(-2000));
  return { ok: false, detail: res.stderr || res.stdout };
}
