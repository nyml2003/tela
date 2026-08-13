//! CPU wasm ABI：只做浏览器原始导出与 `application::runtime::App` 转发。

use crate::application::runtime::App;
use crate::with_app;
use tela_contract::{Point, PointerEvent};

#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn demo_tick() -> u32 {
    u32::from(with_app(App::render_cpu_if_needed))
}
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn demo_set_viewport(width: f32, height: f32) -> u32 {
    u32::from(with_app(|app| app.set_viewport(width, height)))
}
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn demo_set_raster_dpi(dpi: f32) {
    with_app(|app| app.set_raster_dpi(dpi));
}
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn demo_pointer_down(x: f32, y: f32) -> u32 {
    with_app(|app| {
        app.handle_pointer(PointerEvent::Down {
            position: Point { x, y },
        })
    })
}
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn demo_pointer_move(x: f32, y: f32) -> u32 {
    with_app(|app| {
        app.handle_pointer(PointerEvent::Move {
            position: Point { x, y },
        })
    })
}
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn demo_pointer_scroll(x: f32, y: f32, dx: f32, dy: f32) -> u32 {
    with_app(|app| {
        app.handle_pointer(PointerEvent::Scroll {
            position: Point { x, y },
            delta: Point { x: dx, y: dy },
        })
    })
}
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn demo_input_focused() -> u32 {
    u32::from(with_app(|app| app.input_focused()))
}
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn demo_input_composition_start() -> u32 {
    with_app(|app| app.composition_start())
}
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn demo_input_composition_end() -> u32 {
    with_app(|app| app.composition_end())
}
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn demo_input_enter() -> u32 {
    with_app(|app| app.input_enter())
}
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn demo_input_cancel() -> u32 {
    with_app(|app| app.input_cancel())
}
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn demo_input_blur() -> u32 {
    with_app(|app| app.input_blur())
}
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn demo_input_value_ptr() -> *const u8 {
    with_app(App::input_value_ptr)
}
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn demo_input_value_len() -> u32 {
    with_app(App::input_value_len)
}
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn demo_pointer_cursor() -> u32 {
    with_app(|app| app.pointer_cursor())
}
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn demo_input_value_begin(bytes: u32) -> *mut u8 {
    with_app(|app| app.begin_input_upload(bytes as usize))
}
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn demo_input_value_finish(bytes: u32) -> u32 {
    with_app(|app| app.finish_input_upload(bytes as usize))
}
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn demo_frame_ptr() -> *const u8 {
    with_app(|app| app.cpu_bitmap().as_ptr())
}
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn demo_frame_size() -> u32 {
    with_app(|app| {
        let (w, h) = app.raster_size();
        w | (h << 16)
    })
}
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn demo_frame_trace_ptr() -> *const u8 {
    with_app(|app| {
        app.ensure_frame();
        app.frame_trace().as_ptr()
    })
}
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn demo_frame_trace_len() -> u32 {
    with_app(|app| {
        app.ensure_frame();
        app.frame_trace().len() as u32
    })
}
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn demo_wasm_version() -> u32 {
    option_env!("TELA_BUILD_TS")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}
