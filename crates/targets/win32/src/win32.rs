//! Thin Win32 shell for the portable tela application bundle.
//!
//! This is the SDK's only unsafe boundary. Win32 stores a raw `WindowState` pointer in
//! `GWLP_USERDATA`, and WGPU receives an HWND-derived raw window handle. The pointer remains
//! owned by `run_window`; GPU resources are dropped from `WM_DESTROY`, while the HWND is valid.

use std::{
    cell::RefCell,
    env,
    ffi::c_void,
    num::NonZeroIsize,
    path::PathBuf,
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, TryRecvError},
    },
    thread,
    time::Instant,
};

use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, RawDisplayHandle, RawWindowHandle,
    Win32WindowHandle, WindowsDisplayHandle,
};
use tela_app_abi::{
    AppEvent, AppFrameInput, AppFrameToken, AppPointerEvent, AppPointerKind, AppPointerPhase,
    CursorKind,
};
use tela_bridge::BridgeDispatcher;
use tela_contract::{Color, UiFrame};
use tela_desktop_runtime::bridge::{common::BuildConstants, process_bridge_requests};
use tela_render_wgpu::WgpuRenderer;

#[path = "providers.rs"]
mod providers;
use providers::{WindowMetrics, build_dispatcher};
use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
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
                TrackMouseEvent, VK_BACK, VK_CONTROL, VK_DELETE, VK_DOWN, VK_END, VK_ESCAPE,
                VK_HOME, VK_LEFT, VK_LWIN, VK_MENU, VK_NEXT, VK_PRIOR, VK_RETURN, VK_RIGHT,
                VK_RWIN, VK_SHIFT, VK_TAB, VK_UP,
            },
            WindowsAndMessaging::{
                CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW,
                DestroyWindow, DispatchMessageW, GWLP_USERDATA, GetClientRect, GetMessageW,
                GetWindowLongPtrW, IDC_ARROW, IDC_HAND, IDC_IBEAM, KillTimer, LoadCursorW, MSG,
                PostMessageW, PostQuitMessage, RegisterClassW, SIZE_MINIMIZED, SW_SHOW, SetCursor,
                SetTimer, SetWindowLongPtrW, SetWindowPos, ShowWindow, TranslateMessage,
                WINDOW_EX_STYLE, WM_APP, WM_CANCELMODE, WM_CAPTURECHANGED, WM_CHAR, WM_CLOSE,
                WM_DESTROY, WM_DPICHANGED, WM_KEYDOWN, WM_KILLFOCUS, WM_LBUTTONDOWN, WM_LBUTTONUP,
                WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCCREATE, WM_NCDESTROY, WM_PAINT, WM_SETCURSOR,
                WM_SETFOCUS, WM_SIZE, WM_TIMER, WNDCLASSW, WS_OVERLAPPEDWINDOW,
            },
        },
    },
    core::PCWSTR,
};

use tela_desktop_runtime::{
    BundleLoader, BundleSource, DeviceLossAction, GuestRuntime, PlatformLaunchOptions,
    ShellLifecycle, ShellPhase, TextChannelAction,
};

const WINDOW_CLASS: &str = "TelaWin32DevelopmentShell";
const WINDOW_TITLE: &str = "TELA Files";
const WM_TELA_STARTUP_READY: u32 = WM_APP + 1;
const WM_TELA_DEVICE_LOST: u32 = WM_APP + 2;
const SURFACE_RETRY_TIMER: usize = 1;
// `windows` exposes this ordinary client-area message through its Controls namespace. The shell
// does not otherwise need common-control APIs, so retain the SDK-defined message value locally.
const WM_MOUSELEAVE: u32 = 0x02a3;

/// Owns the Windows marker display handle in the Send + Sync form required by wgpu 30.
#[derive(Debug)]
struct Win32Display;

impl HasDisplayHandle for Win32Display {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        Ok(DisplayHandle::windows())
    }
}

/// Starts one native shell instance and performs exactly one background startup bundle request.
pub fn run(options: PlatformLaunchOptions) -> Result<(), String> {
    let cache_path = cache_path()?;
    if options.verbose {
        eprintln!(
            "tela-win32-host: startup index={} cache={}",
            options.bundle_index_url,
            cache_path.display()
        );
    }
    run_window(options, cache_path)
}

