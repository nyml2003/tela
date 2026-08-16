//! UIKit lifecycle, Winit events, and Metal surface management for iPhone.

use std::sync::Arc;

use tela_contract::{Color, UiFrame};
use tela_mobile_demo::MobileApp;
use tela_render_wgpu::WgpuRenderer;
use winit::{
    application::ApplicationHandler,
    event::{ElementState, KeyEvent, Touch, TouchPhase as WinitTouchPhase, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop},
    keyboard::{Key, NamedKey},
    platform::ios::{ValidOrientations, WindowExtIOS},
    window::{Window, WindowId},
};

use crate::{
    input::ControlledTextInput,
    safe_area,
    touch::{TouchAdapter, TouchPhase, logical_coordinate},
};

const TOUCH_SLOP_PT: f32 = 12.0;

/// Starts the UIKit-owned event loop. The outer Objective-C `main` invokes this exactly once.
pub(super) fn run() -> Result<(), String> {
    let event_loop =
        EventLoop::new().map_err(|error| format!("create UIKit event loop: {error}"))?;
    let mut host = IosHost::new();
    event_loop
        .run_app(&mut host)
        .map_err(|error| format!("run UIKit event loop: {error}"))
}

struct IosHost {
    window: Option<Arc<Window>>,
    gpu: Option<GpuSession>,
    app: MobileApp,
    text: ControlledTextInput,
    touch: TouchAdapter,
}

impl IosHost {
    fn new() -> Self {
        Self {
            window: None,
            gpu: None,
            app: MobileApp::new(),
            text: ControlledTextInput::default(),
            touch: TouchAdapter::new(TOUCH_SLOP_PT),
        }
    }

    fn ensure_window_and_gpu(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
        if self.window.is_none() {
            let window = event_loop
                .create_window(Window::default_attributes().with_title("Tela Mobile"))
                .map_err(|error| format!("create UIKit window: {error}"))?;
            window.set_valid_orientations(ValidOrientations::Portrait);
            self.window = Some(Arc::new(window));
        }
        if self.gpu.is_none() {
            let window = self
                .window
                .as_ref()
                .expect("window is installed before creating Metal")
                .clone();
            self.gpu = Some(GpuSession::new(window)?);
        }
        self.update_metrics();
        Ok(())
    }

    fn update_metrics(&mut self) {
        let Some(window) = self.window.as_ref().cloned() else {
            return;
        };
        let size = window.inner_size();
        if size.width == 0 || size.height == 0 {
            return;
        }
        let scale = window.scale_factor().max(1.0) as f32;
        self.app
            .set_viewport(size.width as f32 / scale, size.height as f32 / scale);
        self.app.set_safe_area(safe_area::for_window(&window));
        if let Some(gpu) = self.gpu.as_mut() {
            gpu.resize(size.width, size.height);
        }
        self.sync_text_channel();
    }

    fn sync_text_channel(&mut self) {
        let sync = self.text.publish(self.app.status());
        if sync.focus_changed
            && let Some(window) = self.window.as_ref()
        {
            window.set_ime_allowed(self.app.status().input_focused);
        }
    }

    fn request_redraw(&self) {
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }

    fn fail(&mut self, error: impl AsRef<str>) {
        eprintln!("tela-ios-sdk: {}", error.as_ref());
        self.request_redraw();
    }

    fn handle_touch(&mut self, touch: Touch) {
        let Some(phase) = touch_phase(touch.phase) else {
            return;
        };
        let scale = self
            .window
            .as_ref()
            .map(|window| window.scale_factor())
            .unwrap_or(1.0);
        let x = logical_coordinate(touch.location.x, scale);
        let y = logical_coordinate(touch.location.y, scale);
        for event in self.touch.handle(touch.id, phase, x, y) {
            self.app.dispatch_pointer(event);
        }
        self.sync_text_channel();
        self.request_redraw();
    }

    fn handle_keyboard(&mut self, event: KeyEvent) {
        let KeyEvent {
            state,
            logical_key,
            text,
            ..
        } = event;
        if state != ElementState::Pressed {
            return;
        }
        match logical_key {
            Key::Named(NamedKey::Backspace) => {
                if let Some(value) = self.text.delete_backward() {
                    self.app.set_input_value(value);
                }
            }
            Key::Named(NamedKey::Enter) => {
                self.app.input_enter();
            }
            Key::Named(NamedKey::Escape) => {
                self.app.dispatch_key(0x29);
            }
            _ => {
                if let Some(text) = text.as_ref().map(|text| text.as_str()) {
                    if text == "\n" || text == "\r" {
                        self.app.input_enter();
                    } else if !text.chars().all(char::is_control)
                        && let Some(value) = self.text.append(text)
                    {
                        self.app.set_input_value(value);
                    }
                }
            }
        }
        self.sync_text_channel();
        self.request_redraw();
    }

