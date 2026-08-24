// src/webview-sdk/bindings.ts
async function loadTelaWebviewBindings() {
  const glueUrl = new URL("/tela_webview_host.js", window.location.href).href;
  const glue = await import(
    /* webpackIgnore: true */
    glueUrl
  );
  const wasmUrl = new URL("/tela_webview_host_bg.wasm", window.location.href);
  const response = await fetch(wasmUrl, { cache: "no-store" });
  if (!response.ok) {
    throw new Error(`\u52A0\u8F7D WebView WGPU \u58F3\u5931\u8D25: ${response.status} ${response.statusText}`);
  }
  await glue.default(response);
  return glue;
}

// src/webview-sdk/bundle-loader.ts
async function loadDevelopmentBundle(bindings, bundleIndex) {
  const indexUrl = new URL(bundleIndex, window.location.href);
  const indexResponse = await fetch(indexUrl, { cache: "no-store" });
  if (!indexResponse.ok) {
    throw new Error(`\u8BF7\u6C42\u5F00\u53D1 bundle \u7D22\u5F15\u5931\u8D25: ${indexResponse.status} ${indexResponse.statusText}`);
  }
  const index = bindings.parse_development_index(
    new Uint8Array(await indexResponse.arrayBuffer())
  );
  const archiveUrl = new URL(index.bundle_url, indexUrl);
  const archiveResponse = await fetch(archiveUrl, { cache: "no-store" });
  if (!archiveResponse.ok) {
    throw new Error(`\u8BF7\u6C42\u5F00\u53D1 bundle \u5931\u8D25: ${archiveResponse.status} ${archiveResponse.statusText}`);
  }
  const bundle = bindings.validate_development_bundle(
    index,
    new Uint8Array(await archiveResponse.arrayBuffer())
  );
  return {
    indexUrl,
    archiveUrl,
    bundleId: bundle.bundle_id,
    guestWasm: bundle.app_wasm()
  };
}

