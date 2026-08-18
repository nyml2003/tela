//! WASM ABI 导出与 desktop 产品资源组合。

use std::cell::{Cell, RefCell};

use tela_app_abi::{AppEvent, AppFrameInput, AppFrameToken, AppStatus, CursorKind};
use tela_contract::UiResourceSet;
use tela_desktop_demo::App;
use tela_icon_resources::MaterialIconFontProvider;
use tela_text_resources::ControlledTextMeasurer;

static RESOURCES: UiResourceSet<ControlledTextMeasurer, MaterialIconFontProvider> =
    UiResourceSet::new(ControlledTextMeasurer, MaterialIconFontProvider);

thread_local! {
    static APP: RefCell<App> = RefCell::new(App::new(&RESOURCES));
    static FRAME_TOKEN: Cell<u64> = const { Cell::new(0) };
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
    FRAME_TOKEN.with(|token| token.set(0));
}

fn apply_event(app: &mut App, event: AppEvent) -> bool {
    match event {
        AppEvent::Viewport { width, height } => app.set_viewport(width, height),
        AppEvent::FrameInput {
            source_frame_token,
            input,
        } => {
            ensure_frame(app);
            if active_frame_token() != Some(source_frame_token) {
                return false;
            }
            match input {
                AppFrameInput::Pointer(pointer) => app.handle_pointer(pointer.into()) != 0,
                AppFrameInput::KeyDown {
                    physical_key,
                    modifier_bits,
                    repeat,
                } => app.handle_raw_key_codes(physical_key, modifier_bits, repeat) != 0,
                AppFrameInput::SetInputValue(value) => app.set_input_value(value) != 0,
                AppFrameInput::InputFocus => app.input_focus() != 0,
                AppFrameInput::InputBlur => app.input_blur() != 0,
                AppFrameInput::InputEnter => app.input_enter() != 0,
                AppFrameInput::InputCancel => app.input_cancel() != 0,
                AppFrameInput::InputCompositionStart => app.composition_start() != 0,
                AppFrameInput::InputCompositionEnd => app.composition_end() != 0,
            }
        }
        AppEvent::ReplaceKeymapJson(json) => app.replace_keymap_json(&json).is_ok(),
    }
}

fn publish_app(app: &mut App) -> Result<(&tela_contract::UiFrame, AppStatus), String> {
    ensure_frame(app);
    let cursor = match app.pointer_cursor() {
        1 => CursorKind::Text,
        2 => CursorKind::Pointer,
        _ => CursorKind::Default,
    };
    Ok((
        app.frame(),
        AppStatus {
            frame_token: active_frame_token(),
            cursor,
            input_focused: app.input_focused(),
            input_value: app.input_value(),
        },
    ))
}

fn ensure_frame(app: &mut App) {
    if app.ensure_frame() {
        FRAME_TOKEN.with(|token| {
            let next = token
                .get()
                .checked_add(1)
                .expect("desktop guest frame token counter exhausted");
            token.set(next);
        });
    }
}

fn active_frame_token() -> Option<AppFrameToken> {
    FRAME_TOKEN.with(|token| AppFrameToken::new(token.get()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_input_from_a_frame_replaced_by_a_viewport_update() {
        reset_app();
        let first = with_app(|app| {
            publish_app(app)
                .expect("publish initial desktop frame")
                .1
                .frame_token
                .expect("initial desktop frame token")
        });
        let second = with_app(|app| {
            assert!(apply_event(
                app,
                AppEvent::Viewport {
                    width: 1103.0,
                    height: 721.0,
                },
            ));
            publish_app(app)
                .expect("publish resized desktop frame")
                .1
                .frame_token
                .expect("resized desktop frame token")
        });
        assert_ne!(first, second);

        let changed = with_app(|app| {
            apply_event(
                app,
                AppEvent::FrameInput {
                    source_frame_token: first,
                    input: AppFrameInput::KeyDown {
                        physical_key: 0x29,
                        modifier_bits: 0,
                        repeat: false,
                    },
                },
            )
        });
        assert!(!changed);
    }
}
