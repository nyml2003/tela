// 桥包 postcard 编解码（TLBR 信封 + 16 std 桥载荷）。
// 与 Rust 侧 serde(postcard) 编码严格对齐：varint=LEB128、有符号=zigzag、
// f32/f64=固定宽度 LE、String/Vec=varint 长度前缀、enum=externally tagged u32 varint。
// 只实现本会话需要的最小读写面；新增桥载荷时在此扩展。

export interface CapabilityId {
  scope: 'std' | { named: string };
  group: string;
  name: string;
}

export interface BridgeRequest {
  request_id: bigint;
  version: VersionPolicy;
  capability: CapabilityId;
  payload: Uint8Array;
}

export type VersionPolicy =
  | { kind: 'latest' }
  | { kind: 'exact'; version: Version }
  | { kind: 'range'; lower: Version | undefined; upper: Version | undefined };

export interface Version {
  major: number;
  minor: number;
  patch: number;
}

export type BridgeResult =
  | { kind: 'ok'; payload: Uint8Array }
  | { kind: 'err'; error: BridgeError };

export type BridgeError =
  | { kind: 'unknownCapability' }
  | { kind: 'versionMismatch'; policy: VersionPolicy; available: Version }
  | { kind: 'permissionDenied' }
  | { kind: 'keyNotFound' }
  | { kind: 'timeout' };

const MAGIC = [0x54, 0x4c, 0x42, 0x52]; // "TLBR"
const PACKET_VERSION = 1;

export class PostcardReader {
  private offset: number;
  constructor(private readonly data: Uint8Array, start = 0) {
    this.offset = start;
  }

  /** 当前读取位置（包内偏移，不含 magic 头）。 */
  position(): number {
    return this.offset;
  }

  private require(n: number): void {
    if (this.offset + n > this.data.length) {
      throw new Error('桥包越界');
    }
  }

  u8(): number {
    this.require(1);
    return this.data[this.offset++]!;
  }

  bool(): boolean {
    return this.u8() !== 0;
  }

  /** LEB128 varint，最高 64 位。 */
  varint(): bigint {
    let result = 0n;
    let shift = 0n;
    for (let i = 0; i < 10; i++) {
      const byte = this.u8();
      result |= BigInt(byte & 0x7f) << shift;
      if ((byte & 0x80) === 0) return result;
      shift += 7n;
    }
    throw new Error('桥包 varint 过长');
  }

  u32(): number {
    const value = this.varint();
    if (value > 0xffffffffn) throw new Error('桥包 u32 越界');
    return Number(value);
  }

  u64(): bigint {
    return this.varint();
  }

  /** zigzag varint（postcard 对 i32 的编码）。 */
  i32(): number {
    const zig = this.varint();
    const value = (zig >> 1n) ^ -(zig & 1n);
    if (value > 0x7fffffffn || value < -0x80000000n) throw new Error('桥包 i32 越界');
    return Number(value);
  }

  f32(): number {
    this.require(4);
    const view = new DataView(this.data.buffer, this.data.byteOffset + this.offset, 4);
    this.offset += 4;
    return view.getFloat32(0, true);
  }

  f64(): number {
    this.require(8);
    const view = new DataView(this.data.buffer, this.data.byteOffset + this.offset, 8);
    this.offset += 8;
    return view.getFloat64(0, true);
  }

  string(): string {
    const length = this.u32();
    this.require(length);
    const bytes = this.data.subarray(this.offset, this.offset + length);
    this.offset += length;
    return new TextDecoder().decode(bytes);
  }

  bytes(): Uint8Array {
    const length = this.u32();
    this.require(length);
    const bytes = this.data.slice(this.offset, this.offset + length);
    this.offset += length;
    return bytes;
  }

  version(): Version {
    return { major: this.u32(), minor: this.u32(), patch: this.u32() };
  }

  optionVersion(): Version | undefined {
    return this.u8() === 0 ? undefined : this.version();
  }

  versionPolicy(): VersionPolicy {
    const tag = this.u32();
    switch (tag) {
      case 0:
        return { kind: 'latest' };
      case 1:
        return { kind: 'exact', version: this.version() };
      case 2:
        return { kind: 'range', lower: this.optionVersion(), upper: this.optionVersion() };
      default:
        throw new Error(`未知 VersionPolicy tag ${tag}`);
    }
  }

  capability(): CapabilityId {
    const scopeTag = this.u32();
    const scope = scopeTag === 0 ? 'std' : { named: this.string() };
    return { scope, group: this.string(), name: this.string() };
  }
}

export class PostcardWriter {
  private readonly chunks: number[] = [];

