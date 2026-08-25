// Rust WebView SDK 的 wasm-bindgen 边界。浏览器侧不重新实现 bundle、postcard 或 frame
// 编码；这里只描述 Rust 导出的最小接口，并负责加载静态 WGPU 壳。

export interface DevelopmentBundleIndex {
  readonly bundle_url: string;
  readonly bundle_id: string;
}

export interface ValidatedBundle {
  readonly bundle_id: string;
  app_wasm(): Uint8Array;
}

export interface WebAppStatus {
  /** Guest frame identity. A browser host may use it only after a successful present. */
  readonly frame_token: bigint | undefined;
  readonly cursor: number;
  readonly input_focused: boolean;
  readonly input_value: string;
  readonly animation_active: boolean;
  readonly next_deadline_ms: bigint | undefined;
}

export interface WebAppPublication {
  frame_packet(): Uint8Array;
  status(): WebAppStatus;
}

export interface TelaWebviewBindings {
  host_app_abi_version(): number;
  parse_development_index(bytes: Uint8Array): DevelopmentBundleIndex;
  validate_development_bundle(index: DevelopmentBundleIndex, bytes: Uint8Array): ValidatedBundle;
  decode_app_publication(bytes: Uint8Array): WebAppPublication;
  decode_app_status(bytes: Uint8Array): WebAppStatus;
  event_viewport(width: number, height: number): Uint8Array;
  event_tick(timestampMs: bigint): Uint8Array;
  event_pointer(
    sourceFrameToken: bigint,
    pointerId: bigint,
    kind: number,
    phase: number,
    x: number,
    y: number,
    buttons: number,
    timestampMicros: bigint,
    deltaX: number,
    deltaY: number,
  ): Uint8Array;
  event_key_down(sourceFrameToken: bigint, physicalKey: number, modifierBits: number, repeat: boolean): Uint8Array;
  event_set_input_value(sourceFrameToken: bigint, value: string): Uint8Array;
  event_input_focus(sourceFrameToken: bigint): Uint8Array;
  event_input_blur(sourceFrameToken: bigint): Uint8Array;
  event_input_enter(sourceFrameToken: bigint): Uint8Array;
  event_input_cancel(sourceFrameToken: bigint): Uint8Array;
  event_input_composition_start(sourceFrameToken: bigint): Uint8Array;
  event_input_composition_end(sourceFrameToken: bigint): Uint8Array;
  event_replace_keymap_json(json: string): Uint8Array;
  start_gpu(canvas: HTMLCanvasElement): Promise<void>;
  render_gpu(framePacket: Uint8Array): boolean;
  shutdown_gpu(): void;
  gpu_diagnostics(): string;
}

interface WasmGlue extends TelaWebviewBindings {
  default(input?: RequestInfo | URL | Response | BufferSource | WebAssembly.Module): Promise<unknown>;
}

/**
 * Loads the static browser shell with `no-store` semantics.
 *
 * The development application is fetched separately from its bundle index. There is no browser
 * Cache Storage or IndexedDB fallback in this path: a page startup always observes the current
 * server response or fails visibly.
 */
export async function loadTelaWebviewBindings(): Promise<TelaWebviewBindings> {
  const glueUrl = new URL('/tela_webview_host.js', window.location.href).href;
  const glue = (await import(/* webpackIgnore: true */ glueUrl)) as WasmGlue;
  const wasmUrl = new URL('/tela_webview_host_bg.wasm', window.location.href);
  const response = await fetch(wasmUrl, { cache: 'no-store' });
  if (!response.ok) {
    throw new Error(`加载 WebView WGPU 壳失败: ${response.status} ${response.statusText}`);
  }
  await glue.default(response);
  return glue;
}
