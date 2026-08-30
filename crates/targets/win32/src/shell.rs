//! 统一 Win32 壳：一个消息循环 + 三个轴（Session 来源 / 启动 / chrome）。
//!
//! 这是本 crate 在 Windows 上的唯一 unsafe 边界：`GWLP_USERDATA` 持有 `Box<WindowState>`
//! 指针（经 `WM_NCCREATE`/`lpCreateParams` 安装），WGPU 持有 HWND 派生的原始句柄。

#![allow(unsafe_code)]
use std::{
    cell::RefCell,
    ffi::c_void,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
    time::Instant,
};

use crate::chrome::{WindowChrome, hit_test};
use crate::driver::SessionDriver;
use crate::gpu::{GpuSession, RenderOutcome};
use crate::input;
use crate::providers::{WindowMetrics, build_dispatcher};
use crate::startup::{self, WM_TELA_DEVICE_LOST, WM_TELA_STARTUP_READY};
use tela_app_abi::{
    AppDispatchOutcome, AppEvent, AppFrameInput, AppPointerEvent, AppPointerKind, AppPointerPhase,
    ApplicationSession, CursorKind,
};
use tela_desktop_runtime::bridge::common::BuildConstants;
use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::Dwm::DwmFlush,
        Graphics::Gdi::{
            BeginPaint, COLOR_WINDOW, DT_LEFT, DT_TOP, DT_WORDBREAK, DrawTextW, EndPaint, FillRect,
            GetSysColorBrush, InvalidateRect, PAINTSTRUCT, ScreenToClient, UpdateWindow,
        },
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            HiDpi::{
                DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForWindow,
                SetProcessDpiAwarenessContext,
            },
            Input::KeyboardAndMouse::{
                GetAsyncKeyState, ReleaseCapture, SetCapture, SetFocus, TME_LEAVE, TRACKMOUSEEVENT,
                TrackMouseEvent, VK_CONTROL, VK_ESCAPE, VK_LWIN, VK_MENU, VK_RETURN, VK_RWIN,
                VK_SHIFT, VK_TAB,
            },
            WindowsAndMessaging::{
                CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW,
                DestroyWindow, DispatchMessageW, GWLP_USERDATA, GetClientRect, GetWindowLongPtrW,
                HTCLIENT, IDC_ARROW, IDC_HAND, IDC_IBEAM, IsZoomed, KillTimer, LoadCursorW, MSG,
                MWMO_INPUTAVAILABLE, MsgWaitForMultipleObjectsEx, PM_REMOVE, PeekMessageW,
                PostMessageW, PostQuitMessage, QS_ALLINPUT, RegisterClassW, SIZE_MINIMIZED,
                SW_SHOW, SetCursor, SetTimer, SetWindowLongPtrW, SetWindowPos, ShowWindow,
                TranslateMessage, WINDOW_EX_STYLE, WM_CANCELMODE, WM_CAPTURECHANGED, WM_CHAR,
                WM_CLOSE, WM_DESTROY, WM_DPICHANGED, WM_KEYDOWN, WM_KILLFOCUS, WM_LBUTTONDOWN,
                WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCCREATE, WM_NCDESTROY, WM_PAINT,
                WM_QUIT, WM_SETCURSOR, WM_SETFOCUS, WM_SIZE, WM_TIMER, WNDCLASSW,
            },
        },
    },
    core::PCWSTR,
};

use tela_desktop_runtime::{
    DeviceLossAction, GuestRuntime, GuestSession, PlatformLaunchOptions, ShellLifecycle,
    ShellPhase, TextChannelAction,
};

const WINDOW_CLASS: &str = "TelaWin32Shell";
const WINDOW_TITLE: &str = "TELA Files";
const TIMER_WAKE: usize = 1;
const TIMER_ANIMATION: usize = 2;
const TIMER_SURFACE_RETRY: usize = 3;

/// 会话来源轴。
pub enum SessionSource {
    /// 进程内应用会话：建窗前同步 initialize + 首帧发布（编辑器/变速齿轮）。
    Immediate(Box<dyn ApplicationSession>),
    /// 后台加载 bundle → WASM guest（SDK 开发壳）。
    Background(PlatformLaunchOptions),
}

/// 统一壳配置。
pub struct ShellOptions {
    /// 窗口标题。
    pub title: String,
    /// 初始窗口宽度（物理像素）。
    pub width: i32,
    /// 初始窗口高度（物理像素）。
    pub height: i32,
    /// chrome 形态。
    pub chrome: WindowChrome,
    /// 会话来源。
    pub source: SessionSource,
}

/// 静态产品的窗口选项（`run_native_window` 入参）。
#[derive(Clone, Debug)]
pub struct NativeWindowOptions {
    /// 窗口标题。
    pub title: String,
    /// 初始宽度（物理像素）。
    pub width: i32,
    /// 初始高度（物理像素）。
    pub height: i32,
    /// chrome 形态；静态产品默认自绘标题栏。
    pub chrome: WindowChrome,
}

impl NativeWindowOptions {
    /// 用标题创建默认（自绘 chrome）选项。
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            width: 960,
            height: 640,
            chrome: WindowChrome::CustomTitleBar,
        }
    }

    /// 设置初始窗口尺寸（物理像素）。
    pub fn size(mut self, width: i32, height: i32) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    /// 切换为系统外框 chrome。
    pub fn system_chrome(mut self) -> Self {
        self.chrome = WindowChrome::SystemOverlapped;
        self
    }
}

/// 以进程内会话启动一个自绘 chrome 窗口（静态产品入口；阻塞至窗口关闭）。
pub fn run_native_window(
    app: Box<dyn ApplicationSession>,
    options: NativeWindowOptions,
) -> Result<(), String> {
    run_window(ShellOptions {
        title: options.title,
        width: options.width,
        height: options.height,
        chrome: options.chrome,
        source: SessionSource::Immediate(app),
    })
}