// src/webview-sdk/guest-runtime.ts
var MAX_PACKET_BYTES = 64 * 1024 * 1024;
var TelaGuestRuntime = class _TelaGuestRuntime {
  constructor(bindings, exports) {
    this.bindings = bindings;
    this.exports = exports;
  }
  bindings;
  exports;
  latest;
  static async create(bindings, guestWasm) {
    if (guestWasm.byteLength === 0) throw new Error("\u5E94\u7528 guest WASM \u4E3A\u7A7A");
    const source = guestWasm.slice().buffer;
    const instantiated = await WebAssembly.instantiate(source, {});
    const exports = requireGuestExports(instantiated.instance.exports);
    const guestAbi = exports.tela_app_abi_version() >>> 0;
    const hostAbi = bindings.host_app_abi_version() >>> 0;
    if (guestAbi !== hostAbi) {
      throw new Error(`\u5E94\u7528 ABI \u4E0D\u5339\u914D: host=${hostAbi}, guest=${guestAbi}`);
    }
    return new _TelaGuestRuntime(bindings, exports);
  }
  /** Runs one guest initialization and reads its first frame/status publication. */
  initialize() {
    const initialized = this.exports.tela_app_init() >>> 0;
    if (initialized === 0) {
      throw new Error(this.guestError() || "\u5E94\u7528 guest \u521D\u59CB\u5316\u5931\u8D25");
    }
    return this.refresh(true);
  }
  /** Delivers one Rust-encoded event and atomically refreshes the public frame/status pair. */
  dispatch(packet) {
    if (packet.byteLength > MAX_PACKET_BYTES) {
      throw new Error(`\u5E94\u7528\u4E8B\u4EF6\u5305\u8D85\u8FC7 ${MAX_PACKET_BYTES / 1024 / 1024} MiB \u9650\u5236`);
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
  bridgeAvailable() {
    return this.exports.tela_app_request_begin !== void 0 && this.exports.tela_app_request_len !== void 0 && this.exports.tela_app_bridge_dispatch_begin !== void 0 && this.exports.tela_app_bridge_dispatch !== void 0;
  }
  /** Length of the guest's queued bridge request packets; `0` when unavailable. */
  bridgeRequestLen() {
    const requestLen = this.exports.tela_app_request_len;
    if (!requestLen) return 0;
    return requestLen() >>> 0;
  }
  /** Reads the guest's queued bridge request packets (raw bytes, caller decodes). */
  bridgeReadRequests() {
    const requestBegin = this.exports.tela_app_request_begin;
    if (!requestBegin) return new Uint8Array(0);
    const length = this.bridgeRequestLen();
    if (length === 0) return new Uint8Array(0);
    const pointer = requestBegin(0) >>> 0;
    return this.copyFromGuest(pointer, length, "bridge requests");
  }
  /** Delivers one encoded bridge event packet (response) to the guest. */
  bridgeDeliver(packet) {
    const dispatchBegin = this.exports.tela_app_bridge_dispatch_begin;
    const dispatch = this.exports.tela_app_bridge_dispatch;
    if (!dispatchBegin || !dispatch) {
      throw new Error("\u5E94\u7528 guest \u672A\u66B4\u9732\u6865 ABI");
    }
    if (packet.byteLength > MAX_PACKET_BYTES) {
      throw new Error(`\u6865\u4E8B\u4EF6\u5305\u8D85\u8FC7 ${MAX_PACKET_BYTES / 1024 / 1024} MiB \u9650\u5236`);
    }
    const pointer = dispatchBegin(packet.byteLength) >>> 0;
    this.copyIntoGuest(pointer, packet);
    dispatch(packet.byteLength);
  }
  /** Latest guest frame; valid only after initialization. */
  framePacket() {
    return this.publication().framePacket;
  }
  /** Latest host-visible non-drawing application state. */
  status() {
    return this.publication().status;
  }
  refresh(changed) {
    const framePacket = this.readGuestExport(
      this.exports.tela_app_frame_ptr,
      this.exports.tela_app_frame_len,
      "frame"
    );
    const statusPacket = this.readGuestExport(
      this.exports.tela_app_status_ptr,
      this.exports.tela_app_status_len,
      "status"
    );
    const status = this.bindings.decode_app_status(statusPacket);
    const publication = { changed, framePacket, status };
    this.latest = publication;
    return publication;
  }
  guestError() {
    const bytes = this.readGuestExport(
      this.exports.tela_app_error_ptr,
      this.exports.tela_app_error_len,
      "error"
    );
    return new TextDecoder().decode(bytes).trim();
  }
  readGuestExport(pointerExport, lengthExport, label) {
    const pointer = pointerExport() >>> 0;
    const length = lengthExport() >>> 0;
    if (length > MAX_PACKET_BYTES) {
      throw new Error(`\u5E94\u7528 ${label} \u5305\u8D85\u8FC7 ${MAX_PACKET_BYTES / 1024 / 1024} MiB \u9650\u5236`);
    }
    return this.copyFromGuest(pointer, length, label);
  }
  copyIntoGuest(pointer, bytes) {
    this.requireRange(pointer, bytes.byteLength, "event");
    new Uint8Array(this.exports.memory.buffer, pointer, bytes.byteLength).set(bytes);
  }
  copyFromGuest(pointer, length, label) {
    this.requireRange(pointer, length, label);
    return new Uint8Array(this.exports.memory.buffer, pointer, length).slice();
  }
  requireRange(pointer, length, label) {
    const memoryBytes = this.exports.memory.buffer.byteLength;
    if (pointer > memoryBytes || length > memoryBytes - pointer) {
      throw new Error(
        `\u5E94\u7528 ${label} \u6307\u9488\u8D8A\u8FC7 WASM \u7EBF\u6027\u5185\u5B58: ptr=${pointer}, len=${length}, memory=${memoryBytes}`
      );
    }
  }
  publication() {
    if (!this.latest) throw new Error("\u5E94\u7528 guest \u5C1A\u672A\u521D\u59CB\u5316");
    return this.latest;
  }
};
function requireGuestExports(exports) {
  const memory = exports.memory;
  if (!(memory instanceof WebAssembly.Memory)) {
    throw new Error("\u5E94\u7528 guest \u5FC5\u987B\u5C06\u7EBF\u6027\u5185\u5B58\u5BFC\u51FA\u4E3A `memory`");
  }
  const names = [
    "tela_app_abi_version",
    "tela_app_init",
    "tela_app_input_begin",
    "tela_app_dispatch",
    "tela_app_frame_ptr",
    "tela_app_frame_len",
    "tela_app_status_ptr",
    "tela_app_status_len",
    "tela_app_error_ptr",
    "tela_app_error_len"
  ];
  const required = Object.fromEntries(names.map((name) => [name, exports[name]]));
  for (const name of names) {
    if (typeof required[name] !== "function") {
      throw new Error(`\u5E94\u7528 guest \u7F3A\u5C11\u51FD\u6570\u5BFC\u51FA: ${name}`);
    }
  }
  return { memory, ...required };
}

// src/webview-sdk/input-bridge.ts
var MODIFIER_SHIFT = 1 << 0;
var MODIFIER_CTRL = 1 << 1;
var MODIFIER_ALT = 1 << 2;
var MODIFIER_META = 1 << 3;
var PHYSICAL_KEY_CODES = {
  KeyA: 4,
  KeyB: 5,
  KeyC: 6,
  KeyD: 7,
  KeyE: 8,
  KeyF: 9,
  KeyG: 10,
  KeyH: 11,
  KeyI: 12,
  KeyJ: 13,
  KeyK: 14,
  KeyL: 15,
  KeyM: 16,
  KeyN: 17,
  KeyO: 18,
  KeyP: 19,
  KeyQ: 20,
  KeyR: 21,
  KeyS: 22,
  KeyT: 23,
  KeyU: 24,
  KeyV: 25,
  KeyW: 26,
  KeyX: 27,
  KeyY: 28,
  KeyZ: 29,
  Digit1: 30,
  Digit2: 31,
  Digit3: 32,
  Digit4: 33,
  Digit5: 34,
  Digit6: 35,
  Digit7: 36,
  Digit8: 37,
  Digit9: 38,
  Digit0: 39,
  Enter: 40,
  Escape: 41,
  Backspace: 42,
  Tab: 43,
  Space: 44,
  Insert: 73,
  Home: 74,
  PageUp: 75,
  Delete: 76,
  End: 77,
  PageDown: 78,
  ArrowRight: 79,
  ArrowLeft: 80,
  ArrowDown: 81,
  ArrowUp: 82
};
function installInputBridge(options) {
  const { canvas, bindings, dispatch, status, presentedFrameToken, viewport } = options;
  let composing = false;
  let closed = false;
  const editor = document.createElement("textarea");
  editor.setAttribute("aria-label", "tela text input");
  editor.autocapitalize = "off";
  editor.autocomplete = "off";
  editor.spellcheck = false;
  Object.assign(editor.style, {
    position: "fixed",
    left: "0",
    top: "0",
    width: "1px",
    height: "1px",
    opacity: "0",
    pointerEvents: "none",
    border: "0",
    padding: "0",
    resize: "none"
  });
  document.body.append(editor);
  const point = (event) => {
    const bounds = canvas.getBoundingClientRect();
    const logical = viewport();
    return {
      x: (event.clientX - bounds.left) * logical.width / Math.max(bounds.width, 1),
      y: (event.clientY - bounds.top) * logical.height / Math.max(bounds.height, 1)
    };
  };
  const syncCursor = () => {
    canvas.style.cursor = ["default", "text", "pointer"][status().cursor] ?? "default";
  };
  const synchronize = (restoreCanvas = false) => {
    if (closed) return;
    const appStatus = status();
    syncCursor();
    if (appStatus.input_focused) {
      if (document.activeElement !== editor) {
        editor.value = appStatus.input_value;
        editor.focus({ preventScroll: true });
      } else if (!composing && editor.value !== appStatus.input_value) {
        editor.value = appStatus.input_value;
      }
    } else if (document.activeElement === editor) {
      editor.blur();
    }
    if (restoreCanvas && !appStatus.input_focused && document.activeElement !== canvas) {
      canvas.focus({ preventScroll: true });
    }
  };
  const send = (packet) => {
    const consumed = dispatch(packet);
    synchronize();
    return consumed;
  };
  const sendFrameInput = (encode) => {
    const sourceFrameToken = presentedFrameToken();
    return sourceFrameToken === void 0 ? false : send(encode(sourceFrameToken));
  };
  const pointerKind = (pointerType) => {
    if (pointerType === "touch") return 1;
    if (pointerType === "pen") return 2;
    return 0;
  };
  const timestampMicros = (timestamp) => BigInt(Math.max(0, Math.round(timestamp * 1e3)));
  const pointerId = (id) => BigInt(Math.max(0, Math.trunc(id)));
  const sendPointer = (event, phase, position = point(event), deltaX = 0, deltaY = 0) => sendFrameInput((sourceFrameToken) => bindings.event_pointer(
    sourceFrameToken,
    pointerId(event.pointerId),
    pointerKind(event.pointerType),
    phase,
    position.x,
    position.y,
    Math.max(0, Math.min(65535, Math.trunc(event.buttons))),
    timestampMicros(event.timeStamp),
    deltaX,
    deltaY
  ));
  const onPointerDown = (event) => {
    event.preventDefault();
    canvas.setPointerCapture?.(event.pointerId);
    sendPointer(event, 0);
    synchronize(true);
  };
  const onPointerUp = (event) => {
    event.preventDefault();
    sendPointer(event, 2);
  };
  const onPointerMove = (event) => {
    sendPointer(event, 1);
  };
  const onPointerCancel = (event) => {
    sendPointer(event, 3);
  };
  const onPointerLeave = (event) => {
    sendPointer(event, 1, { x: -1, y: -1 });
  };
  const onWheel = (event) => {
    event.preventDefault();
    const position = point(event);
    const logical = viewport();
    const unit = event.deltaMode === WheelEvent.DOM_DELTA_LINE ? 16 : event.deltaMode === WheelEvent.DOM_DELTA_PAGE ? logical.height : 1;
    const bounds = canvas.getBoundingClientRect();
    sendFrameInput((sourceFrameToken) => bindings.event_pointer(
      sourceFrameToken,
      0n,
      0,
      4,
      position.x,
      position.y,
      0,
      timestampMicros(event.timeStamp),
      event.deltaX * unit * logical.width / Math.max(bounds.width, 1),
      event.deltaY * unit * logical.height / Math.max(bounds.height, 1)
    ));
  };
  const onCanvasKeyDown = (event) => {
    if (event.isComposing || status().input_focused) return;
    const physicalKey = PHYSICAL_KEY_CODES[event.code];
    if (physicalKey === void 0) return;
    if (sendFrameInput((sourceFrameToken) => bindings.event_key_down(
      sourceFrameToken,
      physicalKey,
      modifierBits(event),
      event.repeat
    ))) {
      event.preventDefault();
    }
  };
  const onEditorFocus = () => {
    sendFrameInput((sourceFrameToken) => bindings.event_input_focus(sourceFrameToken));
  };
  const onEditorInput = () => {
    sendFrameInput((sourceFrameToken) => bindings.event_set_input_value(sourceFrameToken, editor.value));
  };
  const onCompositionStart = () => {
    composing = true;
    sendFrameInput((sourceFrameToken) => bindings.event_input_composition_start(sourceFrameToken));
  };
  const onCompositionEnd = () => {
    sendFrameInput((sourceFrameToken) => bindings.event_input_composition_end(sourceFrameToken));
    composing = false;
    synchronize();
  };
  const onEditorKeyDown = (event) => {
    if (event.key === "Enter" && !event.isComposing) {
      event.preventDefault();
      sendFrameInput((sourceFrameToken) => bindings.event_input_enter(sourceFrameToken));
      return;
    }
    if (event.key === "Escape") {
      event.preventDefault();
      sendFrameInput((sourceFrameToken) => bindings.event_input_cancel(sourceFrameToken));
      return;
    }
    if (event.code !== "Tab" || event.isComposing) return;
    const physicalKey = PHYSICAL_KEY_CODES[event.code];
    if (physicalKey !== void 0 && sendFrameInput((sourceFrameToken) => bindings.event_key_down(
      sourceFrameToken,
      physicalKey,
      modifierBits(event),
      event.repeat
    ))) {
      event.preventDefault();
      synchronize(true);
    }
  };
  const onEditorBlur = () => {
    composing = false;
    sendFrameInput((sourceFrameToken) => bindings.event_input_blur(sourceFrameToken));
  };
  canvas.addEventListener("pointerdown", onPointerDown);
  canvas.addEventListener("pointerup", onPointerUp);
  canvas.addEventListener("pointermove", onPointerMove);
  canvas.addEventListener("pointercancel", onPointerCancel);
  canvas.addEventListener("pointerleave", onPointerLeave);
  canvas.addEventListener("wheel", onWheel, { passive: false });
  canvas.addEventListener("keydown", onCanvasKeyDown);
  editor.addEventListener("focus", onEditorFocus);
  editor.addEventListener("input", onEditorInput);
  editor.addEventListener("compositionstart", onCompositionStart);
  editor.addEventListener("compositionend", onCompositionEnd);
  editor.addEventListener("keydown", onEditorKeyDown);
  editor.addEventListener("blur", onEditorBlur);
  return {
    synchronize: () => synchronize(),
    close: () => {
      if (closed) return;
      closed = true;
      canvas.removeEventListener("pointerdown", onPointerDown);
      canvas.removeEventListener("pointerup", onPointerUp);
      canvas.removeEventListener("pointermove", onPointerMove);
      canvas.removeEventListener("pointercancel", onPointerCancel);
      canvas.removeEventListener("pointerleave", onPointerLeave);
      canvas.removeEventListener("wheel", onWheel);
      canvas.removeEventListener("keydown", onCanvasKeyDown);
      editor.removeEventListener("focus", onEditorFocus);
      editor.removeEventListener("input", onEditorInput);
      editor.removeEventListener("compositionstart", onCompositionStart);
      editor.removeEventListener("compositionend", onCompositionEnd);
      editor.removeEventListener("keydown", onEditorKeyDown);
      editor.removeEventListener("blur", onEditorBlur);
      editor.remove();
      canvas.style.cursor = "";
    }
  };
}
function modifierBits(event) {
  return (event.shiftKey ? MODIFIER_SHIFT : 0) | (event.ctrlKey ? MODIFIER_CTRL : 0) | (event.altKey ? MODIFIER_ALT : 0) | (event.metaKey ? MODIFIER_META : 0);
}

// src/webview-sdk/surface.ts
function syncCanvasSurface(canvas) {
  const bounds = canvas.getBoundingClientRect();
  const ratio = window.devicePixelRatio > 0 ? window.devicePixelRatio : 1;
  const pixelWidth = Math.max(1, Math.round(bounds.width * ratio));
  const pixelHeight = Math.max(1, Math.round(bounds.height * ratio));
  if (canvas.width !== pixelWidth) canvas.width = pixelWidth;
  if (canvas.height !== pixelHeight) canvas.height = pixelHeight;
  return {
    logicalWidth: Math.max(320, Math.round(bounds.width)),
    logicalHeight: Math.max(240, Math.round(bounds.height)),
    pixelWidth,
    pixelHeight
  };
}
function observeCanvasSurface(canvas, onChange) {
  const sync = () => onChange(syncCanvasSurface(canvas));
  const observer = new ResizeObserver(sync);
  observer.observe(canvas);
  window.addEventListener("resize", sync);
  let resolutionQuery;
  const installResolutionQuery = () => {
    resolutionQuery?.removeEventListener("change", onResolutionChange);
    resolutionQuery = window.matchMedia(`(resolution: ${window.devicePixelRatio}dppx)`);
    resolutionQuery.addEventListener("change", onResolutionChange, { once: true });
  };
  const onResolutionChange = () => {
    sync();
    installResolutionQuery();
  };
  installResolutionQuery();
  return () => {
    observer.disconnect();
    window.removeEventListener("resize", sync);
    resolutionQuery?.removeEventListener("change", onResolutionChange);
  };
}

// src/webview-sdk/bridge-codec.ts
var MAGIC = [84, 76, 66, 82];
var PACKET_VERSION = 1;
var PostcardReader = class {
  constructor(data, start2 = 0) {
    this.data = data;
    this.offset = start2;
  }
  data;
  offset;
  /** 当前读取位置（包内偏移，不含 magic 头）。 */
  position() {
    return this.offset;
  }
  require(n) {
    if (this.offset + n > this.data.length) {
      throw new Error("\u6865\u5305\u8D8A\u754C");
    }
  }
  u8() {
    this.require(1);
    return this.data[this.offset++];
  }
  bool() {
    return this.u8() !== 0;
  }
  /** LEB128 varint，最高 64 位。 */
  varint() {
    let result = 0n;
    let shift = 0n;
    for (let i = 0; i < 10; i++) {
      const byte = this.u8();
      result |= BigInt(byte & 127) << shift;
      if ((byte & 128) === 0) return result;
      shift += 7n;
    }
    throw new Error("\u6865\u5305 varint \u8FC7\u957F");
  }
  u32() {
    const value = this.varint();
    if (value > 0xffffffffn) throw new Error("\u6865\u5305 u32 \u8D8A\u754C");
    return Number(value);
  }
  u64() {
    return this.varint();
  }
  /** zigzag varint（postcard 对 i32 的编码）。 */
  i32() {
    const zig = this.varint();
    const value = zig >> 1n ^ -(zig & 1n);
    if (value > 0x7fffffffn || value < -0x80000000n) throw new Error("\u6865\u5305 i32 \u8D8A\u754C");
    return Number(value);
  }
  f32() {
    this.require(4);
    const view = new DataView(this.data.buffer, this.data.byteOffset + this.offset, 4);
    this.offset += 4;
    return view.getFloat32(0, true);
  }
  f64() {
    this.require(8);
    const view = new DataView(this.data.buffer, this.data.byteOffset + this.offset, 8);
    this.offset += 8;
    return view.getFloat64(0, true);
  }
  string() {
    const length = this.u32();
    this.require(length);
    const bytes = this.data.subarray(this.offset, this.offset + length);
    this.offset += length;
    return new TextDecoder().decode(bytes);
  }
  bytes() {
    const length = this.u32();
    this.require(length);
    const bytes = this.data.slice(this.offset, this.offset + length);
    this.offset += length;
    return bytes;
  }
  version() {
    return { major: this.u32(), minor: this.u32(), patch: this.u32() };
  }
  optionVersion() {
    return this.u8() === 0 ? void 0 : this.version();
  }
  versionPolicy() {
    const tag = this.u32();
    switch (tag) {
      case 0:
        return { kind: "latest" };
      case 1:
        return { kind: "exact", version: this.version() };
      case 2:
        return { kind: "range", lower: this.optionVersion(), upper: this.optionVersion() };
      default:
        throw new Error(`\u672A\u77E5 VersionPolicy tag ${tag}`);
    }
  }
  capability() {
    const scopeTag = this.u32();
    const scope = scopeTag === 0 ? "std" : { named: this.string() };
    return { scope, group: this.string(), name: this.string() };
  }
};
var PostcardWriter = class {
  chunks = [];
  u8(value) {
    this.chunks.push(value & 255);
  }
  bool(value) {
    this.u8(value ? 1 : 0);
  }
  varint(value) {
    let remaining = value;
    for (; ; ) {
      const byte = Number(remaining & 0x7fn);
      remaining >>= 7n;
      if (remaining === 0n) {
        this.chunks.push(byte);
        return;
      }
      this.chunks.push(byte | 128);
    }
  }
  u32(value) {
    this.varint(BigInt(value >>> 0));
  }
  u64(value) {
    this.varint(value);
  }
  /** zigzag varint（postcard 对 i32 的编码）。 */
  i32(value) {
    const zig = BigInt(value) << 1n ^ BigInt(value) >> 63n;
    this.varint(zig);
  }
  f32(value) {
    const buffer = new ArrayBuffer(4);
    new DataView(buffer).setFloat32(0, value, true);
    this.chunks.push(...new Uint8Array(buffer));
  }
  f64(value) {
    const buffer = new ArrayBuffer(8);
    new DataView(buffer).setFloat64(0, value, true);
    this.chunks.push(...new Uint8Array(buffer));
  }
  string(value) {
    const bytes = new TextEncoder().encode(value);
    this.u32(bytes.length);
    this.chunks.push(...bytes);
  }
  bytes(value) {
    this.u32(value.byteLength);
    this.chunks.push(...value);
  }
  version(value) {
    this.u32(value.major);
    this.u32(value.minor);
    this.u32(value.patch);
  }
  optionVersion(value) {
    if (value === void 0) {
      this.u8(0);
    } else {
      this.u8(1);
      this.version(value);
    }
  }
  versionPolicy(value) {
    switch (value.kind) {
      case "latest":
        this.u32(0);
        break;
      case "exact":
        this.u32(1);
        this.version(value.version);
        break;
      case "range":
        this.u32(2);
        this.optionVersion(value.lower);
        this.optionVersion(value.upper);
        break;
    }
  }
  capability(value) {
    if (value.scope === "std") {
      this.u32(0);
    } else {
      this.u32(1);
      this.string(value.scope.named);
    }
    this.string(value.group);
    this.string(value.name);
  }
  toBytes() {
    return new Uint8Array(this.chunks);
  }
};
function packetHeader() {
  return new Uint8Array([...MAGIC, PACKET_VERSION & 255, PACKET_VERSION >> 8 & 255]);
}
function validatePacketHeader(bytes) {
  if (bytes.byteLength < 6) throw new Error("\u6865\u5305\u8FC7\u77ED");
  for (let i = 0; i < 4; i++) {
    if (bytes[i] !== MAGIC[i]) throw new Error("\u6865\u5305 magic \u4E0D\u5339\u914D");
  }
  const version = bytes[4] | bytes[5] << 8;
  if (version !== PACKET_VERSION) throw new Error(`\u6865\u5305\u7248\u672C\u4E0D\u5339\u914D: ${version}`);
}
function decodeRequestPacket(bytes) {
  validatePacketHeader(bytes);
  const reader = new PostcardReader(bytes, 6);
  const request = {
    request_id: reader.u64(),
    version: reader.versionPolicy(),
    capability: reader.capability(),
    payload: reader.bytes()
  };
  return { request, consumed: reader.position() };
}
function encodeResponseEvent(requestId, result) {
  const writer = new PostcardWriter();
  writer.u32(0);
  writer.u64(requestId);
  if (result.kind === "ok") {
    writer.u32(0);
    writer.bytes(result.payload);
  } else {
    writer.u32(1);
    switch (result.error.kind) {
      case "unknownCapability":
        writer.u32(0);
        break;
      case "versionMismatch":
        writer.u32(1);
        writer.versionPolicy(result.error.policy);
        writer.version(result.error.available);
        break;
      case "permissionDenied":
        writer.u32(2);
        break;
      case "keyNotFound":
        writer.u32(3);
        break;
      case "timeout":
        writer.u32(4);
        break;
    }
  }
  return new Uint8Array([...packetHeader(), ...writer.toBytes()]);
}

// src/webview-sdk/bridge-providers.ts
var V1_0_0 = { major: 1, minor: 0, patch: 0 };
var STD_CAPABILITIES = [
  { scope: "std", group: "base", name: "canIUse" },
  { scope: "std", group: "device", name: "getAppName" },
  { scope: "std", group: "device", name: "getAppVersion" },
  { scope: "std", group: "device", name: "getAppBuildId" },
  { scope: "std", group: "device", name: "getBundleVersion" },
  { scope: "std", group: "device", name: "getBundleBuildId" },
  { scope: "std", group: "device", name: "getTimeStamp" },
  { scope: "std", group: "device", name: "getViewportSize" },
  { scope: "std", group: "device", name: "getViewportDpr" },
  { scope: "std", group: "device", name: "getBatteryLevel" },
  { scope: "std", group: "device", name: "getBatteryCharging" },
  { scope: "std", group: "device", name: "getNetworkOnline" },
  { scope: "std", group: "device", name: "getNetworkKind" },
  { scope: "std", group: "position", name: "getCoordinates" },
  { scope: "std", group: "config", name: "getConfig" }
];
var STD_BY_ID = new Map(
  STD_CAPABILITIES.map((capability) => [capabilityIdString(capability), V1_0_0])
);
function capabilityIdString(id) {
  return id.scope === "std" ? `std.${id.group}.${id.name}` : `${id.scope.named}.${id.group}.${id.name}`;
}
var BUILD = {
  appName: "\u6587\u4EF6\u7BA1\u7406\u5668",
  appVersion: { major: 0, minor: 1, patch: 0 },
  appBuildId: 1,
  bundleVersion: { major: 0, minor: 1, patch: 0 },
  bundleBuildId: 1
};
var CONFIG = /* @__PURE__ */ new Map([["app.theme", '"default"']]);
function ok(payload) {
  return { immediate: true, payload };
}
function fail(error) {
  return { immediate: true, payload: new Uint8Array(0), error };
}
function asyncOutcome(promise) {
  return { immediate: false, payload: new Uint8Array(0), promise };
}
function handleBridgeRequest(request) {
  const id = capabilityIdString(request.capability);
  switch (id) {
    case "std.base.canIUse":
      return handleCanIUse(request);
    case "std.device.getAppName":
      return ok(encodeAppName(BUILD.appName));
    case "std.device.getAppVersion":
      return ok(encodeVersion(BUILD.appVersion));
    case "std.device.getAppBuildId":
      return ok(encodeBuildId(BUILD.appBuildId));
    case "std.device.getBundleVersion":
      return ok(encodeVersion(BUILD.bundleVersion));
    case "std.device.getBundleBuildId":
      return ok(encodeBuildId(BUILD.bundleBuildId));
    case "std.device.getTimeStamp":
      return handleTimeStamp();
    case "std.device.getViewportSize":
      return ok(encodeViewportSize(window.innerWidth, window.innerHeight));
    case "std.device.getViewportDpr":
      return ok(encodeViewportDpr(window.devicePixelRatio || 1));
    case "std.device.getBatteryLevel":
      return handleBattery((battery) => encodeBatteryLevel(battery.level));
    case "std.device.getBatteryCharging":
      return handleBattery((battery) => encodeBatteryCharging(battery.charging));
    case "std.device.getNetworkOnline":
      return ok(encodeBool(navigator.onLine));
    case "std.device.getNetworkKind":
      return ok(encodeNetworkKind(navigator.onLine ? detectNetworkKind() : 3));
    case "std.position.getCoordinates":
      return handleCoordinates();
    case "std.config.getConfig":
      return handleConfig(request);
    default:
      return fail({ kind: "unknownCapability" });
  }
}
function handleCanIUse(request) {
  const reader = new PostcardReader(request.payload);
  const target = reader.capability();
  const hit = STD_BY_ID.get(capabilityIdString(target));
  if (hit === void 0) return fail({ kind: "unknownCapability" });
  if (!versionPolicyMatches(request.version, hit)) {
    return fail({ kind: "versionMismatch", policy: request.version, available: hit });
  }
  return ok(encodeCanIUse(hit));
}
function versionPolicyMatches(policy, available) {
  switch (policy.kind) {
    case "latest":
      return true;
    case "exact":
      return policy.version.major === available.major && policy.version.minor === available.minor && policy.version.patch === available.patch;
    case "range": {
      const lower = policy.lower ?? { major: 0, minor: 0, patch: 0 };
      const upper = policy.upper ?? { major: 255, minor: 255, patch: 255 };
      return versionLessEqual(lower, available) && versionLessEqual(available, upper);
    }
  }
}
function versionLessEqual(left, right) {
  return left.major < right.major || left.major === right.major && (left.minor < right.minor || left.minor === right.minor && left.patch <= right.patch);
}
function handleTimeStamp() {
  const now = Date.now();
  const offsetSeconds = -new Date(now).getTimezoneOffset() * 60;
  const timezoneId = Intl.DateTimeFormat().resolvedOptions().timeZone ?? "UTC";
  const writer = new PostcardWriter();
  writer.u64(BigInt(now));
  writer.i32(offsetSeconds);
  writer.string(timezoneId);
  return ok(writer.toBytes());
}
function handleBattery(select) {
  const navigatorAny = navigator;
  const getBattery = navigatorAny.getBattery?.();
  if (getBattery === void 0) {
    return ok(select({ level: 0, charging: false }));
  }
  return asyncOutcome(
    getBattery.then((battery) => ({ payload: select(battery) }))
  );
}
function handleCoordinates() {
  const geolocation = navigator.geolocation;
  if (geolocation === void 0) {
    return fail({ kind: "permissionDenied" });
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
          writer.u32(0);
          resolve({ payload: writer.toBytes() });
        },
        () => resolve({ payload: new Uint8Array(0), error: { kind: "permissionDenied" } }),
        { enableHighAccuracy: false, timeout: 15e3, maximumAge: 6e4 }
      );
    })
  );
}
function handleConfig(request) {
  const reader = new PostcardReader(request.payload);
  const key = reader.string();
  const value = CONFIG.get(key);
  if (value === void 0) return fail({ kind: "keyNotFound" });
  return ok(encodeConfig(value));
}
function detectNetworkKind() {
  const connection = navigator.connection;
  const effective = connection?.effectiveType;
  if (effective === void 0) return 3;
  if (effective === "wifi") return 0;
  if (effective === "ethernet") return 2;
  return 1;
}
function encodeCanIUse(hit) {
  const writer = new PostcardWriter();
  writer.version(hit);
  return writer.toBytes();
}
function encodeVersion(version) {
  const writer = new PostcardWriter();
  writer.version(version);
  return writer.toBytes();
}
function encodeAppName(name) {
  const writer = new PostcardWriter();
  writer.string(name);
  return writer.toBytes();
}
function encodeBuildId(buildId) {
  const writer = new PostcardWriter();
  writer.u32(buildId);
  return writer.toBytes();
}
function encodeBool(value) {
  const writer = new PostcardWriter();
  writer.bool(value);
  return writer.toBytes();
}
function encodeViewportSize(width, height) {
  const writer = new PostcardWriter();
  writer.u32(width);
  writer.u32(height);
  return writer.toBytes();
}
function encodeViewportDpr(dpr) {
  const writer = new PostcardWriter();
  writer.f32(dpr);
  return writer.toBytes();
}
function encodeBatteryLevel(level) {
  const writer = new PostcardWriter();
  writer.f32(level);
  return writer.toBytes();
}
function encodeBatteryCharging(charging) {
  const writer = new PostcardWriter();
  writer.bool(charging);
  return writer.toBytes();
}
function encodeNetworkKind(kind) {
  const writer = new PostcardWriter();
  writer.u32(kind);
  return writer.toBytes();
}
function encodeConfig(value) {
  const writer = new PostcardWriter();
  writer.string(value);
  return writer.toBytes();
}

