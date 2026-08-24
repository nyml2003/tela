// 通用 DOM WebView SDK 入口。它将 `.tela` 开发包、浏览器 guest、输入桥和 WGPU 壳
// 组织为一个可关闭的会话；不定义 Android/iOS 原生桥，也不持久化浏览器缓存。

import { loadTelaWebviewBindings } from './bindings';
import { loadDevelopmentBundle } from './bundle-loader';
import { TelaGuestRuntime } from './guest-runtime';
import { installInputBridge, type InputBridgeHandle } from './input-bridge';
import { observeCanvasSurface, syncCanvasSurface, type CanvasSurfaceSize } from './surface';
import { decodeRequestPacket, encodeResponseEvent } from './bridge-codec';
import { handleBridgeRequest } from './bridge-providers';

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
  let presentedFrameToken: bigint | undefined;

  const scheduleRender = () => {
    if (closed || animationFrame !== undefined) return;
    animationFrame = requestAnimationFrame((timestamp) => {
      animationFrame = undefined;
      if (closed) return;
      try {
        if (guest.status().animation_active) {
          dispatch(bindings.event_tick(BigInt(Math.floor(timestamp))), false);
        }
        const framePacket = guest.framePacket();
        const frameToken = guest.status().frame_token;
        if (!bindings.render_gpu(framePacket)) {
          presentedFrameToken = undefined;
          retryTimer ??= window.setTimeout(() => {
            retryTimer = undefined;
            scheduleRender();
          }, 100);
        } else {
          presentedFrameToken = frameToken;
          renderError = undefined;
          if (guest.status().animation_active) scheduleRender();
        }
      } catch (error) {
        const message = `tela WebView WGPU render failed: ${String(error)}; ${bindings.gpu_diagnostics()}`;
        if (message !== renderError) {
          console.error(message);
          renderError = message;
        }
        presentedFrameToken = undefined;
      }
    });
  };
  const dispatch = (packet: Uint8Array, synchronizeClock = true): boolean => {
    if (closed) return false;
    if (synchronizeClock) {
      guest.dispatch(bindings.event_tick(BigInt(Math.floor(performance.now()))));
    }
    const publication = guest.dispatch(packet);
    processBridgeRequests();
    input?.synchronize();
    scheduleRender();
    return publication.changed;
  };
  /** 桥队列处理：读队列 → 逐个 provider → 回投；异步 provider 完成后再回投。 */
  const processBridgeRequests = (): void => {
    if (closed || !guest.bridgeAvailable()) return;
    const raw = guest.bridgeReadRequests();
    if (raw.byteLength === 0) return;
    let offset = 0;
    let handled = 0;
    while (offset < raw.byteLength) {
      let decoded;
      try {
        decoded = decodeRequestPacket(raw.subarray(offset));
      } catch (error) {
        console.error(`桥请求包解码失败: ${String(error)}`);
        break;
      }
      const request = decoded.request;
      offset += decoded.consumed;
      const outcome = handleBridgeRequest(request);
      if (outcome.immediate) {
        guest.bridgeDeliver(
          encodeResponseEvent(
            request.request_id,
            outcome.error
              ? { kind: 'err', error: outcome.error }
              : { kind: 'ok', payload: outcome.payload },
          ),
        );
      } else {
        outcome.promise
          ?.then((result) => {
            if (closed) return;
            guest.bridgeDeliver(
              encodeResponseEvent(
                request.request_id,
                result.error
                  ? { kind: 'err', error: result.error }
                  : { kind: 'ok', payload: result.payload },
              ),
            );
            scheduleRender();
          })
          .catch((error: unknown) => {
            console.error(`桥异步 provider 失败: ${String(error)}`);
          });
      }
      handled++;
    }
    if (handled > 0) {
      console.debug(`tela bridge: 处理 ${handled} 个请求`);
    }
  };
  const dispatchViewport = (surface: CanvasSurfaceSize) => {
    currentSurface = surface;
    presentedFrameToken = undefined;
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
      presentedFrameToken: () => presentedFrameToken,
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
      presentedFrameToken = undefined;
      bindings.shutdown_gpu();
    },
  };
}