/// SDK 开发壳入口：后台加载 bundle 并以 WASM guest 运行（阻塞至窗口关闭）。
pub fn run_sdk_window(options: PlatformLaunchOptions) -> Result<(), String> {
    if options.verbose {
        eprintln!(
            "tela-win32-host: startup index={} cache={}",
            options.bundle_index_url,
            startup::cache_path()?.display()
        );
    }
    run_window(ShellOptions {
        title: WINDOW_TITLE.to_owned(),
        width: 1280,
        height: 840,
        chrome: WindowChrome::SystemOverlapped,
        source: SessionSource::Background(options),
    })
    .map_err(|error| format!("tela-win32-host: {error}"))
}
// `windows` exposes this ordinary client-area message through its Controls namespace. The shell
// does not otherwise need common-control APIs, so retain the SDK-defined message value locally.
const WM_MOUSELEAVE: u32 = 0x02a3;

#[derive(Clone, Copy)]
struct ClientMetrics {
    width: u32,
    height: u32,
    dpi_scale: f32,
}

struct WindowState {
    hwnd: HWND,
    lifecycle: ShellLifecycle,
    chrome: WindowChrome,
    /// 会话驱动器：GuestSession(WASM) 经统一握手驱动；安装前为 `None`。
    session: Option<SessionDriver>,
    gpu: Option<GpuSession>,
    dpi_scale: f32,
    startup_rx: Option<Receiver<Result<GuestRuntime, String>>>,
    startup_cancel: Arc<AtomicBool>,
    input_epoch: Instant,
    mouse_leave_tracking: bool,
    pointer_captured: bool,
    startup_error: Option<String>,
    terminal_error: Option<String>,
    bridge_metrics: Rc<RefCell<WindowMetrics>>,
}

impl WindowState {
    fn new(
        chrome: WindowChrome,
        startup_rx: Option<Receiver<Result<GuestRuntime, String>>>,
        startup_cancel: Arc<AtomicBool>,
    ) -> Self {
        Self {
            hwnd: HWND::default(),
            lifecycle: ShellLifecycle::new(),
            chrome,
            session: None,
            gpu: None,
            dpi_scale: 1.0,
            startup_rx,
            startup_cancel,
            input_epoch: Instant::now(),
            mouse_leave_tracking: false,
            pointer_captured: false,
            startup_error: None,
            terminal_error: None,
            bridge_metrics: Rc::new(RefCell::new(WindowMetrics::default())),
        }
    }

    fn receive_startup_result(&mut self) {
        match startup::poll_startup(&mut self.startup_rx) {
            startup::StartupPoll::Pending => return,
            startup::StartupPoll::Ready(result) => match result {
                Ok(runtime) => self.install_runtime(runtime),
                Err(error) => self.fail_startup(error),
            },
            startup::StartupPoll::Disconnected => {
                self.fail_startup(
                    "startup worker disconnected before returning a result".to_owned(),
                );
            }
        }
    }

    fn install_runtime(&mut self, runtime: GuestRuntime) {
        if self.lifecycle.phase() != ShellPhase::Loading {
            return;
        }
        // 桥由壳统一装配；GuestSession 的每次派发都会排空 guest 请求并投递响应。
        let dispatcher = build_dispatcher(
            Rc::clone(&self.bridge_metrics),
            &BuildConstants::default(),
            vec![],
        );
        let installation: Result<(), String> = (|| {
            let guest = GuestSession::new(runtime, Some(dispatcher))?;
            let session = SessionDriver::new(Box::new(guest))?;
            self.session = Some(session);
            Ok(())
        })();
        match installation {
            Ok(()) => self.finish_startup(),
            Err(error) => self.fail_startup(format!("initialize guest session: {error}")),
        }
    }

    /// Immediate 轴：安装建窗前已就绪的会话（首帧已发布）并完成启动收尾。
    fn install_immediate(&mut self, session: SessionDriver) {
        if self.lifecycle.phase() != ShellPhase::Loading {
            return;
        }
        self.session = Some(session);
        self.finish_startup();
    }

    /// 会话就绪后的启动收尾：GPU、相位推进、首视口与最大化同步。
    fn finish_startup(&mut self) {
        let activation: Result<(), String> = (|| {
            let Some(metrics) = self.client_metrics()? else {
                self.lifecycle.startup_succeeded(false);
                return Ok(());
            };
            self.initialize_gpu(metrics)?;
            self.lifecycle.startup_succeeded(true);
            self.dispatch_viewport(metrics)?;
            self.sync_window_maximized();
            Ok(())
        })();
        if let Err(error) = activation {
            self.fail_startup(format!("initialize native renderer: {error}"));
            return;
        }
        self.request_redraw();
    }

    fn fail_startup(&mut self, error: String) {
        if self.lifecycle.phase() != ShellPhase::Loading {
            return;
        }
        eprintln!("tela-win32-host: startup failed: {error}");
        self.session = None;
        self.gpu = None;
        self.startup_error = Some(error);
        self.lifecycle.startup_failed();
        self.request_redraw();
    }

    fn client_metrics(&mut self) -> Result<Option<ClientMetrics>, String> {
        let mut rect = RECT::default();
        // SAFETY: `self.hwnd` is live for the lifetime of WindowState while the message loop runs.
        unsafe { GetClientRect(self.hwnd, &mut rect) }
            .map_err(|error| format!("read Win32 client rect: {error}"))?;
        // SAFETY: this only reads monitor DPI for the same live HWND.
        let dpi_scale = unsafe { GetDpiForWindow(self.hwnd) }.max(96) as f32 / 96.0;
        self.dpi_scale = dpi_scale;
        let width = rect.right - rect.left;
        let height = rect.bottom - rect.top;
        if width <= 0 || height <= 0 {
            return Ok(None);
        }
        Ok(Some(ClientMetrics {
            width: width as u32,
            height: height as u32,
            dpi_scale,
        }))
    }

    fn initialize_gpu(&mut self, metrics: ClientMetrics) -> Result<(), String> {
        let gpu = GpuSession::new(
            self.hwnd,
            metrics.width,
            metrics.height,
            metrics.dpi_scale,
            WM_TELA_DEVICE_LOST,
        )?;
        self.dpi_scale = metrics.dpi_scale;
        self.gpu = Some(gpu);
        Ok(())
    }

    fn dispatch_viewport(&mut self, metrics: ClientMetrics) -> Result<(), String> {
        // A frame from the previous client geometry must not route input after this point.
        if let Some(session) = self.session.as_mut() {
            session.invalidate_presented();
        }
        self.bridge_metrics.replace(WindowMetrics {
            width: (metrics.width as f32 / metrics.dpi_scale) as u32,
            height: (metrics.height as f32 / metrics.dpi_scale) as u32,
            dpr: metrics.dpi_scale,
        });
        self.dispatch_guest(AppEvent::Viewport {
            width: metrics.width as f32 / metrics.dpi_scale,
            height: metrics.height as f32 / metrics.dpi_scale,
        })?;
        Ok(())
    }

