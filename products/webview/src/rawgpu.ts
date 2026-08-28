// 原生 JS WebGPU 环境自检：与 tela/wgpu 路径分开，定位浏览器环境问题。

interface RawGpuTelemetry {
  type: 'console-error' | 'gpu-probe' | 'log';
  message?: string;
  rgb?: readonly [number, number, number];
  source?: 'rawgpu';
}

// TypeScript 7 当前的 lib.dom 只声明 flag 类型，未声明这三个浏览器全局常量。
// 保持 WebGPU 规范中的稳定位值，避免把未类型化全局扩散到诊断逻辑。
const TEXTURE_COPY_SRC = 0x01;
const TEXTURE_RENDER_ATTACHMENT = 0x10;
const BUFFER_MAP_READ = 0x01;
const BUFFER_COPY_DST = 0x08;
const MAP_MODE_READ = 0x01;

function requiredElement<T extends Element>(selector: string): T {
  const element = document.querySelector<T>(selector);
  if (element === null) throw new Error(`缺少元素: ${selector}`);
  return element;
}

const log = requiredElement<HTMLElement>('#log');

function show(message: string, className?: 'err' | 'info'): void {
  const line = document.createElement('div');
  line.textContent = message;
  if (className) line.className = className;
  log.append(line);
}

/** 无服务时静默；页面仍可独立打开检查 WebGPU。 */
function report(event: RawGpuTelemetry): void {
  void fetch('/api/telemetry', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify([{ ...event, ts: Date.now() }]),
  }).catch(() => undefined);
}

async function runProbe(): Promise<void> {
  if (!navigator.gpu) {
    show('✗ navigator.gpu 不存在（浏览器无 WebGPU）', 'err');
    report({ type: 'console-error', message: 'rawgpu: navigator.gpu 不存在' });
    throw new Error('no WebGPU');
  }

  const adapter = await navigator.gpu.requestAdapter();
  if (!adapter) {
    show('✗ requestAdapter() 返回 null（无可用 GPU 适配器）', 'err');
    report({ type: 'console-error', message: 'rawgpu: requestAdapter 返回 null' });
    throw new Error('no adapter');
  }
  const info = adapter.info ?? {};
  const adapterLine = `adapter: vendor=${info.vendor ?? '?'} device=${info.device ?? '?'} arch=${info.architecture ?? '?'} desc=${info.description ?? '?'}`;
  show(`✓ ${adapterLine}`, 'info');
  report({ type: 'log', message: `[tela-rawgpu] ${adapterLine}` });

  const device = await adapter.requestDevice();
  device.addEventListener('uncapturederror', (event) => {
    const message = `未捕获 GPU 错误: ${event.error?.message ?? event.error}`;
    show(`✗ ${message}`, 'err');
    report({ type: 'console-error', message: `rawgpu: ${message}` });
  });
  show('✓ 设备创建成功');

  const format = 'rgba8unorm';
  const offscreen = device.createTexture({
    size: [64, 64],
    format,
    usage: TEXTURE_RENDER_ATTACHMENT | TEXTURE_COPY_SRC,
  });
  const shader = device.createShaderModule({
    code: `
      @vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
        var pos = array<vec2<f32>, 3>(vec2(0.0, 0.6), vec2(-0.6, -0.4), vec2(0.6, -0.4));
        return vec4<f32>(pos[i], 0.0, 1.0);
      }
      @fragment fn fs() -> @location(0) vec4<f32> { return vec4<f32>(1.0, 0.0, 0.0, 1.0); }
    `,
  });
  const pipeline = device.createRenderPipeline({
    layout: 'auto',
    vertex: { module: shader, entryPoint: 'vs' },
    fragment: { module: shader, entryPoint: 'fs', targets: [{ format }] },
    primitive: { topology: 'triangle-list' },
  });
  const encoder = device.createCommandEncoder();
  const pass = encoder.beginRenderPass({
    colorAttachments: [{
      view: offscreen.createView(),
      clearValue: { r: 0.05, g: 0.08, b: 0.1, a: 1 },
      loadOp: 'clear',
      storeOp: 'store',
    }],
  });
  pass.setPipeline(pipeline);
  pass.draw(3, 1, 0, 0);
  pass.end();
  device.queue.submit([encoder.finish()]);

  const bytesPerRow = 64 * 4;
  const readback = device.createBuffer({
    size: bytesPerRow * 64,
    usage: BUFFER_COPY_DST | BUFFER_MAP_READ,
  });
  const readbackEncoder = device.createCommandEncoder();
  readbackEncoder.copyTextureToBuffer(
    { texture: offscreen },
    { buffer: readback, bytesPerRow, rowsPerImage: 64 },
    { width: 64, height: 64 },
  );
  device.queue.submit([readbackEncoder.finish()]);
  await readback.mapAsync(MAP_MODE_READ);
  const data = new Uint8Array(readback.getMappedRange());
  const offset = (32 * 64 + 32) * 4;
  const rgb: [number, number, number] = [data[offset]!, data[offset + 1]!, data[offset + 2]!];
  readback.unmap();
  show(`离屏三角形中心像素: RGB(${rgb.join(',')})`);
  show(rgb[0] > 150 ? '✓ 环境正常：原生 WebGPU 能渲染（问题在 wgpu 层）' : '✗ 环境异常：原生 WebGPU 也无法渲染');
  report({ type: 'gpu-probe', rgb, source: 'rawgpu' });

  const canvas = requiredElement<HTMLCanvasElement>('#ui');
  const context = canvas.getContext('webgpu') as GPUCanvasContext | null;
  if (context) {
    const canvasFormat = navigator.gpu.getPreferredCanvasFormat();
    context.configure({ device, format: canvasFormat, alphaMode: 'opaque' });
    const canvasPipeline = device.createRenderPipeline({
      layout: 'auto',
      vertex: { module: shader, entryPoint: 'vs' },
      fragment: { module: shader, entryPoint: 'fs', targets: [{ format: canvasFormat }] },
      primitive: { topology: 'triangle-list' },
    });
    const canvasEncoder = device.createCommandEncoder();
    const canvasPass = canvasEncoder.beginRenderPass({
      colorAttachments: [{
        view: context.getCurrentTexture().createView(),
        clearValue: { r: 0.05, g: 0.08, b: 0.1, a: 1 },
        loadOp: 'clear',
        storeOp: 'store',
      }],
    });
    canvasPass.setPipeline(canvasPipeline);
    canvasPass.draw(3, 1, 0, 0);
    canvasPass.end();
    device.queue.submit([canvasEncoder.finish()]);
    show('✓ canvas 三角形已绘制（可见确认）');
  }
  show('自检完成，结果已上报 CLI（ops verify gpu）', 'info');
}

void runProbe().catch((error: unknown) => {
  const message = error instanceof Error ? error.message : String(error);
  show(`✗ 自检失败: ${message}`, 'err');
  report({ type: 'console-error', message: `rawgpu: 自检失败 ${message}` });
});
