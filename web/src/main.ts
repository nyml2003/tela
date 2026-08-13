// 最小浏览器宿主：选择 CPU/WGPU 后端，驱动同一 tela 场景帧。

type BackendMode = 'auto' | 'raster' | 'wgpu';

interface GpuGlue {
  default(options?: { module_or_path?: string }): Promise<unknown>;
  set_viewport(width: number, height: number): boolean;
  frame_size(): number;
  start_gpu(canvas: HTMLCanvasElement): Promise<void>;
  tick_gpu(): number;
  pointer_down(x: number, y: number): number;
  pointer_move(x: number, y: number): number;
  pointer_scroll(x: number, y: number, deltaX: number, deltaY: number): number;
  pointer_cursor(): number;
  input_focused(): boolean;
  input_value(): string;
  input_composition_start(): number;
  input_composition_end(): number;
  input_enter(): number;
  input_cancel(): number;
  input_blur(): number;
  set_input_value(value: string): number;
  frame_trace(): string;
  gpu_diagnostics(): string;
  gpu_probe(): Promise<number>;
}

interface CpuExports {
  demo_tick(): number;
  demo_set_viewport(width: number, height: number): number;
  demo_set_raster_dpi(dpi: number): void;
  demo_pointer_down(x: number, y: number): number;
  demo_pointer_move(x: number, y: number): number;
  demo_pointer_scroll(x: number, y: number, deltaX: number, deltaY: number): number;
  demo_pointer_cursor(): number;
  demo_input_focused(): number;
  demo_input_composition_start(): number;
  demo_input_composition_end(): number;
  demo_input_enter(): number;
  demo_input_cancel(): number;
  demo_input_blur(): number;
  demo_input_value_ptr(): number;
  demo_input_value_len(): number;
  demo_input_value_begin(bytes: number): number;
  demo_input_value_finish(bytes: number): number;
  demo_frame_size(): number;
  demo_frame_ptr(): number;
  demo_frame_trace_ptr(): number;
  demo_frame_trace_len(): number;
  memory: WebAssembly.Memory;
}

interface PointerBridge {
  pointer_down(x: number, y: number): number;
  pointer_move(x: number, y: number): number;
  pointer_scroll(x: number, y: number, deltaX: number, deltaY: number): number;
}

