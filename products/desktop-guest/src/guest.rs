//! WASM ABI 导出与 desktop 产品资源组合。

use std::cell::RefCell;

use tela_app_abi::{AppEvent, AppStatus, CursorKind};
use tela_contract::UiResourceSet;
use tela_desktop_demo::App;
use tela_icon_resources::MaterialIconFontProvider;
use tela_text_resources::ControlledTextMeasurer;

static RESOURCES: UiResourceSet<ControlledTextMeasurer, MaterialIconFontProvider> =
    UiResourceSet::new(ControlledTextMeasurer, MaterialIconFontProvider);

thread_local! {
    static APP: RefCell<App> = RefCell::new(App::new(&RESOURCES));
}

tela_app_abi::export_guest! {
    reset = reset_app;
    with_app = with_app;
    apply = apply_event;
    publish = publish_app;
}

fn with_app<T>(f: impl FnOnce(&mut App) -> T) -> T {
    APP.with(|app| f(&mut app.borrow_mut()))
}

fn reset_app() {
    APP.with(|app| *app.borrow_mut() = App::new(&RESOURCES));
}

fn apply_event(app: &mut App, event: AppEvent) -> bool {
    match event {
        AppEvent::Viewport { width, height } => app.set_viewport(width, height),
        AppEvent::PointerDown { x, y } => {
            app.handle_pointer(tela_contract::PointerEvent::Down {
                position: tela_contract::Point { x, y },
            });
            true
        }
        AppEvent::PointerUp { x, y } => {
            app.handle_pointer(tela_contract::PointerEvent::Up {
                position: tela_contract::Point { x, y },
            });
            true
        }
        AppEvent::PointerMove { x, y } => {
            app.handle_pointer(tela_contract::PointerEvent::Move {
                position: tela_contract::Point { x, y },
            });
            true
        }
        AppEvent::PointerScroll {
            x,
            y,
            delta_x,
            delta_y,
        } => {
            app.handle_pointer(tela_contract::PointerEvent::Scroll {
                position: tela_contract::Point { x, y },
                delta: tela_contract::Point {
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

fn publish_app(app: &mut App) -> Result<(&tela_contract::UiFrame, AppStatus), String> {
    app.ensure_frame();
    let cursor = match app.pointer_cursor() {
        1 => CursorKind::Text,
        2 => CursorKind::Pointer,
        _ => CursorKind::Default,
    };
    Ok((
        app.frame(),
        AppStatus {
            cursor,
            input_focused: app.input_focused(),
            input_value: app.input_value(),
        },
    ))
}
