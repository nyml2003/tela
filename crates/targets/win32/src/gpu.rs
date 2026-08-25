//! WGPU presentation for the static Win32 shell (copied and simplified from tela-target-win32).

#![allow(unsafe_code)]

use std::time::Instant;

use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, RawDisplayHandle, RawWindowHandle,
    Win32WindowHandle,
};
use tela_contract::UiFrame;
use tela_render_wgpu::WgpuRenderer;
use windows::Win32::Foundation::HWND;

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

/// Surface, renderer, and presentation configuration.
pub struct GpuSession {
    instance: wgpu::Instance,
    surface: wgpu::Surface<'static>,
    /// Presentation configuration.
    pub config: wgpu::SurfaceConfiguration,
    renderer: WgpuRenderer,
}

impl GpuSession {
    /// Creates the renderer and configures the surface for the given client size.
    pub fn new(hwnd: HWND, width: u32, height: u32, dpr: f32) -> Result<Self, String> {
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
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
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
        gpu_trace!(
            "tela-win32-trace: event=gpu_init stage=surface_configured elapsed_us={} physical={}x{}",
            init_started.elapsed().as_micros(),
            config.width,
            config.height
        );
        let _ = dpr;
        gpu_trace!(
            "tela-win32-trace: event=gpu_init stage=complete elapsed_us={}",
            init_started.elapsed().as_micros()
        );
        Ok(Self {
            instance,
            surface,
            config,
            renderer,
        })
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
    pub fn render(&mut self, frame: &UiFrame) -> Result<RenderOutcome, String> {
        let (texture, suboptimal) = match self.surface.get_current_texture() {
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
        self.renderer
            .render_frame(frame, &view, self.config.width, self.config.height);
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
