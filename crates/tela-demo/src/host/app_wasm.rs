//! Portable WASM application guest for native platform SDKs.
//!
//! The ABI export and packet-buffer mechanics are shared by `tela_app_abi::export_guest!`.
//! This module only owns the desktop application's concrete event and status projection.

use tela_app_abi::{AppEvent, AppStatus, CursorKind};
use tela_contract::{Point, PointerEvent, UiFrame};

tela_app_abi::export_guest! {
    reset = crate::reset_app;
    with_app = crate::with_app;
    apply = apply_event;
    publish = publish_app;
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

fn publish_app(app: &mut crate::App) -> Result<(&UiFrame, AppStatus), String> {
    app.ensure_frame();
    let cursor = match app.pointer_cursor() {
        1 => CursorKind::Text,
        2 => CursorKind::Pointer,
        _ => CursorKind::Default,
    };
    let status = AppStatus {
        cursor,
        input_focused: app.input_focused(),
        input_value: app.input_value(),
    };
    Ok((app.frame(), status))
}
