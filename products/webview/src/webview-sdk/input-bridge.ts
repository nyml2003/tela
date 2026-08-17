// DOM input/IME 到平台无关 ABI 的适配。`KeyboardEvent.code` 在这里映射为物理键；
// 语义意图和运行时键位表始终在 application guest 内解析。

import type { TelaWebviewBindings, WebAppStatus } from './bindings';

const MODIFIER_SHIFT = 1 << 0;
const MODIFIER_CTRL = 1 << 1;
const MODIFIER_ALT = 1 << 2;
const MODIFIER_META = 1 << 3;

const PHYSICAL_KEY_CODES: Readonly<Record<string, number>> = {
  KeyA: 0x04, KeyB: 0x05, KeyC: 0x06, KeyD: 0x07, KeyE: 0x08, KeyF: 0x09,
  KeyG: 0x0a, KeyH: 0x0b, KeyI: 0x0c, KeyJ: 0x0d, KeyK: 0x0e, KeyL: 0x0f,
  KeyM: 0x10, KeyN: 0x11, KeyO: 0x12, KeyP: 0x13, KeyQ: 0x14, KeyR: 0x15,
  KeyS: 0x16, KeyT: 0x17, KeyU: 0x18, KeyV: 0x19, KeyW: 0x1a, KeyX: 0x1b,
  KeyY: 0x1c, KeyZ: 0x1d,
  Digit1: 0x1e, Digit2: 0x1f, Digit3: 0x20, Digit4: 0x21, Digit5: 0x22,
  Digit6: 0x23, Digit7: 0x24, Digit8: 0x25, Digit9: 0x26, Digit0: 0x27,
  Enter: 0x28, Escape: 0x29, Backspace: 0x2a, Tab: 0x2b, Space: 0x2c,
  Insert: 0x49, Home: 0x4a, PageUp: 0x4b, Delete: 0x4c, End: 0x4d,
  PageDown: 0x4e, ArrowRight: 0x4f, ArrowLeft: 0x50, ArrowDown: 0x51, ArrowUp: 0x52,
};

export interface InputBridgeHandle {
  /** Reconciles cursor and hidden editor state after a non-DOM host event. */
  synchronize(): void;
  /** Removes every DOM listener and the hidden native text editor. */
  close(): void;
}

export interface InputBridgeOptions {
  canvas: HTMLCanvasElement;
  bindings: TelaWebviewBindings;
  dispatch(packet: Uint8Array): boolean;
  status(): WebAppStatus;
  viewport(): { width: number; height: number };
}

