//! WASM ABI 导出与 desktop 产品资源组合。

use std::cell::{Cell, RefCell};

use tela_app_abi::{AppEvent, AppFrameInput, AppFrameToken, AppStatus, CursorKind};
use tela_bridge::{BridgeResult, GuestBridge};
use tela_contract::UiResourceSet;
use tela_desktop_demo::App;
use tela_icon_resources::MaterialIconFontProvider;
use tela_text_resources::ControlledTextMeasurer;

static RESOURCES: UiResourceSet<ControlledTextMeasurer, MaterialIconFontProvider> =
    UiResourceSet::new(ControlledTextMeasurer, MaterialIconFontProvider);

thread_local! {
    static APP: RefCell<App> = RefCell::new(App::new(&RESOURCES));
    static FRAME_TOKEN: Cell<u64> = const { Cell::new(0) };
    // 桥：guest 侧参考实现实例 + host 写入响应的保留区 + 演示回调状态。
    static BRIDGE: RefCell<GuestBridge> = RefCell::new(GuestBridge::new());
    static BRIDGE_INPUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    static BRIDGE_DEMO: Cell<Option<Vec<u8>>> = const { Cell::new(None) };
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
    BRIDGE.with(|bridge| {
        let mut bridge = bridge.borrow_mut();
        // 演示：初始化时排队两个只读桥请求，回调把结果写入演示状态，
        // 由下一次 apply 消费（与输入事件同路径，不直接改帧）。
        bridge.get_app_name(|result| {
            let name = match result {
                BridgeResult::Ok(payload) => tela_bridge::decode_app_name_response(&payload)
                    .ok()
                    .map(|info| info.name),
                BridgeResult::Err(_) => None,
            };
            BRIDGE_DEMO.with(|slot| slot.set(name.map(|n| n.into_bytes())));
        });
        bridge.get_time_stamp(|result| {
            let stamp = match result {
                BridgeResult::Ok(payload) => tela_bridge::decode_time_stamp_response(&payload)
                    .ok()
                    .map(|info| info.unix_millis),
                BridgeResult::Err(_) => None,
            };
            BRIDGE_DEMO.with(|slot| {
                let mut bytes = slot.take().unwrap_or_default();
                if let Some(millis) = stamp {
                    bytes.extend_from_slice(&millis.to_le_bytes());
                }
                slot.set(Some(bytes));
            });
        });
    });
}

fn apply_event(app: &mut App, event: AppEvent) -> bool {
    // 上一轮请求队列已被宿主读取；帧边界清空，避免重复执行。
    BRIDGE.with(|bridge| {
        bridge.borrow_mut().take_request_queue();
    });
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

// ---------------------------------------------------------------------------
// 桥 ABI 导出（零宏，手写实现；规范见 docs/032 §2.2）。
// ---------------------------------------------------------------------------

/// 保留请求队列内存并返回当前缓冲指针；`len > 0` 时预留容量。
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn tela_app_request_begin(len: u32) -> *mut u8 {
    BRIDGE.with(|bridge| {
        let mut bridge = bridge.borrow_mut();
        let queue = bridge.request_queue_bytes();
        if len > 0 {
            queue.reserve(len as usize);
        }
        queue.as_mut_ptr()
    })
}

/// 返回当前排队请求长度（host 每次 dispatch 后读取）。
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn tela_app_request_len() -> u32 {
    BRIDGE.with(|bridge| bridge.borrow().request_queue_len())
}

/// 保留 host 写入响应字节的内存并返回指针。
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn tela_app_bridge_dispatch_begin(len: u32) -> *mut u8 {
    BRIDGE_INPUT.with(|input| {
        let mut input = input.borrow_mut();
        input.resize(len as usize, 0);
        input.as_mut_ptr()
    })
}

/// 处理宿主回投的 BridgeEvent：按 request_id 触发回调（回调只更新状态，
/// 由下一次 apply 消费并随该帧发布）。
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn tela_app_bridge_dispatch(len: u32) {
    let bytes = BRIDGE_INPUT.with(|input| {
        let input = input.borrow();
        if input.len() != len as usize {
            return None;
        }
        Some(input[..len as usize].to_vec())
    });
    let Some(bytes) = bytes else {
        return;
    };
    BRIDGE.with(|bridge| {
        let _ = bridge.borrow_mut().handle_event_packet(&bytes);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use tela_bridge::{BridgeEvent, BridgeResult};

    #[test]
    fn bridge_demo_requests_are_queued_and_callbacks_fire_through_exports() {
        reset_app();
        // reset_app 排队了 getAppName + getTimeStamp 两个请求。
        let len = tela_app_request_len();
        assert!(len > 0);
        let requests = BRIDGE.with(|bridge| {
            let queue = bridge.borrow_mut().take_request_queue();
            tela_bridge::decode_request_stream(&queue).expect("decode queued requests")
        });
        assert_eq!(requests.len(), 2);

        // 模拟 host：为第一个请求回投 getAppName 响应（经导出写入并触发回调）。
        let event = BridgeEvent::Response {
            request_id: requests[0].request_id,
            result: BridgeResult::ok(tela_bridge::encode_app_name_response(
                &tela_bridge::AppNameInfo {
                    name: "文件管理器".to_owned(),
                },
            )),
        };
        let packet = tela_bridge::encode_event(&event).expect("encode event");
        let ptr = tela_app_bridge_dispatch_begin(packet.len() as u32);
        assert!(!ptr.is_null());
        BRIDGE_INPUT.with(|input| {
            let mut input = input.borrow_mut();
            input.copy_from_slice(&packet);
        });
        tela_app_bridge_dispatch(packet.len() as u32);

        // 回调已把应用名写入演示状态。
        let name = BRIDGE_DEMO.with(|slot| slot.take());
        assert_eq!(
            name,
            Some("文件管理器".as_bytes().to_vec()),
            "bridge callback must update demo state through the exports"
        );

        // 第二次 apply 会清空队列；再排队一个请求，模拟宿主读取闭环。
        let mut app = App::new(&RESOURCES);
        let _ = apply_event(
            &mut app,
            AppEvent::Viewport {
                width: 100.0,
                height: 100.0,
            },
        );
        assert_eq!(tela_app_request_len(), 0);
    }

    #[test]
    fn bridge_dispatch_ignores_length_mismatch() {
        BRIDGE_INPUT.with(|input| {
            input.borrow_mut().clear();
        });
        tela_app_bridge_dispatch(1);
    }

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
