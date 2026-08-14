// 基础设施层：Node 读取发布 wasm，执行最小 CPU 渲染闭环。
import { readFile } from 'node:fs/promises';
import type { WasmSmokePort, WasmSmokeResult } from '../domain/ports.ts';

interface DemoWasmExports {
  demo_set_viewport(width: number, height: number): number;
  demo_set_raster_dpi(dpi: number): void;
  demo_frame_size(): number;
  demo_tick(): number;
  demo_frame_ptr(): number;
  demo_frame_trace_ptr(): number;
  demo_frame_trace_len(): number;
  memory: { buffer: ArrayBuffer };
}

const REQUIRED_TRACE_LABELS = ['TELA 文件', '新建', 'README.md', '工作区已准备就绪'];

function assert(condition: boolean, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

/** 从 dist/ 加载 CPU wasm，验证首帧、全视口尺寸、frame trace 与壳层像素分区。 */
export class NodeWasmSmokePort implements WasmSmokePort {
  async verify(path: string): Promise<WasmSmokeResult> {
    try {
      const bytes = await readFile(path);
      const { instance } = await WebAssembly.instantiate(bytes, {
        env: { tela_now: () => Date.now() },
      });
      const wasm = instance.exports as unknown as DemoWasmExports;
      wasm.demo_set_viewport(1366, 768);
      wasm.demo_set_raster_dpi(1);

      const size = wasm.demo_frame_size();
      const width = size & 0xffff;
      const height = size >>> 16;
      assert(wasm.demo_tick() === 1, '首帧必须提交');
      assert(wasm.demo_tick() === 0, '静态场景不应重复提交');
      assert(width === 1366 && height === 768, `逻辑画布尺寸应为 1366x768，实际 ${width}x${height}`);

      const pixels = new Uint8Array(wasm.memory.buffer);
      const base = wasm.demo_frame_ptr();
      const pixel = (x: number, y: number): readonly [number, number, number, number] => {
        const offset = base + (y * width + x) * 4;
        return [pixels[offset] ?? 0, pixels[offset + 1] ?? 0, pixels[offset + 2] ?? 0, pixels[offset + 3] ?? 0];
      };
      const trace = new TextDecoder().decode(new Uint8Array(
        wasm.memory.buffer,
        wasm.demo_frame_trace_ptr(),
        wasm.demo_frame_trace_len(),
      ));
      for (const label of REQUIRED_TRACE_LABELS) {
        assert(trace.includes(label), `首帧缺少 ${label}`);
      }
      const header = pixel(4, 4);
      const footer = pixel(4, height - 4);
      assert(
        header[3] === 255 && footer[3] === 255 && header.join() !== footer.join(),
        `客户端顶栏/状态栏像素未分区：header=${header} footer=${footer}`,
      );
      return { ok: true, detail: `${width}x${height}, client-shell/frame-trace` };
    } catch (error: unknown) {
      return { ok: false, detail: error instanceof Error ? error.message : String(error) };
    }
  }
}
