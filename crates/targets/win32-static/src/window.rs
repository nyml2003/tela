//! Static Win32 shell: window class, message loop, input normalization, and session driving.

#![allow(unsafe_code)]

use std::time::Instant;

use tela_contract::{
    Point, PointerButtons, PointerEvent, PointerId, PointerKind, PointerPhase, UiFrame,
    WindowCommand,
};
use windows::Win32::{
    Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
    Graphics::Gdi::{
        BeginPaint, EndPaint, InvalidateRect, PAINTSTRUCT, ScreenToClient, UpdateWindow,
    },
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext},
        Input::KeyboardAndMouse::{
            GetAsyncKeyState, ReleaseCapture, SetCapture, SetFocus, TME_LEAVE, TRACKMOUSEEVENT,
            TrackMouseEvent, VK_BACK, VK_CONTROL, VK_DELETE, VK_DOWN, VK_END, VK_ESCAPE, VK_HOME,
            VK_LEFT, VK_LWIN, VK_MENU, VK_NEXT, VK_PRIOR, VK_RETURN, VK_RIGHT, VK_RWIN, VK_SHIFT,
            VK_TAB, VK_UP,
        },
        WindowsAndMessaging::{
            CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, DestroyWindow,
            DispatchMessageW, GWLP_USERDATA, GetClientRect, GetMessageW, GetWindowLongPtrW,
            HTCAPTION, HTCLIENT, IDC_ARROW, IDC_HAND, LoadCursorW, PostMessageW, PostQuitMessage,
            RegisterClassW, SW_MAXIMIZE, SW_MINIMIZE, SW_RESTORE, SetCursor, SetWindowLongPtrW,
            ShowWindow, WINDOW_EX_STYLE, WM_CANCELMODE, WM_CAPTURECHANGED, WM_CHAR, WM_CLOSE,
            WM_DESTROY, WM_DPICHANGED, WM_KEYDOWN, WM_KILLFOCUS, WM_LBUTTONDOWN, WM_LBUTTONUP,
            WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCCREATE, WM_NCHITTEST, WM_NCMOUSEMOVE, WM_PAINT,
            WM_SETCURSOR, WM_SIZE, WNDCLASSW, WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_POPUP,
            WS_THICKFRAME,
        },
    },
};

use crate::gpu::{GpuSession, RenderOutcome};

/// A statically assembled Tela application driven by this shell's message loop.
pub trait Win32StaticSession {
    /// Ensures the current frame exists (rebuilds after invalidation); the shell calls this
    /// before every paint.
    fn ensure_frame(&mut self) -> bool;
    /// Updates the logical content area (CSS points) and the DPI scale.
    fn set_viewport(&mut self, width: f32, height: f32, dpr: f32) -> bool;
    /// Delivers one normalized pointer event; returns consumed action count.
    fn dispatch_pointer(&mut self, event: PointerEvent) -> u32;
    /// Delivers a physical key event; returns consumed action count.
    fn dispatch_key(&mut self, physical_key: u16, modifier_bits: u8, repeat: bool) -> u32;
    /// Replaces the focused text input value (may contain `\n` for multiline).
    fn set_input_value(&mut self, value: String) -> u32;
    /// The native text channel gained focus.
    fn input_focus(&mut self) -> u32;
    /// The native text channel lost focus.
    fn input_blur(&mut self) -> u32;
    /// Commits the current text interaction.
    fn input_enter(&mut self) -> u32;
    /// Cancels the current text interaction.
    fn input_cancel(&mut self) -> u32;
    /// Whether the native text channel is currently attached.
    fn input_focused(&self) -> bool;
    /// Whether the pointer currently hovers an interactive (hoverable) node.
    fn hover_interactive(&self) -> bool {
        false
    }
    /// 自绘标题栏待执行的窗口命令（App 经动作产生，shell 消费执行）。
    fn take_window_command(&mut self) -> Option<WindowCommand> {
        None
    }
    /// Current controlled text value.
    fn input_value(&self) -> String;
    /// The latest resolved frame to render.
    fn frame(&self) -> &UiFrame;
}

