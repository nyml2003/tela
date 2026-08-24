//! WASM ABI 导出与 mobile 产品资源组合。

use std::cell::RefCell;

use tela_app_abi::{AppEvent, AppFrameInput, AppFrameToken, AppStatus, CursorKind};
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
        AppEvent::Tick { timestamp_ms } => app.animation_tick(timestamp_ms),
        AppEvent::Viewport { width, height } => app.set_viewport(width, height),
        AppEvent::FrameInput {
            source_frame_token,
            input,
        } => match input {
            AppFrameInput::Pointer(pointer) => {
                app.handle_pointer_for_frame(source_frame_token.get(), pointer.into()) != 0
            }
            AppFrameInput::KeyDown { physical_key, .. } => {
                app.handle_key_for_frame(source_frame_token.get(), physical_key) != 0
            }
            AppFrameInput::SetInputValue(value) => {
                app.set_input_value_for_frame(source_frame_token.get(), value) != 0
            }
            AppFrameInput::InputFocus => app.input_focus_for_frame(source_frame_token.get()) != 0,
            AppFrameInput::InputBlur => app.input_blur_for_frame(source_frame_token.get()) != 0,
            AppFrameInput::InputEnter => app.input_enter_for_frame(source_frame_token.get()) != 0,
            AppFrameInput::InputCancel => app.input_cancel_for_frame(source_frame_token.get()) != 0,
            AppFrameInput::InputCompositionStart | AppFrameInput::InputCompositionEnd => {
                app.composition_changed_for_frame(source_frame_token.get()) != 0
            }
        },
        AppEvent::ReplaceKeymapJson(_) => false,
    }
}

fn publish_app(app: &mut App) -> Result<(&tela_contract::UiFrame, AppStatus), String> {
    app.ensure_frame();
    Ok((
        app.frame(),
        AppStatus {
            frame_token: AppFrameToken::new(app.active_frame_token()),
            cursor: if app.input_focused() {
                CursorKind::Text
            } else {
                CursorKind::Default
            },
            input_focused: app.input_focused(),
            input_value: app.input_value(),
            animation_active: app.animation_schedule().active,
            next_deadline_ms: app.animation_schedule().next_deadline_ms,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_input_from_a_frame_replaced_by_a_viewport_update() {
        reset_app();
        let first = with_app(|app| {
            publish_app(app)
                .expect("publish initial mobile frame")
                .1
                .frame_token
                .expect("initial mobile frame token")
        });
        let second = with_app(|app| {
            assert!(apply_event(
                app,
                AppEvent::Viewport {
                    width: 411.0,
                    height: 891.0,
                },
            ));
            publish_app(app)
                .expect("publish resized mobile frame")
                .1
                .frame_token
                .expect("resized mobile frame token")
        });
        assert_ne!(first, second);

        let changed = with_app(|app| {
            apply_event(
                app,
                AppEvent::FrameInput {
                    source_frame_token: first,
                    input: AppFrameInput::SetInputValue("must not reach the new frame".to_owned()),
                },
            )
        });
        assert!(!changed);
        assert_eq!(with_app(|app| app.input_value()), "");
    }
}
