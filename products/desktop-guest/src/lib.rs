//! Desktop dynamic guest 的产品装配根。
//!
//! WebView、Win32 和 macOS 都可交付这个受 ABI 约束的 guest，但各自的窗口循环、surface
//! 和 bundle 生命周期仍留在对应 Target。这里唯一做的是选择 desktop application 与实际
//! 视觉资源；它不是跨 Target Host。

// `export_guest!` emits the audited extern "C" ABI boundary with local `allow(unsafe_code)`.
// `deny` still rejects any unsafe code written by this product outside that macro expansion.
#![deny(unsafe_code)]
#![warn(missing_docs)]

#[cfg(target_arch = "wasm32")]
mod guest;
