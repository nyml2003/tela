// WebView 宿主桥 provider：16 个 std 只读桥（canIUse 静态表 + 浏览器探测）。
// 同步能力立即回投；getBatteryLevel/getBatteryCharging（Promise）与
// getCoordinates（geolocation 回调）异步完成。payload 格式与 Rust tela-bridge 对齐。

import {
  PostcardReader,
  PostcardWriter,
  type BridgeError,
  type BridgeRequest,
  type BridgeResult,
  type CapabilityId,
  type Version,
  type VersionPolicy,
} from './bridge-codec';

const V1_0_0: Version = { major: 1, minor: 0, patch: 0 };

const STD_CAPABILITIES: readonly CapabilityId[] = [
  { scope: 'std', group: 'base', name: 'canIUse' },
  { scope: 'std', group: 'device', name: 'getAppName' },
  { scope: 'std', group: 'device', name: 'getAppVersion' },
  { scope: 'std', group: 'device', name: 'getAppBuildId' },
  { scope: 'std', group: 'device', name: 'getBundleVersion' },
  { scope: 'std', group: 'device', name: 'getBundleBuildId' },
  { scope: 'std', group: 'device', name: 'getTimeStamp' },
  { scope: 'std', group: 'device', name: 'getViewportSize' },
  { scope: 'std', group: 'device', name: 'getViewportDpr' },
  { scope: 'std', group: 'device', name: 'getBatteryLevel' },
  { scope: 'std', group: 'device', name: 'getBatteryCharging' },
  { scope: 'std', group: 'device', name: 'getNetworkOnline' },
  { scope: 'std', group: 'device', name: 'getNetworkKind' },
  { scope: 'std', group: 'position', name: 'getCoordinates' },
  { scope: 'std', group: 'config', name: 'getConfig' },
];

const STD_BY_ID: Map<string, Version> = new Map(
  STD_CAPABILITIES.map((capability) => [capabilityIdString(capability), V1_0_0]),
);

function capabilityIdString(id: CapabilityId): string {
  return id.scope === 'std'
    ? `std.${id.group}.${id.name}`
    : `${id.scope.named}.${id.group}.${id.name}`;
}

/** 宿主注入的构建常量（由构建期生成；当前为开发缺省值）。 */
interface BuildConstants {
  appName: string;
  appVersion: Version;
  appBuildId: number;
  bundleVersion: Version;
  bundleBuildId: number;
}

const BUILD: BuildConstants = {
  appName: '文件管理器',
  appVersion: { major: 0, minor: 1, patch: 0 },
  appBuildId: 1,
  bundleVersion: { major: 0, minor: 1, patch: 0 },
  bundleBuildId: 1,
};

/** 宿主注入的静态配置表。 */
const CONFIG: ReadonlyMap<string, string> = new Map([['app.theme', '"default"']]);

export interface ProviderOutcome {
  readonly immediate: boolean;
  readonly payload: Uint8Array;
  readonly error?: BridgeError;
  readonly promise?: Promise<{ payload: Uint8Array; error?: BridgeError }>;
}

function ok(payload: Uint8Array): ProviderOutcome {
  return { immediate: true, payload };
}

function fail(error: BridgeError): ProviderOutcome {
  return { immediate: true, payload: new Uint8Array(0), error };
}

function asyncOutcome(
  promise: Promise<{ payload: Uint8Array; error?: BridgeError }>,
): ProviderOutcome {
  return { immediate: false, payload: new Uint8Array(0), promise };
}

/** 处理一个请求，返回即时或异步的载荷。 */
export function handleBridgeRequest(request: BridgeRequest): ProviderOutcome {
  const id = capabilityIdString(request.capability);
  switch (id) {
    case 'std.base.canIUse':
      return handleCanIUse(request);
    case 'std.device.getAppName':
      return ok(encodeAppName(BUILD.appName));
    case 'std.device.getAppVersion':
      return ok(encodeVersion(BUILD.appVersion));
    case 'std.device.getAppBuildId':
      return ok(encodeBuildId(BUILD.appBuildId));
    case 'std.device.getBundleVersion':
      return ok(encodeVersion(BUILD.bundleVersion));
    case 'std.device.getBundleBuildId':
      return ok(encodeBuildId(BUILD.bundleBuildId));
    case 'std.device.getTimeStamp':
      return handleTimeStamp();
    case 'std.device.getViewportSize':
      return ok(encodeViewportSize(window.innerWidth, window.innerHeight));
    case 'std.device.getViewportDpr':
      return ok(encodeViewportDpr(window.devicePixelRatio || 1));
    case 'std.device.getBatteryLevel':
      return handleBattery((battery) => encodeBatteryLevel(battery.level));
    case 'std.device.getBatteryCharging':
      return handleBattery((battery) => encodeBatteryCharging(battery.charging));
    case 'std.device.getNetworkOnline':
      return ok(encodeBool(navigator.onLine));
    case 'std.device.getNetworkKind':
      return ok(encodeNetworkKind(navigator.onLine ? detectNetworkKind() : 3));
    case 'std.position.getCoordinates':
      return handleCoordinates();
    case 'std.config.getConfig':
      return handleConfig(request);
    default:
      return fail({ kind: 'unknownCapability' });
  }
}

function handleCanIUse(request: BridgeRequest): ProviderOutcome {
  const reader = new PostcardReader(request.payload);
  const target = reader.capability();
  const hit = STD_BY_ID.get(capabilityIdString(target));
  if (hit === undefined) return fail({ kind: 'unknownCapability' });
  if (!versionPolicyMatches(request.version, hit)) {
    return fail({ kind: 'versionMismatch', policy: request.version, available: hit });
  }
  return ok(encodeCanIUse(hit));
}