fn cache_path() -> Result<PathBuf, String> {
    let root = env::var_os("LOCALAPPDATA")
        .or_else(|| env::var_os("TEMP"))
        .ok_or_else(|| {
            "LOCALAPPDATA or TEMP is required for development bundle cache".to_owned()
        })?;
    Ok(PathBuf::from(root)
        .join("tela")
        .join("development")
        .join("last-valid.tela"))
}

fn fetch_http(url: &str) -> Result<Vec<u8>, String> {
    let response = ureq::get(url)
        .call()
        .map_err(|error| format!("GET {url}: {error}"))?;
    response
        .into_body()
        .into_with_config()
        .limit(64 * 1024 * 1024)
        .read_to_vec()
        .map_err(|error| format!("read {url}: {error}"))
}

fn load_guest(options: PlatformLaunchOptions, cache_path: PathBuf) -> Result<GuestRuntime, String> {
    let loader = BundleLoader::new(cache_path);
    let bundle = loader
        .load_with(&options.bundle_index_url, fetch_http)
        .map_err(|error| error.to_string())?;
    let source = match bundle.source {
        BundleSource::Network => "network",
        BundleSource::Cache => "cache fallback",
    };
    if options.verbose {
        eprintln!(
            "tela-win32-host: bundle={source} archive={}KB download={}ms; initializing guest",
            bundle.metrics.archive_bytes / 1024,
            bundle.metrics.download.as_millis(),
        );
        if let Some(warning) = bundle.cache_warning.as_deref() {
            eprintln!("tela-win32-host: bundle cache warning: {warning}");
        }
    }
    let runtime = GuestRuntime::new(&bundle.archive.app_wasm).map_err(|error| error.to_string())?;
    if options.verbose {
        eprintln!(
            "tela-win32-host: guest initialized compile={}ms init={}ms init_fuel={}",
            runtime.metrics().module_compile.as_millis(),
            runtime.metrics().initialize.as_millis(),
            runtime.metrics().initialize_fuel_consumed,
        );
    }
    Ok(runtime)
}

#[derive(Clone, Copy)]
struct ClientMetrics {
    width: u32,
    height: u32,
    dpi_scale: f32,
}

struct GpuSession {
    // Field order guarantees renderer and surface release before the instance-owned display handle.
    renderer: WgpuRenderer,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    instance: wgpu::Instance,
}

enum RenderOutcome {
    Presented { suboptimal: bool },
    Outdated,
    Lost,
    Timeout,
    Occluded,
    Validation,
}

#[derive(Debug)]
struct DeviceLossReport {
    generation: u64,
    detail: String,
}

struct WindowState {
    hwnd: HWND,
    lifecycle: ShellLifecycle,
    runtime: Option<GuestRuntime>,
    frame: Option<UiFrame>,
    frame_token: Option<AppFrameToken>,
    presented_frame_token: Option<AppFrameToken>,
    gpu: Option<GpuSession>,
    dpi_scale: f32,
    startup_rx: Option<Receiver<Result<GuestRuntime, String>>>,
    startup_cancel: Arc<AtomicBool>,
    device_loss: Arc<Mutex<Option<DeviceLossReport>>>,
    gpu_generation: u64,
    input_epoch: Instant,
    mouse_leave_tracking: bool,
    pointer_captured: bool,
    startup_error: Option<String>,
    terminal_error: Option<String>,
    bridge: Option<BridgeDispatcher>,
    bridge_metrics: Rc<RefCell<WindowMetrics>>,
}

impl WindowState {
    fn new(
        startup_rx: Receiver<Result<GuestRuntime, String>>,
        startup_cancel: Arc<AtomicBool>,
    ) -> Self {
        Self {
            hwnd: HWND::default(),
            lifecycle: ShellLifecycle::new(),
            runtime: None,
            frame: None,
            frame_token: None,
            presented_frame_token: None,
            gpu: None,
            dpi_scale: 1.0,
            startup_rx: Some(startup_rx),
            startup_cancel,
            device_loss: Arc::new(Mutex::new(None)),
            gpu_generation: 0,
            input_epoch: Instant::now(),
            mouse_leave_tracking: false,
            pointer_captured: false,
            startup_error: None,
            terminal_error: None,
            bridge_metrics: Rc::new(RefCell::new(WindowMetrics::default())),
            bridge: None,
        }
    }

