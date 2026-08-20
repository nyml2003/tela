// 桥 wire 一致性测试：JS 编解码器与 Rust tela-bridge 字节样本逐字节对齐。
// 样本由 Rust 侧生成（/tmp/opencode/bridge-sample），此处硬编码断言。

import { decodeRequestPacket, encodeResponseEvent, PostcardReader, PostcardWriter } from './src/webview-sdk/bridge-codec.ts';
import { handleBridgeRequest } from './src/webview-sdk/bridge-providers.ts';

const hexToBytes = (hex) => new Uint8Array(hex.match(/../g).map((b) => parseInt(b, 16)));

let failures = 0;
const check = (name, actual, expected) => {
  const ok = actual === expected;
  if (!ok) failures++;
  console.log(`${ok ? '✓' : '✗'} ${name}${ok ? '' : `\n  actual:   ${JSON.stringify(actual)}\n  expected: ${JSON.stringify(expected)}`}`);
};

// ---- Rust 样本 ----
const REQ_GET_APP_NAME = '544c42520100070000066465766963650a6765744170704e616d6500';
const REQ_GET_CONFIG = '544c4252010003000006636f6e66696709676574436f6e6669670a096170702e7468656d65';
const EV_TIME_STAMP = '544c425201000007001780d095ffbc3180c2030d417369612f5368616e67686169';
const EV_KEY_NOT_FOUND = '544c4252010000090103';
const REQ_CAN_I_USE_RANGE = '544c42520100050201010000000004626173650763616e495573651800066465766963650f676574426174746572794c6576656c';

// ---- 1. 解码：getAppName 请求 ----
{
  const { request, consumed } = decodeRequestPacket(hexToBytes(REQ_GET_APP_NAME));
  check('req_get_app_name.request_id', request.request_id, 7n);
  check('req_get_app_name.version', request.version.kind, 'latest');
  check('req_get_app_name.capability', JSON.stringify(request.capability), JSON.stringify({ scope: 'std', group: 'device', name: 'getAppName' }));
  check('req_get_app_name.payload', request.payload.byteLength, 0);
  check('req_get_app_name.consumed', consumed, REQ_GET_APP_NAME.length / 2);
}

// ---- 2. 解码：getConfig 请求（payload = key） ----
{
  const { request } = decodeRequestPacket(hexToBytes(REQ_GET_CONFIG));
  const reader = new PostcardReader(request.payload);
  check('req_get_config.key', reader.string(), 'app.theme');
}

// ---- 3. 解码：Range 策略 canIUse 请求 ----
{
  const { request } = decodeRequestPacket(hexToBytes(REQ_CAN_I_USE_RANGE));
  check('req_can_i_use_range.version.kind', request.version.kind, 'range');
  check('req_can_i_use_range.lower', JSON.stringify(request.version.lower), JSON.stringify({ major: 1, minor: 0, patch: 0 }));
  check('req_can_i_use_range.upper', request.version.upper, undefined);
  check('req_can_i_use_range.capability', JSON.stringify(request.capability), JSON.stringify({ scope: 'std', group: 'base', name: 'canIUse' }));
  const target = new PostcardReader(request.payload).capability();
  check('req_can_i_use_range.target', JSON.stringify(target), JSON.stringify({ scope: 'std', group: 'device', name: 'getBatteryLevel' }));
}

// ---- 4. 编码：getTimeStamp 响应 == Rust 样本 ----
{
  const writer = new PostcardWriter();
  writer.u64(BigInt(1700000000000));
  writer.i32(28800);
  writer.string('Asia/Shanghai');
  const event = encodeResponseEvent(7n, { kind: 'ok', payload: writer.toBytes() });
  const hex = [...event].map((b) => b.toString(16).padStart(2, '0')).join('');
  check('ev_time_stamp', hex, EV_TIME_STAMP);
}

// ---- 5. 编码：KeyNotFound 错误响应 == Rust 样本 ----
{
  const event = encodeResponseEvent(9n, { kind: 'err', error: { kind: 'keyNotFound' } });
  const hex = [...event].map((b) => b.toString(16).padStart(2, '0')).join('');
  check('ev_key_not_found', hex, EV_KEY_NOT_FOUND);
}

// ---- 6. 解码 → provider → 回投闭环（同步能力） ----
{
  const { request } = decodeRequestPacket(hexToBytes(REQ_GET_APP_NAME));
  const outcome = handleBridgeRequest(request);
  check('provider.getAppName.immediate', outcome.immediate, true);
  const reader = new PostcardReader(outcome.payload);
  check('provider.getAppName.name', reader.string(), '文件管理器');
  const event = encodeResponseEvent(request.request_id, { kind: 'ok', payload: outcome.payload });
  check('provider.getAppName.event 含 magic', String.fromCharCode(...event.subarray(0, 4)), 'TLBR');
}

// ---- 7. 配置命中/未命中 ----
{
  const { request } = decodeRequestPacket(hexToBytes(REQ_GET_CONFIG));
  const hit = handleBridgeRequest(request);
  check('provider.getConfig.hit', new PostcardReader(hit.payload).string(), '"default"');
  const missWriter = new PostcardWriter();
  missWriter.string('no.such.key');
  const miss = handleBridgeRequest({ ...request, payload: missWriter.toBytes() });
  check('provider.getConfig.miss', miss.error?.kind, 'keyNotFound');
}

// ---- 8. 未知能力 ----
{
  const { request } = decodeRequestPacket(hexToBytes(REQ_GET_APP_NAME));
  const unknown = handleBridgeRequest({ ...request, capability: { scope: 'std', group: 'device', name: 'getClipboard' } });
  check('provider.unknownCapability', unknown.error?.kind, 'unknownCapability');
}

console.log(failures === 0 ? '\n全部通过' : `\n${failures} 项失败`);
process.exit(failures === 0 ? 0 : 1);