struct StaticWindowState {
    hwnd: HWND,
    session: Box<dyn Win32StaticSession>,
    gpu: Option<GpuSession>,
    dpi_scale: f32,
    pointer_captured: bool,
    mouse_leave_tracking: bool,
    input_epoch: Instant,
    /// 连续未成功呈现次数；达到阈值后暂停自动重绘（防 Outdated/Suboptimal 忙循环）。
    render_retries: u32,
    /// 高频诊断日志节流：拖拽缩放时 WM_SIZE/WM_PAINT 每帧触发，避免日志刷屏。
    last_log_at: Instant,
    /// 上次打印 viewport 日志的尺寸（变化超过阈值才打印）。
    last_logged_viewport: (f32, f32),
}

/// Creates the window and runs the message loop until the window closes.
pub fn run_static_window(app: Box<dyn Win32StaticSession>) -> Result<(), String> {
    // SAFETY: DPI awareness is a process-wide startup setting; calling it once is documented.
    let _ = unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    let hmodule = unsafe { GetModuleHandleW(None) }.map_err(|error| error.to_string())?;
    let hinstance = HINSTANCE(hmodule.0);
    register_window_class(hinstance)?;
    let hwnd = create_window(hinstance)?;
    // SAFETY: the freshly created window has no user data; this stores our Box before ShowWindow.
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_ptr(app, hwnd));
    }
    // SAFETY: the window belongs to this thread and is fully initialized at this point.
    let _ = unsafe {
        windows::Win32::UI::WindowsAndMessaging::ShowWindow(
            hwnd,
            windows::Win32::UI::WindowsAndMessaging::SW_SHOW,
        )
    };
    let _ = unsafe { UpdateWindow(hwnd) };

    // SAFETY: standard message loop on the thread that owns hwnd.
    let mut message = windows::Win32::UI::WindowsAndMessaging::MSG::default();
    let mut message_count = 0u64;
    loop {
        let retrieved = unsafe { GetMessageW(&mut message, None, 0, 0) };
        if retrieved.0 == 0 {
            break; // WM_QUIT
        }
        if retrieved.0 == -1 {
            return Err("Win32 message loop failed".to_owned());
        }
        // SAFETY: dispatch is the standard loop step for the owning thread.
        unsafe { DispatchMessageW(&message) };
        // 自绘标题栏命令（最小化/最大化/关闭）在每次 dispatch 后统一执行。
        execute_window_command(hwnd);
        message_count += 1;
    }
    eprintln!("tela win32-static: message loop exited after {message_count} messages");
    Ok(())
}

/// 自绘标题栏拖动带高度（逻辑像素，与应用的 TITLE_BAR_H 主题常量对齐）。
const TITLE_BAR_DRAG_H: f32 = 40.0;

/// 消费并执行自绘标题栏的窗口命令（最小化/最大化/关闭）。
fn execute_window_command(hwnd: HWND) {
    let command = unsafe { with_state(hwnd, |state| state.session.take_window_command()) };
    let Some(command) = command else {
        return;
    };
    // SAFETY: hwnd 属于 UI 线程，命令执行是标准的窗口管理调用。
    unsafe {
        match command {
            WindowCommand::Minimize => {
                let _ = ShowWindow(hwnd, SW_MINIMIZE);
            }
            WindowCommand::Maximize => {
                let zoomed = windows::Win32::UI::WindowsAndMessaging::IsZoomed(hwnd).as_bool();
                let _ = ShowWindow(hwnd, if zoomed { SW_RESTORE } else { SW_MAXIMIZE });
            }
            WindowCommand::Close => {
                // 走 WM_CLOSE 完整流程（input_blur 等清理）。
                let _ = PostMessageW(
                    Some(hwnd),
                    WM_CLOSE,
                    windows::Win32::Foundation::WPARAM(0),
                    windows::Win32::Foundation::LPARAM(0),
                );
            }
        }
    }
}

fn state_ptr(app: Box<dyn Win32StaticSession>, hwnd: HWND) -> isize {
    let state = Box::new(StaticWindowState {
        hwnd,
        session: app,
        gpu: None,
        dpi_scale: 1.0,
        pointer_captured: false,
        mouse_leave_tracking: false,
        input_epoch: Instant::now(),
        render_retries: 0,
        last_log_at: Instant::now(),
        last_logged_viewport: (0.0, 0.0),
    });
    Box::into_raw(state) as isize
}

