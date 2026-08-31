//! WASM ABI 导出与 CC Remote 产品资源组合。
//!
//! 相对 desktop-guest 多一层桥：四个桥 ABI 导出（docs/032 §2.2）+ `net.http.request` 作业
//! 编排。每个 apply 帧边界先清请求队列，再排空上一轮回投的响应（`NET_INBOX`），最后把
//! 应用新产出的作业排进桥；宿主在 dispatch 之后 drain，异步完成后经
//! `tela_app_bridge_dispatch` 回投并补一次 `Wake`。

use std::cell::{Cell, RefCell};

use tela_app_abi::{AppEvent, AppFrameInput, AppFrameToken, AppPublication, AppStatus, CursorKind};
use tela_bridge::{BridgeResult, CapabilityId, GuestBridge, VersionPolicy};
use tela_cc_protocol::{
    NetHttpRequest, NetHttpResponse, decode_net_http_response, encode_net_http_request,
};
use tela_cc_remote::App;
use tela_contract::{DirtyFlags, FrameDamage, UiResourceSet};
use tela_icon_resources::MaterialIconFontProvider;
use tela_text_resources::ControlledTextMeasurer;

static RESOURCES: UiResourceSet<ControlledTextMeasurer, MaterialIconFontProvider> =
    UiResourceSet::new(ControlledTextMeasurer, MaterialIconFontProvider);

/// `net.http.request` 具名能力；base_url 与 token 只存在宿主侧。
fn net_http_request_capability() -> CapabilityId {
    CapabilityId::named(
        tela_cc_protocol::NET_SCOPE,
        tela_cc_protocol::NET_GROUP,
        tela_cc_protocol::NET_NAME,
    )
}

thread_local! {
    static APP: RefCell<App> = RefCell::new(App::new(&RESOURCES));
    static FRAME_TOKEN: Cell<u64> = const { Cell::new(0) };
    static BRIDGE: RefCell<GuestBridge> = RefCell::new(GuestBridge::new());
    static BRIDGE_INPUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    /// 桥回调收到的响应载荷；回调是 void ABI，只能先落盘，下一次 apply 消费。
    static NET_INBOX: RefCell<Vec<BridgeResult>> = const { RefCell::new(Vec::new()) };
    /// 最近一次宿主注入的单调时钟（Wake/Tick 时间戳）；作业调度的时间基。
    static NOW_MS: Cell<u64> = const { Cell::new(0) };
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
    BRIDGE.with(|bridge| *bridge.borrow_mut() = GuestBridge::new());
    NET_INBOX.with(|inbox| inbox.borrow_mut().clear());
}

fn apply_event(app: &mut App, event: AppEvent) -> bool {
    // 帧边界：上一轮请求队列已被宿主读取，清空避免重复执行（desktop-guest 语义）。
    BRIDGE.with(|bridge| {
        bridge.borrow_mut().take_request_queue();
    });
    let mut changed = match event {
        AppEvent::Wake { timestamp_ms } => {
            NOW_MS.with(|now| now.set(timestamp_ms));
            false
        }
        AppEvent::Tick { timestamp_ms } => {
            NOW_MS.with(|now| now.set(timestamp_ms));
            app.animation_tick(timestamp_ms)
        }
        AppEvent::Viewport { width, height } => app.set_viewport(width, height),
        AppEvent::WindowState { .. } => false,
        AppEvent::FrameInput {
            source_frame_token,
            input,
        } => {
            if active_frame_token() != Some(source_frame_token) {
                return false;
            }
            match input {
                AppFrameInput::Pointer(pointer) => app.handle_pointer(pointer.into()) != 0,
                AppFrameInput::KeyDown {
                    physical_key,
                    modifier_bits: _,
                    repeat: _,
                } => app.handle_key(physical_key) != 0,
                AppFrameInput::SetInputValue(value) => app.set_input_value(value) != 0,
                AppFrameInput::InputFocus => app.input_focus() != 0,
                AppFrameInput::InputBlur => app.input_blur() != 0,
                AppFrameInput::InputEnter => app.input_enter() != 0,
                AppFrameInput::InputCancel => app.input_cancel() != 0,
                AppFrameInput::InputCompositionStart => app.composition_changed() != 0,
                AppFrameInput::InputCompositionEnd => app.composition_changed() != 0,
            }
        }
        // CC Remote 没有可替换的键位表；接受事件但不产生新帧。
        AppEvent::ReplaceKeymapJson(_) => false,
    };
    // 排空上一轮回投的网络响应；再问应用要新作业排进桥。
    NET_INBOX.with(|inbox| {
        let mut inbox = inbox.borrow_mut();
        for result in inbox.drain(..) {
            let response = match result {
                BridgeResult::Ok(payload) => decode_net_http_response(&payload).ok(),
                BridgeResult::Err(_) => {
                    Some(NetHttpResponse::transport_error("bridge capability error"))
                }
            };
            if let Some(response) = response {
                let now = NOW_MS.with(|now| now.get());
                changed |= app.ingest_net_response(response, now);
            }
        }
    });
    let now = NOW_MS.with(|now| now.get());
    let jobs = app.take_pending_net_jobs(now);
    if !jobs.is_empty() {
        BRIDGE.with(|bridge| {
            let mut bridge = bridge.borrow_mut();
            for job in jobs {
                queue_net_job(&mut bridge, job);
            }
        });
        // 出站作业本身不改变呈现，但受理后状态可能刷新；交由回投后的 Wake 驱动。
    }
    changed
}

