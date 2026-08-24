//! UIKit lifecycle, Winit events, and Metal surface management for iPhone.

use std::{cell::OnceCell, sync::Arc};

use objc2::{DefinedClass, MainThreadOnly, define_class, msg_send, rc::Retained, sel};
use objc2_foundation::{MainThreadMarker, NSObject, NSRunLoop, NSRunLoopCommonModes};
use objc2_quartz_core::{CACurrentMediaTime, CADisplayLink};
use tela_contract::{Color, UiFrame};
use tela_render_wgpu::WgpuRenderer;
use winit::{
    application::ApplicationHandler,
    event::{ElementState, KeyEvent, Touch, TouchPhase as WinitTouchPhase, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy},
    keyboard::{Key, NamedKey},
    platform::ios::{ValidOrientations, WindowExtIOS},
    window::{Window, WindowId},
};

enum HostEvent {
    AnimationFrame(u64),
}

struct DisplayLinkIvars {
    proxy: EventLoopProxy<HostEvent>,
    display_link: OnceCell<Retained<CADisplayLink>>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "TelaIOSDisplayLinkTarget"]
    #[ivars = DisplayLinkIvars]
    struct DisplayLinkTarget;

    impl DisplayLinkTarget {
        #[unsafe(method(animationFrame:))]
        fn animation_frame(&self, display_link: &CADisplayLink) {
            let timestamp_ms = seconds_to_millis(display_link.timestamp());
            let _ = self
                .ivars()
                .proxy
                .send_event(HostEvent::AnimationFrame(timestamp_ms));
        }
    }
);

impl DisplayLinkTarget {
    fn new(mtm: MainThreadMarker, proxy: EventLoopProxy<HostEvent>) -> Retained<Self> {
        let this = mtm.alloc().set_ivars(DisplayLinkIvars {
            proxy,
            display_link: OnceCell::new(),
        });
        // SAFETY: NSObject's initializer is valid for a freshly allocated main-thread target.
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        // SAFETY: `animationFrame:` is declared on this exact target above.
        let display_link =
            unsafe { CADisplayLink::displayLinkWithTarget_selector(&this, sel!(animationFrame:)) };
        display_link.setPaused(true);
        // SAFETY: the display link and target are main-thread-only and invalidated on shutdown.
        unsafe {
            display_link.addToRunLoop_forMode(&NSRunLoop::mainRunLoop(), NSRunLoopCommonModes);
        }
        let _ = this.ivars().display_link.set(display_link);
        this
    }

    fn set_active(&self, active: bool) {
        if let Some(display_link) = self.ivars().display_link.get() {
            display_link.setPaused(!active);
        }
    }

    fn invalidate(&self) {
        if let Some(display_link) = self.ivars().display_link.get() {
            display_link.setPaused(true);
            display_link.invalidate();
        }
    }
}

fn seconds_to_millis(seconds: f64) -> u64 {
    (seconds * 1_000.0).clamp(0.0, u64::MAX as f64) as u64
}

fn current_media_time_ms() -> u64 {
    seconds_to_millis(CACurrentMediaTime())
}

use crate::{
    IosMobileSession,
    input::ControlledTextInput,
    safe_area,
    touch::{TouchAdapter, TouchPhase, logical_coordinate},
};

/// Starts the UIKit-owned event loop for a product-supplied direct mobile session.
pub(super) fn run<A: IosMobileSession + 'static>(app: A) -> Result<(), String> {
    let event_loop = EventLoop::<HostEvent>::with_user_event()
        .build()
        .map_err(|error| format!("create UIKit event loop: {error}"))?;
    let mtm = MainThreadMarker::new()
        .ok_or_else(|| "create iOS display link outside the main thread".to_owned())?;
    let display_link = DisplayLinkTarget::new(mtm, event_loop.create_proxy());
    let mut host = IosHost::new(app, display_link);
    event_loop
        .run_app(&mut host)
        .map_err(|error| format!("run UIKit event loop: {error}"))
}

struct IosHost<A: IosMobileSession> {
    window: Option<Arc<Window>>,
    gpu: Option<GpuSession>,
    app: A,
    text: ControlledTextInput,
    touch: TouchAdapter,
    /// Provenance of the last frame Metal actually presented. A newer logical frame is not
    /// enough: input can still arrive from an older drawable while a redraw is queued.
    presented_frame_token: Option<u64>,
    display_link: Retained<DisplayLinkTarget>,
}

