//! Win32 静态 Agent workbench 产品装配根。
//!
//! 该产品与浏览器 Agent 产品共享同一个 application 和受控文本资源，只更换 Target host。
//! 因此它可用于判断 viewport/文字布局问题是否来自 Wasm 宿主，而不会引入第二份 UI 实现。

#![warn(missing_docs)]

#[cfg(target_os = "windows")]
use tela_agent_demo::new_agent_demo;
#[cfg(target_os = "windows")]
use tela_contract::UiResourceSet;
#[cfg(target_os = "windows")]
use tela_icon_resources::MaterialIconFontProvider;
#[cfg(target_os = "windows")]
use tela_target_win32::{NativeWindowOptions, run_native_window};
#[cfg(target_os = "windows")]
use tela_text_resources::{CONTROLLED_FONT_CATALOG, ControlledTextMeasurer};

#[cfg(target_os = "windows")]
const APP_NAME: &str = "Tela Agent Lab (Win32)";

#[cfg(target_os = "windows")]
static RESOURCES: UiResourceSet<ControlledTextMeasurer, MaterialIconFontProvider> =
    UiResourceSet::new(ControlledTextMeasurer, MaterialIconFontProvider)
        .with_fonts(CONTROLLED_FONT_CATALOG);

/// 启动 Win32 静态 Agent workbench 窗口（阻塞至窗口关闭）。
#[cfg(target_os = "windows")]
pub fn run() -> Result<(), String> {
    let application = new_agent_demo(&RESOURCES);
    run_native_window(
        Box::new(application),
        NativeWindowOptions::new(APP_NAME)
            .size(1200, 760)
            .system_chrome(),
    )
}
