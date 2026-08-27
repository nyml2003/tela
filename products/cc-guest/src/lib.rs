//! CC Remote 动态 guest 的产品装配根（M0 骨架，逐步填充）。
//!
//! 相对 mobile-guest 多一层桥：`guest` 模块实现四个桥 ABI 导出，并在 apply 帧边界把
//! `net.http.request` 作业排给宿主、把回投响应送回应用。

// 手写导出逐条 `allow(unsafe_code)`；`deny` 拒绝产品其余代码里的 unsafe。
#![deny(unsafe_code)]
#![warn(missing_docs)]

#[cfg(any(target_arch = "wasm32", test))]
mod guest;
