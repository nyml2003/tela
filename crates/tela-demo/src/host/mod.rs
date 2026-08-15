//! 平台宿主：CPU wasm ABI 与可选 WebGPU surface 适配。

pub mod cpu;

/// 平台 SDK 共用的无浏览器依赖 WASM 应用 guest。
#[cfg(all(feature = "app-wasm", target_arch = "wasm32"))]
pub mod app_wasm;

#[cfg(all(feature = "webgpu", target_arch = "wasm32"))]
#[path = "../wasm.rs"]
pub mod webgpu;