    fn receive_startup_result(&mut self) {
        let result = match self.startup_rx.as_ref() {
            None => return,
            Some(receiver) => match receiver.try_recv() {
                Ok(result) => result,
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => {
                    self.startup_rx = None;
                    self.fail_startup(
                        "startup worker disconnected before returning a result".to_owned(),
                    );
                    return;
                }
            },
        };
        self.startup_rx = None;
        match result {
            Ok(runtime) => self.install_runtime(runtime),
            Err(error) => self.fail_startup(error),
        }
    }

    fn install_runtime(&mut self, runtime: GuestRuntime) {
        if self.lifecycle.phase() != ShellPhase::Loading {
            return;
        }
        if self.bridge.is_none() {
            self.bridge = Some(build_dispatcher(
                Rc::clone(&self.bridge_metrics),
                &BuildConstants::default(),
                vec![],
            ));
        }
        let activation: Result<(), String> = (|| {
            let frame = runtime.frame().map_err(|error| error.to_string())?;
            let frame_token = runtime.status().frame_token;
            self.frame = Some(frame);
            self.frame_token = frame_token;
            self.presented_frame_token = None;
            self.runtime = Some(runtime);
            let Some(metrics) = self.client_metrics()? else {
                self.lifecycle.startup_succeeded(false);
                return Ok(());
            };
            self.initialize_gpu(metrics)?;
            self.lifecycle.startup_succeeded(true);
            self.dispatch_viewport(metrics)?;
            self.request_redraw();
            Ok(())
        })();
        if let Err(error) = activation {
            self.fail_startup(format!("initialize native renderer: {error}"));
        }
    }

