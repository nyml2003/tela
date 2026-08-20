// 原生浏览器 WebAssembly guest 适配。这里仅处理模块实例、线性内存复制和导出边界；
// 事件/status/frame 的二进制格式全部委托 Rust tela-target-webview。

import type { TelaWebviewBindings, WebAppStatus } from './bindings';

const MAX_PACKET_BYTES = 64 * 1024 * 1024;

type GuestFunction = (...args: number[]) => number;

interface GuestExports {
  memory: WebAssembly.Memory;
  tela_app_abi_version: GuestFunction;
  tela_app_init: GuestFunction;
  tela_app_input_begin: GuestFunction;
  tela_app_dispatch: GuestFunction;
  tela_app_frame_ptr: GuestFunction;
  tela_app_frame_len: GuestFunction;
  tela_app_status_ptr: GuestFunction;
  tela_app_status_len: GuestFunction;
  tela_app_error_ptr: GuestFunction;
  tela_app_error_len: GuestFunction;
  tela_app_request_begin?: GuestFunction;
  tela_app_request_len?: GuestFunction;
  tela_app_bridge_dispatch_begin?: GuestFunction;
  tela_app_bridge_dispatch?: GuestFunction;
}

export interface GuestPublication {
  readonly changed: boolean;
  readonly framePacket: Uint8Array;
  readonly status: WebAppStatus;
}

/** A bounded, browser-native instance of the portable Tela application guest. */
export class TelaGuestRuntime {
  private latest: GuestPublication | undefined;

  private constructor(
    private readonly bindings: TelaWebviewBindings,
    private readonly exports: GuestExports,
  ) {}

  static async create(
    bindings: TelaWebviewBindings,
    guestWasm: Uint8Array,
  ): Promise<TelaGuestRuntime> {
    if (guestWasm.byteLength === 0) throw new Error('应用 guest WASM 为空');
    const source = guestWasm.slice().buffer as ArrayBuffer;
    const instantiated = await WebAssembly.instantiate(source, {});
    const exports = requireGuestExports(instantiated.instance.exports);
    const guestAbi = exports.tela_app_abi_version() >>> 0;
    const hostAbi = bindings.host_app_abi_version() >>> 0;
    if (guestAbi !== hostAbi) {
      throw new Error(`应用 ABI 不匹配: host=${hostAbi}, guest=${guestAbi}`);
    }
    return new TelaGuestRuntime(bindings, exports);
  }

  /** Runs one guest initialization and reads its first frame/status publication. */
  initialize(): GuestPublication {
    const initialized = this.exports.tela_app_init() >>> 0;
    if (initialized === 0) {
      throw new Error(this.guestError() || '应用 guest 初始化失败');
    }
    return this.refresh(true);
  }

  /** Delivers one Rust-encoded event and atomically refreshes the public frame/status pair. */
  dispatch(packet: Uint8Array): GuestPublication {
    if (packet.byteLength > MAX_PACKET_BYTES) {
      throw new Error(`应用事件包超过 ${MAX_PACKET_BYTES / 1024 / 1024} MiB 限制`);
    }
    const pointer = this.exports.tela_app_input_begin(packet.byteLength) >>> 0;
    this.copyIntoGuest(pointer, packet);
    const changed = this.exports.tela_app_dispatch(packet.byteLength) !== 0;
    if (!changed) {
      const diagnostic = this.guestError();
      if (diagnostic) throw new Error(diagnostic);
    }
    return this.refresh(changed);
  }

  /** Whether the guest exposes the full bridge ABI (all four exports present). */
  bridgeAvailable(): boolean {
    return (
      this.exports.tela_app_request_begin !== undefined &&
      this.exports.tela_app_request_len !== undefined &&
      this.exports.tela_app_bridge_dispatch_begin !== undefined &&
      this.exports.tela_app_bridge_dispatch !== undefined
    );
  }

  /** Length of the guest's queued bridge request packets; `0` when unavailable. */
  bridgeRequestLen(): number {
    const requestLen = this.exports.tela_app_request_len;
    if (!requestLen) return 0;
    return requestLen() >>> 0;
  }

  /** Reads the guest's queued bridge request packets (raw bytes, caller decodes). */
  bridgeReadRequests(): Uint8Array {
    const requestBegin = this.exports.tela_app_request_begin;
    if (!requestBegin) return new Uint8Array(0);
    const length = this.bridgeRequestLen();
    if (length === 0) return new Uint8Array(0);
    const pointer = requestBegin(0) >>> 0;
    return this.copyFromGuest(pointer, length, 'bridge requests');
  }

