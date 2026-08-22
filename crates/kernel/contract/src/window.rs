//! 平台无关的窗口命令契约（自绘标题栏控制按钮 → 宿主 shell 执行）。

/// 自绘标题栏窗口控制命令。
///
/// 应用（DSL 层）通过动作产生窗口命令，宿主 shell 负责执行系统调用
/// （最小化/最大化/关闭）。平台无关：Win32/macOS 壳各自映射。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowCommand {
    /// 最小化窗口。
    Minimize,
    /// 最大化 / 还原切换。
    Maximize,
    /// 关闭窗口。
    Close,
}