    fn resize(&mut self, minimized: bool) -> Result<(), String> {
        let metrics = if minimized {
            None
        } else {
            self.client_metrics()?
        };
        if self.session.is_none() {
            return Ok(());
        }
        let Some(metrics) = metrics else {
            self.lifecycle.client_area_changed(false);
            self.cancel_surface_retry();
            return Ok(());
        };

        if self.gpu.is_none() {
            self.initialize_gpu(metrics)?;
        } else if let Some(gpu) = self.gpu.as_mut()
            && (gpu.config.width != metrics.width || gpu.config.height != metrics.height)
        {
            gpu.reconfigure(metrics.width, metrics.height);
        }
        self.lifecycle.client_area_changed(true);
        self.dispatch_viewport(metrics)?;
        self.sync_window_maximized();
        self.request_redraw();
        Ok(())
    }

    fn dispatch_guest(&mut self, event: AppEvent) -> Result<AppDispatchOutcome, String> {
        let outcome = self.dispatch_guest_without_text_reconcile(event)?;
        self.synchronize_text_channel(None)?;
        Ok(outcome)
    }

    fn dispatch_guest_without_text_reconcile(
        &mut self,
        event: AppEvent,
    ) -> Result<AppDispatchOutcome, String> {
        let session = self
            .session
            .as_mut()
            .ok_or_else(|| "dispatch without a live guest session".to_owned())?;
        Ok(session.dispatch(event))
    }

    fn dispatch_presented_input(
        &mut self,
        input: AppFrameInput,
    ) -> Result<AppDispatchOutcome, String> {
        let outcome = self.dispatch_presented_input_without_text_reconcile(input)?;
        self.synchronize_text_channel(None)?;
        Ok(outcome)
    }

    fn dispatch_presented_input_without_text_reconcile(
        &mut self,
        input: AppFrameInput,
    ) -> Result<AppDispatchOutcome, String> {
        let session = self
            .session
            .as_mut()
            .ok_or_else(|| "dispatch without a live guest session".to_owned())?;
        Ok(session.dispatch_frame_input(input))
    }

    fn synchronize_text_channel(&mut self, window_focus: Option<bool>) -> Result<(), String> {
        let guest_wants_text = self
            .session
            .as_ref()
            .is_some_and(|session| session.input_focused());
        let first = match window_focus {
            Some(focused) => self.lifecycle.set_window_focus(focused, guest_wants_text),
            None => self.lifecycle.reconcile_text_channel(guest_wants_text),
        };
        self.dispatch_text_channel_action(first)?;

        // A Blur can cause a guest to advance between two text fields. Reconcile that one semantic
        // edge immediately, but never loop indefinitely around guest code from a native callback.
        let guest_wants_text = self
            .session
            .as_ref()
            .is_some_and(|session| session.input_focused());
        let second = self.lifecycle.reconcile_text_channel(guest_wants_text);
        self.dispatch_text_channel_action(second)
    }

    fn dispatch_text_channel_action(
        &mut self,
        action: Option<TextChannelAction>,
    ) -> Result<(), String> {
        let Some(action) = action else {
            return Ok(());
        };
        let input = match action {
            TextChannelAction::Focus => AppFrameInput::InputFocus,
            TextChannelAction::Blur => AppFrameInput::InputBlur,
        };
        // This acknowledgement originates from the native editor, so it remains frame-owned. Do
        // not call `dispatch_guest` here: this method runs inside text-channel reconciliation and
        // intentionally avoids recursive lifecycle transitions.
        let _ = self.dispatch_presented_input_without_text_reconcile(input)?;
        Ok(())
    }

    fn pointer(&mut self, input: AppFrameInput) -> Result<(), String> {
        if !self.lifecycle.can_render() {
            return Ok(());
        }
        let outcome = self.dispatch_presented_input(input)?;
        if outcome.publish_requested {
            self.request_redraw();
        }
        Ok(())
    }

    fn mouse_pointer_event(
        &self,
        phase: AppPointerPhase,
        x: f32,
        y: f32,
        buttons: u16,
        delta_x: f32,
        delta_y: f32,
    ) -> AppFrameInput {
        let timestamp_micros = self
            .input_epoch
            .elapsed()
            .as_micros()
            .min(u128::from(u64::MAX)) as u64;
        AppFrameInput::Pointer(AppPointerEvent::new(
            0,
            AppPointerKind::Mouse,
            phase,
            x,
            y,
            buttons,
            timestamp_micros,
            delta_x,
            delta_y,
        ))
    }

    fn key_down(&mut self, virtual_key: u16, repeat: bool) -> Result<bool, String> {
        if !self.lifecycle.can_render() {
            return Ok(false);
        }
        let input_focused = self
            .session
            .as_ref()
            .is_some_and(|session| session.input_focused());
        if input_focused {
            match virtual_key {
                key if key == VK_RETURN.0 => {
                    let outcome = self.dispatch_presented_input(AppFrameInput::InputEnter)?;
                    if outcome.publish_requested {
                        self.request_redraw();
                    }
                    return Ok(true);
                }
                key if key == VK_ESCAPE.0 => {
                    let outcome = self.dispatch_presented_input(AppFrameInput::InputCancel)?;
                    if outcome.publish_requested {
                        self.request_redraw();
                    }
                    return Ok(true);
                }
                key if key != VK_TAB.0 && !has_command_modifier() => return Ok(false),
                _ => {}
            }
        }
        let Some(physical_key) = input::physical_key(virtual_key) else {
            return Ok(false);
        };
        let outcome = self.dispatch_presented_input(AppFrameInput::KeyDown {
            physical_key,
            modifier_bits: modifier_bits(),
            repeat,
        })?;
        if outcome.publish_requested {
            self.request_redraw();
        }
        Ok(outcome.handled)
    }