function versionPolicyMatches(policy: VersionPolicy, available: Version): boolean {
  switch (policy.kind) {
    case 'latest':
      return true;
    case 'exact':
      return (
        policy.version.major === available.major &&
        policy.version.minor === available.minor &&
        policy.version.patch === available.patch
      );
    case 'range': {
      const lower = policy.lower ?? { major: 0, minor: 0, patch: 0 };
      const upper = policy.upper ?? { major: 255, minor: 255, patch: 255 };
      return versionLessEqual(lower, available) && versionLessEqual(available, upper);
    }
  }
}

function versionLessEqual(left: Version, right: Version): boolean {
  return (
    left.major < right.major ||
    (left.major === right.major &&
      (left.minor < right.minor || (left.minor === right.minor && left.patch <= right.patch)))
  );
}

function handleTimeStamp(): ProviderOutcome {
  const now = Date.now();
  const offsetSeconds = -new Date(now).getTimezoneOffset() * 60;
  const timezoneId =
    Intl.DateTimeFormat().resolvedOptions().timeZone ?? 'UTC';
  const writer = new PostcardWriter();
  writer.u64(BigInt(now));
  writer.i32(offsetSeconds);
  writer.string(timezoneId);
  return ok(writer.toBytes());
}

function handleBattery(
  select: (battery: { level: number; charging: boolean }) => Uint8Array,
): ProviderOutcome {
  const navigatorAny = navigator as unknown as {
    getBattery?: () => Promise<{ level: number; charging: boolean }>;
  };
  const getBattery = navigatorAny.getBattery?.();
  if (getBattery === undefined) {
    // 无 Battery API：回退默认值（语义与文档一致：未知回 0.0/false）。
    return ok(select({ level: 0, charging: false }));
  }
  return asyncOutcome(
    getBattery.then((battery) => ({ payload: select(battery) })),
  );
}

function handleCoordinates(): ProviderOutcome {
  const geolocation = navigator.geolocation;
  if (geolocation === undefined) {
    return fail({ kind: 'permissionDenied' });
  }
  return asyncOutcome(
    new Promise((resolve) => {
      geolocation.getCurrentPosition(
        (position) => {
          const writer = new PostcardWriter();
          writer.f64(position.coords.latitude);
          writer.f64(position.coords.longitude);
          writer.f32(position.coords.accuracy);
          writer.u64(BigInt(position.timestamp));
          writer.u32(0); // Datum::Wgs84
          resolve({ payload: writer.toBytes() });
        },
        () => resolve({ payload: new Uint8Array(0), error: { kind: 'permissionDenied' } }),
        { enableHighAccuracy: false, timeout: 15_000, maximumAge: 60_000 },
      );
    }),
  );
}

function handleConfig(request: BridgeRequest): ProviderOutcome {
  const reader = new PostcardReader(request.payload);
  const key = reader.string();
  const value = CONFIG.get(key);
  if (value === undefined) return fail({ kind: 'keyNotFound' });
  return ok(encodeConfig(value));
}

function detectNetworkKind(): number {
  const connection = (navigator as unknown as {
    connection?: { effectiveType?: string };
  }).connection;
  const effective = connection?.effectiveType;
  if (effective === undefined) return 3; // Unknown：映射不明确回 Unknown
  if (effective === 'wifi') return 0;
  if (effective === 'ethernet') return 2;
  return 1; // 2g/3g/4g/5g → Cellular
}

// ---------------------------------------------------------------------------
// payload 编码（与 Rust tela-bridge payload.rs 对齐）。
// ---------------------------------------------------------------------------

function encodeCanIUse(hit: Version): Uint8Array {
  const writer = new PostcardWriter();
  writer.version(hit);
  return writer.toBytes();
}

function encodeVersion(version: Version): Uint8Array {
  const writer = new PostcardWriter();
  writer.version(version);
  return writer.toBytes();
}

function encodeAppName(name: string): Uint8Array {
  const writer = new PostcardWriter();
  writer.string(name);
  return writer.toBytes();
}

function encodeBuildId(buildId: number): Uint8Array {
  const writer = new PostcardWriter();
  writer.u32(buildId);
  return writer.toBytes();
}

function encodeBool(value: boolean): Uint8Array {
  const writer = new PostcardWriter();
  writer.bool(value);
  return writer.toBytes();
}

function encodeViewportSize(width: number, height: number): Uint8Array {
  const writer = new PostcardWriter();
  writer.u32(width);
  writer.u32(height);
  return writer.toBytes();
}

function encodeViewportDpr(dpr: number): Uint8Array {
  const writer = new PostcardWriter();
  writer.f32(dpr);
  return writer.toBytes();
}

function encodeBatteryLevel(level: number): Uint8Array {
  const writer = new PostcardWriter();
  writer.f32(level);
  return writer.toBytes();
}

function encodeBatteryCharging(charging: boolean): Uint8Array {
  const writer = new PostcardWriter();
  writer.bool(charging);
  return writer.toBytes();
}

function encodeNetworkKind(kind: number): Uint8Array {
  const writer = new PostcardWriter();
  writer.u32(kind);
  return writer.toBytes();
}

function encodeConfig(value: string): Uint8Array {
  const writer = new PostcardWriter();
  writer.string(value);
  return writer.toBytes();
}

/** 供宿主侧 canIUse 静态表展示/调试。 */
export function registeredCapabilities(): readonly CapabilityId[] {
  return STD_CAPABILITIES;
}

export type { BridgeResult };