interface InteractionBridge extends PointerBridge {
  input_focused(): boolean;
  input_value(): string;
  input_composition_start(): number;
  input_composition_end(): number;
  input_enter(): number;
  input_cancel(): number;
  input_blur(): number;
  pointer_cursor(): number;
  set_input_value(value: string): number;
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

function logicalViewport(canvas: HTMLCanvasElement): { width: number; height: number } {
  const bounds = canvas.getBoundingClientRect();
  return { width: Math.max(320, Math.round(bounds.width)), height: Math.max(240, Math.round(bounds.height)) };
}

/** 把浏览器 CSS 坐标转换成 tela 逻辑坐标，再交给核心交互层命中测试。 */
function installPointerEvents(
  canvas: HTMLCanvasElement,
  viewport: () => { width: number; height: number },
  bridge: InteractionBridge,
): () => void {
  const point = (event: PointerEvent): { x: number; y: number } => {
    const bounds = canvas.getBoundingClientRect();
    const logical = viewport();
    return {
      x: (event.clientX - bounds.left) * logical.width / Math.max(bounds.width, 1),
      y: (event.clientY - bounds.top) * logical.height / Math.max(bounds.height, 1),
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
  canvas.addEventListener('wheel', (event) => {
    event.preventDefault();
    const position = point(event);
    const logical = viewport();
    const unit = event.deltaMode === WheelEvent.DOM_DELTA_LINE
      ? 16
      : event.deltaMode === WheelEvent.DOM_DELTA_PAGE
        ? logical.height
        : 1;
    const bounds = canvas.getBoundingClientRect();
    bridge.pointer_scroll(
      position.x,
      position.y,
      event.deltaX * unit * logical.width / Math.max(bounds.width, 1),
      event.deltaY * unit * logical.height / Math.max(bounds.height, 1),
    );
  }, { passive: false });

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
      if (document.activeElement !== editor) {
        editor.value = bridge.input_value();
        editor.focus({ preventScroll: true });
      }
    } else if (document.activeElement === editor) {
      editor.blur();
    }
  };
  editor.addEventListener('input', () => {
    bridge.set_input_value(editor.value);
  });
  editor.addEventListener('compositionstart', () => {
    bridge.input_composition_start();
  });
  editor.addEventListener('compositionend', () => {
    bridge.input_composition_end();
  });
  editor.addEventListener('keydown', (event) => {
    if (event.key === 'Enter' && !event.isComposing) {
      event.preventDefault();
      bridge.input_enter();
    } else if (event.key === 'Escape') {
      event.preventDefault();
      bridge.input_cancel();
      editor.value = bridge.input_value();
    }
  });
  editor.addEventListener('blur', () => {
    bridge.input_blur();
  });
  return syncTextFocus;
}

function unpackCanvasSize(packed: number): { width: number; height: number } {
  return { width: packed & 0xffff, height: packed >>> 16 };
}

/** CSS 视口驱动逻辑布局，backing store 独立跟随设备像素比。 */
function syncGpuCanvasSize(canvas: HTMLCanvasElement): CanvasSurfaceSize {
  const { width: logicalWidth, height: logicalHeight } = logicalViewport(canvas);
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
function observeGpuCanvasSize(canvas: HTMLCanvasElement, syncViewport: (size: CanvasSurfaceSize) => void): void {
  const sync = () => {
    syncViewport(syncGpuCanvasSize(canvas));
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

function setRasterInputValue(wasm: CpuExports, value: string): number {
  const bytes = new TextEncoder().encode(value);
  const ptr = wasm.demo_input_value_begin(bytes.byteLength);
  if (bytes.byteLength > 0) {
    new Uint8Array(wasm.memory.buffer, ptr, bytes.byteLength).set(bytes);
  }
  return wasm.demo_input_value_finish(bytes.byteLength);
}

function rasterInputValue(wasm: CpuExports): string {
  const ptr = wasm.demo_input_value_ptr();
  const len = wasm.demo_input_value_len();
  return new TextDecoder().decode(new Uint8Array(wasm.memory.buffer, ptr, len));
}

async function startRaster(canvas: HTMLCanvasElement): Promise<() => void> {
  const { instance } = await WebAssembly.instantiateStreaming(fetch(`/tela_demo.wasm?v=${Date.now()}`), {
    env: { tela_now: () => performance.now() },
  });
  const wasm = instance.exports as unknown as CpuExports;
  let viewport = logicalViewport(canvas);
  const syncViewport = () => {
    viewport = logicalViewport(canvas);
    wasm.demo_set_viewport(viewport.width, viewport.height);
    wasm.demo_set_raster_dpi(window.devicePixelRatio || 1);
  };
  syncViewport();
  const syncTextFocus = installPointerEvents(canvas, () => viewport, {
    pointer_down: (x, y) => wasm.demo_pointer_down(x, y),
    pointer_move: (x, y) => wasm.demo_pointer_move(x, y),
    pointer_scroll: (x, y, deltaX, deltaY) => wasm.demo_pointer_scroll(x, y, deltaX, deltaY),
    pointer_cursor: () => wasm.demo_pointer_cursor(),
    input_focused: () => wasm.demo_input_focused() !== 0,
    input_value: () => rasterInputValue(wasm),
    input_composition_start: () => wasm.demo_input_composition_start(),
    input_composition_end: () => wasm.demo_input_composition_end(),
    input_enter: () => wasm.demo_input_enter(),
    input_cancel: () => wasm.demo_input_cancel(),
    input_blur: () => wasm.demo_input_blur(),
    set_input_value: (value) => setRasterInputValue(wasm, value),
  });
  const observer = new ResizeObserver(syncViewport);
  observer.observe(canvas);
  window.addEventListener('resize', syncViewport);
  let logged = false;
  return () => {
    syncTextFocus();
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
  let viewport = logicalViewport(canvas);
  glue.set_viewport(viewport.width, viewport.height);
  // WebGPU surface 必须在 canvas 物理尺寸确定后创建。
  const surfaceSize = syncGpuCanvasSize(canvas);
  await glue.start_gpu(canvas);
  const syncTextFocus = installPointerEvents(canvas, () => viewport, glue);
  observeGpuCanvasSize(canvas, (size) => {
    viewport = { width: size.logicalWidth, height: size.logicalHeight };
    glue.set_viewport(viewport.width, viewport.height);
  });
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
    syncTextFocus();
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