// src/webview-sdk/index.ts
async function startTelaWebview(options) {
  const { canvas, bundleIndex } = options;
  if (!navigator.gpu) {
    throw new Error("\u5F53\u524D WebView \u4E0D\u652F\u6301 WebGPU\uFF1B\u672C\u9636\u6BB5\u6D4F\u89C8\u5668 SDK \u4EC5\u63D0\u4F9B WGPU \u8DEF\u5F84");
  }
  const bindings = await loadTelaWebviewBindings();
  const bundle = await loadDevelopmentBundle(bindings, bundleIndex);
  const guest = await TelaGuestRuntime.create(bindings, bundle.guestWasm);
  const initialSurface = syncCanvasSurface(canvas);
  let currentSurface = initialSurface;
  let closed = false;
  let input;
  let stopSurfaceObservation;
  let animationFrame;
  let retryTimer;
  let renderError;
  let presentedFrameToken;
  const scheduleRender = () => {
    if (closed || animationFrame !== void 0) return;
    animationFrame = requestAnimationFrame((timestamp) => {
      animationFrame = void 0;
      if (closed) return;
      try {
        if (guest.status().animation_active) {
          dispatch(bindings.event_tick(BigInt(Math.floor(timestamp))), false);
        }
        const framePacket = guest.framePacket();
        const frameToken = guest.status().frame_token;
        if (!bindings.render_gpu(framePacket)) {
          presentedFrameToken = void 0;
          retryTimer ??= window.setTimeout(() => {
            retryTimer = void 0;
            scheduleRender();
          }, 100);
        } else {
          presentedFrameToken = frameToken;
          renderError = void 0;
          if (guest.status().animation_active) scheduleRender();
        }
      } catch (error) {
        const message = `tela WebView WGPU render failed: ${String(error)}; ${bindings.gpu_diagnostics()}`;
        if (message !== renderError) {
          console.error(message);
          renderError = message;
        }
        presentedFrameToken = void 0;
      }
    });
  };
  const dispatch = (packet, synchronizeClock = true) => {
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
  const processBridgeRequests = () => {
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
        console.error(`\u6865\u8BF7\u6C42\u5305\u89E3\u7801\u5931\u8D25: ${String(error)}`);
        break;
      }
      const request = decoded.request;
      offset += decoded.consumed;
      const outcome = handleBridgeRequest(request);
      if (outcome.immediate) {
        guest.bridgeDeliver(
          encodeResponseEvent(
            request.request_id,
            outcome.error ? { kind: "err", error: outcome.error } : { kind: "ok", payload: outcome.payload }
          )
        );
      } else {
        outcome.promise?.then((result) => {
          if (closed) return;
          guest.bridgeDeliver(
            encodeResponseEvent(
              request.request_id,
              result.error ? { kind: "err", error: result.error } : { kind: "ok", payload: result.payload }
            )
          );
          scheduleRender();
        }).catch((error) => {
          console.error(`\u6865\u5F02\u6B65 provider \u5931\u8D25: ${String(error)}`);
        });
      }
      handled++;
    }
    if (handled > 0) {
      console.debug(`tela bridge: \u5904\u7406 ${handled} \u4E2A\u8BF7\u6C42`);
    }
  };
  const dispatchViewport = (surface) => {
    currentSurface = surface;
    presentedFrameToken = void 0;
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
        height: currentSurface.logicalHeight
      })
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
    replaceKeymap(snapshot) {
      const json = typeof snapshot === "string" ? snapshot : JSON.stringify(snapshot);
      return dispatch(bindings.event_replace_keymap_json(json));
    },
    close() {
      if (closed) return;
      closed = true;
      if (animationFrame !== void 0) cancelAnimationFrame(animationFrame);
      if (retryTimer !== void 0) window.clearTimeout(retryTimer);
      stopSurfaceObservation?.();
      input?.close();
      presentedFrameToken = void 0;
      bindings.shutdown_gpu();
    }
  };
}

// src/main.ts
async function start() {
  const canvas = document.querySelector("canvas");
  if (!canvas) throw new Error("\u7F3A\u5C11\u6587\u4EF6\u7BA1\u7406\u5668 canvas");
  const session = await startTelaWebview({
    canvas,
    bundleIndex: new URL("/tela-dev/latest.json", window.location.href)
  });
  window.telaReplaceKeymap = (snapshot) => session.replaceKeymap(snapshot);
  window.addEventListener("pagehide", () => {
    session.close();
    delete window.telaReplaceKeymap;
  }, { once: true });
}
function showStartupError(error) {
  const message = error instanceof Error ? error.message : String(error);
  console.error(error);
  document.body.dataset.error = message;
  const notice = document.querySelector("#startup-error");
  if (notice) {
    notice.hidden = false;
    notice.textContent = `\u542F\u52A8\u5931\u8D25\uFF1A${message}`;
  }
}
void start().catch(showStartupError);
//# sourceMappingURL=app.js.map
