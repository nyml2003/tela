//! tela 最小浏览器演示。
//!
//! 场景只包含一个居中的蓝色矩形，但 CPU 与 WebGPU 都必须消费同一个
//! `UiNode -> UiTree -> UiFrame` 结果。后端差异只存在于最后的帧提交。

#![cfg_attr(feature = "webgpu", allow(dead_code))]

mod frame_trace;
#[cfg(feature = "webgpu")]
mod wasm;

use std::cell::RefCell;
use std::collections::HashMap;

use tela_contract::{
    Color, Fill, FlexDirection, LayoutConcern, Size, TextMeasureRequest, TextMeasurer, TextMetrics,
    UiFrame, Viewport, VisualConcern,
};
use tela_core::UiTree;
use tela_core::builder::{LayoutContainer, Primitive};

/// 所有后端共享的逻辑画布尺寸。
pub const VIEWPORT: Viewport = Viewport {
    width: 480.0,
    height: 360.0,
};

const HEADER: Color = Color::rgba(0.12, 0.31, 0.58, 1.0);
const SIDEBAR: Color = Color::rgba(0.90, 0.36, 0.20, 1.0);
const MAIN: Color = Color::rgba(0.16, 0.60, 0.43, 1.0);
const FOOTER: Color = Color::rgba(0.22, 0.25, 0.31, 1.0);

thread_local! {
    static APP: RefCell<App> = RefCell::new(App::new());
}

/// 无文字的最小场景仍要满足 layout 的 `TextMeasurer` 合约。
struct EmptyTextMeasurer;

impl TextMeasurer for EmptyTextMeasurer {
    fn measure(&self, _request: &TextMeasureRequest<'_>) -> TextMetrics {
        TextMetrics {
            width: 0.0,
            height: 0.0,
            line_count: 0,
        }
    }
}

/// 最小 demo 运行时。逻辑帧缓存与 renderer 的呈现节奏独立。
struct App {
    frame: Option<UiFrame>,
    frame_trace: Vec<u8>,
    cpu_rendered: bool,
    cpu_bitmap: Vec<u8>,
}

impl App {
    fn new() -> Self {
        Self {
            frame: None,
            frame_trace: Vec::new(),
            cpu_rendered: false,
            cpu_bitmap: Vec::new(),
        }
    }

    /// 确保共享逻辑帧存在；返回值表示本次是否发生了场景构建与布局。
    fn ensure_frame(&mut self) -> bool {
        if self.frame.is_none() {
            let frame = scene_frame();
            self.frame_trace = frame_trace::to_json(&frame).into_bytes();
            self.frame = Some(frame);
            self.cpu_rendered = false;
            true
        } else {
            false
        }
    }

    /// 已缓存的唯一逻辑帧。调用方先经 `ensure_frame` 保证其存在。
    fn frame(&self) -> &UiFrame {
        self.frame.as_ref().expect("共享逻辑帧必须已构建")
    }

    /// 与 `frame` 同时缓存的、由同一 `UiFrame` 投影出的 UTF-8 调试 JSON。
    fn frame_trace(&self) -> &[u8] {
        debug_assert!(self.frame.is_some());
        &self.frame_trace
    }

    /// CPU 仅在共享逻辑帧变更时重新光栅化。
    fn render_cpu_if_needed(&mut self) -> bool {
        self.ensure_frame();
        if self.cpu_rendered {
            return false;
        }
        let config =
            tela_render_raster::RasterConfig::default_with(Color::rgba(1.0, 1.0, 1.0, 1.0));
        self.cpu_bitmap = tela_render_raster::render_frame(self.frame(), &config).pixels;
        self.cpu_rendered = true;
        true
    }
}

/// 单个无文字矩形，作为所有区域的唯一视觉原语。
fn solid_rect(color: Color, width: Size, height: Size) -> tela_contract::UiNode {
    Primitive::rect()
        .layout(LayoutConcern {
            width: Some(width),
            height: Some(height),
            ..LayoutConcern::default()
        })
        .visual(VisualConcern {
            fill: Some(Fill::Solid(color)),
            ..VisualConcern::default()
        })
        .into()
}

/// 共享 tela 场景：header / 等宽侧栏与内容 / footer，全程只使用纯色矩形。
fn scene_node() -> tela_contract::UiNode {
    let header = solid_rect(HEADER, Size::fixed(480.0), Size::fixed(56.0));
    let sidebar = solid_rect(SIDEBAR, Size::fixed(240.0), Size::fixed(264.0));
    let main = solid_rect(MAIN, Size::fixed(240.0), Size::fixed(264.0));
    let content = LayoutContainer::flex([sidebar, main])
        .layout(LayoutConcern {
            width: Some(Size::fixed(480.0)),
            height: Some(Size::fixed(264.0)),
            direction: FlexDirection::Row,
            ..LayoutConcern::default()
        })
        .into();
    let footer = solid_rect(FOOTER, Size::fixed(480.0), Size::fixed(40.0));
    LayoutContainer::flex([header, content, footer])
        .layout(LayoutConcern {
            width: Some(Size::fixed(VIEWPORT.width)),
            height: Some(Size::fixed(VIEWPORT.height)),
            direction: FlexDirection::Column,
            ..LayoutConcern::default()
        })
        .into()
}

