//! iPhone Target Runtime for Tela's statically linked mobile product.
//!
//! This crate deliberately owns UIKit lifecycle, Metal presentation, touch, and text input. It
//! does not select a business application, visual resource implementation or downloadable bundle.
//! The iOS product root supplies one local mobile session through [`IosMobileSession`].

#![warn(missing_docs)]

use tela_contract::{Insets, PointerEvent, UiFrame};

#[cfg(any(test, target_os = "ios"))]
mod input;
#[cfg(target_os = "ios")]
mod ios;
#[cfg(target_os = "ios")]
mod safe_area;
#[cfg(any(test, target_os = "ios"))]
mod touch;

/// State that the Target publishes into its UIKit-controlled text channel.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MobileTextStatus {
    /// Whether UIKit should attach or release native IME focus.
    pub input_focused: bool,
    /// Full controlled text value mirrored by UIKit.
    pub input_value: String,
}

/// Local iOS adapter for one statically linked mobile application.
///
/// This is deliberately a Target-specific session surface, not a universal Host trait. It models
/// only the values UIKit/Metal must exchange with the selected direct mobile application. The
/// iOS product root implements it by adapting its own Application and injected resources.
pub trait IosMobileSession {
    /// Updates the logical iPhone viewport.
    fn set_viewport(&mut self, width: f32, height: f32) -> bool;

    /// Updates system-bar and gesture exclusion insets in logical points.
    fn set_safe_area(&mut self, safe_area: Insets) -> bool;

    /// Delivers a normalized touch or scroll event.
    fn dispatch_pointer(&mut self, event: PointerEvent) -> bool;

    /// Delivers one platform-neutral physical key code.
    fn dispatch_key(&mut self, physical_key: u16) -> bool;

    /// Replaces the complete controlled native text value.
    fn set_input_value(&mut self, value: String) -> bool;

    /// Records an IME composition transition.
    fn composition_changed(&mut self) -> bool;

    /// Commits the active native text interaction.
    fn input_enter(&mut self) -> bool;

    /// Notifies the application that native text focus was lost.
    fn input_blur(&mut self) -> bool;

    /// Resolves and borrows the portable drawing frame for Metal rendering.
    fn frame(&mut self) -> &UiFrame;

    /// Returns the state UIKit needs after an input or lifecycle transition.
    fn text_status(&self) -> MobileTextStatus;
}

/// Starts the UIKit-owned Winit event loop for one statically linked mobile session.
#[cfg(target_os = "ios")]
pub fn run_mobile_session(session: impl IosMobileSession + 'static) -> Result<(), String> {
    ios::run(session)
}
