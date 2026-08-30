//! WGPU 呈现会话：静态壳与 bundle 壳共用的 surface/renderer/设备丢失守卫。

#![allow(unsafe_code)]

use std::sync::{Arc, Mutex};
use std::time::Instant;

use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, RawDisplayHandle, RawWindowHandle,
    Win32WindowHandle,
};
use tela_contract::{FrameDamage, UiFrame};
use tela_render_wgpu::{WgpuRenderer, renderer::RetainedFrameTarget};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

/// 全局单调代际：同一窗口多次重建 GPU 会话时区分迟到的旧回调。
static DEVICE_LOSS_GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

macro_rules! gpu_trace {
    ($($arg:tt)*) => {
        if crate::trace_enabled() {
            eprintln!($($arg)*);
        }
    };
}

/// Outcome of one surface presentation attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderOutcome {
    /// A frame was presented; `suboptimal` requests an immediate surface reconfiguration.
    Presented {
        /// Whether the presentation happened on a suboptimal configuration.
        suboptimal: bool,
    },
    /// The surface is outdated and must be reconfigured before the next present.
    Outdated,
    /// The surface was lost and must be recreated.
    Lost,
    /// The acquire timed out; the shell should retry shortly.
    Timeout,
    /// The surface was occluded; skip the present.
    Occluded,
    /// The surface configuration failed validation.
    Validation,
}

/// 一次设备丢失报告：`generation` 用于丢弃旧 GPU 代际的迟到回调。
#[derive(Clone, Debug)]
pub struct DeviceLossReport {
    /// 丢失设备的 GPU 会话代际。
    pub generation: u64,
    /// 驱动给出的丢失原因。
    pub detail: String,
}

/// Surface, renderer, and presentation configuration.
pub struct GpuSession {
    instance: wgpu::Instance,
    surface: wgpu::Surface<'static>,
    /// Presentation configuration.
    pub config: wgpu::SurfaceConfiguration,
    renderer: WgpuRenderer,
    backing: RetainedFrameTarget,
    /// 设备丢失回调的跨线程报告槽与代际号。
    device_loss: Arc<Mutex<Option<DeviceLossReport>>>,
    generation: u64,
}

