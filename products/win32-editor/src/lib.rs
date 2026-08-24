//! Win32 静态文本编辑器产品装配根。
//!
//! 明确选择编辑器应用、受控文本/Material 图标资源与 Win32 静态壳；不引入 bundle、WASM
//! ABI 或 guest executor。桥按静态路径语义（进程内 dispatcher）为关于页提供构建信息。
//! 壳协议由 `tela_target_win32_static::Application` 一次性实现，产品只装配资源、
//! 控制器与配置。

#![warn(missing_docs)]

#[cfg(target_os = "windows")]
use tela_contract::UiResourceSet;
#[cfg(target_os = "windows")]
use tela_desktop_runtime::bridge::common::BuildConstants;
#[cfg(target_os = "windows")]
use tela_icon_resources::MaterialIconFontProvider;
#[cfg(target_os = "windows")]
use tela_target_win32_static::{
    Application, ApplicationConfig, WindowMetrics, build_dispatcher, run_static_window,
};
#[cfg(target_os = "windows")]
use tela_text_resources::{CONTROLLED_FONT_CATALOG, ControlledTextMeasurer};
#[cfg(target_os = "windows")]
use tela_win32_editor::{EditorController, FOCUS_APPEARANCE};

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
    UiResourceSet::new(ControlledTextMeasurer, MaterialIconFontProvider)
        .with_fonts(CONTROLLED_FONT_CATALOG);

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
    let dispatcher = build_dispatcher(
        std::rc::Rc::new(std::cell::RefCell::new(WindowMetrics::default())),
        &BuildConstants {
            app_name: APP_NAME.to_owned(),
            app_version: tela_utils::Version::new(0, 1, 0),
            app_build_id: app_build_id(),
            bundle_version: tela_utils::Version::new(0, 1, 0),
            bundle_build_id: bundle_build_id(),
        },
        vec![],
    );
    let controller = EditorController::new(&RESOURCES, dispatcher);
    let application = Application::new(
        &RESOURCES,
        controller,
        ApplicationConfig {
            focus_appearance: Some(FOCUS_APPEARANCE),
            ..ApplicationConfig::default()
        },
    );
    run_static_window(Box::new(application))
}
