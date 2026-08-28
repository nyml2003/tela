// Static Tela Agent product adapter. The wasm-bindgen module is loaded by agent-bootstrap.js with
// a static ESM import; this file never resolves a bundle or instantiates another WebAssembly guest.

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

interface AgentDispatchOutcome {
  readonly handled: boolean;
  readonly publish_requested: boolean;
}

interface AgentWebSessionBinding {
  initialize(): AgentDispatchOutcome;
  dispatch(packet: Uint8Array): AgentDispatchOutcome;
  publish(): WebAppPublication;
  presented(token: bigint): AgentDispatchOutcome;
  rejected(token: bigint): void;
  close(): void;
  free?(): void;
}

interface AgentDemoBindings extends TelaWebviewBindings {
  readonly AgentWebSession: new () => AgentWebSessionBinding;
}

interface DisposableStatus extends WebAppStatus {
  free?(): void;
}

interface DisposablePublication extends WebAppPublication {
  free?(): void;
}

class StaticAgentRuntime implements TelaApplicationRuntime {
  private readonly session: AgentWebSessionBinding;
  private latest: ApplicationPublication | undefined;
  private pendingToken: bigint | undefined;
  private closed = false;

  constructor(bindings: AgentDemoBindings) {
    this.session = new bindings.AgentWebSession();
  }

  initialize(): ApplicationPublication {
    const outcome = this.session.initialize();
    if (!outcome.publish_requested) {
      throw new Error('静态 Agent 会话初始化后未请求首帧发布');
    }
    return this.publish();
  }

  dispatch(packet: Uint8Array): ApplicationDispatchResult {
    const outcome = this.session.dispatch(packet);
    if (outcome.publish_requested) this.publish();
    return { handled: outcome.handled, published: outcome.publish_requested };
  }

  acknowledgePresented(token: bigint): ApplicationDispatchResult {
    if (this.pendingToken !== token) {
      throw new Error(`静态 Agent 呈现回执不是当前候选: token=${token}`);
    }
    const outcome = this.session.presented(token);
    this.pendingToken = undefined;
    if (outcome.publish_requested) this.publish();
    return { handled: outcome.handled, published: outcome.publish_requested };
  }

  rejectPublication(token: bigint): void {
    if (this.pendingToken !== token) {
      throw new Error(`静态 Agent 拒绝的不是当前候选: token=${token}`);
    }
    this.session.rejected(token);
    this.pendingToken = undefined;
  }

  framePacket(): Uint8Array {
    return this.publication().framePacket;
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

  private publish(): ApplicationPublication {
    const publication = this.session.publish() as DisposablePublication;
    const next = {
      framePacket: publication.frame_packet(),
      status: publication.status() as DisposableStatus,
    };
    publication.free?.();
    this.disposeLatest();
    const token = next.status.frame_token;
    if (token === undefined) throw new Error('静态 Agent publication 未携带 frame token');
    this.latest = next;
    this.pendingToken = token;
    return next;
  }

  private publication(): ApplicationPublication {
    if (!this.latest) throw new Error('静态 Agent 会话尚未初始化');
    return this.latest;
  }

  private disposeLatest(): void {
    (this.latest?.status as DisposableStatus | undefined)?.free?.();
    this.latest = undefined;
  }
}

declare global {
  interface Window {
    telaAgentSession?: TelaRuntimeSession;
  }
}

/** Starts the statically imported single-Wasm Agent product. */
export async function startAgentDemo(rawBindings: unknown): Promise<void> {
  const bindings = rawBindings as AgentDemoBindings;
  if (typeof bindings.AgentWebSession !== 'function') {
    throw new Error('Agent Wasm 缺少 AgentWebSession 导出');
  }
  const canvas = document.querySelector<HTMLCanvasElement>('#agent-canvas');
  if (!canvas) throw new Error('缺少 Agent canvas');
  const runtime = new StaticAgentRuntime(bindings);
  const session = await startTelaRuntime({ canvas, bindings, runtime });
  window.telaAgentSession = session;
  document.body.dataset.ready = 'true';
  const bootStatus = document.querySelector<HTMLElement>('#boot-status');
  if (bootStatus) bootStatus.hidden = true;
  window.addEventListener('pagehide', () => {
    session.close();
    delete window.telaAgentSession;
  }, { once: true });
}

/** Presents a startup failure without hiding the original exception from developer tools. */
export function showAgentStartupError(error: unknown): void {
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