    fn character(&mut self, code_unit: u16) -> Result<bool, String> {
        let Some(session) = self.session.as_ref() else {
            return Ok(false);
        };
        if !self.lifecycle.can_render() || !session.input_focused() {
            return Ok(false);
        }
        let mut value = session.input_value();
        if !input::apply_character_code_unit(&mut value, code_unit) {
            return Ok(false);
        }
        let outcome = self.dispatch_presented_input(AppFrameInput::SetInputValue(value))?;
        if outcome.publish_requested {
            self.request_redraw();
        }
        Ok(true)
    }

    fn render(&mut self) -> Result<RenderOutcome, String> {
        if !self.lifecycle.can_render() {
            return Ok(RenderOutcome::Occluded);
        }
        let (frame, damage) = self
            .session
            .as_ref()
            .map(|session| (session.frame().clone(), session.frame_damage().clone()))
            .ok_or_else(|| "render without a resolved UI frame".to_owned())?;
        let gpu = self
            .gpu
            .as_mut()
            .ok_or_else(|| "render without a live GPU session".to_owned())?;
        gpu.render(&frame, &damage)
    }

    fn paint_guest(&mut self) -> Result<(), String> {
        match self.render()? {
            RenderOutcome::Presented { suboptimal } => {
                if let Some(session) = self.session.as_mut() {
                    session.frame_presented();
                }
                self.lifecycle.surface_presented();
                self.cancel_surface_retry();
                if suboptimal {
                    self.reconfigure_surface()?;
                    self.request_redraw();
                }
            }
            RenderOutcome::Outdated => {
                if let Some(session) = self.session.as_mut() {
                    session.invalidate_presented();
                }
                self.reconfigure_surface()?;
                self.request_redraw();
            }
            RenderOutcome::Lost => {
                if let Some(session) = self.session.as_mut() {
                    session.invalidate_presented();
                }
                self.recreate_surface()?;
                self.request_redraw();
            }
            RenderOutcome::Timeout => self.schedule_surface_retry()?,
            RenderOutcome::Occluded => {}
            RenderOutcome::Validation => {
                return Err("WGPU surface validation failed while acquiring a frame".to_owned());
            }
        }
        Ok(())
    }

    fn reconfigure_surface(&mut self) -> Result<(), String> {
        if let Some(session) = self.session.as_mut() {
            session.invalidate_presented();
        }
        let Some(metrics) = self.client_metrics()? else {
            self.lifecycle.client_area_changed(false);
            return Ok(());
        };
        let gpu = self
            .gpu
            .as_mut()
            .ok_or_else(|| "reconfigure without a live GPU session".to_owned())?;
        gpu.reconfigure(metrics.width, metrics.height);
        self.dpi_scale = metrics.dpi_scale;
        Ok(())
    }

    fn recreate_surface(&mut self) -> Result<(), String> {
        if let Some(session) = self.session.as_mut() {
            session.invalidate_presented();
        }
        let Some(metrics) = self.client_metrics()? else {
            self.lifecycle.client_area_changed(false);
            return Ok(());
        };
        let gpu = self
            .gpu
            .as_mut()
            .ok_or_else(|| "recreate surface without a live GPU session".to_owned())?;
        gpu.recreate(self.hwnd)?;
        gpu.reconfigure(metrics.width, metrics.height);
        self.dpi_scale = metrics.dpi_scale;
        self.dispatch_viewport(metrics)?;
        Ok(())
    }

    fn schedule_surface_retry(&mut self) -> Result<(), String> {
        let Some(delay_ms) = self.lifecycle.surface_timeout() else {
            return Ok(());
        };
        // SAFETY: this sets a window-owned one-shot timer; WM_TIMER kills it before repainting.
        let timer = unsafe { SetTimer(Some(self.hwnd), TIMER_SURFACE_RETRY, delay_ms, None) };
        if timer == 0 {
            return Err("schedule WGPU surface retry timer".to_owned());
        }
        Ok(())
    }

    fn on_surface_retry_timer(&mut self) {
        // SAFETY: it is harmless to cancel an already-fired one-shot window timer.
        let _ = unsafe { KillTimer(Some(self.hwnd), TIMER_SURFACE_RETRY) };
        if self.lifecycle.take_surface_retry() {
            self.request_redraw();
        }
    }

    fn cancel_surface_retry(&mut self) {
        // The lifecycle will ignore a stale WM_TIMER after this. Kill it too, so recovery does not
        // cause an unnecessary later paint.
        let _ = unsafe { KillTimer(Some(self.hwnd), TIMER_SURFACE_RETRY) };
        self.lifecycle.cancel_surface_retry();
    }

    fn receive_device_loss(&mut self) {
        let Some(gpu) = self.gpu.as_ref() else {
            return;
        };
        let Some(report) = gpu.take_device_loss_report() else {
            return;
        };
        if report.generation != gpu.generation() {
            return;
        }
        let detail = report.detail;
        match self.lifecycle.device_lost() {
            None => {}
            Some(DeviceLossAction::RecreateGpu) => {
                eprintln!("tela-win32-host: WGPU device lost, rebuilding once: {detail}");
                self.gpu = None;
                if let Some(session) = self.session.as_mut() {
                    session.invalidate_presented();
                }
                if self.lifecycle.phase() == ShellPhase::Suspended {
                    return;
                }
                let recovery: Result<(), String> = (|| {
                    let Some(metrics) = self.client_metrics()? else {
                        self.lifecycle.client_area_changed(false);
                        return Ok(());
                    };
                    self.initialize_gpu(metrics)?;
                    self.dispatch_viewport(metrics)?;
                    self.request_redraw();
                    Ok(())
                })();
                if let Err(error) = recovery {
                    self.terminate(format!("recreate WGPU after device loss: {error}"));
                }
            }
            Some(DeviceLossAction::Exit) => {
                self.terminate(format!(
                    "WGPU device was lost again after recovery: {detail}"
                ));
            }
        }
    }

    /// 动画驱动：活动动画按 deadline（钳制 1..16ms）排定时器；空闲时撤销。
    fn sync_animation_timer(&mut self) {
        let Some((animation_active, next_deadline_ms)) = self
            .session
            .as_ref()
            .and_then(|session| session.status())
            .map(|status| (status.animation_active, status.next_deadline_ms))
        else {
            return;
        };
        if animation_active {
            let now_ms = self.input_epoch.elapsed().as_millis() as u64;
            let delay_ms = next_deadline_ms
                .map(|deadline| deadline.saturating_sub(now_ms).clamp(1, 16))
                .unwrap_or(16) as u32;
            // SAFETY: window-owned timer on the UI thread.
            let _ = unsafe { SetTimer(Some(self.hwnd), TIMER_ANIMATION, delay_ms, None) };
        } else {
            // SAFETY: harmless to revoke an idle timer.
            let _ = unsafe { KillTimer(Some(self.hwnd), TIMER_ANIMATION) };
        }
    }