/// 把一个网络作业排进桥；回投载荷进 `NET_INBOX`，由下一次 apply 消费。
fn queue_net_job(bridge: &mut GuestBridge, job: NetHttpRequest) {
    let payload = encode_net_http_request(&job);
    bridge.request(
        net_http_request_capability(),
        VersionPolicy::Latest,
        payload,
        |result| {
            NET_INBOX.with(|inbox| inbox.borrow_mut().push(result));
        },
    );
}

fn publish_app(app: &mut App) -> Result<AppPublication, String> {
    ensure_frame(app);
    let status = AppStatus {
        frame_token: active_frame_token(),
        cursor: CursorKind::Default,
        input_focused: app.input_focused(),
        input_value: app.input_value(),
        animation_active: app.animation_schedule().active,
        next_deadline_ms: app.animation_schedule().next_deadline_ms,
    };
    let token = status
        .frame_token
        .ok_or_else(|| "cc guest has no active frame token".to_owned())?;
    let frame = app.frame().clone();
    Ok(AppPublication {
        token,
        damage: FrameDamage::full(frame.viewport, DirtyFlags::ALL),
        frame,
        spine: Vec::new(),
        retained_tree: None,
        status,
    })
}

fn ensure_frame(app: &mut App) {
    if app.ensure_frame() {
        FRAME_TOKEN.with(|token| {
            let next = token
                .get()
                .checked_add(1)
                .expect("cc guest frame token counter exhausted");
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

    #[test]
    fn net_jobs_queue_into_the_bridge_and_responses_reach_the_inbox() {
        reset_app();
        // 直接向 app 注入一个需要轮询的时间基，然后从 apply 路径拿作业。
        // reset 后 apply(Wake) 应产生首个 sync 请求。
        APP.with(|app| app.borrow_mut().animation_tick(10));
        let _ = apply_event(
            &mut App::new(&RESOURCES),
            AppEvent::Wake { timestamp_ms: 10 },
        );
        let len = tela_app_request_len();
        assert!(len > 0, "首个 Wake 必须排出 /v1/sync 请求");
        let queue = BRIDGE.with(|bridge| bridge.borrow_mut().take_request_queue());
        let requests = tela_bridge::decode_request_stream(&queue).expect("decode requests");
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].capability, net_http_request_capability());

        // 回投一个传输失败响应：回调把载荷压入 INBOX，下一次 apply 消费。
        let event = tela_bridge::BridgeEvent::Response {
            request_id: requests[0].request_id,
            result: BridgeResult::ok(tela_cc_protocol::encode_net_http_response(
                &NetHttpResponse::transport_error("dial refused"),
            )),
        };
        let packet = tela_bridge::encode_event(&event).expect("encode event");
        let pointer = tela_app_bridge_dispatch_begin(packet.len() as u32);
        assert!(!pointer.is_null());
        BRIDGE_INPUT.with(|input| {
            input.borrow_mut().copy_from_slice(&packet);
        });
        tela_app_bridge_dispatch(packet.len() as u32);
        assert!(!NET_INBOX.with(|inbox| inbox.borrow().is_empty()));
    }
}
