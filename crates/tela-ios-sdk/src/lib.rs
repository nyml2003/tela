//! iPhone host for Tela's statically linked mobile application.
//!
//! This crate deliberately owns UIKit lifecycle, Metal presentation, touch, and text input. It
//! links the mobile file-browser application directly and does not interpret downloadable bundles
//! or reuse the desktop macOS business view.

#![warn(missing_docs)]

#[cfg(any(test, target_os = "ios"))]
mod input;
#[cfg(target_os = "ios")]
mod ios;
#[cfg(target_os = "ios")]
mod safe_area;
#[cfg(any(test, target_os = "ios"))]
mod touch;

/// Starts the UIKit-owned Winit application loop from the Xcode process entrypoint.
#[cfg(target_os = "ios")]
#[unsafe(no_mangle)]
pub extern "C" fn tela_ios_start() {
    if let Err(error) = ios::run() {
        eprintln!("tela-ios-sdk: {error}");
    }
}