    /// 动画节拍：DwmFlush 对齐合成器；失败场景（RDP/禁用 DWM）仍由短定时器保底。
    fn on_animation_timer(&mut self) -> bool {
        // SAFETY: DwmFlush 是同步合成器等待，UI 线程安全。
        let _ = unsafe { DwmFlush() };
        self.tick(AppEvent::Tick {
            timestamp_ms: self.input_epoch.elapsed().as_millis() as u64,
        })
    }

    fn wake(&mut self) -> bool {
        self.tick(AppEvent::Wake {
            timestamp_ms: self.input_epoch.elapsed().as_millis() as u64,
        })
    }

    fn tick(&mut self, event: AppEvent) -> bool {
        match self.dispatch_guest_without_text_reconcile(event) {
            Ok(outcome) => {
                if outcome.publish_requested {
                    self.request_redraw();
                }
                outcome.publish_requested
            }
            Err(error) => {
                self.terminate(error);
                false
            }
        }
    }

    /// 自绘 chrome 的最大化状态同步；返回是否变化。
    fn sync_window_maximized(&mut self) -> bool {
        if self.chrome != WindowChrome::CustomTitleBar {
            return false;
        }
        // SAFETY: IsZoomed 是同步窗口查询。
        let maximized = unsafe { IsZoomed(self.hwnd) }.as_bool();
        self.session
            .as_mut()
            .map(|session| {
                session
                    .dispatch(AppEvent::WindowState { maximized })
                    .handled
            })
            .unwrap_or(false)
    }

    /// 取出并执行一条随呈现排空的窗口命令（自绘 chrome 专用）。
    fn execute_pending_window_command(&mut self) {
        if self.chrome != WindowChrome::CustomTitleBar {
            return;
        }
        let Some(session) = self.session.as_mut() else {
            return;
        };
        let Some(command) = session.take_window_command() else {
            return;
        };
        // SAFETY: hwnd 属于 UI 线程，命令执行是标准的窗口管理调用。
        unsafe { crate::chrome::execute_window_command(self.hwnd, command) };
        if self.sync_window_maximized() {
            self.request_redraw();
        }
    }

    fn request_redraw(&mut self) {
        if self.lifecycle.request_redraw() {
            // SAFETY: hwnd belongs to this thread and no rectangle pointer is retained by Win32.
            let _ = unsafe { InvalidateRect(Some(self.hwnd), None, false) };
        }
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
        // SAFETY: tracking holds only the live HWND and is copied by the system during this call.
        if unsafe { TrackMouseEvent(&mut tracking) }.is_ok() {
            self.mouse_leave_tracking = true;
        }
    }

    fn pointer_left_client(&mut self) -> Result<(), String> {
        self.mouse_leave_tracking = false;
        let event = self.mouse_pointer_event(AppPointerPhase::Move, -1.0, -1.0, 0, 0.0, 0.0);
        self.pointer(event)
    }

    fn window_focus_changed(&mut self, focused: bool) -> Result<(), String> {
        self.synchronize_text_channel(Some(focused))?;
        if focused && self.lifecycle.can_render() {
            self.request_redraw();
        }
        Ok(())
    }

    fn begin_pointer_capture(&mut self) {
        // SAFETY: the current UI thread owns hwnd; capture is released on button-up and lifecycle exits.
        unsafe { SetCapture(self.hwnd) };
        self.pointer_captured = true;
    }

    fn end_pointer_capture(&mut self, release_native_capture: bool) {
        if !self.pointer_captured {
            return;
        }
        self.pointer_captured = false;
        if release_native_capture {
            // SAFETY: ReleaseCapture is harmless when capture was already transferred by Windows.
            let _ = unsafe { ReleaseCapture() };
        }
    }

    fn begin_close(&mut self) {
        if self.lifecycle.phase() == ShellPhase::Closing {
            return;
        }
        self.startup_cancel.store(true, Ordering::Release);
        if let Some(session) = self.session.as_mut() {
            session.invalidate_presented();
        }
        self.end_pointer_capture(true);
        let _ = self.pointer_left_client();
        self.cancel_surface_retry();
        let action = self.lifecycle.begin_close();
        if let Err(error) = self.dispatch_text_channel_action(action) {
            eprintln!("tela-win32-host: close text channel: {error}");
        }
    }

    fn terminate(&mut self, error: String) {
        if self.terminal_error.is_some() {
            return;
        }
        eprintln!("tela-win32-host: {error}");
        self.terminal_error = Some(error);
        self.begin_close();
        // SAFETY: posting a scalar close message lets the current callback return before destruction.
        let _ = unsafe {
            PostMessageW(
                Some(self.hwnd),
                WM_CLOSE,
                WPARAM::default(),
                LPARAM::default(),
            )
        };
    }

    /// 客户区光标：输入聚焦 → I-beam；hover 命中可点击 → 手型；否则箭头。
    ///
    /// 在 WM_SETCURSOR（系统请求）与 WM_MOUSEMOVE（实时刷新，覆盖拖拽缩放结束等
    /// 系统不再发 WM_SETCURSOR 的场景）两处调用。
    fn apply_client_cursor(&self) {
        let cursor = match self
            .session
            .as_ref()
            .map(|session| session.cursor())
            .unwrap_or(CursorKind::Default)
        {
            CursorKind::Text => IDC_IBEAM,
            CursorKind::Pointer => IDC_HAND,
            CursorKind::Default => IDC_ARROW,
        };
        // SAFETY: system cursor resources are process-owned and do not transfer ownership.
        if let Ok(cursor) = unsafe { LoadCursorW(None, cursor) } {
            // SAFETY: setting the current thread cursor does not borrow Rust state.
            unsafe { SetCursor(Some(cursor)) };
        }
    }