impl<A: IosMobileSession> IosHost<A> {
    fn new(app: A, display_link: Retained<DisplayLinkTarget>) -> Self {
        Self {
            window: None,
            gpu: None,
            app,
            text: ControlledTextInput::default(),
            touch: TouchAdapter::new(),
            presented_frame_token: None,
            display_link,
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
        let viewport_changed = self
            .app
            .set_viewport(size.width as f32 / scale, size.height as f32 / scale);
        let safe_area_changed = self.app.set_safe_area(safe_area::for_window(&window));
        if viewport_changed || safe_area_changed {
            // Physical coordinates from the previous drawable no longer have a valid logical
            // viewport. Do not route input until Metal presents the replacement frame.
            self.presented_frame_token = None;
        }
        if let Some(gpu) = self.gpu.as_mut() {
            gpu.resize(size.width, size.height);
        }
        self.sync_text_channel();
    }

    fn sync_text_channel(&mut self) {
        let status = self.app.text_status();
        let sync = self.text.publish(status.clone());
        if sync.focus_changed
            && let Some(window) = self.window.as_ref()
        {
            window.set_ime_allowed(status.input_focused);
        }
    }

    fn request_redraw(&self) {
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }

    fn synchronize_animation_clock(&mut self) {
        let _ = self.app.animation_tick(current_media_time_ms());
    }

    fn synchronize_display_link(&self) {
        self.display_link.set_active(self.app.animation_active());
    }

    fn fail(&mut self, error: impl AsRef<str>) {
        eprintln!("tela-target-ios: {}", error.as_ref());
        self.request_redraw();
    }

    fn handle_touch(&mut self, touch: Touch) {
        self.synchronize_animation_clock();
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
        let pointer = self.touch.handle(touch.id, phase, x, y);
        let Some(frame_token) = self.presented_frame_token else {
            return;
        };
        self.app.dispatch_pointer(frame_token, pointer);
        self.sync_text_channel();
        self.synchronize_display_link();
        self.request_redraw();
    }

    fn handle_keyboard(&mut self, event: KeyEvent) {
        self.synchronize_animation_clock();
        let KeyEvent {
            state,
            logical_key,
            text,
            ..
        } = event;
        if state != ElementState::Pressed {
            return;
        }
        let Some(frame_token) = self.presented_frame_token else {
            return;
        };
        match logical_key {
            Key::Named(NamedKey::Backspace) => {
                if let Some(value) = self.text.delete_backward() {
                    self.app.set_input_value(frame_token, value);
                }
            }
            Key::Named(NamedKey::Enter) => {
                self.app.input_enter(frame_token);
            }
            Key::Named(NamedKey::Escape) => {
                self.app.dispatch_key(frame_token, 0x29);
            }
            _ => {
                if let Some(text) = text.as_ref().map(|text| text.as_str()) {
                    if text == "\n" || text == "\r" {
                        self.app.input_enter(frame_token);
                    } else if !text.chars().all(char::is_control)
                        && let Some(value) = self.text.append(text)
                    {
                        self.app.set_input_value(frame_token, value);
                    }
                }
            }
        }
        self.sync_text_channel();
        self.synchronize_display_link();
        self.request_redraw();
    }

    fn redraw(&mut self) {
        let (frame, frame_token) = self.app.frame();
        let frame = frame.clone();
        let outcome = match self.gpu.as_mut() {
            Some(gpu) => gpu.render(&frame),
            None => return,
        };
        match outcome {
            RenderOutcome::Presented { suboptimal } => {
                self.presented_frame_token = Some(frame_token);
                if suboptimal {
                    if let Some(gpu) = self.gpu.as_mut() {
                        gpu.reconfigure();
                    }
                    self.request_redraw();
                }
                self.synchronize_display_link();
            }
            RenderOutcome::Outdated => {
                self.presented_frame_token = None;
                if let Some(gpu) = self.gpu.as_mut() {
                    gpu.reconfigure();
                }
                self.request_redraw();
            }
            RenderOutcome::Lost => {
                self.presented_frame_token = None;
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
                self.presented_frame_token = None;
                self.fail("Metal surface validation failed while acquiring a frame")
            }
        }
    }
}

impl<A: IosMobileSession> ApplicationHandler<HostEvent> for IosHost<A> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if let Err(error) = self.ensure_window_and_gpu(event_loop) {
            self.fail(error);
        }
        self.synchronize_display_link();
        self.request_redraw();
    }

    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        self.display_link.set_active(false);
        self.synchronize_animation_clock();
        let frame_token = self.presented_frame_token;
        for event in self.touch.cancel_all() {
            if let Some(frame_token) = frame_token {
                self.app.dispatch_pointer(frame_token, event);
            }
        }
        if let Some(frame_token) = frame_token {
            self.app.input_blur(frame_token);
        }
        self.presented_frame_token = None;
        self.sync_text_channel();
        // UIKit may invalidate its Metal drawable in the background. The next `resumed` callback
        // retains the business session but creates a fresh surface and device.
        self.gpu = None;
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: HostEvent) {
        match event {
            HostEvent::AnimationFrame(timestamp_ms) => {
                if self.app.animation_active() {
                    let _ = self.app.animation_tick(timestamp_ms);
                    self.request_redraw();
                } else {
                    self.display_link.set_active(false);
                }
            }
        }
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
                self.synchronize_animation_clock();
                if let Some(frame_token) = self.presented_frame_token {
                    self.app.composition_changed(frame_token);
                }
                self.sync_text_channel();
                self.synchronize_display_link();
                self.request_redraw();
            }
            WindowEvent::Focused(false) => {
                self.synchronize_animation_clock();
                if let Some(frame_token) = self.presented_frame_token {
                    self.app.input_blur(frame_token);
                }
                self.sync_text_channel();
                self.synchronize_display_link();
                self.request_redraw();
            }
            WindowEvent::CloseRequested => event_loop.exit(),
            _ => {}
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        self.display_link.invalidate();
        self.presented_frame_token = None;
        self.gpu = None;
        self.window = None;
    }

    fn memory_warning(&mut self, _event_loop: &ActiveEventLoop) {
        eprintln!("tela-target-ios: iOS reported low memory");
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
