// 最小浏览器宿主：选择 CPU/WGPU 后端，驱动同一 tela 场景帧。

import { decodeImageRgba8, type DecodedImage } from './resource_adapter';

type BackendMode = 'auto' | 'raster' | 'wgpu';

interface GpuGlue {
  default(options?: { module_or_path?: string }): Promise<unknown>;
  frame_size(): number;
  start_gpu(canvas: HTMLCanvasElement): Promise<void>;
  upload_image(texture: string, width: number, height: number, rgba8: Uint8Array): void;
  tick_gpu(): number;
  pointer_down(x: number, y: number): number;
  pointer_move(x: number, y: number): number;
  pointer_cursor(): number;
  input_focused(): boolean;
  set_input_value(value: string): number;
  frame_trace(): string;
  gpu_diagnostics(): string;
  gpu_probe(): Promise<number>;
}

interface CpuExports {
  demo_tick(): number;
  demo_pointer_down(x: number, y: number): number;
  demo_pointer_move(x: number, y: number): number;
  demo_pointer_cursor(): number;
  demo_input_focused(): number;
  demo_input_value_begin(bytes: number): number;
  demo_input_value_finish(bytes: number): number;
  demo_frame_size(): number;
  demo_frame_ptr(): number;
  demo_frame_trace_ptr(): number;
  demo_frame_trace_len(): number;
  demo_image_upload_begin(bytes: number): number;
  demo_image_upload_finish(width: number, height: number): number;
  memory: WebAssembly.Memory;
}

interface PointerBridge {
  pointer_down(x: number, y: number): number;
  pointer_move(x: number, y: number): number;
}

interface InteractionBridge extends PointerBridge {
  input_focused(): boolean;
  pointer_cursor(): number;
  set_input_value(value: string): number;
}

const DEMO_IMAGE_ID = 'demo.image';
// 源图明显大于 176x132 的展示框，避免低分辨率随机图被放大后发糊。
const DEMO_IMAGE_URL = 'https://picsum.photos/800/600';

async function loadDemoImage(): Promise<DecodedImage> {
  return decodeImageRgba8(DEMO_IMAGE_ID, DEMO_IMAGE_URL);
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

/** 把浏览器 CSS 坐标转换成 tela 逻辑坐标，再交给核心交互层命中测试。 */
function installPointerEvents(
  canvas: HTMLCanvasElement,
  packed: number,
  bridge: InteractionBridge,
): void {
  const { width, height } = unpackCanvasSize(packed);
  const point = (event: PointerEvent): { x: number; y: number } => {
    const bounds = canvas.getBoundingClientRect();
    return {
      x: (event.clientX - bounds.left) * width / Math.max(bounds.width, 1),
      y: (event.clientY - bounds.top) * height / Math.max(bounds.height, 1),
    };
  };
  const syncCursor = () => {
    const cursor = ['default', 'text', 'pointer'][bridge.pointer_cursor()] ?? 'default';
    canvas.style.cursor = cursor;
  };
  canvas.addEventListener('pointerdown', (event) => {
    event.preventDefault();
    const position = point(event);
    canvas.setPointerCapture?.(event.pointerId);
    bridge.pointer_down(position.x, position.y);
    syncCursor();
    syncTextFocus();
  });
  canvas.addEventListener('pointermove', (event) => {
    const position = point(event);
    bridge.pointer_move(position.x, position.y);
    syncCursor();
  });
  canvas.addEventListener('pointerleave', () => {
    bridge.pointer_move(-1, -1);
    syncCursor();
  });

  // Canvas 没有原生文本编辑面；保留一个不可见 textarea 承接键盘和 IME，
  // 输入值仍通过受控 `ValueChange` 回写 tela，而不是由 DOM 绘制。
  const editor = document.createElement('textarea');
  editor.setAttribute('aria-label', 'tela text input');
  editor.autocapitalize = 'off';
  editor.autocomplete = 'off';
  editor.spellcheck = false;
  Object.assign(editor.style, {
    position: 'fixed',
    left: '0',
    top: '0',
    width: '1px',
    height: '1px',
    opacity: '0',
    pointerEvents: 'none',
    border: '0',
    padding: '0',
    resize: 'none',
  });
  document.body.append(editor);
  const syncTextFocus = () => {
    if (bridge.input_focused()) {
      editor.focus({ preventScroll: true });
    } else if (document.activeElement === editor) {
      editor.blur();
    }
  };
  editor.addEventListener('input', () => {
    bridge.set_input_value(editor.value);
  });
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

function uploadRasterImage(wasm: CpuExports, image: DecodedImage): void {
  const ptr = wasm.demo_image_upload_begin(image.rgba8.byteLength);
  if (ptr === 0) throw new Error('CPU 图片上传缓冲区分配失败');
  new Uint8Array(wasm.memory.buffer, ptr, image.rgba8.byteLength).set(image.rgba8);
  if (wasm.demo_image_upload_finish(image.width, image.height) !== 1) {
    throw new Error('CPU 图片上传校验失败');
  }
}

function setRasterInputValue(wasm: CpuExports, value: string): number {
  const bytes = new TextEncoder().encode(value);
  const ptr = wasm.demo_input_value_begin(bytes.byteLength);
  if (bytes.byteLength > 0) {
    new Uint8Array(wasm.memory.buffer, ptr, bytes.byteLength).set(bytes);
  }
  return wasm.demo_input_value_finish(bytes.byteLength);
}

async function startRaster(canvas: HTMLCanvasElement): Promise<() => void> {
  const { instance } = await WebAssembly.instantiateStreaming(fetch(`/tela_demo.wasm?v=${Date.now()}`), {
    env: { tela_now: () => performance.now() },
  });
  const wasm = instance.exports as unknown as CpuExports;
  const logicalSize = wasm.demo_frame_size();
  setCanvasSize(canvas, logicalSize);
  installPointerEvents(canvas, logicalSize, {
    pointer_down: (x, y) => wasm.demo_pointer_down(x, y),
    pointer_move: (x, y) => wasm.demo_pointer_move(x, y),
    pointer_cursor: () => wasm.demo_pointer_cursor(),
    input_focused: () => wasm.demo_input_focused() !== 0,
    set_input_value: (value) => setRasterInputValue(wasm, value),
  });
  void loadDemoImage().then(
    (image) => {
      uploadRasterImage(wasm, image);
      console.info('[tela/resource]', { backend: 'raster', id: image.id, width: image.width, height: image.height });
    },
    (error: unknown) => console.warn('[tela/resource] raster image unavailable:', error),
  );
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
  installPointerEvents(canvas, logicalSize, glue);
  observeGpuCanvasSize(canvas, logicalSize);
  void loadDemoImage().then(
    (image) => {
      glue.upload_image(image.id, image.width, image.height, image.rgba8);
      console.info('[tela/resource]', { backend: 'wgpu', id: image.id, width: image.width, height: image.height });
    },
    (error: unknown) => console.warn('[tela/resource] wgpu image unavailable:', error),
  );
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
