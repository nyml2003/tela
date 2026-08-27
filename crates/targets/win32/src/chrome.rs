//! 窗口 chrome 轴：系统外框（SDK 壳默认）与自绘标题栏（静态产品默认）。
//!
//! 自绘 chrome 使用无边框样式 + `WM_NCHITTEST` 拖拽带，保留系统免费的拖动、双击最大
//! 化、边缘缩放与右键菜单；系统 chrome 完全交给 `DefWindowProcW`。

#![allow(unsafe_code)]

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::UI::WindowsAndMessaging::{
    DefWindowProcW, HTCAPTION, HTCLIENT, SW_MAXIMIZE, SW_MINIMIZE, SW_RESTORE, ShowWindow,
    WINDOW_STYLE, WM_CLOSE, WM_NCHITTEST, WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_OVERLAPPEDWINDOW,
    WS_POPUP, WS_THICKFRAME,
};

use tela_contract::HitRole;

/// 窗口 chrome 形态。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WindowChrome {
    /// 系统标题栏/边框/菜单（bundle 开发壳）。
    #[default]
    SystemOverlapped,
    /// 自绘 chrome：无边框样式 + 拖拽带命中 + 应用自绘标题栏（静态产品）。
    CustomTitleBar,
}

impl WindowChrome {
    /// `CreateWindowExW` 的窗口样式位。
    pub fn window_style(self) -> WINDOW_STYLE {
        match self {
            Self::SystemOverlapped => WS_OVERLAPPEDWINDOW,
            Self::CustomTitleBar => WS_POPUP | WS_THICKFRAME | WS_MINIMIZEBOX | WS_MAXIMIZEBOX,
        }
    }
}

/// `WM_NCHITTEST` 策略：自绘 chrome 在客户区顶部把拖拽带判为 `HTCAPTION`；
/// 系统 chrome 完全遵循 `DefWindowProcW`。
pub fn hit_test(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    chrome: WindowChrome,
    role_at: impl FnOnce(tela_contract::Point) -> HitRole,
    dpi_scale: f32,
) -> LRESULT {
    let default = unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
    if chrome != WindowChrome::CustomTitleBar || default.0 != HTCLIENT as isize {
        return default;
    }
    // 自绘标题栏：客户区顶部拖动带内、未命中交互节点 → HT_CAPTION（系统免费提供
    // 拖动、双击最大化、右键菜单）；按钮区保持 HTCLIENT。
    let packed = lparam.0 as u32;
    let (x, y) = crate::input::client_point(packed);
    let mut point = windows::Win32::Foundation::POINT { x, y };
    // SAFETY: ScreenToClient 是同步查询，hwnd 属于本线程。
    let _ = unsafe { ScreenToClient(hwnd, &mut point) };
    let role = role_at(tela_contract::Point {
        x: point.x as f32 / dpi_scale,
        y: point.y as f32 / dpi_scale,
    });
    LRESULT(if role == HitRole::WindowDrag {
        HTCAPTION as isize
    } else {
        HTCLIENT as isize
    })
}

/// 执行一条自绘 chrome 的窗口命令（最小化/最大化-还原/关闭）。
///
/// SAFETY: hwnd 属于 UI 线程，命令执行是标准的窗口管理调用。
pub unsafe fn execute_window_command(hwnd: HWND, command: tela_contract::WindowCommand) {
    unsafe {
        match command {
            tela_contract::WindowCommand::Minimize => {
                let _ = ShowWindow(hwnd, SW_MINIMIZE);
            }
            tela_contract::WindowCommand::Maximize => {
                let zoomed = windows::Win32::UI::WindowsAndMessaging::IsZoomed(hwnd).as_bool();
                let show = if zoomed { SW_RESTORE } else { SW_MAXIMIZE };
                let _ = ShowWindow(hwnd, show);
            }
            tela_contract::WindowCommand::Close => {
                // 走 WM_CLOSE 完整流程（文本通道清理等）。
                let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                    Some(hwnd),
                    WM_CLOSE,
                    WPARAM::default(),
                    LPARAM::default(),
                );
            }
        }
    }
}

/// 保留 `WM_NCHITTEST` 消息值常量引用（chrome 模块自身在 match 之外不需要它）。
pub(crate) const WM_NCHITTEST_MESSAGE: u32 = WM_NCHITTEST;