  u8(value: number): void {
    this.chunks.push(value & 0xff);
  }

  bool(value: boolean): void {
    this.u8(value ? 1 : 0);
  }

  varint(value: bigint): void {
    let remaining = value;
    for (;;) {
      const byte = Number(remaining & 0x7fn);
      remaining >>= 7n;
      if (remaining === 0n) {
        this.chunks.push(byte);
        return;
      }
      this.chunks.push(byte | 0x80);
    }
  }

  u32(value: number): void {
    this.varint(BigInt(value >>> 0));
  }

  u64(value: bigint): void {
    this.varint(value);
  }

  /** zigzag varint（postcard 对 i32 的编码）。 */
  i32(value: number): void {
    const zig = (BigInt(value) << 1n) ^ (BigInt(value) >> 63n);
    this.varint(zig);
  }

  f32(value: number): void {
    const buffer = new ArrayBuffer(4);
    new DataView(buffer).setFloat32(0, value, true);
    this.chunks.push(...new Uint8Array(buffer));
  }

  f64(value: number): void {
    const buffer = new ArrayBuffer(8);
    new DataView(buffer).setFloat64(0, value, true);
    this.chunks.push(...new Uint8Array(buffer));
  }

  string(value: string): void {
    const bytes = new TextEncoder().encode(value);
    this.u32(bytes.length);
    this.chunks.push(...bytes);
  }

  bytes(value: Uint8Array): void {
    this.u32(value.byteLength);
    this.chunks.push(...value);
  }

  version(value: Version): void {
    this.u32(value.major);
    this.u32(value.minor);
    this.u32(value.patch);
  }

  optionVersion(value: Version | undefined): void {
    if (value === undefined) {
      this.u8(0);
    } else {
      this.u8(1);
      this.version(value);
    }
  }

  versionPolicy(value: VersionPolicy): void {
    switch (value.kind) {
      case 'latest':
        this.u32(0);
        break;
      case 'exact':
        this.u32(1);
        this.version(value.version);
        break;
      case 'range':
        this.u32(2);
        this.optionVersion(value.lower);
        this.optionVersion(value.upper);
        break;
    }
  }

  capability(value: CapabilityId): void {
    if (value.scope === 'std') {
      this.u32(0);
    } else {
      this.u32(1);
      this.string(value.scope.named);
    }
    this.string(value.group);
    this.string(value.name);
  }

  toBytes(): Uint8Array {
    return new Uint8Array(this.chunks);
  }
}

function packetHeader(): Uint8Array {
  return new Uint8Array([...MAGIC, PACKET_VERSION & 0xff, (PACKET_VERSION >> 8) & 0xff]);
}

function validatePacketHeader(bytes: Uint8Array): void {
  if (bytes.byteLength < 6) throw new Error('桥包过短');
  for (let i = 0; i < 4; i++) {
    if (bytes[i] !== MAGIC[i]) throw new Error('桥包 magic 不匹配');
  }
  const version = bytes[4]! | (bytes[5]! << 8);
  if (version !== PACKET_VERSION) throw new Error(`桥包版本不匹配: ${version}`);
}

/** 解码一个请求包（含 magic 头），并返回本包消耗的总字节数（流式队列推进用）。 */
export function decodeRequestPacket(bytes: Uint8Array): {
  request: BridgeRequest;
  consumed: number;
} {
  validatePacketHeader(bytes);
  const reader = new PostcardReader(bytes, 6);
  const request: BridgeRequest = {
    request_id: reader.u64(),
    version: reader.versionPolicy(),
    capability: reader.capability(),
    payload: reader.bytes(),
  };
  return { request, consumed: reader.position() };
}

/** 编码一个响应事件包（含 magic 头），供 bridgeDeliver 回投。 */
export function encodeResponseEvent(requestId: bigint, result: BridgeResult): Uint8Array {
  const writer = new PostcardWriter();
  writer.u32(0); // BridgeEvent::Response tag
  writer.u64(requestId);
  if (result.kind === 'ok') {
    writer.u32(0); // BridgeResult::Ok
    writer.bytes(result.payload);
  } else {
    writer.u32(1); // BridgeResult::Err
    switch (result.error.kind) {
      case 'unknownCapability':
        writer.u32(0);
        break;
      case 'versionMismatch':
        writer.u32(1);
        writer.versionPolicy(result.error.policy);
        writer.version(result.error.available);
        break;
      case 'permissionDenied':
        writer.u32(2);
        break;
      case 'keyNotFound':
        writer.u32(3);
        break;
      case 'timeout':
        writer.u32(4);
        break;
    }
  }
  return new Uint8Array([...packetHeader(), ...writer.toBytes()]);
}
