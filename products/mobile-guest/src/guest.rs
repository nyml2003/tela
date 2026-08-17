//! WASM ABI 导出与 mobile 产品资源组合。

use std::cell::RefCell;

use tela_app_abi::{AppEvent, AppStatus, CursorKind};
use tela_contract::UiResourceSet;
use tela_icon_resources::MaterialIconFontProvider;
use tela_mobile_demo::App;
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

fn publish_app(app: &mut App) -> Result<(&tela_contract::UiFrame, AppStatus), String> {
    app.ensure_frame();
    Ok((
        app.frame(),
        AppStatus {
            cursor: if app.input_focused() {
                CursorKind::Text
            } else {
                CursorKind::Default
            },
            input_focused: app.input_focused(),
            input_value: app.input_value(),
        },
    ))
}