    fn redraw(&mut self) {
        let frame = self.app.frame().clone();
        let outcome = match self.gpu.as_mut() {
            Some(gpu) => gpu.render(&frame),
            None => return,
        };
        match outcome {
            RenderOutcome::Presented { suboptimal } => {
                if suboptimal {
                    if let Some(gpu) = self.gpu.as_mut() {
                        gpu.reconfigure();
                    }
                    self.request_redraw();
                }
            }
            RenderOutcome::Outdated => {
                if let Some(gpu) = self.gpu.as_mut() {
                    gpu.reconfigure();
                }
                self.request_redraw();
            }
            RenderOutcome::Lost => {
                let Some(window) = self.window.as_ref().cloned() else {
                    return;
                };
                match GpuSession::new(window) {
                    Ok(gpu) => {
                        self.gpu = Some(gpu);
                        self.request_redraw();
                    }
                    Err(error) => self.fail(format!("recreate lost Metal surface: {error}")),
                }
            }
            RenderOutcome::Timeout => self.request_redraw(),
            RenderOutcome::Occluded => {}
            RenderOutcome::Validation => {
                self.fail("Metal surface validation failed while acquiring a frame")
            }
        }
    }
}

impl ApplicationHandler for IosHost {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if let Err(error) = self.ensure_window_and_gpu(event_loop) {
            self.fail(error);
        }
        self.request_redraw();
    }

    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        self.app.input_blur();
        self.sync_text_channel();
        // UIKit may invalidate its Metal drawable in the background. The next `resumed` callback
        // retains the business session but creates a fresh surface and device.
        self.gpu = None;
        self.touch.reset();
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                self.update_metrics();
                self.request_redraw();
            }
            WindowEvent::RedrawRequested => self.redraw(),
            WindowEvent::Touch(touch) => self.handle_touch(touch),
            WindowEvent::KeyboardInput { event, .. } => self.handle_keyboard(event),
            WindowEvent::Ime(_) => {
                self.app.composition_changed();
                self.sync_text_channel();
                self.request_redraw();
            }
            WindowEvent::Focused(false) => {
                self.app.input_blur();
                self.sync_text_channel();
                self.request_redraw();
            }
            WindowEvent::CloseRequested => event_loop.exit(),
            _ => {}
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        self.gpu = None;
        self.window = None;
    }

    fn memory_warning(&mut self, _event_loop: &ActiveEventLoop) {
        eprintln!("tela-ios-sdk: iOS reported low memory");
    }
}

struct GpuSession {
    // Drop order keeps the Metal surface valid while the renderer is destroyed.
    renderer: WgpuRenderer,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    _instance: wgpu::Instance,
}

#[derive(Clone, Copy)]
enum RenderOutcome {
    Presented { suboptimal: bool },
    Outdated,
    Lost,
    Timeout,
    Occluded,
    Validation,
}

impl GpuSession {
    fn new(window: Arc<Window>) -> Result<Self, String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::METAL,
            flags: wgpu::InstanceFlags::default(),
            memory_budget_thresholds: Default::default(),
            backend_options: Default::default(),
            display: None,
        });
        let surface = instance
            .create_surface(window.clone())
            .map_err(|error| format!("create Metal surface: {error}"))?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        }))
        .map_err(|error| format!("request Metal adapter: {error}"))?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("tela iOS Metal device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            experimental_features: Default::default(),
            memory_hints: wgpu::MemoryHints::default(),
            trace: wgpu::Trace::Off,
        }))
        .map_err(|error| format!("create Metal device: {error}"))?;
        let size = window.inner_size();
        let config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .ok_or_else(|| "Metal surface has no supported default format".to_owned())?;
        surface.configure(&device, &config);
        Ok(Self {
            renderer: WgpuRenderer::new(
                device,
                queue,
                config.format,
                Color::rgba(0.97, 0.98, 1.0, 1.0),
            ),
            surface,
            config,
            _instance: instance,
        })
    }

    fn resize(&mut self, width: u32, height: u32) {
        self.config.width = width.max(1);
        self.config.height = height.max(1);
        self.reconfigure();
    }

    fn reconfigure(&mut self) {
        self.surface.configure(self.renderer.device(), &self.config);
    }

    fn render(&mut self, frame: &UiFrame) -> RenderOutcome {
        let (texture, suboptimal) = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture) => (texture, false),
            wgpu::CurrentSurfaceTexture::Suboptimal(texture) => (texture, true),
            wgpu::CurrentSurfaceTexture::Outdated => return RenderOutcome::Outdated,
            wgpu::CurrentSurfaceTexture::Lost => return RenderOutcome::Lost,
            wgpu::CurrentSurfaceTexture::Timeout => return RenderOutcome::Timeout,
            wgpu::CurrentSurfaceTexture::Occluded => return RenderOutcome::Occluded,
            wgpu::CurrentSurfaceTexture::Validation => return RenderOutcome::Validation,
        };
        let target = texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        self.renderer
            .render_frame(frame, &target, self.config.width, self.config.height);
        self.renderer.present(texture);
        RenderOutcome::Presented { suboptimal }
    }
}

fn touch_phase(phase: WinitTouchPhase) -> Option<TouchPhase> {
    match phase {
        WinitTouchPhase::Started => Some(TouchPhase::Started),
        WinitTouchPhase::Moved => Some(TouchPhase::Moved),
        WinitTouchPhase::Ended => Some(TouchPhase::Ended),
        WinitTouchPhase::Cancelled => Some(TouchPhase::Cancelled),
    }
}
