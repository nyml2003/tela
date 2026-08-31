//! Win32 surface-probe product assembly.
//!
//! This product deliberately selects system chrome. Its client area contains only the minimal
//! probe app, so it can isolate native surface presentation and host resize propagation.

#![warn(missing_docs)]

#[cfg(target_os = "windows")]
use tela_app_runtime::{Application, ApplicationConfig};
#[cfg(target_os = "windows")]
use tela_contract::{UiResourceSet, Viewport};
#[cfg(target_os = "windows")]
use tela_icon_resources::MaterialIconFontProvider;
#[cfg(target_os = "windows")]
use tela_target_win32::{NativeWindowOptions, run_native_window};
#[cfg(target_os = "windows")]
use tela_text_resources::{CONTROLLED_FONT_CATALOG, ControlledTextMeasurer};
#[cfg(target_os = "windows")]
use tela_win32_probe::Win32ProbeController;

#[cfg(target_os = "windows")]
static RESOURCES: UiResourceSet<ControlledTextMeasurer, MaterialIconFontProvider> =
    UiResourceSet::new(ControlledTextMeasurer, MaterialIconFontProvider)
        .with_fonts(CONTROLLED_FONT_CATALOG);

/// Starts the minimal static Win32 surface probe and blocks until its window closes.
#[cfg(target_os = "windows")]
pub fn run() -> Result<(), String> {
    let application = Application::new(
        &RESOURCES,
        Win32ProbeController::new(),
        ApplicationConfig {
            initial_viewport: Viewport {
                width: 640.0,
                height: 480.0,
            },
            ..ApplicationConfig::default()
        },
    );
    run_native_window(
        Box::new(application),
        NativeWindowOptions::new("Tela Win32 Surface Probe")
            .size(640, 480)
            .system_chrome(),
    )
}
