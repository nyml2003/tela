// Static Tela WebView Probe product adapter. The wasm-bindgen module is loaded by
// card-bootstrap.js with a static ESM import; this file never resolves a bundle or instantiates
// another WebAssembly guest.

import type {
  TelaWebviewBindings,
  WebAppPublication,
  WebAppStatus,
} from './webview-sdk/bindings';
import {
  startTelaRuntime,
  type ApplicationDispatchResult,
  type ApplicationPublication,
  type TelaApplicationRuntime,
  type TelaRuntimeSession,
} from './webview-sdk/runtime-driver';

interface CardDispatchOutcome {
  readonly handled: boolean;
  readonly publish_requested: boolean;
}

interface CardWebSessionBinding {
  initialize(): CardDispatchOutcome;
  dispatch(packet: Uint8Array): CardDispatchOutcome;
  publish(): WebAppPublication;
  presented(token: bigint): CardDispatchOutcome;
  rejected(token: bigint): void;
  close(): void;
  free?(): void;
}

interface CardProbeBindings extends TelaWebviewBindings {
  readonly CardWebSession: new () => CardWebSessionBinding;
}

interface DisposableStatus extends WebAppStatus {
  free?(): void;
}

interface DisposablePublication extends WebAppPublication {
  free?(): void;
}

class StaticCardRuntime implements TelaApplicationRuntime {
  private readonly session: CardWebSessionBinding;
  private latest: ApplicationPublication | undefined;
  private pendingToken: bigint | undefined;
  private needsPublish = false;
  private closed = false;

  constructor(bindings: CardProbeBindings) {
    this.session = new bindings.CardWebSession();
  }

  initialize(): ApplicationPublication {
    const outcome = this.session.initialize();
    if (!outcome.publish_requested) {
      throw new Error('静态 WebView Probe 会话初始化后未请求首帧发布');
    }
    return this.publish();
  }

  // 输入事件到达频率（pointermove 可达 125Hz）远高于呈现节奏（rAF 60Hz）。会话一次只
  // 允许一个 pending 候选：逐事件立即 publish 会把上一个候选 rejected 掉，而候选里的
  // 组件动画进度随候选一起回滚，hover 缩放在持续移动时会被反复重启。这里把 publish
  // 推迟到渲染读取（framePacket 等，由 rAF 调用）时统一 flush，保证每个候选先呈现再
  // 被下一个取代。
  dispatch(packet: Uint8Array): ApplicationDispatchResult {
    const outcome = this.session.dispatch(packet);
    if (outcome.publish_requested) this.needsPublish = true;
    return { handled: outcome.handled, published: outcome.publish_requested };
  }

  acknowledgePresented(token: bigint): ApplicationDispatchResult {
    if (this.pendingToken !== token) {
      throw new Error(`静态 WebView Probe 呈现回执不是当前候选: token=${token}`);
    }
    const outcome = this.session.presented(token);
    this.pendingToken = undefined;
    // acknowledge 只发生在 rAF 渲染成功之后，这里立即 publish 不会挤掉未呈现的候选。
    if (outcome.publish_requested) this.publish();
    return { handled: outcome.handled, published: outcome.publish_requested };
  }

  rejectPublication(token: bigint): void {
    if (this.pendingToken !== token) {
      throw new Error(`静态 WebView Probe 拒绝的不是当前候选: token=${token}`);
    }
    this.session.rejected(token);
    this.pendingToken = undefined;
  }

  framePacket(): Uint8Array {
    this.flushPendingPublish();
    return this.publication().framePacket;
  }

  frameDamage(): { readonly flags: number; readonly rects: Float32Array } {
    this.flushPendingPublish();
    const publication = this.publication();
    return { flags: publication.damageFlags, rects: publication.damageRects };
  }

  frameTransport(): {
    readonly sequence: bigint;
    readonly baseSequence: bigint | undefined;
    readonly snapshot: boolean;
    readonly spine: readonly string[];
  } {
    this.flushPendingPublish();
    const publication = this.publication();
    return {
      sequence: publication.transportSequence,
      baseSequence: publication.transportBaseSequence,
      snapshot: publication.transportSnapshot,
      spine: publication.transportSpine,
    };
  }

  status(): WebAppStatus {
    return this.publication().status;
  }

  close(): void {
    if (this.closed) return;
    this.closed = true;
    this.disposeLatest();
    this.session.close();
    this.session.free?.();
    this.pendingToken = undefined;
  }

  private flushPendingPublish(): void {
    if (this.needsPublish) {
      this.needsPublish = false;
      this.publish();
    }
  }

  private publish(): ApplicationPublication {
    const publication = this.session.publish() as DisposablePublication;
    const next = {
      framePacket: publication.frame_packet(),
      damageFlags: publication.damage_flags(),
      damageRects: publication.damage_rects(),
      transportSequence: publication.transport_sequence(),
      transportBaseSequence: publication.transport_base_sequence,
      transportSnapshot: publication.transport_snapshot,
      transportSpine: publication.transport_spine(),
      status: publication.status() as DisposableStatus,
    };
    publication.free?.();
    this.disposeLatest();
    const token = next.status.frame_token;
    if (token === undefined) throw new Error('静态 WebView Probe publication 未携带 frame token');
    this.latest = next;
    this.pendingToken = token;
    return next;
  }

  private publication(): ApplicationPublication {
    if (!this.latest) throw new Error('静态 WebView Probe 会话尚未初始化');
    return this.latest;
  }

  private disposeLatest(): void {
    (this.latest?.status as DisposableStatus | undefined)?.free?.();
    this.latest = undefined;
  }
}

declare global {
  interface Window {
    telaCardSession?: TelaRuntimeSession;
  }
}

/** Starts the statically imported single-Wasm WebView probe product. */
export async function startCardProbe(rawBindings: unknown): Promise<void> {
  const bindings = rawBindings as CardProbeBindings;
  if (typeof bindings.CardWebSession !== 'function') {
    throw new Error('WebView Probe Wasm 缺少 CardWebSession 导出');
  }
  const canvas = document.querySelector<HTMLCanvasElement>('#card-canvas');
  if (!canvas) throw new Error('缺少 WebView Probe canvas');
  const runtime = new StaticCardRuntime(bindings);
  const session = await startTelaRuntime({ canvas, bindings, runtime });
  window.telaCardSession = session;
  document.body.dataset.ready = 'true';
  const bootStatus = document.querySelector<HTMLElement>('#boot-status');
  if (bootStatus) bootStatus.hidden = true;
  window.addEventListener('pagehide', () => {
    session.close();
    delete window.telaCardSession;
  }, { once: true });
}

/** Presents a startup failure without hiding the original exception from developer tools. */
export function showCardStartupError(error: unknown): void {
  const message = error instanceof Error ? error.message : String(error);
  console.error(error);
  document.body.dataset.error = message;
  const bootStatus = document.querySelector<HTMLElement>('#boot-status');
  if (bootStatus) bootStatus.hidden = true;
  const notice = document.querySelector<HTMLElement>('#startup-error');
  if (notice) {
    notice.hidden = false;
    notice.textContent = `启动失败：${message}`;
  }
}
