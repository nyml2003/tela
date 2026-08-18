//! iPhone 静态产品装配根。
//!
//! 这里明确选择 mobile application、受控文本/Material 图标资源和 iOS Target Runtime。它
//! 不引入 bundle、WASM ABI、下载或 guest executor；这些属于 Android 与 desktop 的动态路线。

#![warn(missing_docs)]

#[cfg(target_os = "ios")]
use tela_contract::UiResourceSet;
#[cfg(target_os = "ios")]
use tela_icon_resources::MaterialIconFontProvider;
#[cfg(target_os = "ios")]
use tela_mobile_demo::{MobileApp, MobileAppStatus};
#[cfg(target_os = "ios")]
use tela_target_ios::{IosMobileSession, MobileTextStatus};
#[cfg(target_os = "ios")]
use tela_text_resources::ControlledTextMeasurer;

#[cfg(target_os = "ios")]
static RESOURCES: UiResourceSet<ControlledTextMeasurer, MaterialIconFontProvider> =
    UiResourceSet::new(ControlledTextMeasurer, MaterialIconFontProvider);

#[cfg(target_os = "ios")]
struct ProductMobileSession {
    app: MobileApp,
}

#[cfg(target_os = "ios")]
impl ProductMobileSession {
    fn new() -> Self {
        Self {
            app: MobileApp::new(&RESOURCES),
        }
    }
}

#[cfg(target_os = "ios")]
impl IosMobileSession for ProductMobileSession {
    fn set_viewport(&mut self, width: f32, height: f32) -> bool {
        self.app.set_viewport(width, height)
    }

    fn set_safe_area(&mut self, safe_area: tela_contract::Insets) -> bool {
        self.app.set_safe_area(safe_area)
    }

    fn dispatch_pointer(&mut self, frame_token: u64, event: tela_contract::PointerEvent) -> bool {
        self.app.dispatch_pointer(frame_token, event)
    }

    fn dispatch_key(&mut self, frame_token: u64, physical_key: u16) -> bool {
        self.app.dispatch_key(frame_token, physical_key)
    }

    fn set_input_value(&mut self, frame_token: u64, value: String) -> bool {
        self.app.set_input_value(frame_token, value)
    }

    fn composition_changed(&mut self, frame_token: u64) -> bool {
        self.app.composition_changed(frame_token)
    }

    fn input_enter(&mut self, frame_token: u64) -> bool {
        self.app.input_enter(frame_token)
    }

    fn input_blur(&mut self, frame_token: u64) -> bool {
        self.app.input_blur(frame_token)
    }

    fn frame(&mut self) -> (&tela_contract::UiFrame, u64) {
        self.app.frame()
    }

    fn text_status(&self) -> MobileTextStatus {
        let MobileAppStatus {
            input_focused,
            input_value,
        } = self.app.status();
        MobileTextStatus {
            input_focused,
            input_value,
        }
    }
}

/// Static entrypoint called by the thin Xcode UIApplication target.
#[cfg(target_os = "ios")]
#[unsafe(no_mangle)]
pub extern "C" fn tela_product_ios_start() {
    if let Err(error) = tela_target_ios::run_mobile_session(ProductMobileSession::new()) {
        eprintln!("tela-product-ios: {error}");
    }
}