/** Installs pointer, physical keyboard and IME bridges for one active WebView canvas. */
export function installInputBridge(options: InputBridgeOptions): InputBridgeHandle {
  const { canvas, bindings, dispatch, status, viewport } = options;
  let composing = false;
  let closed = false;

  const editor = document.createElement('textarea');
  editor.setAttribute('aria-label', 'tela text input');
  editor.autocapitalize = 'off';
  editor.autocomplete = 'off';
  editor.spellcheck = false;
  Object.assign(editor.style, {
    position: 'fixed',
    left: '0',
    top: '0',
    width: '1px',
    height: '1px',
    opacity: '0',
    pointerEvents: 'none',
    border: '0',
    padding: '0',
    resize: 'none',
  });
  document.body.append(editor);

  const point = (event: MouseEvent): { x: number; y: number } => {
    const bounds = canvas.getBoundingClientRect();
    const logical = viewport();
    return {
      x: (event.clientX - bounds.left) * logical.width / Math.max(bounds.width, 1),
      y: (event.clientY - bounds.top) * logical.height / Math.max(bounds.height, 1),
    };
  };
  const syncCursor = () => {
    canvas.style.cursor = ['default', 'text', 'pointer'][status().cursor] ?? 'default';
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
  const send = (packet: Uint8Array): boolean => {
    const consumed = dispatch(packet);
    synchronize();
    return consumed;
  };
  const onPointerDown = (event: PointerEvent) => {
    event.preventDefault();
    const position = point(event);
    canvas.setPointerCapture?.(event.pointerId);
    send(bindings.event_pointer_down(position.x, position.y));
    synchronize(true);
  };
  const onPointerUp = (event: PointerEvent) => {
    const position = point(event);
    send(bindings.event_pointer_up(position.x, position.y));
  };
  const onPointerMove = (event: PointerEvent) => {
    const position = point(event);
    send(bindings.event_pointer_move(position.x, position.y));
  };
  const onPointerLeave = () => {
    send(bindings.event_pointer_move(-1, -1));
  };
  const onWheel = (event: WheelEvent) => {
    event.preventDefault();
    const position = point(event);
    const logical = viewport();
    const unit = event.deltaMode === WheelEvent.DOM_DELTA_LINE
      ? 16
      : event.deltaMode === WheelEvent.DOM_DELTA_PAGE
        ? logical.height
        : 1;
    const bounds = canvas.getBoundingClientRect();
    send(bindings.event_pointer_scroll(
      position.x,
      position.y,
      event.deltaX * unit * logical.width / Math.max(bounds.width, 1),
      event.deltaY * unit * logical.height / Math.max(bounds.height, 1),
    ));
  };
  const onCanvasKeyDown = (event: KeyboardEvent) => {
    if (event.isComposing || status().input_focused) return;
    const physicalKey = PHYSICAL_KEY_CODES[event.code];
    if (physicalKey === undefined) return;
    if (send(bindings.event_key_down(physicalKey, modifierBits(event), event.repeat))) {
      event.preventDefault();
    }
  };
  const onEditorFocus = () => {
    send(bindings.event_input_focus());
  };
  const onEditorInput = () => {
    send(bindings.event_set_input_value(editor.value));
  };
  const onCompositionStart = () => {
    composing = true;
    send(bindings.event_input_composition_start());
  };
  const onCompositionEnd = () => {
    send(bindings.event_input_composition_end());
    composing = false;
    synchronize();
  };
  const onEditorKeyDown = (event: KeyboardEvent) => {
    if (event.key === 'Enter' && !event.isComposing) {
      event.preventDefault();
      send(bindings.event_input_enter());
      return;
    }
    if (event.key === 'Escape') {
      event.preventDefault();
      send(bindings.event_input_cancel());
      return;
    }
    if (event.code !== 'Tab' || event.isComposing) return;
    const physicalKey = PHYSICAL_KEY_CODES[event.code];
    if (physicalKey !== undefined && send(bindings.event_key_down(physicalKey, modifierBits(event), event.repeat))) {
      event.preventDefault();
      synchronize(true);
    }
  };
  const onEditorBlur = () => {
    composing = false;
    send(bindings.event_input_blur());
  };

  canvas.addEventListener('pointerdown', onPointerDown);
  canvas.addEventListener('pointerup', onPointerUp);
  canvas.addEventListener('pointermove', onPointerMove);
  canvas.addEventListener('pointerleave', onPointerLeave);
  canvas.addEventListener('wheel', onWheel, { passive: false });
  canvas.addEventListener('keydown', onCanvasKeyDown);
  editor.addEventListener('focus', onEditorFocus);
  editor.addEventListener('input', onEditorInput);
  editor.addEventListener('compositionstart', onCompositionStart);
  editor.addEventListener('compositionend', onCompositionEnd);
  editor.addEventListener('keydown', onEditorKeyDown);
  editor.addEventListener('blur', onEditorBlur);

  return {
    synchronize: () => synchronize(),
    close: () => {
      if (closed) return;
      closed = true;
      canvas.removeEventListener('pointerdown', onPointerDown);
      canvas.removeEventListener('pointerup', onPointerUp);
      canvas.removeEventListener('pointermove', onPointerMove);
      canvas.removeEventListener('pointerleave', onPointerLeave);
      canvas.removeEventListener('wheel', onWheel);
      canvas.removeEventListener('keydown', onCanvasKeyDown);
      editor.removeEventListener('focus', onEditorFocus);
      editor.removeEventListener('input', onEditorInput);
      editor.removeEventListener('compositionstart', onCompositionStart);
      editor.removeEventListener('compositionend', onCompositionEnd);
      editor.removeEventListener('keydown', onEditorKeyDown);
      editor.removeEventListener('blur', onEditorBlur);
      editor.remove();
      canvas.style.cursor = '';
    },
  };
}

function modifierBits(event: KeyboardEvent): number {
  return (event.shiftKey ? MODIFIER_SHIFT : 0)
    | (event.ctrlKey ? MODIFIER_CTRL : 0)
    | (event.altKey ? MODIFIER_ALT : 0)
    | (event.metaKey ? MODIFIER_META : 0);
}
