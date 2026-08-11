// 最小浏览器宿主：选择 CPU/WGPU 后端，驱动同一 tela 场景帧。

type BackendMode = 'auto' | 'raster' | 'wgpu';

interface GpuGlue {
  default(options?: { module_or_path?: string }): Promise<unknown>;
  frame_size(): number;
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

interface CanvasSurfaceSize {
  logicalWidth: number;
  logicalHeight: number;
  cssWidth: number;
  cssHeight: number;
  pixelWidth: number;
  pixelHeight: number;
  pixelRatio: number;
}

function modeFromUrl(): BackendMode {
  const value = new URLSearchParams(location.search).get('backend');
  return value === 'raster' || value === 'wgpu' ? value : 'auto';
}

function setCanvasSize(canvas: HTMLCanvasElement, packed: number): void {
  const { width, height } = unpackCanvasSize(packed);
  canvas.width = width;
  canvas.height = height;
}

function unpackCanvasSize(packed: number): { width: number; height: number } {
  return { width: packed & 0xffff, height: packed >>> 16 };
}

/** 逻辑画布维持不变，仅把 WGPU 的 backing store 提升到屏幕的实际像素密度。 */
function syncGpuCanvasSize(canvas: HTMLCanvasElement, packed: number): CanvasSurfaceSize {
  const { width: logicalWidth, height: logicalHeight } = unpackCanvasSize(packed);
  const bounds = canvas.getBoundingClientRect();
  const pixelRatio = window.devicePixelRatio > 0 ? window.devicePixelRatio : 1;
  const pixelWidth = Math.max(1, Math.round(bounds.width * pixelRatio));
  const pixelHeight = Math.max(1, Math.round(bounds.height * pixelRatio));
  if (canvas.width !== pixelWidth) canvas.width = pixelWidth;
  if (canvas.height !== pixelHeight) canvas.height = pixelHeight;
  return {
    logicalWidth,
    logicalHeight,
    cssWidth: bounds.width,
    cssHeight: bounds.height,
    pixelWidth,
    pixelHeight,
    pixelRatio,
  };
}

/** CSS 尺寸、窗口缩放或跨屏倍率变化时，更新 WGPU backing store。 */
function observeGpuCanvasSize(canvas: HTMLCanvasElement, packed: number): void {
  const sync = () => {
    syncGpuCanvasSize(canvas, packed);
  };
  const observer = new ResizeObserver(sync);
  observer.observe(canvas);
  window.addEventListener('resize', sync);

  let resolutionQuery: MediaQueryList;
  const onResolutionChange = () => {
    sync();
    resolutionQuery.removeEventListener('change', onResolutionChange);
    resolutionQuery = window.matchMedia(`(resolution: ${window.devicePixelRatio}dppx)`);
    resolutionQuery.addEventListener('change', onResolutionChange, { once: true });
  };
  resolutionQuery = window.matchMedia(`(resolution: ${window.devicePixelRatio}dppx)`);
  resolutionQuery.addEventListener('change', onResolutionChange, { once: true });
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
  const logicalSize = glue.frame_size();
  // 先建立 4:3 逻辑宽高比，使 CSS `height: auto` 的初始测量正确。
  setCanvasSize(canvas, logicalSize);
  // WebGPU surface 必须在 canvas 物理尺寸确定后创建。
  const surfaceSize = syncGpuCanvasSize(canvas, logicalSize);
  await glue.start_gpu(canvas);
  observeGpuCanvasSize(canvas, logicalSize);
  const submitted = glue.tick_gpu();
  console.info('[tela/frame]', JSON.parse(glue.frame_trace()));
  console.info('[tela/wgpu/surface]', surfaceSize);
  console.info('[tela/wgpu/backend]', { firstTick: submitted, diagnostics: glue.gpu_diagnostics() });
  void glue.gpu_probe().then(
    (rgb) => {
      const value = `#${rgb.toString(16).padStart(6, '0')}`;
      console.info('[tela/wgpu/backend]', { roundedCornerProbe: value });
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