impl GpuSession {
    /// Creates the renderer and configures the surface for the given client size.
    ///
    /// `device_lost_message` 是设备丢失时向 `hwnd` 投递的私有消息 ID（`WM_APP` 起算）；
    /// UI 线程收到后调用 [`GpuSession::take_device_loss_report`] 并按代际判定是否恢复。
    pub fn new(
        hwnd: HWND,
        width: u32,
        height: u32,
        dpr: f32,
        device_lost_message: u32,
    ) -> Result<Self, String> {
        let init_started = Instant::now();
        gpu_trace!(
            "tela-win32-trace: event=gpu_init stage=begin width={} height={} dpr={:.2}",
            width,
            height,
            dpr
        );
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            flags: wgpu::InstanceFlags::default(),
            memory_budget_thresholds: Default::default(),
            backend_options: Default::default(),
            // wgpu 30 requires an explicit display owner for native presentation.
            display: Some(Box::new(Win32Display)),
        });
        gpu_trace!(
            "tela-win32-trace: event=gpu_init stage=instance_ready elapsed_us={}",
            init_started.elapsed().as_micros()
        );
        let surface = create_surface(&instance, hwnd).map_err(|error| {
            gpu_trace!(
                "tela-win32-trace: event=gpu_init stage=surface_failed elapsed_us={} error={}",
                init_started.elapsed().as_micros(),
                error
            );
            error
        })?;
        gpu_trace!(
            "tela-win32-trace: event=gpu_init stage=surface_ready elapsed_us={}",
            init_started.elapsed().as_micros()
        );
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        }))
        .map_err(|error| {
            gpu_trace!(
                "tela-win32-trace: event=gpu_init stage=adapter_failed elapsed_us={} error={}",
                init_started.elapsed().as_micros(),
                error
            );
            format!("request WGPU adapter: {error}")
        })?;
        gpu_trace!(
            "tela-win32-trace: event=gpu_init stage=adapter_ready elapsed_us={} info={:?}",
            init_started.elapsed().as_micros(),
            adapter.get_info()
        );
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("tela static win32"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            experimental_features: Default::default(),
            memory_hints: wgpu::MemoryHints::default(),
            trace: wgpu::Trace::Off,
        }))
        .map_err(|error| {
            gpu_trace!(
                "tela-win32-trace: event=gpu_init stage=device_failed elapsed_us={} error={}",
                init_started.elapsed().as_micros(),
                error
            );
            format!("create WGPU device: {error}")
        })?;
        gpu_trace!(
            "tela-win32-trace: event=gpu_init stage=device_ready elapsed_us={}",
            init_started.elapsed().as_micros()
        );
        let capabilities = surface.get_capabilities(&adapter);
        gpu_trace!(
            "tela-win32-trace: event=gpu_init stage=capabilities_ready elapsed_us={} formats={} alpha_modes={}",
            init_started.elapsed().as_micros(),
            capabilities.formats.len(),
            capabilities.alpha_modes.len()
        );
        let renderer = WgpuRenderer::new(
            device,
            queue,
            capabilities.formats[0],
            tela_contract::Color::rgba(1.0, 1.0, 1.0, 1.0),
        );
        gpu_trace!(
            "tela-win32-trace: event=gpu_init stage=renderer_ready elapsed_us={}",
            init_started.elapsed().as_micros()
        );
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_DST,
            format: capabilities.formats[0],
            width: width.max(1),
            height: height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: capabilities.alpha_modes[0],
            view_formats: Vec::new(),
            desired_maximum_frame_latency: 2,
            color_space: wgpu::SurfaceColorSpace::Srgb,
        };
        surface.configure(renderer.device(), &config);
        let backing = renderer.retained_target(config.width, config.height);
        gpu_trace!(
            "tela-win32-trace: event=gpu_init stage=surface_configured elapsed_us={} physical={}x{}",
            init_started.elapsed().as_micros(),
            config.width,
            config.height
        );
        let _ = dpr;
        // 设备丢失回调在驱动线程触发：只写入标量报告并向 UI 线程投递消息，
        // 不触碰任何 Rust 窗口状态。
        let device_loss = Arc::new(Mutex::new(None));
        let generation =
            DEVICE_LOSS_GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        let report_slot = Arc::clone(&device_loss);
        let hwnd_bits = hwnd.0 as isize;
        let device = renderer.device().clone();
        if device_lost_message == 0 {
            // 调用方（尚未接入统一壳的旧路径）不处理设备丢失消息：不武装回调。
            return Ok(Self {
                instance,
                surface,
                config,
                renderer,
                backing,
                device_loss,
                generation,
            });
        }
        device.set_device_lost_callback(move |reason, message| {
            if let Ok(mut report) = report_slot.lock() {
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
            // window-state pointer is captured; the UI thread owns both recovery and state.
            let hwnd = HWND(hwnd_bits as *mut core::ffi::c_void);
            let _ = unsafe {
                PostMessageW(
                    Some(hwnd),
                    device_lost_message,
                    WPARAM::default(),
                    LPARAM::default(),
                )
            };
        });
        gpu_trace!(
            "tela-win32-trace: event=gpu_init stage=complete elapsed_us={}",
            init_started.elapsed().as_micros()
        );
        Ok(Self {
            instance,
            surface,
            config,
            renderer,
            backing,
            device_loss,
            generation,
        })
    }

    /// 本会话的 GPU 代际号（设备丢失报告按它过滤）。
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// 取走一次设备丢失报告；旧代际报告由调用侧用 [`GpuSession::generation`] 丢弃。
    pub fn take_device_loss_report(&self) -> Option<DeviceLossReport> {
        self.device_loss
            .lock()
            .ok()
            .and_then(|mut slot| slot.take())
    }

    /// Reconfigures the surface for a new client size.
    pub fn reconfigure(&mut self, width: u32, height: u32) {
        self.config.width = width.max(1);
        self.config.height = height.max(1);
        self.surface.configure(self.renderer.device(), &self.config);
    }

    /// Recreates the surface after a loss.
    pub fn recreate(&mut self, hwnd: HWND) -> Result<(), String> {
        let surface = create_surface(&self.instance, hwnd)?;
        surface.configure(self.renderer.device(), &self.config);
        self.surface = surface;
        Ok(())
    }

    /// Acquires the next texture, renders the frame, and presents.
    pub fn render(
        &mut self,
        frame: &UiFrame,
        damage: &FrameDamage,
    ) -> Result<RenderOutcome, String> {
        let (texture, suboptimal) = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture) => (texture, false),
            wgpu::CurrentSurfaceTexture::Suboptimal(texture) => (texture, true),
            wgpu::CurrentSurfaceTexture::Outdated => return Ok(RenderOutcome::Outdated),
            wgpu::CurrentSurfaceTexture::Lost => return Ok(RenderOutcome::Lost),
            wgpu::CurrentSurfaceTexture::Timeout => return Ok(RenderOutcome::Timeout),
            wgpu::CurrentSurfaceTexture::Occluded => return Ok(RenderOutcome::Occluded),
            wgpu::CurrentSurfaceTexture::Validation => return Ok(RenderOutcome::Validation),
        };
        self.renderer.render_retained_frame(
            frame,
            damage,
            &mut self.backing,
            self.config.width,
            self.config.height,
        );
        self.renderer
            .copy_retained_to_texture(&self.backing, &texture.texture);
        self.renderer.present(texture);
        Ok(RenderOutcome::Presented { suboptimal })
    }
}

/// Owns the Windows marker display handle in the Send + Sync form required by wgpu 30.
#[derive(Debug)]
struct Win32Display;

impl HasDisplayHandle for Win32Display {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        Ok(DisplayHandle::windows())
    }
}

/// Creates a WGPU surface for the given window handle.
pub fn create_surface(
    instance: &wgpu::Instance,
    hwnd: HWND,
) -> Result<wgpu::Surface<'static>, String> {
    let hwnd_bits = std::num::NonZeroIsize::new(hwnd.0 as isize)
        .ok_or_else(|| "Win32 window handle is null".to_owned())?;
    let raw_window_handle = RawWindowHandle::Win32(Win32WindowHandle::new(hwnd_bits));
    let raw_display_handle =
        RawDisplayHandle::Windows(raw_window_handle::WindowsDisplayHandle::new());
    // SAFETY: the HWND belongs to the UI thread and outlives the returned surface.
    unsafe {
        instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
            raw_display_handle: Some(raw_display_handle),
            raw_window_handle,
        })
    }
    .map_err(|error| format!("create WGPU surface: {error}"))
}
