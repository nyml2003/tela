// 通用 DOM WebView SDK 入口。它将 `.tela` 开发包、浏览器 guest、输入桥和 WGPU 壳
// 组织为一个可关闭的会话；不定义 Android/iOS 原生桥，也不持久化浏览器缓存。

import { loadTelaWebviewBindings } from './bindings';
import { loadDevelopmentBundle } from './bundle-loader';
import { TelaGuestRuntime } from './guest-runtime';
import { installInputBridge, type InputBridgeHandle } from './input-bridge';
import { observeCanvasSurface, syncCanvasSurface, type CanvasSurfaceSize } from './surface';

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
  const initialSurface = syncCanvasSurface(canvas);
  let currentSurface = initialSurface;

  let closed = false;
  let input: InputBridgeHandle | undefined;
  let stopSurfaceObservation: (() => void) | undefined;
  let animationFrame: number | undefined;
  let retryTimer: number | undefined;
  let renderError: string | undefined;

  const scheduleRender = () => {
    if (closed || animationFrame !== undefined) return;
    animationFrame = requestAnimationFrame(() => {
      animationFrame = undefined;
      if (closed) return;
      try {
        if (!bindings.render_gpu(guest.framePacket())) {
          retryTimer ??= window.setTimeout(() => {
            retryTimer = undefined;
            scheduleRender();
          }, 100);
        } else {
          renderError = undefined;
        }
      } catch (error) {
        const message = `tela WebView WGPU render failed: ${String(error)}; ${bindings.gpu_diagnostics()}`;
        if (message !== renderError) {
          console.error(message);
          renderError = message;
        }
      }
    });
  };
  const dispatch = (packet: Uint8Array): boolean => {
    if (closed) return false;
    const publication = guest.dispatch(packet);
    input?.synchronize();
    scheduleRender();
    return publication.changed;
  };
  const dispatchViewport = (surface: CanvasSurfaceSize) => {
    currentSurface = surface;
    dispatch(bindings.event_viewport(surface.logicalWidth, surface.logicalHeight));
  };

  try {
    await bindings.start_gpu(canvas);
    guest.initialize();
    dispatchViewport(initialSurface);
    input = installInputBridge({
      canvas,
      bindings,
      dispatch,
      status: () => guest.status(),
      viewport: () => ({
        width: currentSurface.logicalWidth,
        height: currentSurface.logicalHeight,
      }),
    });
    input.synchronize();
    stopSurfaceObservation = observeCanvasSurface(canvas, dispatchViewport);
    scheduleRender();
  } catch (error) {
    stopSurfaceObservation?.();
    input?.close();
    bindings.shutdown_gpu();
    throw error;
  }

  return {
    bundleId: bundle.bundleId,
    replaceKeymap(snapshot: string | object): boolean {
      const json = typeof snapshot === 'string' ? snapshot : JSON.stringify(snapshot);
      return dispatch(bindings.event_replace_keymap_json(json));
    },
    close(): void {
      if (closed) return;
      closed = true;
      if (animationFrame !== undefined) cancelAnimationFrame(animationFrame);
      if (retryTimer !== undefined) window.clearTimeout(retryTimer);
      stopSurfaceObservation?.();
      input?.close();
      bindings.shutdown_gpu();
    },
  };
}
