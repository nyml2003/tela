// 领域层：浏览器 WebGPU 环境验证的遥测模型。

/** 浏览器 → CLI 的上报事件。 */
export type TelemetryEvent =
  | {
      type: 'console-error';
      ts: number;
      message: string;
      stack?: string;
      url?: string;
      line?: number;
      col?: number;
    }
  | {
      type: 'console-warn';
      ts: number;
      message: string;
    }
  | {
      type: 'unhandled-rejection';
      ts: number;
      message: string;
      stack?: string;
    }
  | {
      type: 'backend';
      ts: number;
      /** 后端描述（含 GPU 探针结果，如 "WebGPU（wgpu）· 探针 RGB(200,0,0) ✓"）。 */
      backend: string;
    }
  | {
      type: 'gpu-probe';
      ts: number;
      /** 探针中心像素 (r,g,b)。三态：红=渲染器正常 / 绿=pass 执行但 draw 无输出 / 黑=pass 未执行。 */
      rgb: [number, number, number];
      /** 探针来源：tela = wgpu 渲染器探针；rawgpu = 原生 JS WebGPU 自检（环境验证）。 */
      source?: 'tela' | 'rawgpu';
    }
  | {
      type: 'frame';
      ts: number;
      pressure: number;
      logicFps: number;
      rasterMs: number;
      frames: number;
    }
  | {
      type: 'log';
      ts: number;
      message: string;
    };

/** CLI → 浏览器的命令（经 SSE /api/events 下发）。 */
export type CliCommand =
  | { type: 'set-pressure'; level: number }
  | { type: 'probe' }
  | { type: 'reset-stats' }
  | { type: 'ping' };

/** 遥测环形缓冲容量（内存上限）。 */
export const TELEMETRY_CAPACITY = 500;

/** 探针三态结论（渲染器健康度诊断）。 */
export type ProbeVerdict = 'ok' | 'draw-empty' | 'pass-not-run' | 'unknown';

/** 根据探针中心像素判定渲染器状态（红=正常，绿=draw 无输出，黑=pass 未执行）。 */
export function probeVerdict(rgb: [number, number, number]): ProbeVerdict {
  const [r, g, b] = rgb;
  if (r > 150 && g < 80 && b < 80) return 'ok';
  if (g > 150 && r < 80 && b < 80) return 'draw-empty';
  if (r < 80 && g < 80 && b < 80) return 'pass-not-run';
  return 'unknown';
}

/** 探针三态的中文描述。 */
export function probeVerdictText(verdict: ProbeVerdict): string {
  switch (verdict) {
    case 'ok':
      return '渲染器正常';
    case 'draw-empty':
      return 'pass 执行但 draw 无输出（渲染器批次/绘制 bug）';
    case 'pass-not-run':
      return 'pass 未执行（渲染提交链路问题）';
    case 'unknown':
      return '探针结果异常';
  }
}

/** 诊断汇总数据。 */
export interface DebugSummary {
  backend?: string;
  probe?: { rgb: [number, number, number]; verdict: ProbeVerdict };
  consoleErrors: number;
  consoleWarns: number;
  frames: number;
  avgLogicFps: number;
  avgRasterMs: number;
}
