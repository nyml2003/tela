// src/rawgpu.ts
var TEXTURE_COPY_SRC = 4;
var TEXTURE_RENDER_ATTACHMENT = 16;
var BUFFER_MAP_READ = 1;
var BUFFER_COPY_DST = 8;
var MAP_MODE_READ = 1;
function requiredElement(selector) {
  const element = document.querySelector(selector);
  if (element === null) throw new Error(`\u7F3A\u5C11\u5143\u7D20: ${selector}`);
  return element;
}
var log = requiredElement("#log");
function show(message, className) {
  const line = document.createElement("div");
  line.textContent = message;
  if (className) line.className = className;
  log.append(line);
}
function report(event) {
  void fetch("/api/telemetry", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify([{ ...event, ts: Date.now() }])
  }).catch(() => void 0);
}
async function runProbe() {
  if (!navigator.gpu) {
    show("\u2717 navigator.gpu \u4E0D\u5B58\u5728\uFF08\u6D4F\u89C8\u5668\u65E0 WebGPU\uFF09", "err");
    report({ type: "console-error", message: "rawgpu: navigator.gpu \u4E0D\u5B58\u5728" });
    throw new Error("no WebGPU");
  }
  const adapter = await navigator.gpu.requestAdapter();
  if (!adapter) {
    show("\u2717 requestAdapter() \u8FD4\u56DE null\uFF08\u65E0\u53EF\u7528 GPU \u9002\u914D\u5668\uFF09", "err");
    report({ type: "console-error", message: "rawgpu: requestAdapter \u8FD4\u56DE null" });
    throw new Error("no adapter");
  }
  const info = adapter.info ?? {};
  const adapterLine = `adapter: vendor=${info.vendor ?? "?"} device=${info.device ?? "?"} arch=${info.architecture ?? "?"} desc=${info.description ?? "?"}`;
  show(`\u2713 ${adapterLine}`, "info");
  report({ type: "log", message: `[tela-rawgpu] ${adapterLine}` });
  const device = await adapter.requestDevice();
  device.addEventListener("uncapturederror", (event) => {
    const message = `\u672A\u6355\u83B7 GPU \u9519\u8BEF: ${event.error?.message ?? event.error}`;
    show(`\u2717 ${message}`, "err");
    report({ type: "console-error", message: `rawgpu: ${message}` });
  });
  show("\u2713 \u8BBE\u5907\u521B\u5EFA\u6210\u529F");
  const format = "rgba8unorm";
  const offscreen = device.createTexture({
    size: [64, 64],
    format,
    usage: TEXTURE_RENDER_ATTACHMENT | TEXTURE_COPY_SRC
  });
  const shader = device.createShaderModule({
    code: `
      @vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
        var pos = array<vec2<f32>, 3>(vec2(0.0, 0.6), vec2(-0.6, -0.4), vec2(0.6, -0.4));
        return vec4<f32>(pos[i], 0.0, 1.0);
      }
      @fragment fn fs() -> @location(0) vec4<f32> { return vec4<f32>(1.0, 0.0, 0.0, 1.0); }
    `
  });
  const pipeline = device.createRenderPipeline({
    layout: "auto",
    vertex: { module: shader, entryPoint: "vs" },
    fragment: { module: shader, entryPoint: "fs", targets: [{ format }] },
    primitive: { topology: "triangle-list" }
  });
  const encoder = device.createCommandEncoder();
  const pass = encoder.beginRenderPass({
    colorAttachments: [{
      view: offscreen.createView(),
      clearValue: { r: 0.05, g: 0.08, b: 0.1, a: 1 },
      loadOp: "clear",
      storeOp: "store"
    }]
  });
  pass.setPipeline(pipeline);
  pass.draw(3, 1, 0, 0);
  pass.end();
  device.queue.submit([encoder.finish()]);
  const bytesPerRow = 64 * 4;
  const readback = device.createBuffer({
    size: bytesPerRow * 64,
    usage: BUFFER_COPY_DST | BUFFER_MAP_READ
  });
  const readbackEncoder = device.createCommandEncoder();
  readbackEncoder.copyTextureToBuffer(
    { texture: offscreen },
    { buffer: readback, bytesPerRow, rowsPerImage: 64 },
    { width: 64, height: 64 }
  );
  device.queue.submit([readbackEncoder.finish()]);
  await readback.mapAsync(MAP_MODE_READ);
  const data = new Uint8Array(readback.getMappedRange());
  const offset = (32 * 64 + 32) * 4;
  const rgb = [data[offset], data[offset + 1], data[offset + 2]];
  readback.unmap();
  show(`\u79BB\u5C4F\u4E09\u89D2\u5F62\u4E2D\u5FC3\u50CF\u7D20: RGB(${rgb.join(",")})`);
  show(rgb[0] > 150 ? "\u2713 \u73AF\u5883\u6B63\u5E38\uFF1A\u539F\u751F WebGPU \u80FD\u6E32\u67D3\uFF08\u95EE\u9898\u5728 wgpu \u5C42\uFF09" : "\u2717 \u73AF\u5883\u5F02\u5E38\uFF1A\u539F\u751F WebGPU \u4E5F\u65E0\u6CD5\u6E32\u67D3");
  report({ type: "gpu-probe", rgb, source: "rawgpu" });
  const canvas = requiredElement("#ui");
  const context = canvas.getContext("webgpu");
  if (context) {
    const canvasFormat = navigator.gpu.getPreferredCanvasFormat();
    context.configure({ device, format: canvasFormat, alphaMode: "opaque" });
    const canvasPipeline = device.createRenderPipeline({
      layout: "auto",
      vertex: { module: shader, entryPoint: "vs" },
      fragment: { module: shader, entryPoint: "fs", targets: [{ format: canvasFormat }] },
      primitive: { topology: "triangle-list" }
    });
    const canvasEncoder = device.createCommandEncoder();
    const canvasPass = canvasEncoder.beginRenderPass({
      colorAttachments: [{
        view: context.getCurrentTexture().createView(),
        clearValue: { r: 0.05, g: 0.08, b: 0.1, a: 1 },
        loadOp: "clear",
        storeOp: "store"
      }]
    });
    canvasPass.setPipeline(canvasPipeline);
    canvasPass.draw(3, 1, 0, 0);
    canvasPass.end();
    device.queue.submit([canvasEncoder.finish()]);
    show("\u2713 canvas \u4E09\u89D2\u5F62\u5DF2\u7ED8\u5236\uFF08\u53EF\u89C1\u786E\u8BA4\uFF09");
  }
  show("\u81EA\u68C0\u5B8C\u6210\uFF0C\u7ED3\u679C\u5DF2\u4E0A\u62A5 CLI\uFF08ops verify gpu\uFF09", "info");
}
void runProbe().catch((error) => {
  const message = error instanceof Error ? error.message : String(error);
  show(`\u2717 \u81EA\u68C0\u5931\u8D25: ${message}`, "err");
  report({ type: "console-error", message: `rawgpu: \u81EA\u68C0\u5931\u8D25 ${message}` });
});
//# sourceMappingURL=rawgpu.js.map
