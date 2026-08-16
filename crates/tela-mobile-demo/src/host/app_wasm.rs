//! Mobile application's concrete mapping onto the stable Tela guest ABI.

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
        AppEvent::KeyDown { physical_key, .. } => app.handle_key(physical_key) != 0,
        AppEvent::SetInputValue(value) => app.set_input_value(value) != 0,
        AppEvent::InputFocus => app.input_focus() != 0,
        AppEvent::InputBlur => app.input_blur() != 0,
        AppEvent::InputEnter => app.input_enter() != 0,
        AppEvent::InputCancel => app.input_cancel() != 0,
        AppEvent::InputCompositionStart | AppEvent::InputCompositionEnd => {
            app.composition_changed() != 0
        }
        AppEvent::ReplaceKeymapJson(_) => false,
    }
}

fn publish_app(app: &mut crate::App) -> Result<(&UiFrame, AppStatus), String> {
    app.ensure_frame();
    let status = AppStatus {
        cursor: if app.input_focused() {
            CursorKind::Text
        } else {
            CursorKind::Default
        },
        input_focused: app.input_focused(),
        input_value: app.input_value(),
    };
    Ok((app.frame(), status))
}
