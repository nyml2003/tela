// 应用层：verify gpu 用例——原生 JS WebGPU 环境自检（不经 wgpu）。
// 起服务 → 输出自检页 URL（用户打开）→ 页面自动离屏三角形 + 回读上报 →
// CLI 输出 adapter 信息 + 三角形像素结论（区分"环境问题"与"wgpu 层问题"）。
import type { ProcessPort, Reporter, ServerPort } from '../domain/ports.ts';
import type { WorkspacePaths } from '../domain/workspace.ts';
import type { TelemetryEvent } from '../domain/telemetry.ts';
import { probeVerdict, probeVerdictText } from '../domain/telemetry.ts';
import type { TelemetryStore } from '../infrastructure/telemetry-store.ts';
import { runBuildFrontend } from './build-frontend.ts';

export interface VerifyGpuDeps {
  process: ProcessPort;
  server: ServerPort;
  telemetry: TelemetryStore;
  reporter: Reporter;
  workspace: WorkspacePaths;
}

export interface VerifyGpuOptions {
  preferredPort: number;
  /** 等待上报超时（毫秒）。 */
  timeoutMs: number;
}

/** 运行原生 JS 自检会话：等 rawgpu 上报（gpu-probe source=rawgpu）后输出结论。 */
export async function runVerifyGpu(deps: VerifyGpuDeps, opts: VerifyGpuOptions): Promise<boolean> {
  const { process, server, telemetry, reporter, workspace } = deps;
  reporter.section('GPU 环境自检（verify gpu）');
  const frontend = await runBuildFrontend({ process, reporter, workspace });
  if (!frontend.ok) return false;
  const result = await server.serve(
    workspace.distDir,
    opts.preferredPort,
    (msg) => reporter.info(msg),
    telemetry,
  );
  const url = `http://127.0.0.1:${result.port}/rawgpu.html`;
  reporter.ok(`监听 0.0.0.0:${result.port}`);
  reporter.ok(`自检页（打开即自动运行，无需操作）:`);
  reporter.ok(url);
  reporter.info('等待页面自检上报（adapter 信息 + 三角形像素回读）…');

  // 等 rawgpu 的 gpu-probe 上报。
  const t0 = Date.now();
  let adapterLine: string | undefined;
  while (Date.now() - t0 < opts.timeoutMs) {
    const events = telemetry.snapshot();
    for (const e of events) {
      if (e.type === 'log' && e.message.includes('[tela-rawgpu] adapter:')) {
        adapterLine = e.message.replace('[tela-rawgpu] ', '');
      }
      if (e.type === 'gpu-probe' && e.source === 'rawgpu') {
        const [r, g, b] = e.rgb;
        const verdict = probeVerdict(e.rgb);
        reporter.section('自检结果');
        if (adapterLine) reporter.info(adapterLine);
        reporter.info(`离屏三角形中心 RGB=(${r},${g},${b})`);
        if (verdict === 'ok') {
          reporter.ok('环境正常：原生 WebGPU 能渲染 → 问题在 wgpu 层（tela 渲染路径）');
          reporter.ok('修复方向：渲染路径直调 WebGPU JS API（绕开 wgpu 后端），或换环境/浏览器');
        } else {
          reporter.fail(`环境异常：原生 WebGPU 也无法渲染（${probeVerdictText(verdict)}）`);
          reporter.fail('修复方向：浏览器/GPU/驱动问题——换浏览器或开 --enable-unsafe-swiftshader');
        }
        await result.close();
        telemetry.dispose();
        return verdict === 'ok';
      }
    }
    await new Promise((r) => setTimeout(r, 200));
  }
  reporter.fail(`超时（${opts.timeoutMs / 1000}s）：未收到自检上报——请确认已打开自检页 ${url}`);
  await result.close();
  telemetry.dispose();
  return false;
}

export type { TelemetryEvent };
