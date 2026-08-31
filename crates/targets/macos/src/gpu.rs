//! Metal-backed WGPU surface ownership for the AppKit shell.

use std::{
    ffi::c_void,
    ptr::NonNull,
    sync::{Arc, Mutex},
};

use raw_window_handle::{
    AppKitDisplayHandle, AppKitWindowHandle, DisplayHandle, HandleError, HasDisplayHandle,
    RawDisplayHandle, RawWindowHandle,
};
use tela_contract::{Color, FrameDamage, RenderPlan};
use tela_render_wgpu::{WgpuRenderer, renderer::RetainedFrameTarget};

use crate::view::TelaView;

/// Logical and backing-store dimensions for the current AppKit content view.
#[derive(Clone, Copy, Debug)]
pub struct ClientMetrics {
    /// Logical AppKit content width in points.
    pub logical_width: f32,
    /// Logical AppKit content height in points.
    pub logical_height: f32,
    /// Backing-store width in pixels.
    pub width: u32,
    /// Backing-store height in pixels.
    pub height: u32,
}

/// A device-loss notification that may be produced by an arbitrary WGPU thread.
#[derive(Debug)]
pub struct DeviceLossReport {
    /// GPU session generation that produced the notification.
    pub generation: u64,
    /// Human-readable driver/runtime diagnostic.
    pub detail: String,
}

/// Result of acquiring and rendering one presentable surface texture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderOutcome {
    /// The frame was presented; WGPU may still request reconfiguration afterwards.
    Presented { suboptimal: bool },
    /// The surface dimensions/configuration no longer match its drawable.
    Outdated,
    /// The drawable surface was lost and has to be recreated.
    Lost,
    /// No drawable was available during this attempt.
    Timeout,
    /// The window is currently hidden or fully occluded.
    Occluded,
    /// WGPU rejected the acquire attempt as invalid.
    Validation,
}

/// Owns the WGPU objects that must stay on the AppKit main thread with the `NSView`.
pub struct GpuSession {
    renderer: WgpuRenderer,
    backing: RetainedFrameTarget,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    instance: wgpu::Instance,
}

/// Marker display handle used both by the instance and the raw AppKit surface target.
#[derive(Debug)]
struct AppKitDisplay;

impl HasDisplayHandle for AppKitDisplay {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        Ok(DisplayHandle::appkit())
    }
}

impl GpuSession {
    /// Creates a Metal-compatible WGPU device and configures it for a non-zero AppKit view.
    pub fn new(
        view: &TelaView,
        metrics: ClientMetrics,
        generation: u64,
        device_loss: Arc<Mutex<Option<DeviceLossReport>>>,
    ) -> Result<Self, String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            flags: wgpu::InstanceFlags::default(),
            memory_budget_thresholds: Default::default(),
            backend_options: Default::default(),
            // wgpu 30 requires a display owner for a native presentable surface. AppKit has an
            // empty marker handle, but it must still agree with `create_surface` below.
            display: Some(Box::new(AppKitDisplay)),
        });
        let surface = create_surface(&instance, view)?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        }))
        .map_err(|error| format!("request WGPU adapter: {error}"))?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("tela macOS device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            experimental_features: Default::default(),
            memory_hints: wgpu::MemoryHints::default(),
            trace: wgpu::Trace::Off,
        }))
        .map_err(|error| format!("create WGPU device: {error}"))?;
        let mut config = surface
            .get_default_config(&adapter, metrics.width, metrics.height)
            .ok_or_else(|| "WGPU surface has no default configuration".to_owned())?;
        config.usage |= wgpu::TextureUsages::COPY_DST;
        let format = config.format;
        surface.configure(&device, &config);
        let renderer = WgpuRenderer::new(device, queue, format, Color::rgba(1.0, 1.0, 1.0, 1.0));
        let backing = renderer.retained_target(config.width, config.height);

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
        });

        Ok(Self {
            renderer,
            backing,
            surface,
            config,
            instance,
        })
    }

    /// Reconfigures the existing surface for the latest backing-store dimensions.
    pub fn reconfigure(&mut self, metrics: ClientMetrics) {
        self.config.width = metrics.width;
        self.config.height = metrics.height;
        self.surface.configure(self.renderer.device(), &self.config);
    }

    /// Recreates the surface itself after an AppKit drawable was lost.
    pub fn recreate_surface(
        &mut self,
        view: &TelaView,
        metrics: ClientMetrics,
    ) -> Result<(), String> {
        let surface = create_surface(&self.instance, view)?;
        self.config.width = metrics.width;
        self.config.height = metrics.height;
        surface.configure(self.renderer.device(), &self.config);
        let previous = std::mem::replace(&mut self.surface, surface);
        drop(previous);
        Ok(())
    }

    /// Renders the current portable frame into the next AppKit drawable.
    pub fn render(&mut self, frame: &RenderPlan, damage: &FrameDamage) -> RenderOutcome {
        let (texture, suboptimal) = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture) => (texture, false),
            wgpu::CurrentSurfaceTexture::Suboptimal(texture) => (texture, true),
            wgpu::CurrentSurfaceTexture::Outdated => return RenderOutcome::Outdated,
            wgpu::CurrentSurfaceTexture::Lost => return RenderOutcome::Lost,
            wgpu::CurrentSurfaceTexture::Timeout => return RenderOutcome::Timeout,
            wgpu::CurrentSurfaceTexture::Occluded => return RenderOutcome::Occluded,
            wgpu::CurrentSurfaceTexture::Validation => return RenderOutcome::Validation,
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
        RenderOutcome::Presented { suboptimal }
    }

    /// Current configured width in backing pixels.
    pub fn width(&self) -> u32 {
        self.config.width
    }

    /// Current configured height in backing pixels.
    pub fn height(&self) -> u32 {
        self.config.height
    }
}

fn create_surface(
    instance: &wgpu::Instance,
    view: &TelaView,
) -> Result<wgpu::Surface<'static>, String> {
    let ns_view = NonNull::from(view).cast::<c_void>();
    let raw_window_handle = RawWindowHandle::AppKit(AppKitWindowHandle::new(ns_view));
    let raw_display_handle = RawDisplayHandle::AppKit(AppKitDisplayHandle::new());
    // SAFETY: `view` is an installed AppKit `NSView`, is accessed only on the main thread, and
    // outlives `GpuSession`. The raw AppKit display marker matches the marker given to the WGPU
    // instance. `WindowState` drops the session before AppKit releases the view.
    unsafe {
        instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
            raw_display_handle: Some(raw_display_handle),
            raw_window_handle,
        })
    }
    .map_err(|error| format!("create WGPU surface: {error}"))
}
