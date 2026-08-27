//! Android GameActivity host for the first Tela mobile application bundle.
//!
//! The ordinary modules are target-independent input contracts and have unit tests on every
//! development machine. The Android module is intentionally compiled only for an Android target:
//! it owns the GameActivity event loop, Vulkan surface, JNI text bridge, and strict remote bundle
//! startup policy.

#![warn(missing_docs)]

#[cfg(any(test, target_os = "android"))]
mod ime;
#[cfg(any(test, target_os = "android"))]
mod touch;

#[cfg(target_os = "android")]
#[allow(unsafe_code)]
mod android;

// 桥接线依赖 android-gated 的 HostEvent 与 ureq；纯逻辑测试随之只在 Android 目标编译
// （对齐 win32 providers.rs 的 `#[cfg(all(test, target_os = "windows"))]` 先例）。
#[cfg(target_os = "android")]
mod bridge;