    fn paint_native_status(&self, paint: &PAINTSTRUCT) {
        // SAFETY: system brush is process-owned and valid for this fill call.
        let brush = unsafe { GetSysColorBrush(COLOR_WINDOW) };
        // SAFETY: BeginPaint created this HDC and the rectangle points into the same PAINTSTRUCT.
        unsafe { FillRect(paint.hdc, &paint.rcPaint, brush) };
        let message = match self.lifecycle.phase() {
            ShellPhase::Loading => "TELA is starting...",
            ShellPhase::Failed => {
                "TELA could not start.\n\nClose this window and inspect the terminal output.\n\n"
            }
            _ => return,
        };
        let detail = self.startup_error.as_deref().unwrap_or_default();
        let mut text = if detail.is_empty() {
            message.to_owned()
        } else {
            format!("{message}{detail}")
        }
        .encode_utf16()
        .collect::<Vec<_>>();
        let mut bounds = RECT {
            left: 28,
            top: 28,
            right: (paint.rcPaint.right - 28).max(28),
            bottom: (paint.rcPaint.bottom - 28).max(28),
        };
        // SAFETY: DrawTextW consumes the supplied writable UTF-16 buffer only during this call.
        unsafe {
            DrawTextW(
                paint.hdc,
                &mut text,
                &mut bounds,
                DT_LEFT | DT_TOP | DT_WORDBREAK,
            )
        };
    }
}

