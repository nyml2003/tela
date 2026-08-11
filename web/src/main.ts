// 最小浏览器宿主：选择 CPU/WGPU 后端，驱动同一 tela 场景帧。

type BackendMode = 'auto' | 'raster' | 'wgpu';

interface GpuGlue {
  default(options?: { module_or_path?: string }): Promise<unknown>;
  start_gpu(canvas: HTMLCanvasElement): Promise<void>;
  tick_gpu(): number;
  frame_trace(): string;
  gpu_diagnostics(): string;
  gpu_probe(): Promise<number>;
}

interface CpuExports {
  demo_tick(): number;
  demo_frame_size(): number;
  demo_frame_ptr(): number;
  demo_frame_trace_ptr(): number;
  demo_frame_trace_len(): number;
  memory: WebAssembly.Memory;
}

function modeFromUrl(): BackendMode {
  const value = new URLSearchParams(location.search).get('backend');
  return value === 'raster' || value === 'wgpu' ? value : 'auto';
}

function setCanvasSize(canvas: HTMLCanvasElement, packed: number): void {
  canvas.width = packed & 0xffff;
  canvas.height = packed >>> 16;
}

function presentRaster(canvas: HTMLCanvasElement, wasm: CpuExports): void {
  const packed = wasm.demo_frame_size();
  const width = packed & 0xffff;
  const height = packed >>> 16;
  const pixels = new Uint8ClampedArray(wasm.memory.buffer, wasm.demo_frame_ptr(), width * height * 4);
  canvas.getContext('2d')!.putImageData(new ImageData(pixels, width, height), 0, 0);
}

function rasterCenterRgba(wasm: CpuExports): string {
  const packed = wasm.demo_frame_size();
  const width = packed & 0xffff;
  const height = packed >>> 16;
  const pixels = new Uint8Array(wasm.memory.buffer, wasm.demo_frame_ptr(), width * height * 4);
  const offset = ((height >>> 1) * width + (width >>> 1)) * 4;
  return `rgba(${pixels[offset]},${pixels[offset + 1]},${pixels[offset + 2]},${pixels[offset + 3]})`;
}

function rasterFrameTrace(wasm: CpuExports): string {
  // `demo_frame_trace_ptr` 首次调用可能建立缓存并增长 wasm memory；因此必须
  // 在取得 `(ptr, len)` 后再读取当前的 `memory.buffer`。
  const ptr = wasm.demo_frame_trace_ptr();
  const len = wasm.demo_frame_trace_len();
  return new TextDecoder().decode(
    new Uint8Array(wasm.memory.buffer, ptr, len),
  );
}

async function startRaster(canvas: HTMLCanvasElement): Promise<() => void> {
  const { instance } = await WebAssembly.instantiateStreaming(fetch(`/tela_demo.wasm?v=${Date.now()}`), {
    env: { tela_now: () => performance.now() },
  });
  const wasm = instance.exports as unknown as CpuExports;
  setCanvasSize(canvas, wasm.demo_frame_size());
  let logged = false;
  return () => {
    const submitted = wasm.demo_tick();
    if (submitted === 0) return;
    presentRaster(canvas, wasm);
    if (!logged) {
      logged = true;
      console.info('[tela/frame]', JSON.parse(rasterFrameTrace(wasm)));
      console.info('[tela/raster/backend]', { firstTick: submitted, center: rasterCenterRgba(wasm) });
    }
  };
}

async function startGpu(canvas: HTMLCanvasElement): Promise<() => void> {
  const glueUrl = '/tela_demo_gpu.js';
  const glue = (await import(/* webpackIgnore: true */ glueUrl)) as GpuGlue;
  await glue.default({ module_or_path: `/tela_demo_gpu_bg.wasm?v=${Date.now()}` });
  // WebGPU surface 必须在 canvas 物理尺寸确定后创建。
  setCanvasSize(canvas, (480 | (360 << 16)) >>> 0);
  await glue.start_gpu(canvas);
  const submitted = glue.tick_gpu();
  console.info('[tela/frame]', JSON.parse(glue.frame_trace()));
  console.info('[tela/wgpu/backend]', { firstTick: submitted, diagnostics: glue.gpu_diagnostics() });
  void glue.gpu_probe().then(
    (rgb) => {
      const value = `#${rgb.toString(16).padStart(6, '0')}`;
      console.info('[tela/wgpu/backend]', { sharedSceneProbe: value });
    },
    (error: unknown) => console.error('[tela/wgpu] shared-scene probe failed:', error),
  );
  let lastFailure = '';
  return () => {
    if (glue.tick_gpu() !== 0) {
      lastFailure = '';
      return;
    }
    const status = glue.gpu_diagnostics();
    if (status !== lastFailure) {
      console.warn(`[tela/wgpu] present skipped; ${status}`);
      lastFailure = status;
    }
  };
}

async function start(): Promise<void> {
  const canvas = document.querySelector('canvas');
  if (!canvas) throw new Error('缺少 demo canvas');
  const mode = modeFromUrl();
  let tick: () => void;
  if (mode !== 'raster' && navigator.gpu) {
    try {
      tick = await startGpu(canvas);
    } catch (error) {
      if (mode === 'wgpu') console.warn('wgpu 初始化失败，回退 raster:', error);
      tick = await startRaster(canvas);
    }
  } else {
    tick = await startRaster(canvas);
  }
  const loop = (): void => {
    tick();
    requestAnimationFrame(loop);
  };
  requestAnimationFrame(loop);
}

void start().catch((error: unknown) => {
  console.error(error);
  document.body.dataset.error = String(error);
});