    fn fail_startup(&mut self, error: String) {
        if self.lifecycle.phase() != ShellPhase::Loading {
            return;
        }
        eprintln!("tela-win32-host: startup failed: {error}");
        self.runtime = None;
        self.frame = None;
        self.frame_token = None;
        self.presented_frame_token = None;
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
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            flags: wgpu::InstanceFlags::default(),
            memory_budget_thresholds: Default::default(),
            backend_options: Default::default(),
            // wgpu 30 requires an explicit display owner for native presentation. Windows has an
            // empty marker handle, but it must agree with the surface target below.
            display: Some(Box::new(Win32Display)),
        });
        let surface = create_surface(&instance, self.hwnd)?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        }))
        .map_err(|error| format!("request WGPU adapter: {error}"))?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("tela Win32 device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            experimental_features: Default::default(),
            memory_hints: wgpu::MemoryHints::default(),
            trace: wgpu::Trace::Off,
        }))
        .map_err(|error| format!("create WGPU device: {error}"))?;
        let config = surface
            .get_default_config(&adapter, metrics.width, metrics.height)
            .ok_or_else(|| "WGPU surface has no default configuration".to_owned())?;
        let format = config.format;
        surface.configure(&device, &config);

        let device_loss = Arc::clone(&self.device_loss);
        let generation = self.gpu_generation.wrapping_add(1);
        let hwnd_bits = self.hwnd.0 as isize;
        device.set_device_lost_callback(move |reason, message| {
            if let Ok(mut report) = device_loss.lock() {
                let replace = report
                    .as_ref()
                    .is_none_or(|current| current.generation <= generation);
                if replace {
                    *report = Some(DeviceLossReport {
                        generation,
                        detail: format!("{reason:?}: {message}"),
                    });
                }
            }
            // SAFETY: PostMessageW copies only scalar values and is safe across threads. No raw
            // WindowState pointer is captured; the UI thread owns both resource recovery and state.
            let hwnd = HWND(hwnd_bits as *mut c_void);
            let _ = unsafe {
                PostMessageW(
                    Some(hwnd),
                    WM_TELA_DEVICE_LOST,
                    WPARAM::default(),
                    LPARAM::default(),
                )
            };
        });

        self.dpi_scale = metrics.dpi_scale;
        self.gpu = Some(GpuSession {
            renderer: WgpuRenderer::new(device, queue, format, Color::rgba(1.0, 1.0, 1.0, 1.0)),
            surface,
            config,
            instance,
        });
        self.gpu_generation = generation;
        Ok(())
    }

    fn dispatch_viewport(&mut self, metrics: ClientMetrics) -> Result<(), String> {
        // A frame from the previous client geometry must not route input after this point.
        self.presented_frame_token = None;
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
        if self.runtime.is_none() {
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
            gpu.config.width = metrics.width;
            gpu.config.height = metrics.height;
            gpu.surface.configure(gpu.renderer.device(), &gpu.config);
        }
        self.lifecycle.client_area_changed(true);
        self.dispatch_viewport(metrics)?;
        self.request_redraw();
        Ok(())
    }

    fn dispatch_guest(&mut self, event: AppEvent) -> Result<bool, String> {
        let changed = self.dispatch_guest_without_text_reconcile(event)?;
        self.synchronize_text_channel(None)?;
        Ok(changed)
    }

    fn dispatch_guest_without_text_reconcile(&mut self, event: AppEvent) -> Result<bool, String> {
        let (changed, frame, frame_token) = {
            let runtime = self
                .runtime
                .as_mut()
                .ok_or_else(|| "dispatch without a live guest runtime".to_owned())?;
            let changed = runtime
                .dispatch(&event)
                .map_err(|error| error.to_string())?;
            let frame = runtime.frame().map_err(|error| error.to_string())?;
            if let Some(dispatcher) = self.bridge.as_mut() {
                process_bridge_requests(runtime, dispatcher)?;
            }
            (changed, frame, runtime.status().frame_token)
        };
        self.frame = Some(frame);
        self.frame_token = frame_token;
        Ok(changed)
    }

    fn dispatch_presented_input(&mut self, input: AppFrameInput) -> Result<bool, String> {
        let Some(source_frame_token) = self.presented_frame_token else {
            return Ok(false);
        };
        self.dispatch_guest(AppEvent::FrameInput {
            source_frame_token,
            input,
        })
    }

    fn dispatch_presented_input_without_text_reconcile(
        &mut self,
        input: AppFrameInput,
    ) -> Result<bool, String> {
        let Some(source_frame_token) = self.presented_frame_token else {
            return Ok(false);
        };
        self.dispatch_guest_without_text_reconcile(AppEvent::FrameInput {
            source_frame_token,
            input,
        })
    }

    fn synchronize_text_channel(&mut self, window_focus: Option<bool>) -> Result<(), String> {
        let guest_wants_text = self
            .runtime
            .as_ref()
            .is_some_and(|runtime| runtime.status().input_focused);
        let first = match window_focus {
            Some(focused) => self.lifecycle.set_window_focus(focused, guest_wants_text),
            None => self.lifecycle.reconcile_text_channel(guest_wants_text),
        };
        self.dispatch_text_channel_action(first)?;

        // A Blur can cause a guest to advance between two text fields. Reconcile that one semantic
        // edge immediately, but never loop indefinitely around guest code from a native callback.
        let guest_wants_text = self
            .runtime
            .as_ref()
            .is_some_and(|runtime| runtime.status().input_focused);
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
        self.dispatch_presented_input(input)?;
        self.request_redraw();
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
            .runtime
            .as_ref()
            .is_some_and(|runtime| runtime.status().input_focused);
        if input_focused {
            match virtual_key {
                key if key == VK_RETURN.0 => {
                    self.dispatch_presented_input(AppFrameInput::InputEnter)?;
                    self.request_redraw();
                    return Ok(true);
                }
                key if key == VK_ESCAPE.0 => {
                    self.dispatch_presented_input(AppFrameInput::InputCancel)?;
                    self.request_redraw();
                    return Ok(true);
                }
                key if key != VK_TAB.0 && !has_command_modifier() => return Ok(false),
                _ => {}
            }
        }
        let Some(physical_key) = physical_key(virtual_key) else {
            return Ok(false);
        };
        let consumed = self.dispatch_presented_input(AppFrameInput::KeyDown {
            physical_key,
            modifier_bits: modifier_bits(),
            repeat,
        })?;
        self.request_redraw();
        Ok(consumed)
    }

    fn character(&mut self, code_unit: u16) -> Result<bool, String> {
        if !self.lifecycle.can_render()
            || !self
                .runtime
                .as_ref()
                .is_some_and(|runtime| runtime.status().input_focused)
        {
            return Ok(false);
        }
        let mut value = self
            .runtime
            .as_ref()
            .expect("runtime checked above")
            .status()
            .input_value
            .clone();
        match code_unit {
            8 => {
                value.pop();
            }
            0x20..=0x7e => {
                value.push(char::from_u32(code_unit as u32).expect("ASCII character"));
            }
            _ => return Ok(false),
        }
        self.dispatch_presented_input(AppFrameInput::SetInputValue(value))?;
        self.request_redraw();
        Ok(true)
    }

    fn render(&mut self) -> Result<RenderOutcome, String> {
        if !self.lifecycle.can_render() {
            return Ok(RenderOutcome::Occluded);
        }
        let frame = self
            .frame
            .as_ref()
            .ok_or_else(|| "render without a resolved UI frame".to_owned())?;
        let gpu = self
            .gpu
            .as_mut()
            .ok_or_else(|| "render without a live GPU session".to_owned())?;
        let (texture, suboptimal) = match gpu.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture) => (texture, false),
            wgpu::CurrentSurfaceTexture::Suboptimal(texture) => (texture, true),
            wgpu::CurrentSurfaceTexture::Outdated => return Ok(RenderOutcome::Outdated),
            wgpu::CurrentSurfaceTexture::Lost => return Ok(RenderOutcome::Lost),
            wgpu::CurrentSurfaceTexture::Timeout => return Ok(RenderOutcome::Timeout),
            wgpu::CurrentSurfaceTexture::Occluded => return Ok(RenderOutcome::Occluded),
            wgpu::CurrentSurfaceTexture::Validation => return Ok(RenderOutcome::Validation),
        };
        let view = texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        gpu.renderer
            .render_frame(frame, &view, gpu.config.width, gpu.config.height);
        gpu.renderer.present(texture);
        Ok(RenderOutcome::Presented { suboptimal })
    }

    fn paint_guest(&mut self) -> Result<(), String> {
        match self.render()? {
            RenderOutcome::Presented { suboptimal } => {
                self.presented_frame_token = self.frame_token;
                self.lifecycle.surface_presented();
                self.cancel_surface_retry();
                if suboptimal {
                    self.reconfigure_surface()?;
                    self.request_redraw();
                }
            }
            RenderOutcome::Outdated => {
                self.presented_frame_token = None;
                self.reconfigure_surface()?;
                self.request_redraw();
            }
            RenderOutcome::Lost => {
                self.presented_frame_token = None;
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
        self.presented_frame_token = None;
        let Some(metrics) = self.client_metrics()? else {
            self.lifecycle.client_area_changed(false);
            return Ok(());
        };
        let gpu = self
            .gpu
            .as_mut()
            .ok_or_else(|| "reconfigure without a live GPU session".to_owned())?;
        gpu.config.width = metrics.width;
        gpu.config.height = metrics.height;
        gpu.surface.configure(gpu.renderer.device(), &gpu.config);
        self.dpi_scale = metrics.dpi_scale;
        Ok(())
    }

    fn recreate_surface(&mut self) -> Result<(), String> {
        self.presented_frame_token = None;
        let Some(metrics) = self.client_metrics()? else {
            self.lifecycle.client_area_changed(false);
            return Ok(());
        };
        let gpu = self
            .gpu
            .as_mut()
            .ok_or_else(|| "recreate surface without a live GPU session".to_owned())?;
        let surface = create_surface(&gpu.instance, self.hwnd)?;
        gpu.config.width = metrics.width;
        gpu.config.height = metrics.height;
        surface.configure(gpu.renderer.device(), &gpu.config);
        let previous = std::mem::replace(&mut gpu.surface, surface);
        drop(previous);
        self.dpi_scale = metrics.dpi_scale;
        self.dispatch_viewport(metrics)?;
        Ok(())
    }

    fn schedule_surface_retry(&mut self) -> Result<(), String> {
        let Some(delay_ms) = self.lifecycle.surface_timeout() else {
            return Ok(());
        };
        // SAFETY: this sets a window-owned one-shot timer; WM_TIMER kills it before repainting.
        let timer = unsafe { SetTimer(Some(self.hwnd), SURFACE_RETRY_TIMER, delay_ms, None) };
        if timer == 0 {
            return Err("schedule WGPU surface retry timer".to_owned());
        }
        Ok(())
    }

    fn on_surface_retry_timer(&mut self) {
        // SAFETY: it is harmless to cancel an already-fired one-shot window timer.
        let _ = unsafe { KillTimer(Some(self.hwnd), SURFACE_RETRY_TIMER) };
        if self.lifecycle.take_surface_retry() {
            self.request_redraw();
        }
    }

    fn cancel_surface_retry(&mut self) {
        // The lifecycle will ignore a stale WM_TIMER after this. Kill it too, so recovery does not
        // cause an unnecessary later paint.
        let _ = unsafe { KillTimer(Some(self.hwnd), SURFACE_RETRY_TIMER) };
        self.lifecycle.cancel_surface_retry();
    }

    fn receive_device_loss(&mut self) {
        let Some(report) = self
            .device_loss
            .lock()
            .ok()
            .and_then(|mut slot| slot.take())
        else {
            return;
        };
        if report.generation != self.gpu_generation {
            return;
        }
        if self.gpu.is_none() {
            return;
        }
        let detail = report.detail;
        match self.lifecycle.device_lost() {
            None => {}
            Some(DeviceLossAction::RecreateGpu) => {
                eprintln!("tela-win32-host: WGPU device lost, rebuilding once: {detail}");
                self.gpu = None;
                self.presented_frame_token = None;
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
        self.presented_frame_token = None;
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

    fn update_cursor(&self) {
        let cursor = self
            .runtime
            .as_ref()
            .map(|runtime| runtime.status().cursor)
            .unwrap_or(CursorKind::Default);
        let cursor = match cursor {
            CursorKind::Default => IDC_ARROW,
            CursorKind::Text => IDC_IBEAM,
            CursorKind::Pointer => IDC_HAND,
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

fn create_surface(instance: &wgpu::Instance, hwnd: HWND) -> Result<wgpu::Surface<'static>, String> {
    let hwnd = NonZeroIsize::new(hwnd.0 as isize)
        .ok_or_else(|| "Win32 window handle is null".to_owned())?;
    let raw_window_handle = RawWindowHandle::Win32(Win32WindowHandle::new(hwnd));
    let raw_display_handle = RawDisplayHandle::Windows(WindowsDisplayHandle::new());
    // SAFETY: the HWND belongs to the UI thread and outlives the returned surface. The Windows
    // display marker has no borrowed data. It matches the `DisplayHandle::windows()` given to the
    // instance, which is required when both instance and target carry display information.
    unsafe {
        instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
            raw_display_handle: Some(raw_display_handle),
            raw_window_handle,
        })
    }
    .map_err(|error| format!("create WGPU surface: {error}"))
}

fn spawn_startup_worker(
    options: PlatformLaunchOptions,
    cache_path: PathBuf,
    sender: mpsc::Sender<Result<GuestRuntime, String>>,
    cancel: Arc<AtomicBool>,
    hwnd: HWND,
) -> Result<(), String> {
    let hwnd_bits = hwnd.0 as isize;
    thread::Builder::new()
        .name("tela-win32-startup".to_owned())
        .spawn(move || {
            let result = load_guest(options, cache_path);
            if cancel.load(Ordering::Acquire) {
                return;
            }
            if sender.send(result).is_ok() && !cancel.load(Ordering::Acquire) {
                let hwnd = HWND(hwnd_bits as *mut c_void);
                // SAFETY: the notification carries no pointer. If the HWND was destroyed first,
                // PostMessageW fails and the receiver/result are simply dropped by the worker.
                let _ = unsafe {
                    PostMessageW(
                        Some(hwnd),
                        WM_TELA_STARTUP_READY,
                        WPARAM::default(),
                        LPARAM::default(),
                    )
                };
            }
        })
        .map(|_| ())
        .map_err(|error| format!("spawn startup worker: {error}"))
}

fn run_window(options: PlatformLaunchOptions, cache_path: PathBuf) -> Result<(), String> {
    // SAFETY: Per-Monitor V2 is selected before this process creates an HWND. Unsupported older
    // Windows versions keep their default DPI mode and are still usable.
    let _ = unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    let module =
        unsafe { GetModuleHandleW(None) }.map_err(|error| format!("get module handle: {error}"))?;
    let instance = HINSTANCE(module.0);
    let class_name = utf16z(WINDOW_CLASS);
    let title = utf16z(WINDOW_TITLE);
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

    let (startup_sender, startup_rx) = mpsc::channel();
    let startup_cancel = Arc::new(AtomicBool::new(false));
    let state = Box::new(WindowState::new(startup_rx, Arc::clone(&startup_cancel)));
    let state_pointer = Box::into_raw(state);
    // SAFETY: WM_NCCREATE stores this allocation in GWLP_USERDATA; run_window recovers it exactly
    // once after the message loop. CreateWindowExW does not retain the UTF-16 vectors after return.
    let hwnd = match unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            PCWSTR(class_name.as_ptr()),
            PCWSTR(title.as_ptr()),
            WS_OVERLAPPEDWINDOW,
            120,
            120,
            1280,
            840,
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

    // Show and paint the loading page before even spawning the potentially multi-second Wasmtime
    // compilation. The UI thread remains available for move/close/focus messages throughout startup.
    // SAFETY: the HWND and state are initialized.
    let _ = unsafe { ShowWindow(hwnd, SW_SHOW) };
    // SAFETY: forces an initial WM_PAINT for the native loading page.
    let _ = unsafe { UpdateWindow(hwnd) };
    if let Err(error) =
        spawn_startup_worker(options, cache_path, startup_sender, startup_cancel, hwnd)
    {
        state.fail_startup(error);
    }

    let mut message = MSG::default();
    let message_result = loop {
        // SAFETY: message is writable and belongs to this thread's queue.
        let result = unsafe { GetMessageW(&mut message, None, 0, 0) };
        if result.0 == -1 {
            break Err(format!(
                "read Win32 message: {}",
                windows::core::Error::from_thread()
            ));
        }
        if result.0 == 0 {
            break Ok(());
        }
        // SAFETY: message was produced by GetMessageW and remains valid for dispatch.
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
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
        WM_TELA_STARTUP_READY => {
            state.receive_startup_result();
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
                -(delta / 120.0) * 48.0,
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
        WM_TIMER if wparam.0 == SURFACE_RETRY_TIMER => {
            state.on_surface_retry_timer();
            LRESULT(0)
        }
        WM_SETCURSOR => {
            state.update_cursor();
            LRESULT(1)
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
    let packed = lparam.0 as u32;
    (
        (packed & 0xffff) as u16 as i16 as i32,
        (packed >> 16) as u16 as i16 as i32,
    )
}

fn modifier_bits() -> u8 {
    const SHIFT: u8 = 1 << 0;
    const CTRL: u8 = 1 << 1;
    const ALT: u8 = 1 << 2;
    const META: u8 = 1 << 3;
    let mut result = 0;
    if key_is_down(VK_SHIFT.0) {
        result |= SHIFT;
    }
    if key_is_down(VK_CONTROL.0) {
        result |= CTRL;
    }
    if key_is_down(VK_MENU.0) {
        result |= ALT;
    }
    if key_is_down(VK_LWIN.0) || key_is_down(VK_RWIN.0) {
        result |= META;
    }
    result
}

fn has_command_modifier() -> bool {
    let modifiers = modifier_bits();
    modifiers & ((1 << 1) | (1 << 3)) != 0
}

fn key_is_down(virtual_key: u16) -> bool {
    // SAFETY: GetAsyncKeyState reads global keyboard state without borrowing Rust memory.
    unsafe { GetAsyncKeyState(virtual_key as i32) < 0 }
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
