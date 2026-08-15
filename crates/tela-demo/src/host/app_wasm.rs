//! Portable WASM application guest for native platform SDKs.
//!
//! This module deliberately has no browser imports. A host copies event bytes into guest memory,
//! invokes `tela_app_dispatch`, then copies the resolved frame and status packets back out.

use std::cell::RefCell;

use tela_app_abi::{
    ABI_VERSION, AppEvent, AppStatus, CursorKind, decode_event, encode_frame, encode_status,
};
use tela_contract::{Point, PointerEvent};

use crate::{reset_app, with_app};

thread_local! {
    static INPUT_BYTES: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    static FRAME_BYTES: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    static STATUS_BYTES: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    static ERROR_BYTES: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

/// Returns the ABI version expected by this guest.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn tela_app_abi_version() -> u32 {
    ABI_VERSION
}

/// Discards all application state and builds the initial frame.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn tela_app_init() -> u32 {
    reset_app();
    u32::from(publish())
}

/// Reserves guest memory for the next encoded input packet and returns its WASM-linear address.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn tela_app_input_begin(bytes: u32) -> *mut u8 {
    INPUT_BYTES.with(|input| {
        let mut input = input.borrow_mut();
        input.resize(bytes as usize, 0);
        input.as_mut_ptr()
    })
}

/// Decodes the bytes staged by [`tela_app_input_begin`], applies one event, and publishes updated
/// frame/status packets. Returns zero for malformed input or frame encoding failures.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn tela_app_dispatch(bytes: u32) -> u32 {
    let event = INPUT_BYTES.with(|input| {
        let mut input = input.borrow_mut();
        if input.len() != bytes as usize {
            input.clear();
            return Err("input byte length changed before dispatch".to_owned());
        }
        decode_event(&input).map_err(|error| error.to_string())
    });
    let Ok(event) = event else {
        set_error(event.unwrap_err());
        return 0;
    };
    let changed = with_app(|app| apply_event(app, event));
    if !publish() {
        return 0;
    }
    u32::from(changed)
}

/// Returns the pointer to the current `FramePacket` in guest linear memory.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn tela_app_frame_ptr() -> *const u8 {
    FRAME_BYTES.with(|frame| frame.borrow().as_ptr())
}

/// Returns the byte length of the current `FramePacket`.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn tela_app_frame_len() -> u32 {
    FRAME_BYTES.with(|frame| frame.borrow().len() as u32)
}

/// Returns the pointer to the current encoded [`AppStatus`] in guest linear memory.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn tela_app_status_ptr() -> *const u8 {
    STATUS_BYTES.with(|status| status.borrow().as_ptr())
}

/// Returns the byte length of the current encoded [`AppStatus`].
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn tela_app_status_len() -> u32 {
    STATUS_BYTES.with(|status| status.borrow().len() as u32)
}

/// Returns a diagnostic message for the last failed ABI call.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn tela_app_error_ptr() -> *const u8 {
    ERROR_BYTES.with(|error| error.borrow().as_ptr())
}

/// Returns the byte length of the last failed ABI call diagnostic.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn tela_app_error_len() -> u32 {
    ERROR_BYTES.with(|error| error.borrow().len() as u32)
}

fn apply_event(app: &mut crate::App, event: AppEvent) -> bool {
    match event {
        AppEvent::Viewport { width, height } => app.set_viewport(width, height),
        AppEvent::PointerDown { x, y } => {
            app.handle_pointer(PointerEvent::Down {
                position: Point { x, y },
            });
            true
        }
        AppEvent::PointerUp { x, y } => {
            app.handle_pointer(PointerEvent::Up {
                position: Point { x, y },
            });
            true
        }
        AppEvent::PointerMove { x, y } => {
            app.handle_pointer(PointerEvent::Move {
                position: Point { x, y },
            });
            true
        }
        AppEvent::PointerScroll {
            x,
            y,
            delta_x,
            delta_y,
        } => {
            app.handle_pointer(PointerEvent::Scroll {
                position: Point { x, y },
                delta: Point {
                    x: delta_x,
                    y: delta_y,
                },
            });
            true
        }
        AppEvent::KeyDown {
            physical_key,
            modifier_bits,
            repeat,
        } => app.handle_raw_key_codes(physical_key, modifier_bits, repeat) != 0,
        AppEvent::SetInputValue(value) => app.set_input_value(value) != 0,
        AppEvent::InputFocus => app.input_focus() != 0,
        AppEvent::InputBlur => app.input_blur() != 0,
        AppEvent::InputEnter => app.input_enter() != 0,
        AppEvent::InputCancel => app.input_cancel() != 0,
        AppEvent::InputCompositionStart => app.composition_start() != 0,
        AppEvent::InputCompositionEnd => app.composition_end() != 0,
        AppEvent::ReplaceKeymapJson(json) => app.replace_keymap_json(&json).is_ok(),
    }
}

fn publish() -> bool {
    let published = with_app(|app| {
        app.ensure_frame();
        let frame = encode_frame(app.frame()).map_err(|error| error.to_string())?;
        let cursor = match app.pointer_cursor() {
            1 => CursorKind::Text,
            2 => CursorKind::Pointer,
            _ => CursorKind::Default,
        };
        let status = encode_status(&AppStatus {
            cursor,
            input_focused: app.input_focused(),
            input_value: app.input_value(),
        })
        .map_err(|error| error.to_string())?;
        Ok::<_, String>((frame, status))
    });
    match published {
        Ok((frame, status)) => {
            FRAME_BYTES.with(|slot| *slot.borrow_mut() = frame);
            STATUS_BYTES.with(|slot| *slot.borrow_mut() = status);
            ERROR_BYTES.with(|slot| slot.borrow_mut().clear());
            true
        }
        Err(error) => {
            set_error(error);
            false
        }
    }
}

fn set_error(error: String) {
    ERROR_BYTES.with(|slot| *slot.borrow_mut() = error.into_bytes());
}
