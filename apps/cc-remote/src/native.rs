//! iOS 静态原生会话（`native-app` feature）。
//!
//! M0 只交付 Android 动态链路；这里的直接原生会话与 `tela-mobile-demo::native` 同构，
//! 等 iOS 端接入 CC Remote 时再启用（见 docs/038 §里程碑）。

use crate::application::App;

/// 静态原生会话包装；UIKit 宿主经 `IosMobileSession` 驱动。
pub struct CcNativeApp {
    app: App,
}

impl CcNativeApp {
    /// 以产品注入的资源构造会话。
    pub fn new(app: App) -> Self {
        Self { app }
    }

    /// 内部应用句柄（宿主只驱动，不拥有业务状态）。
    pub fn app(&mut self) -> &mut App {
        &mut self.app
    }
}
