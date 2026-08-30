// DOM/WebGPU lifecycle shared by dynamically delivered guests and statically linked products.
// The runtime boundary below is concrete session behavior; it does not know how the application
// code reached the page.

import type { TelaWebviewBindings, WebAppStatus } from './bindings';
import { decodeRequestPacket, encodeResponseEvent } from './bridge-codec';
import { handleBridgeRequest } from './bridge-providers';
import { installInputBridge, type InputBridgeHandle } from './input-bridge';
import { observeCanvasSurface, syncCanvasSurface, type CanvasSurfaceSize } from './surface';

export interface ApplicationPublication {
  readonly framePacket: Uint8Array;
  readonly damageFlags: number;
  readonly damageRects: Float32Array;
  readonly transportSequence: bigint;
  readonly transportBaseSequence: bigint | undefined;
  readonly transportSnapshot: boolean;
  readonly transportSpine: readonly string[];
  readonly status: WebAppStatus;
}

export interface ApplicationDispatchResult {
  readonly handled: boolean;
  readonly published: boolean;
}

/** Session behavior required by the DOM/WebGPU driver. */
export interface TelaApplicationRuntime {
  initialize(): ApplicationPublication;
  dispatch(packet: Uint8Array): ApplicationDispatchResult;
  acknowledgePresented(token: bigint): ApplicationDispatchResult;
  rejectPublication(token: bigint): void;
  framePacket(): Uint8Array;
  frameDamage(): { readonly flags: number; readonly rects: Float32Array };
  frameTransport(): {
    readonly sequence: bigint;
    readonly baseSequence: bigint | undefined;
    readonly snapshot: boolean;
    readonly spine: readonly string[];
  };
  status(): WebAppStatus;
  close?(): void;
  bridgeAvailable?(): boolean;
  bridgeReadRequests?(): Uint8Array;
  bridgeDeliver?(packet: Uint8Array): void;
}

export interface StartTelaRuntimeOptions {
  canvas: HTMLCanvasElement;
  bindings: TelaWebviewBindings;
  runtime: TelaApplicationRuntime;
}

export interface TelaRuntimeSession {
  replaceKeymap(snapshot: string | object): boolean;
  close(): void;
}

/** Drives one initialized application runtime through the browser presentation protocol. */
export async function startTelaRuntime(
  options: StartTelaRuntimeOptions,
): Promise<TelaRuntimeSession> {
  const { canvas, bindings, runtime } = options;
  if (!navigator.gpu) {
    throw new Error('当前 WebView 不支持 WebGPU；Tela 浏览器产品仅提供 WGPU 路径');
  }
  const initialSurface = syncCanvasSurface(canvas);
  let currentSurface = initialSurface;
  let surfaceDispatched = false;
  let closed = false;
  let input: InputBridgeHandle | undefined;
  let stopSurfaceObservation: (() => void) | undefined;
  let animationFrame: number | undefined;
  let retryTimer: number | undefined;
  let renderError: string | undefined;
  let presentedFrameToken: bigint | undefined;
  let appliedTransportSequence: bigint | undefined;

  const scheduleRender = () => {
    if (closed || animationFrame !== undefined) return;
    animationFrame = requestAnimationFrame((timestamp) => {
      animationFrame = undefined;
      if (closed) return;
      try {
        if (runtime.status().animation_active) {
          dispatch(bindings.event_tick(BigInt(Math.floor(timestamp))), false);
        }
        const framePacket = runtime.framePacket();
        const damage = runtime.frameDamage();
        const transport = runtime.frameTransport();
        const frameToken = runtime.status().frame_token;
        if (!transport.snapshot && transport.baseSequence !== appliedTransportSequence) {
          throw new Error(
            `收到基于 ${String(transport.baseSequence)} 的 patch，但当前保留帧为 ${String(appliedTransportSequence)}`,
          );
        }
        if (!bindings.render_gpu_damage(framePacket, damage.flags, damage.rects)) {
          retryTimer ??= window.setTimeout(() => {
            retryTimer = undefined;
            scheduleRender();
          }, 100);
          return;
        }
        appliedTransportSequence = transport.sequence;
        if (frameToken !== undefined && frameToken !== presentedFrameToken) {
          const outcome = runtime.acknowledgePresented(frameToken);
          input?.synchronize();
          if (outcome.published) scheduleRender();
        }
        // A publication without a frame token is deliberately non-interactive. Clear any token
        // from an older frame so input cannot cross a presentation boundary after that state.
        presentedFrameToken = frameToken;
        renderError = undefined;
        if (runtime.status().animation_active) scheduleRender();
      } catch (error) {
        // A renderer panic can leave the wasm-side GPU RefCell borrowed. Calling back into
        // gpu_diagnostics here would trigger a second panic and hide the original failure.
        const message = `tela WebView WGPU render failed: ${String(error)}`;
        if (message !== renderError) {
          console.error(message);
          renderError = message;
        }
      }
    });
  };

  const processBridgeRequests = (): void => {
    if (
      closed
      || runtime.bridgeAvailable?.() !== true
      || runtime.bridgeReadRequests === undefined
      || runtime.bridgeDeliver === undefined
    ) return;
    const raw = runtime.bridgeReadRequests();
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
      const providerOutcome = handleBridgeRequest(request);
      if (providerOutcome.immediate) {
        runtime.bridgeDeliver(
          encodeResponseEvent(
            request.request_id,
            providerOutcome.error
              ? { kind: 'err', error: providerOutcome.error }
              : { kind: 'ok', payload: providerOutcome.payload },
          ),
        );
      } else {
        providerOutcome.promise
          ?.then((result) => {
            if (closed || runtime.bridgeDeliver === undefined) return;
            runtime.bridgeDeliver(
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
    if (handled > 0) console.debug(`tela bridge: 处理 ${handled} 个请求`);
  };

  const dispatch = (packet: Uint8Array, synchronizeClock = true): boolean => {
    if (closed) return false;
    let published = false;
    if (synchronizeClock) {
      published = runtime.dispatch(
        bindings.event_tick(BigInt(Math.floor(performance.now()))),
      ).published;
    }
    const outcome = runtime.dispatch(packet);
    processBridgeRequests();
    input?.synchronize();
    if (published || outcome.published) scheduleRender();
    return outcome.handled;
  };

  const dispatchViewport = (surface: CanvasSurfaceSize) => {
    if (
      surfaceDispatched
      && surface.logicalWidth === currentSurface.logicalWidth
      && surface.logicalHeight === currentSurface.logicalHeight
      && surface.pixelWidth === currentSurface.pixelWidth
      && surface.pixelHeight === currentSurface.pixelHeight
    ) return;
    surfaceDispatched = true;
    currentSurface = surface;
    presentedFrameToken = undefined;
    dispatch(bindings.event_viewport(surface.logicalWidth, surface.logicalHeight));
  };

  try {
    await bindings.start_gpu(canvas);
    runtime.initialize();
    dispatchViewport(initialSurface);
    input = installInputBridge({
      canvas,
      bindings,
      dispatch,
      status: () => runtime.status(),
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
    runtime.close?.();
    bindings.shutdown_gpu();
    throw error;
  }

  return {
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
      runtime.close?.();
      bindings.shutdown_gpu();
    },
  };
}
