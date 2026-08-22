//! Win32 静态文本编辑器产品装配根。
//!
//! 明确选择编辑器应用、受控文本/Material 图标资源与 Win32 静态壳；不引入 bundle、WASM
//! ABI 或 guest executor。桥按静态路径语义（进程内 dispatcher）为关于页提供构建信息。

#![warn(missing_docs)]

#[cfg(target_os = "windows")]
use tela_contract::{Point, PointerEvent, UiFrame, UiResourceSet};
#[cfg(target_os = "windows")]
use tela_desktop_runtime::bridge::common::BuildConstants;
#[cfg(target_os = "windows")]
use tela_icon_resources::MaterialIconFontProvider;
#[cfg(target_os = "windows")]
use tela_target_win32_static::{Win32StaticSession, build_dispatcher};
#[cfg(target_os = "windows")]
use tela_text_resources::ControlledTextMeasurer;
#[cfg(target_os = "windows")]
use tela_win32_editor::App;

#[cfg(target_os = "windows")]
const APP_NAME: &str = "Tela 文本编辑器";
#[cfg(target_os = "windows")]
const APP_VERSION: &str = "0.1.0";
#[cfg(target_os = "windows")]
const BUNDLE_VERSION: &str = "0.1.0";

#[cfg(target_os = "windows")]
fn app_build_id() -> u32 {
    option_env!("TELA_APP_BUILD_ID")
        .and_then(|value| value.parse().ok())
        .unwrap_or(1)
}

#[cfg(target_os = "windows")]
fn bundle_build_id() -> u32 {
    option_env!("TELA_BUNDLE_BUILD_ID")
        .and_then(|value| value.parse().ok())
        .unwrap_or(1)
}

#[cfg(target_os = "windows")]
static RESOURCES: UiResourceSet<ControlledTextMeasurer, MaterialIconFontProvider> =
    UiResourceSet::new(ControlledTextMeasurer, MaterialIconFontProvider);

/// 产品会话：编辑器应用 + 静态路径桥 dispatcher。
#[cfg(target_os = "windows")]
struct ProductSession {
    app: App,
}

#[cfg(target_os = "windows")]
impl ProductSession {
    fn new() -> Self {
        let dispatcher = build_dispatcher(
            std::rc::Rc::new(std::cell::RefCell::new(
                tela_target_win32_static::WindowMetrics::default(),
            )),
            &BuildConstants {
                app_name: APP_NAME.to_owned(),
                app_version: tela_utils::Version::new(0, 1, 0),
                app_build_id: app_build_id(),
                bundle_version: tela_utils::Version::new(0, 1, 0),
                bundle_build_id: bundle_build_id(),
            },
            vec![],
        );
        Self {
            app: App::new(&RESOURCES, dispatcher),
        }
    }
}

#[cfg(target_os = "windows")]
impl Win32StaticSession for ProductSession {
    fn ensure_frame(&mut self) -> bool {
        self.app.ensure_frame()
    }

    fn frame_is_current(&self) -> bool {
        self.app.frame_is_current()
    }

    fn set_viewport(&mut self, width: f32, height: f32, dpr: f32) -> bool {
        self.app.set_viewport(width, height, dpr)
    }

    fn set_window_maximized(&mut self, maximized: bool) -> bool {
        self.app.set_window_maximized(maximized)
    }

    fn dispatch_pointer(&mut self, event: PointerEvent) -> u32 {
        self.app.handle_pointer(event)
    }

    fn dispatch_key(&mut self, physical_key: u16, modifier_bits: u8, repeat: bool) -> u32 {
        self.app.handle_key(physical_key, modifier_bits, repeat)
    }

    fn set_input_value(&mut self, value: String) -> u32 {
        self.app.set_input_value(value)
    }

    fn input_focus(&mut self) -> u32 {
        self.app.input_focus()
    }

    fn input_blur(&mut self) -> u32 {
        self.app.input_blur()
    }

    fn input_enter(&mut self) -> u32 {
        self.app.input_enter()
    }

    fn input_cancel(&mut self) -> u32 {
        self.app.input_cancel()
    }

    fn input_focused(&self) -> bool {
        self.app.input_focused()
    }

    fn hover_interactive(&self) -> bool {
        self.app.hover_interactive()
    }

    fn hit_test_interactive(&mut self, point: Point) -> bool {
        self.app.hit_test_interactive_at(point)
    }

    fn take_window_command(&mut self) -> Option<tela_contract::WindowCommand> {
        self.app.take_window_command()
    }

    fn input_value(&self) -> String {
        self.app.input_value()
    }

    fn frame(&self) -> &UiFrame {
        self.app.frame()
    }
}

/// 启动 Win32 静态编辑器窗口（阻塞至窗口关闭）。
#[cfg(target_os = "windows")]
pub fn run() -> Result<(), String> {
    eprintln!(
        "tela-win32-editor: build app_name=\"{}\" app_version={} app_build_id={} bundle_version={} bundle_build_id={}",
        APP_NAME,
        APP_VERSION,
        app_build_id(),
        BUNDLE_VERSION,
        bundle_build_id()
    );
    tela_target_win32_static::run_static_window(Box::new(ProductSession::new()))
}
