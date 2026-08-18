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
    animationFrame = requestAnimationFrame(() => {
      animationFrame = void 0;
      if (closed) return;
      try {
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
  const dispatch = (packet) => {
    if (closed) return false;
    const publication = guest.dispatch(packet);
    input?.synchronize();
    scheduleRender();
    return publication.changed;
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
