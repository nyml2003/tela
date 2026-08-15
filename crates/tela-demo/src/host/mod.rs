//! 平台 SDK 共享的应用 guest ABI 导出。

/// 平台 SDK 共用的无浏览器依赖 WASM 应用 guest。
#[cfg(all(feature = "app-wasm", target_arch = "wasm32"))]
pub mod app_wasm;