/// 所有 renderer 的唯一场景输入。
pub(crate) fn scene_frame() -> UiFrame {
    UiTree::new(scene_node())
        .expect("最小 demo 场景必须合法")
        .resolve(VIEWPORT, &EmptyTextMeasurer, &HashMap::new())
        .expect("最小 demo 场景必须可布局")
}

pub(crate) fn with_app<T>(f: impl FnOnce(&mut App) -> T) -> T {
    APP.with(|app| f(&mut app.borrow_mut()))
}

/// wasm WebGPU 路径的计时辅助。
#[cfg(feature = "webgpu")]
pub(crate) fn now_ms() -> f32 {
    js_sys::Date::now() as f32
}

/// CPU 后端帧推进：仅在共享 `UiFrame` 更新时光栅化。
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn demo_tick() -> u32 {
    u32::from(with_app(App::render_cpu_if_needed))
}

/// CPU 位图指针。宿主必须先调用 `demo_tick` 提交共享帧。
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn demo_frame_ptr() -> *const u8 {
    with_app(|app| app.cpu_bitmap.as_ptr())
}

/// CPU 位图尺寸（RGBA8，逻辑像素与物理像素均为 480×360）。
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn demo_frame_size() -> u32 {
    VIEWPORT.width as u32 | ((VIEWPORT.height as u32) << 16)
}

/// 共享 `UiFrame` 的结构化 JSON 指针。长度由 `demo_frame_trace_len` 返回。
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn demo_frame_trace_ptr() -> *const u8 {
    with_app(|app| {
        app.ensure_frame();
        app.frame_trace().as_ptr()
    })
}

/// 共享 `UiFrame` 的结构化 JSON 字节长度。
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn demo_frame_trace_len() -> u32 {
    with_app(|app| {
        app.ensure_frame();
        u32::try_from(app.frame_trace().len()).expect("trace 长度必须可编码")
    })
}

/// CPU WASM 构建标识。
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn demo_wasm_version() -> u32 {
    option_env!("TELA_BUILD_TS")
        .and_then(|value| value.parse().ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_scene_resolves_to_header_content_footer_rectangles() {
        let mut app = App::new();
        assert!(app.ensure_frame());
        let frame = app.frame();
        assert_eq!(frame.commands.len(), 4);
        let expected = [
            (0.0, 0.0, 480.0, 56.0, HEADER),
            (0.0, 56.0, 240.0, 264.0, SIDEBAR),
            (240.0, 56.0, 240.0, 264.0, MAIN),
            (0.0, 320.0, 480.0, 40.0, FOOTER),
        ];
        for (command, (x, y, w, h, color)) in frame.commands.iter().zip(expected) {
            assert_eq!((command.geometry.x, command.geometry.y), (x, y));
            assert_eq!((command.geometry.w, command.geometry.h), (w, h));
            assert!(matches!(
                command.payload,
                tela_contract::DrawPayload::Rect {
                    fill: Some(fill),
                    border: None,
                } if fill == color
            ));
        }
    }

    #[test]
    fn cached_frame_is_not_recomputed() {
        let mut app = App::new();
        assert!(app.ensure_frame());
        let frame = app.frame() as *const UiFrame;
        assert!(!app.ensure_frame());
        assert!(std::ptr::eq(frame, app.frame()));
    }

    #[test]
    fn cached_trace_describes_the_shared_frame() {
        let mut app = App::new();
        app.ensure_frame();
        assert_eq!(
            std::str::from_utf8(app.frame_trace()).expect("trace 必须是 UTF-8"),
            "{\"viewport\":{\"width\":480,\"height\":360},\"commands\":[{\"geometry\":{\"x\":0,\"y\":0,\"w\":480,\"h\":56},\"clip\":null,\"payload\":{\"kind\":\"rect\",\"fill\":{\"r\":0.12,\"g\":0.31,\"b\":0.58,\"a\":1},\"border\":null}},{\"geometry\":{\"x\":0,\"y\":56,\"w\":240,\"h\":264},\"clip\":null,\"payload\":{\"kind\":\"rect\",\"fill\":{\"r\":0.9,\"g\":0.36,\"b\":0.2,\"a\":1},\"border\":null}},{\"geometry\":{\"x\":240,\"y\":56,\"w\":240,\"h\":264},\"clip\":null,\"payload\":{\"kind\":\"rect\",\"fill\":{\"r\":0.16,\"g\":0.6,\"b\":0.43,\"a\":1},\"border\":null}},{\"geometry\":{\"x\":0,\"y\":320,\"w\":480,\"h\":40},\"clip\":null,\"payload\":{\"kind\":\"rect\",\"fill\":{\"r\":0.22,\"g\":0.25,\"b\":0.31,\"a\":1},\"border\":null}}],\"hit_regions\":[]}"
        );
    }
}
