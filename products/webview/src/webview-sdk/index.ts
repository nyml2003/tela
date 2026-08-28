// 通用 DOM WebView SDK 入口。它将 `.tela` 开发包、浏览器 guest、输入桥和 WGPU 壳
// 组织为一个可关闭的会话；不定义 Android/iOS 原生桥，也不持久化浏览器缓存。

import { loadTelaWebviewBindings } from './bindings';
import { loadDevelopmentBundle } from './bundle-loader';
import { TelaGuestRuntime } from './guest-runtime';
import { startTelaRuntime } from './runtime-driver';

export interface StartTelaWebviewOptions {
  /** One focusable DOM canvas owned by the caller. */
  canvas: HTMLCanvasElement;
  /** Explicit absolute or relative URL for the development bundle index. */
  bundleIndex: string | URL;
}

export interface TelaWebviewSession {
  /** Internal bundle content identifier accepted at startup. */
  readonly bundleId: string;
  /** Validates and atomically asks the guest to replace its runtime keymap snapshot. */
  replaceKeymap(snapshot: string | object): boolean;
  /** Releases DOM listeners, hidden text input and WGPU resources exactly once. */
  close(): void;
}

/** Starts the WGPU-only browser shell and returns its explicit lifecycle handle. */
export async function startTelaWebview(
  options: StartTelaWebviewOptions,
): Promise<TelaWebviewSession> {
  const { canvas, bundleIndex } = options;
  if (!navigator.gpu) {
    throw new Error('当前 WebView 不支持 WebGPU；本阶段浏览器 SDK 仅提供 WGPU 路径');
  }
  const bindings = await loadTelaWebviewBindings();
  const bundle = await loadDevelopmentBundle(bindings, bundleIndex);
  const guest = await TelaGuestRuntime.create(bindings, bundle.guestWasm);
  const session = await startTelaRuntime({ canvas, bindings, runtime: guest });

  return {
    bundleId: bundle.bundleId,
    replaceKeymap: (snapshot) => session.replaceKeymap(snapshot),
    close: () => session.close(),
  };
}