/// 统一壳入口：创建窗口并运行至关闭。
pub fn run_window(options: ShellOptions) -> Result<(), String> {
    // 启动轴分叉：Immediate 在建窗前同步初始化会话并发布首帧（失败仍建窗显示
    // Failed 诊断页）；Background 建窗后由 worker 接管（Loading 页先行）。
    let startup_cancel = Arc::new(AtomicBool::new(false));
    let mut immediate_session = None;
    let mut immediate_error = None;
    let mut background: Option<(
        PlatformLaunchOptions,
        mpsc::Sender<Result<GuestRuntime, String>>,
    )> = None;
    let mut startup_rx: Option<Receiver<Result<GuestRuntime, String>>> = None;
    match options.source {
        SessionSource::Immediate(app) => match SessionDriver::new(app) {
            Ok(session) => immediate_session = Some(session),
            Err(error) => immediate_error = Some(error),
        },
        SessionSource::Background(launch) => {
            let (sender, receiver) = mpsc::channel();
            startup_rx = Some(receiver);
            background = Some((launch, sender));
        }
    }
    let _cache = startup::cache_path()?;
    // SAFETY: Per-Monitor V2 is selected before this process creates an HWND. Unsupported older
    // Windows versions keep their default DPI mode and are still usable.
    let _ = unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    let module =
        unsafe { GetModuleHandleW(None) }.map_err(|error| format!("get module handle: {error}"))?;
    let instance = HINSTANCE(module.0);
    let class_name = utf16z(WINDOW_CLASS);
    let title = utf16z(&options.title);
    let class = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(window_proc),
        hInstance: instance,
        lpszClassName: PCWSTR(class_name.as_ptr()),
        ..Default::default()
    };
    // SAFETY: the class procedure has the system ABI and the class strings stay live through creation.
    if unsafe { RegisterClassW(&class) } == 0 {
        return Err(format!(
            "register Win32 class: {}",
            windows::core::Error::from_thread()
        ));
    }

    let state = Box::new(WindowState::new(
        options.chrome,
        startup_rx,
        Arc::clone(&startup_cancel),
    ));
    let state_pointer = Box::into_raw(state);
    // SAFETY: WM_NCCREATE stores this allocation in GWLP_USERDATA; run_window recovers it exactly
    // once after the message loop. CreateWindowExW does not retain the UTF-16 vectors after return.
    let hwnd = match unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            PCWSTR(class_name.as_ptr()),
            PCWSTR(title.as_ptr()),
            options.chrome.window_style(),
            120,
            120,
            options.width.max(320),
            options.height.max(240),
            None,
            None,
            Some(instance),
            Some(state_pointer.cast::<c_void>()),
        )
    } {
        Ok(hwnd) => hwnd,
        Err(error) => {
            // SAFETY: window creation failed, so no HWND has taken ownership of this pointer.
            unsafe { drop(Box::from_raw(state_pointer)) };
            return Err(format!("create Win32 window: {error}"));
        }
    };
    // SAFETY: WM_NCCREATE installed the exact state pointer supplied to CreateWindowExW.
    let state =
        unsafe { state_from(hwnd) }.ok_or_else(|| "Win32 state was not installed".to_owned())?;
    state.hwnd = hwnd;

    // SAFETY: the HWND and state are initialized.
    let _ = unsafe { ShowWindow(hwnd, SW_SHOW) };
    // SAFETY: forces an initial WM_PAINT (loading page or first application frame).
    let _ = unsafe { UpdateWindow(hwnd) };

    if let Some(session) = immediate_session.take() {
        // Immediate 轴：会话已就绪（首帧已发布），直接完成启动收尾。
        state.install_immediate(session);
    } else if let Some(error) = immediate_error.take() {
        state.fail_startup(format!("application session failed to initialize: {error}"));
    } else if let Some((launch, sender)) = background.take() {
        // Loading 页已经可见；再启动可能耗时数秒的 bundle 下载与 Wasmtime 编译。
        if let Err(error) =
            startup::spawn_startup_worker(launch, _cache, sender, Arc::clone(&startup_cancel), hwnd)
        {
            state.fail_startup(error);
        }
    }

    // Host owns the wake-up cadence. Applications decide whether a tick changes state.
    // SAFETY: window-owned repeating timer on the UI thread.
    let _ = unsafe { SetTimer(Some(hwnd), TIMER_WAKE, 500, None) };

    let mut message = MSG::default();
    let message_result = 'running: loop {
        let quantum_started = Instant::now();
        let mut drained = 0u32;
        while drained < 64 && !input::quantum_expired(quantum_started) {
            // SAFETY: message is writable and belongs to this UI thread's queue.
            if !unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE) }.as_bool() {
                break;
            }
            if message.message == WM_QUIT {
                break 'running Ok(());
            }
            unsafe {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            if let Some(state) = unsafe { state_from(hwnd) } {
                state.execute_pending_window_command();
            }
            drained += 1;
        }

        // Give a coalesced invalid region one synchronous paint opportunity between input
        // quanta, even while WM_MOUSEMOVE continues to arrive.
        let _ = unsafe { UpdateWindow(hwnd) };
        if let Some(state) = unsafe { state_from(hwnd) } {
            state.execute_pending_window_command();
        }

        if drained < 64 {
            let _ = unsafe {
                MsgWaitForMultipleObjectsEx(None, u32::MAX, QS_ALLINPUT, MWMO_INPUTAVAILABLE)
            };
        }
    };

    // SAFETY: no later message dispatch occurs after the loop. WM_NCDESTROY normally cleared this
    // slot already; writing zero is also needed if an external WM_QUIT ended the loop first.
    unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) };
    // SAFETY: run_window is the unique Box owner for the allocation passed through WM_NCCREATE.
    let state = unsafe { Box::from_raw(state_pointer) };
    let terminal_error = state
        .terminal_error
        .clone()
        .or_else(|| state.startup_error.clone());
    drop(state);
    // SAFETY: revoke the shell timers after the loop.
    let _ = unsafe { KillTimer(Some(hwnd), TIMER_WAKE) };
    let _ = unsafe { KillTimer(Some(hwnd), TIMER_ANIMATION) };
    message_result.and_then(|()| terminal_error.map_or(Ok(()), Err))
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_NCCREATE {
        // SAFETY: WM_NCCREATE documents lparam as a CREATESTRUCTW pointer for this call.
        let create = unsafe { &*(lparam.0 as *const CREATESTRUCTW) };
        // SAFETY: run_window owns the Box pointer supplied through lpCreateParams until WM_QUIT.
        unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize) };
    }
    // SAFETY: the pointer was installed by WM_NCCREATE and remains live until run_window exits.
    let Some(state) = (unsafe { state_from(hwnd) }) else {
        // SAFETY: early messages before WM_NCCREATE setup use the default procedure.
        return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
    };

    match message {
        WM_NCHITTEST if state.chrome == WindowChrome::CustomTitleBar => {
            // 自绘 chrome：客户区顶部拖动带 → HTCAPTION；按钮区保持 HTCLIENT。
            // take/还回驱动器：hit_test 回调需要 &SessionDriver，与 wndproc 的 &mut 状态错开。
            let chrome = state.chrome;
            let dpi_scale = state.dpi_scale;
            let session = state.session.take();
            let result = hit_test(
                hwnd,
                message,
                wparam,
                lparam,
                chrome,
                |point| {
                    session
                        .as_ref()
                        .map(|driver| driver.hit_role_at(point))
                        .unwrap_or(tela_contract::HitRole::Client)
                },
                dpi_scale,
            );
            state.session = session;
            result
        }
        WM_TELA_STARTUP_READY => {
            state.receive_startup_result();
            state.sync_animation_timer();
            LRESULT(0)
        }
        WM_TELA_DEVICE_LOST => {
            state.receive_device_loss();
            LRESULT(0)
        }
        WM_PAINT => {
            let mut paint = PAINTSTRUCT::default();
            // SAFETY: WM_PAINT requires a balanced BeginPaint/EndPaint pair for this HWND.
            unsafe { BeginPaint(hwnd, &mut paint) };
            state.lifecycle.begin_paint();
            match state.lifecycle.phase() {
                ShellPhase::Loading | ShellPhase::Failed => state.paint_native_status(&paint),
                ShellPhase::Running => {
                    let result = state.paint_guest();
                    state.sync_animation_timer();
                    handle_window_error(state, result);
                }
                ShellPhase::Suspended | ShellPhase::Closing => {}
            }
            // SAFETY: paint is exactly the PAINTSTRUCT initialized by BeginPaint above.
            let _ = unsafe { EndPaint(hwnd, &paint) };
            LRESULT(0)
        }
        WM_SIZE => {
            let result = state.resize(wparam.0 as u32 == SIZE_MINIMIZED);
            handle_window_error(state, result);
            LRESULT(0)
        }
        WM_DPICHANGED => {
            // SAFETY: lparam is a RECT supplied by Windows for the duration of WM_DPICHANGED.
            let suggested = unsafe { *(lparam.0 as *const RECT) };
            // SAFETY: suggested coordinates originate from Windows and apply to the current HWND.
            let _ = unsafe {
                SetWindowPos(
                    hwnd,
                    None,
                    suggested.left,
                    suggested.top,
                    suggested.right - suggested.left,
                    suggested.bottom - suggested.top,
                    windows::Win32::UI::WindowsAndMessaging::SWP_NOACTIVATE
                        | windows::Win32::UI::WindowsAndMessaging::SWP_NOZORDER,
                )
            };
            let result = state.resize(false);
            handle_window_error(state, result);
            LRESULT(0)
        }
        WM_SETFOCUS => {
            let result = state.window_focus_changed(true);
            handle_window_error(state, result);
            LRESULT(0)
        }
        WM_KILLFOCUS => {
            if state.pointer_captured {
                state.end_pointer_capture(false);
                let event =
                    state.mouse_pointer_event(AppPointerPhase::Cancel, -1.0, -1.0, 0, 0.0, 0.0);
                let cancel_result = state.pointer(event);
                handle_window_error(state, cancel_result);
            }
            let pointer_result = state.pointer_left_client();
            handle_window_error(state, pointer_result);
            let focus_result = state.window_focus_changed(false);
            handle_window_error(state, focus_result);
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            state.begin_mouse_leave_tracking();
            let (x, y) = client_point(lparam);
            let dpi_scale = state.dpi_scale;
            let event = state.mouse_pointer_event(
                AppPointerPhase::Move,
                x as f32 / dpi_scale,
                y as f32 / dpi_scale,
                if state.pointer_captured { 1 } else { 0 },
                0.0,
                0.0,
            );
            let result = state.pointer(event);
            handle_window_error(state, result);
            // 实时刷新客户区光标：拖拽缩放（sizing loop）结束等场景系统不再发
            // WM_SETCURSOR，只有这里能覆盖残留的系统光标。
            state.apply_client_cursor();
            LRESULT(0)
        }
        WM_NCMOUSEMOVE if state.chrome == WindowChrome::CustomTitleBar => {
            // 非客户区（边缘/标题栏）移动期间清空 hover：此时没有客户区 WM_MOUSEMOVE，
            // hover_key 保持陈旧，回到客户区会显示错误的手型。
            let result = state.pointer_left_client();
            handle_window_error(state, result);
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            let result = state.pointer_left_client();
            handle_window_error(state, result);
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            // SAFETY: focusing the live HWND is a synchronous native input operation.
            let _ = unsafe { SetFocus(Some(hwnd)) };
            state.begin_pointer_capture();
            let (x, y) = client_point(lparam);
            let dpi_scale = state.dpi_scale;
            let event = state.mouse_pointer_event(
                AppPointerPhase::Down,
                x as f32 / dpi_scale,
                y as f32 / dpi_scale,
                1,
                0.0,
                0.0,
            );
            let result = state.pointer(event);
            handle_window_error(state, result);
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let (x, y) = client_point(lparam);
            let dpi_scale = state.dpi_scale;
            let event = state.mouse_pointer_event(
                AppPointerPhase::Up,
                x as f32 / dpi_scale,
                y as f32 / dpi_scale,
                0,
                0.0,
                0.0,
            );
            let result = state.pointer(event);
            handle_window_error(state, result);
            state.end_pointer_capture(true);
            LRESULT(0)
        }
        WM_CAPTURECHANGED | WM_CANCELMODE => {
            if state.pointer_captured {
                state.end_pointer_capture(false);
                let event =
                    state.mouse_pointer_event(AppPointerPhase::Cancel, -1.0, -1.0, 0, 0.0, 0.0);
                let result = state.pointer(event);
                handle_window_error(state, result);
            }
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            let mut point = POINT {
                x: (lparam.0 as u32 & 0xffff) as u16 as i16 as i32,
                y: ((lparam.0 as u32 >> 16) & 0xffff) as u16 as i16 as i32,
            };
            // SAFETY: WM_MOUSEWHEEL supplies screen coordinates; this only converts them for hwnd.
            let _ = unsafe { ScreenToClient(hwnd, &mut point) };
            let delta = ((wparam.0 as u32 >> 16) as u16 as i16) as f32;
            let dpi_scale = state.dpi_scale;
            let event = state.mouse_pointer_event(
                AppPointerPhase::Scroll,
                point.x as f32 / dpi_scale,
                point.y as f32 / dpi_scale,
                0,
                0.0,
                input::wheel_delta_y(delta),
            );
            let result = state.pointer(event);
            handle_window_error(state, result);
            LRESULT(0)
        }
        WM_KEYDOWN => {
            let repeat = ((lparam.0 as u32 >> 30) & 1) != 0;
            match state.key_down(wparam.0 as u16, repeat) {
                Ok(true) => LRESULT(0),
                Ok(false) => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
                Err(error) => {
                    state.terminate(error);
                    LRESULT(0)
                }
            }
        }
        WM_CHAR => match state.character(wparam.0 as u16) {
            Ok(true) => LRESULT(0),
            Ok(false) => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
            Err(error) => {
                state.terminate(error);
                LRESULT(0)
            }
        },
        WM_TIMER => match wparam.0 as usize {
            TIMER_WAKE => {
                state.wake();
                state.sync_animation_timer();
                LRESULT(0)
            }
            TIMER_ANIMATION => {
                state.on_animation_timer();
                state.sync_animation_timer();
                LRESULT(0)
            }
            TIMER_SURFACE_RETRY => {
                state.on_surface_retry_timer();
                LRESULT(0)
            }
            _ => LRESULT(0),
        },
        WM_SETCURSOR => {
            // MSDN: wParam = 光标所在窗口句柄；lParam 低位 = hit-test 码，高位 = 触发消息。
            // 只有客户区（HTCLIENT）由应用接管光标；边缘/标题栏/系统菜单必须交回
            // DefWindowProcW，由系统显示 resize/移动等标准光标并支持边缘拖拽缩放。
            let hit_test_code = (lparam.0 & 0xffff) as u16;
            if hit_test_code == HTCLIENT as u16 {
                state.apply_client_cursor();
                LRESULT(1)
            } else {
                unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
            }
        }
        WM_CLOSE => {
            state.begin_close();
            // SAFETY: destruction is requested from the thread that owns the HWND.
            if let Err(error) = unsafe { DestroyWindow(hwnd) } {
                state
                    .terminal_error
                    .get_or_insert_with(|| format!("destroy Win32 window: {error}"));
                // SAFETY: keeps the message loop from waiting if destruction failed unexpectedly.
                unsafe { PostQuitMessage(1) };
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            state.startup_cancel.store(true, Ordering::Release);
            state.cancel_surface_retry();
            // Drop the surface while the HWND remains valid. WGPU holds the raw native handle for
            // the surface lifetime, so waiting for WindowState's later Box drop would be invalid.
            state.gpu = None;
            // SAFETY: posts the terminal marker to this UI thread's message queue.
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        WM_NCDESTROY => {
            // Prevent an accidental late message from exposing a pointer after HWND teardown. The
            // Box remains owned by run_window and is dropped only after the loop observes WM_QUIT.
            unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) };
            // SAFETY: still delegate standard non-client cleanup to Windows.
            unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

unsafe fn state_from(hwnd: HWND) -> Option<&'static mut WindowState> {
    // SAFETY: the caller ensures hwnd belongs to this shell and run_window owns this allocation.
    let pointer = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut WindowState;
    // SAFETY: WM_NCCREATE installed exactly a Box<WindowState> pointer with this lifetime.
    unsafe { pointer.as_mut() }
}

fn handle_window_error(state: &mut WindowState, result: Result<(), String>) {
    if let Err(error) = result {
        state.terminate(error);
    }
}

fn utf16z(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn client_point(lparam: LPARAM) -> (i32, i32) {
    input::client_point(lparam.0 as u32)
}

fn modifier_bits() -> u8 {
    input::modifier_bits_from_key_state(
        key_is_down(VK_SHIFT.0),
        key_is_down(VK_CONTROL.0),
        key_is_down(VK_MENU.0),
        key_is_down(VK_LWIN.0) || key_is_down(VK_RWIN.0),
    )
}

fn has_command_modifier() -> bool {
    input::has_command_modifier(modifier_bits())
}

fn key_is_down(virtual_key: u16) -> bool {
    // SAFETY: GetAsyncKeyState reads global keyboard state without borrowing Rust memory.
    unsafe { GetAsyncKeyState(virtual_key as i32) < 0 }
}