  /** Delivers one encoded bridge event packet (response) to the guest. */
  bridgeDeliver(packet: Uint8Array): void {
    const dispatchBegin = this.exports.tela_app_bridge_dispatch_begin;
    const dispatch = this.exports.tela_app_bridge_dispatch;
    if (!dispatchBegin || !dispatch) {
      throw new Error('应用 guest 未暴露桥 ABI');
    }
    if (packet.byteLength > MAX_PACKET_BYTES) {
      throw new Error(`桥事件包超过 ${MAX_PACKET_BYTES / 1024 / 1024} MiB 限制`);
    }
    const pointer = dispatchBegin(packet.byteLength) >>> 0;
    this.copyIntoGuest(pointer, packet);
    dispatch(packet.byteLength);
  }

  /** Latest guest frame; valid only after initialization. */
  framePacket(): Uint8Array {
    return this.publication().framePacket;
  }

  /** Latest host-visible non-drawing application state. */
  status(): WebAppStatus {
    return this.publication().status;
  }

  private refresh(changed: boolean): GuestPublication {
    const framePacket = this.readGuestExport(
      this.exports.tela_app_frame_ptr,
      this.exports.tela_app_frame_len,
      'frame',
    );
    const statusPacket = this.readGuestExport(
      this.exports.tela_app_status_ptr,
      this.exports.tela_app_status_len,
      'status',
    );
    const status = this.bindings.decode_app_status(statusPacket);
    const publication = { changed, framePacket, status };
    this.latest = publication;
    return publication;
  }

  private guestError(): string {
    const bytes = this.readGuestExport(
      this.exports.tela_app_error_ptr,
      this.exports.tela_app_error_len,
      'error',
    );
    return new TextDecoder().decode(bytes).trim();
  }

  private readGuestExport(
    pointerExport: GuestFunction,
    lengthExport: GuestFunction,
    label: string,
  ): Uint8Array {
    const pointer = pointerExport() >>> 0;
    const length = lengthExport() >>> 0;
    if (length > MAX_PACKET_BYTES) {
      throw new Error(`应用 ${label} 包超过 ${MAX_PACKET_BYTES / 1024 / 1024} MiB 限制`);
    }
    return this.copyFromGuest(pointer, length, label);
  }

  private copyIntoGuest(pointer: number, bytes: Uint8Array): void {
    this.requireRange(pointer, bytes.byteLength, 'event');
    new Uint8Array(this.exports.memory.buffer, pointer, bytes.byteLength).set(bytes);
  }

  private copyFromGuest(pointer: number, length: number, label: string): Uint8Array {
    this.requireRange(pointer, length, label);
    return new Uint8Array(this.exports.memory.buffer, pointer, length).slice();
  }

  private requireRange(pointer: number, length: number, label: string): void {
    const memoryBytes = this.exports.memory.buffer.byteLength;
    if (pointer > memoryBytes || length > memoryBytes - pointer) {
      throw new Error(
        `应用 ${label} 指针越过 WASM 线性内存: ptr=${pointer}, len=${length}, memory=${memoryBytes}`,
      );
    }
  }

  private publication(): GuestPublication {
    if (!this.latest) throw new Error('应用 guest 尚未初始化');
    return this.latest;
  }
}

function requireGuestExports(exports: WebAssembly.Exports): GuestExports {
  const memory = exports.memory;
  if (!(memory instanceof WebAssembly.Memory)) {
    throw new Error('应用 guest 必须将线性内存导出为 `memory`');
  }
  const names = [
    'tela_app_abi_version',
    'tela_app_init',
    'tela_app_input_begin',
    'tela_app_dispatch',
    'tela_app_frame_ptr',
    'tela_app_frame_len',
    'tela_app_status_ptr',
    'tela_app_status_len',
    'tela_app_error_ptr',
    'tela_app_error_len',
  ] as const;
  const required = Object.fromEntries(names.map((name) => [name, exports[name]]));
  for (const name of names) {
    if (typeof required[name] !== 'function') {
      throw new Error(`应用 guest 缺少函数导出: ${name}`);
    }
  }
  return { memory, ...required } as unknown as GuestExports;
}
