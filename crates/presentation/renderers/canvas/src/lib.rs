//! tela-render-canvas — 浏览器 canvas 后端（见 [007-绘制与渲染后端] 6、[009-多环境集成] 3.2）。
//!
//! 通过 `Canvas2D` trait 抽象宿主 canvas 能力（浏览器 2D context 实现），
//! 把有序绘制源的命令翻译为 canvas 调用；按 `BackendCapabilities` 声明式降级
//! （降级是后端本地行为，不改动源计划，见 007-3）。
//!
//! 不依赖具体 wasm 绑定：`Canvas2D` 由宿主实现（浏览器 / 测试 mock）。

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod render;

pub use render::{Canvas2D, render_frame};

#[cfg(test)]
mod tests;
