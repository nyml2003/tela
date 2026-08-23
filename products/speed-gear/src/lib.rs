//! 变速齿轮 Windows 产品装配。

#![warn(missing_docs)]

#[cfg(target_os = "windows")]
use tela_contract::UiResourceSet;
#[cfg(target_os = "windows")]
use tela_icon_resources::MaterialIconFontProvider;
#[cfg(target_os = "windows")]
use tela_speed_gear::{FOCUS_APPEARANCE, SpeedGearController, WindowsSpeedBackend};
#[cfg(target_os = "windows")]
use tela_target_win32_static::{Application, ApplicationConfig, run_static_window};
#[cfg(target_os = "windows")]
use tela_text_resources::ControlledTextMeasurer;

#[cfg(target_os = "windows")]
static RESOURCES: UiResourceSet<ControlledTextMeasurer, MaterialIconFontProvider> =
    UiResourceSet::new(ControlledTextMeasurer, MaterialIconFontProvider);

#[cfg(target_os = "windows")]
/// 启动变速齿轮窗口。
pub fn run() -> Result<(), String> {
    let mut controller = SpeedGearController::new(&RESOURCES, Box::new(WindowsSpeedBackend::new()));
    controller.refresh_processes();
    let application = Application::new(
        &RESOURCES,
        controller,
        ApplicationConfig {
            focus_appearance: Some(FOCUS_APPEARANCE),
            initial_viewport: tela_contract::Viewport {
                width: 980.0,
                height: 680.0,
            },
        },
    );
    run_static_window(Box::new(application))
}