unsafe fn with_state<R>(hwnd: HWND, f: impl FnOnce(&mut StaticWindowState) -> R) -> R {
    let value = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) };
    if value == 0 {
        // Fallback: no state yet (WM_NCCREATE path is not used); return a default.
        return f(&mut StaticWindowState {
            hwnd,
            session: Box::new(NoopSession),
            gpu: None,
            dpi_scale: 1.0,
            pointer_captured: false,
            mouse_leave_tracking: false,
            input_epoch: Instant::now(),
            render_retries: 0,
            last_log_at: Instant::now(),
            last_logged_viewport: (0.0, 0.0),
        });
    }
    f(unsafe { &mut *(value as *mut StaticWindowState) })
}

struct NoopSession;

impl Win32StaticSession for NoopSession {
    fn ensure_frame(&mut self) -> bool {
        false
    }

    fn set_viewport(&mut self, _w: f32, _h: f32, _dpr: f32) -> bool {
        false
    }
    fn dispatch_pointer(&mut self, _event: PointerEvent) -> u32 {
        0
    }
    fn dispatch_key(&mut self, _k: u16, _m: u8, _r: bool) -> u32 {
        0
    }
    fn set_input_value(&mut self, _v: String) -> u32 {
        0
    }
    fn input_focus(&mut self) -> u32 {
        0
    }
    fn input_blur(&mut self) -> u32 {
        0
    }
    fn input_enter(&mut self) -> u32 {
        0
    }
    fn input_cancel(&mut self) -> u32 {
        0
    }
    fn input_focused(&self) -> bool {
        false
    }
    fn input_value(&self) -> String {
        String::new()
    }
    fn frame(&self) -> &UiFrame {
        unreachable!("noop session has no frame")
    }
}

fn register_window_class(hinstance: HINSTANCE) -> Result<(), String> {
    let class = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(wnd_proc),
        hInstance: hinstance,
        lpszClassName: windows::core::w!("TelaStaticWin32Window"),
        ..WNDCLASSW::default()
    };
    // SAFETY: the class name is static and the procedure is a plain extern "system" fn.
    if unsafe { RegisterClassW(&class) } == 0 {
        return Err("register Win32 window class".to_owned());
    }
    Ok(())
}

