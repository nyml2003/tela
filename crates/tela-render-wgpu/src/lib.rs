//! tela-render-wgpu — wgpu 渲染后端（见 [007-绘制与渲染后端] 6）。
//!
//! 消费 `UiFrame` 的绘制命令，翻译为 wgpu 绘制调用；浏览器经 wasm 使用 WebGPU。

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod batch;
mod pipeline;
mod vertex;

pub mod renderer;

pub use renderer::WgpuRenderer;
