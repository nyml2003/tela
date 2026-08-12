//! 平台宿主：CPU wasm ABI 与可选 WebGPU surface 适配。

pub mod cpu;

#[cfg(all(feature = "webgpu", target_arch = "wasm32"))]
#[path = "../wasm.rs"]
pub mod webgpu;