fn create_window(hinstance: HINSTANCE) -> Result<HWND, String> {
    // SAFETY: CreateWindowExW is called on the UI thread with a valid class and instance.
    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            windows::core::w!("TelaStaticWin32Window"),
            windows::core::w!("Tela 文本编辑器"),
            WS_POPUP | WS_THICKFRAME | WS_MINIMIZEBOX | WS_MAXIMIZEBOX,
            100,
            100,
            960,
            640,
            None,
            None,
            Some(hinstance),
            None,
        )
    }
    .map_err(|error| format!("create Win32 window: {error}"))?;
    Ok(hwnd)
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_NCHITTEST => {
            // 自绘标题栏：客户区顶部拖动带内、未命中交互节点 → HT_CAPTION
            // （系统免费提供拖动、双击最大化、右键菜单）；按钮区保持 HTCLIENT。
            let hit = unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
            if hit.0 == HTCLIENT as isize {
                let (x, y) = screen_point(lparam);
                let mut point = POINT { x, y };
                // SAFETY: ScreenToClient 是同步查询，hwnd 属于本线程。
                let _ = unsafe { ScreenToClient(hwnd, &mut point) };
                let interactive =
                    unsafe { with_state(hwnd, |state| state.session.hover_interactive()) };
                if (point.y as f32) < TITLE_BAR_DRAG_H && !interactive {
                    return LRESULT(HTCAPTION as isize);
                }
            }
            hit
        }
        WM_NCCREATE => {
            // SAFETY: WM_NCCREATE precedes all user-data access; store nothing here.
            unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
        }
        WM_SIZE => {
            unsafe {
                with_state(hwnd, |state| {
                    state.update_viewport(hwnd, state.dpi_scale);
                });
            }
            LRESULT(0)
        }
        WM_DPICHANGED => {
            unsafe {
                with_state(hwnd, |state| {
                    let new_dpr = (wparam.0 as u16) as f32 / 96.0;
                    eprintln!("tela win32-static: DPI changed -> {:.2}", new_dpr);
                    state.dpi_scale = new_dpr;
                    state.update_viewport(hwnd, new_dpr);
                });
            }
            LRESULT(0)
        }
        WM_PAINT => {
            unsafe {
                with_state(hwnd, |state| {
                    let mut paint = PAINTSTRUCT::default();
                    let _ = BeginPaint(hwnd, &mut paint);
                    // The first paint can arrive synchronously from UpdateWindow before any
                    // input/viewport event; ensure the frame exists before rendering.
                    let frame_ready = state.session.ensure_frame();
                    if state.gpu.is_none() {
                        let (width, height) = client_size(hwnd);
                        // GetClientRect 返回物理像素：surface config 直接用物理尺寸。
                        match GpuSession::new(hwnd, width as u32, height as u32, state.dpi_scale) {
                            Ok(gpu) => {
                                eprintln!(
                                    "tela win32-static: gpu ready {}x{} dpr={:.2}",
                                    width as u32, height as u32, state.dpi_scale
                                );
                                state.gpu = Some(gpu);
                            }
                            Err(error) => eprintln!("tela win32-static: gpu init: {error}"),
                        }
                    }
                    let mut presented_this_frame = false;
                    let mut presented_suboptimal = false;
                    if let Some(gpu) = state.gpu.as_mut() {
                        if frame_ready {
                            match gpu.render(state.session.frame()) {
                                Ok(RenderOutcome::Presented { suboptimal }) => {
                                    state.render_retries = 0;
                                    presented_this_frame = true;
                                    presented_suboptimal = suboptimal;
                                    if suboptimal {
                                        state.render_retries += 1;
                                        gpu.reconfigure(gpu.config.width, gpu.config.height);
                                        state.redraw_or_backoff(hwnd, "suboptimal");
                                    }
                                }
                                Ok(RenderOutcome::Outdated) => {
                                    state.render_retries += 1;
                                    gpu.reconfigure(gpu.config.width, gpu.config.height);
                                    state.redraw_or_backoff(hwnd, "outdated");
                                }
                                Ok(RenderOutcome::Lost) => {
                                    state.render_retries += 1;
                                    if let Err(error) = gpu.recreate(hwnd) {
                                        eprintln!("tela win32-static: surface recreate: {error}");
                                    }
                                    state.redraw_or_backoff(hwnd, "lost");
                                }
                                Ok(RenderOutcome::Timeout) => {
                                    state.render_retries += 1;
                                    state.redraw_or_backoff(hwnd, "timeout");
                                }
                                Ok(RenderOutcome::Occluded) => {}
                                Ok(RenderOutcome::Validation) => {
                                    state.render_retries += 1;
                                    eprintln!("tela win32-static: surface validation failed");
                                }
                                Err(error) => {
                                    state.render_retries += 1;
                                    eprintln!("tela win32-static: render: {error}");
                                }
                            }
                        }
                    }
                    if presented_this_frame {
                        // 常规呈现节流；suboptimal 是异常路径必须立即打印。
                        if presented_suboptimal {
                            eprintln!("tela win32-static: presented suboptimal=true");
                        } else {
                            state.log_throttled(|| "tela win32-static: presented".to_owned());
                        }
                    }
                    let _ = EndPaint(hwnd, &paint);
                });
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            unsafe {
                with_state(hwnd, |state| {
                    state.begin_mouse_leave_tracking();
                    let (x, y) = client_point(lparam);
                    let scale = state.dpi_scale;
                    let event = state.mouse_pointer_event(
                        PointerPhase::Move,
                        x as f32 / scale,
                        y as f32 / scale,
                        if state.pointer_captured { 1 } else { 0 },
                        0.0,
                        0.0,
                    );
                    // 有动作（hover 变化/焦点/路由）才重绘；纯移动无状态变化时跳过，
                    // 避免无意义的空 paint。
                    if state.session.dispatch_pointer(event) > 0 {
                        request_redraw(hwnd);
                    }
                    // 实时刷新客户区光标：拖拽缩放（sizing loop）结束等场景系统不再
                    // 发 WM_SETCURSOR，只有这里能覆盖残留的系统光标。
                    state.apply_client_cursor();
                });
            }
            LRESULT(0)
        }
        WM_NCMOUSEMOVE => {
            // 非客户区（边缘/标题栏）移动期间清空 hover：此时没有客户区
            // WM_MOUSEMOVE，hover_key 保持陈旧，回到客户区会显示错误的手型。
            unsafe {
                with_state(hwnd, |state| {
                    let event =
                        state.mouse_pointer_event(PointerPhase::Move, -1.0, -1.0, 0, 0.0, 0.0);
                    state.session.dispatch_pointer(event);
                });
            }
            LRESULT(0)
        }
        0x02a3 => {
            // WM_MOUSELEAVE（本地常量作字面量，避免被解析为新绑定）
            unsafe {
                with_state(hwnd, |state| {
                    state.mouse_leave_tracking = false;
                    let event =
                        state.mouse_pointer_event(PointerPhase::Move, -1.0, -1.0, 0, 0.0, 0.0);
                    state.session.dispatch_pointer(event);
                });
            }
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            unsafe {
                let _ = SetFocus(Some(hwnd));
                with_state(hwnd, |state| {
                    state.pointer_captured = true;
                    let _ = SetCapture(hwnd);
                    let (x, y) = client_point(lparam);
                    let scale = state.dpi_scale;
                    let event = state.mouse_pointer_event(
                        PointerPhase::Down,
                        x as f32 / scale,
                        y as f32 / scale,
                        1,
                        0.0,
                        0.0,
                    );
                    // 点击会改变路由/焦点；有动作才重绘，否则画面停在旧帧。
                    if state.session.dispatch_pointer(event) > 0 {
                        request_redraw(hwnd);
                    }
                });
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            unsafe {
                with_state(hwnd, |state| {
                    let (x, y) = client_point(lparam);
                    let scale = state.dpi_scale;
                    let event = state.mouse_pointer_event(
                        PointerPhase::Up,
                        x as f32 / scale,
                        y as f32 / scale,
                        0,
                        0.0,
                        0.0,
                    );
                    let consumed = state.session.dispatch_pointer(event);
                    state.pointer_captured = false;
                    let _ = ReleaseCapture();
                    // 有动作（路由/焦点/输入）才重绘。
                    if consumed > 0 {
                        request_redraw(hwnd);
                    }
                });
            }
            LRESULT(0)
        }
        WM_CAPTURECHANGED | WM_CANCELMODE => {
            unsafe {
                with_state(hwnd, |state| {
                    if state.pointer_captured {
                        state.pointer_captured = false;
                        let event = state.mouse_pointer_event(
                            PointerPhase::Cancel,
                            -1.0,
                            -1.0,
                            0,
                            0.0,
                            0.0,
                        );
                        state.session.dispatch_pointer(event);
                    }
                });
            }
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            unsafe {
                let mut point = POINT {
                    x: (lparam.0 as u32 & 0xffff) as u16 as i16 as i32,
                    y: ((lparam.0 as u32 >> 16) & 0xffff) as u16 as i16 as i32,
                };
                let _ = ScreenToClient(hwnd, &mut point);
                with_state(hwnd, |state| {
                    let delta = ((wparam.0 as u32 >> 16) as u16 as i16) as f32;
                    let scale = state.dpi_scale;
                    let event = state.mouse_pointer_event(
                        PointerPhase::Scroll,
                        point.x as f32 / scale,
                        point.y as f32 / scale,
                        0,
                        0.0,
                        -(delta / 120.0) * 48.0,
                    );
                    state.session.dispatch_pointer(event);
                });
            }
            LRESULT(0)
        }
        WM_KEYDOWN => {
            let repeat = ((lparam.0 as u32 >> 30) & 1) != 0;
            unsafe {
                with_state(hwnd, |state| state.key_down(wparam.0 as u16, repeat));
            }
            LRESULT(0)
        }
        WM_CHAR => {
            unsafe {
                with_state(hwnd, |state| state.character(wparam.0 as u16));
            }
            LRESULT(0)
        }
        WM_KILLFOCUS => {
            unsafe {
                with_state(hwnd, |state| {
                    state.session.input_blur();
                    request_redraw(hwnd);
                });
            }
            LRESULT(0)
        }
        WM_SETCURSOR => {
            // MSDN: wParam = 光标所在窗口句柄；lParam 低位 = hit-test 码（WM_NCHITTEST
            // 返回值），lParam 高位 = 触发消息（如 WM_MOUSEMOVE）。只有客户区
            // （HTCLIENT）才由应用接管光标：hover 交互节点显示手型，否则箭头。
            // 边缘/标题栏/系统菜单等非客户区必须交回 DefWindowProcW，由系统显示
            // resize/移动等标准光标并支持边缘拖拽缩放。
            let hit_test = (lparam.0 & 0xffff) as u16;
            if hit_test == HTCLIENT as u16 {
                unsafe {
                    with_state(hwnd, |state| state.apply_client_cursor());
                }
                LRESULT(1)
            } else {
                unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
            }
        }
        WM_CLOSE => {
            unsafe {
                with_state(hwnd, |state| {
                    state.session.input_blur();
                });
                let _ = DestroyWindow(hwnd);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            unsafe {
                let value = SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                if value != 0 {
                    drop(Box::from_raw(value as *mut StaticWindowState));
                }
                PostQuitMessage(0);
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

impl StaticWindowState {
    /// 统一视口更新：GetClientRect 返回物理像素，逻辑尺寸 = 物理 / dpr；
    /// surface config 用物理像素。尺寸真正变化时才请求重绘。
    fn update_viewport(&mut self, hwnd: HWND, dpr: f32) {
        let (physical_w, physical_h) = client_size(hwnd);
        let logical_w = physical_w / dpr;
        let logical_h = physical_h / dpr;
        let changed = self.session.set_viewport(logical_w, logical_h, dpr);
        if let Some(gpu) = self.gpu.as_mut() {
            gpu.reconfigure(physical_w as u32, physical_h as u32);
        }
        if changed {
            // 拖拽缩放时 WM_SIZE 高频触发（每像素一次）；尺寸变化超过阈值才打印，
            // 避免日志刷屏（跨显示器 DPI 变化由 WM_DPICHANGED 日志覆盖）。
            let (last_w, last_h) = self.last_logged_viewport;
            let jump_w = (logical_w - last_w).abs();
            let jump_h = (logical_h - last_h).abs();
            if jump_w > 24.0 || jump_h > 24.0 {
                eprintln!(
                    "tela win32-static: viewport {:.0}x{:.0} logical, {}x{} physical, dpr={:.2}",
                    logical_w, logical_h, physical_w as u32, physical_h as u32, dpr
                );
                self.last_logged_viewport = (logical_w, logical_h);
            }
            request_redraw(hwnd);
        }
    }

    /// 高频诊断日志时间节流（500ms）：拖拽缩放期间 WM_PAINT 每帧触发。
    fn log_throttled(&mut self, message: impl FnOnce() -> String) {
        if self.last_log_at.elapsed().as_millis() >= 500 {
            self.last_log_at = Instant::now();
            eprintln!("{}", message());
        }
    }

    /// 重绘或熔断：连续失败达到阈值后暂停自动重绘，避免 Outdated/Suboptimal 忙循环。
    fn redraw_or_backoff(&mut self, hwnd: HWND, reason: &str) {
        if self.render_retries >= 5 {
            eprintln!(
                "tela win32-static: render backoff after {} retries ({reason}); waiting for the next viewport/input event",
                self.render_retries
            );
            return;
        }
        eprintln!(
            "tela win32-static: render retry #{}/5 ({reason})",
            self.render_retries
        );
        request_redraw(hwnd);
    }

    /// 客户区光标：hover 交互节点 → 手型，否则箭头。
    ///
    /// 在 WM_SETCURSOR（系统请求）与 WM_MOUSEMOVE（实时刷新，覆盖拖拽缩放结束等
    /// 系统不再发 WM_SETCURSOR 的场景）两处调用。
    fn apply_client_cursor(&mut self) {
        let id = if self.session.hover_interactive() {
            IDC_HAND
        } else {
            IDC_ARROW
        };
        // SAFETY: SetCursor 更新全局光标，UI 线程调用；LoadCursorW 静态系统光标 ID。
        unsafe {
            let _ = SetCursor(LoadCursorW(None, id).ok());
        }
    }

    fn mouse_pointer_event(
        &self,
        phase: PointerPhase,
        x: f32,
        y: f32,
        buttons: u8,
        delta_x: f32,
        delta_y: f32,
    ) -> PointerEvent {
        PointerEvent::new(
            PointerId(0),
            PointerKind::Mouse,
            phase,
            Point { x, y },
            PointerButtons(u16::from(buttons)),
            self.input_epoch
                .elapsed()
                .as_micros()
                .min(u128::from(u64::MAX)) as u64,
            Point {
                x: delta_x,
                y: delta_y,
            },
        )
    }

    fn key_down(&mut self, virtual_key: u16, repeat: bool) {
        let focused = self.session.input_focused();
        if focused {
            match virtual_key {
                key if key == VK_RETURN.0 => {
                    self.session.input_enter();
                    request_redraw(self.hwnd);
                    return;
                }
                key if key == VK_ESCAPE.0 => {
                    self.session.input_cancel();
                    request_redraw(self.hwnd);
                    return;
                }
                key if key != VK_TAB.0 && !has_command_modifier() => return,
                _ => {}
            }
        }
        let Some(physical_key) = physical_key(virtual_key) else {
            return;
        };
        self.session
            .dispatch_key(physical_key, modifier_bits(), repeat);
        request_redraw(self.hwnd);
    }

    fn character(&mut self, code_unit: u16) {
        if !self.session.input_focused() {
            return;
        }
        let mut value = self.session.input_value();
        match code_unit {
            8 => {
                value.pop();
            }
            13 => {
                // Enter: multiline editors insert a newline.
                value.push('\n');
            }
            0x20..=0x7e => {
                value.push(char::from_u32(code_unit as u32).expect("ASCII character"));
            }
            _ => return,
        }
        self.session.set_input_value(value);
        request_redraw(self.hwnd);
    }

    fn begin_mouse_leave_tracking(&mut self) {
        if self.mouse_leave_tracking {
            return;
        }
        let mut tracking = TRACKMOUSEEVENT {
            cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
            dwFlags: TME_LEAVE,
            hwndTrack: self.hwnd,
            dwHoverTime: 0,
        };
        if unsafe { TrackMouseEvent(&mut tracking) }.is_ok() {
            self.mouse_leave_tracking = true;
        }
    }
}

fn request_redraw(hwnd: HWND) {
    // SAFETY: hwnd belongs to this thread and no rectangle pointer is retained.
    let _ = unsafe { InvalidateRect(Some(hwnd), None, false) };
}

fn client_size(hwnd: HWND) -> (f32, f32) {
    let mut rect = RECT::default();
    // SAFETY: reading the client rectangle of our own window is synchronous.
    let _ = unsafe { GetClientRect(hwnd, &mut rect) };
    (
        (rect.right - rect.left) as f32,
        (rect.bottom - rect.top) as f32,
    )
}

fn screen_point(lparam: LPARAM) -> (i32, i32) {
    client_point(lparam)
}

fn client_point(lparam: LPARAM) -> (i32, i32) {
    let raw = lparam.0 as u32;
    (
        (raw & 0xffff) as u16 as i16 as i32,
        ((raw >> 16) & 0xffff) as u16 as i16 as i32,
    )
}

fn modifier_bits() -> u8 {
    let mut bits = 0u8;
    // SAFETY: GetAsyncKeyState is a global key-state query, safe from the UI thread.
    // GetAsyncKeyState returns SHORT; the high bit (pressed) makes it negative.
    if unsafe { GetAsyncKeyState(i32::from(VK_SHIFT.0)) } < 0 {
        bits |= 1;
    }
    if unsafe { GetAsyncKeyState(i32::from(VK_CONTROL.0)) } < 0 {
        bits |= 2;
    }
    if unsafe { GetAsyncKeyState(i32::from(VK_MENU.0)) } < 0 {
        bits |= 4;
    }
    if unsafe { GetAsyncKeyState(i32::from(VK_LWIN.0)) } < 0
        || unsafe { GetAsyncKeyState(i32::from(VK_RWIN.0)) } < 0
    {
        bits |= 8;
    }
    bits
}

fn has_command_modifier() -> bool {
    modifier_bits() & 0b1100 != 0
}

fn physical_key(virtual_key: u16) -> Option<u16> {
    if (b'A' as u16..=b'Z' as u16).contains(&virtual_key) {
        return Some(0x04 + (virtual_key - b'A' as u16));
    }
    if (b'1' as u16..=b'9' as u16).contains(&virtual_key) {
        return Some(0x1e + (virtual_key - b'1' as u16));
    }
    if virtual_key == b'0' as u16 {
        return Some(0x27);
    }
    match virtual_key {
        key if key == VK_RETURN.0 => Some(0x28),
        key if key == VK_ESCAPE.0 => Some(0x29),
        key if key == VK_BACK.0 => Some(0x2a),
        key if key == VK_TAB.0 => Some(0x2b),
        key if key == VK_HOME.0 => Some(0x4a),
        key if key == VK_PRIOR.0 => Some(0x4b),
        key if key == VK_DELETE.0 => Some(0x4c),
        key if key == VK_END.0 => Some(0x4d),
        key if key == VK_NEXT.0 => Some(0x4e),
        key if key == VK_RIGHT.0 => Some(0x4f),
        key if key == VK_LEFT.0 => Some(0x50),
        key if key == VK_DOWN.0 => Some(0x51),
        key if key == VK_UP.0 => Some(0x52),
        _ => None,
    }
}
