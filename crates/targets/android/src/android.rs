//! Android-only GameActivity, Vulkan surface, and JNI text-channel integration.

use std::{
    sync::{Arc, Mutex, OnceLock},
    thread,
};

use jni::{
    Env, EnvUnowned,
    errors::LogErrorAndDefault,
    objects::{JObject, JString},
    sys::{jboolean, jint, jstring},
};
use tela_app_abi::{AppEvent, AppStatus};
use tela_contract::{
    Color, DrawCommand, DrawPayload, Rect, TextContent, TextStyleRef, UiFrame, Viewport,
};
use tela_guest_runtime::{GuestRuntime, load_remote_bundle};
use tela_render_wgpu::WgpuRenderer;
use winit::{
    application::ApplicationHandler,
    event::{ElementState, Touch, TouchPhase as WinitTouchPhase, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy},
    keyboard::{KeyCode, PhysicalKey as WinitPhysicalKey},
    platform::android::{EventLoopBuilderExtAndroid, activity::AndroidApp},
    window::{Window, WindowId},
};

use crate::{
    ime::{ControlledTextSync, TextInputState},
    touch::{TouchAdapter, TouchPhase, logical_coordinate},
};

const BACK_BLURRED_TEXT_INPUT: jint = 1;
const BACK_DISPATCHED_TO_GUEST: jint = 2;

enum HostEvent {
    ConfigureBundleIndex(String),
    Startup(Result<GuestRuntime, String>),
    SetInputValue(String),
    InputFocus,
    InputBlur,
    InputEnter,
    CompositionStart,
    CompositionEnd,
    SystemBack,
}

#[derive(Default)]
struct NativeBridge {
    proxy: Option<EventLoopProxy<HostEvent>>,
    text: ControlledTextSync,
    bundle_index: Option<String>,
    finish_requested: bool,
}

static NATIVE_BRIDGE: OnceLock<Mutex<NativeBridge>> = OnceLock::new();

fn bridge() -> &'static Mutex<NativeBridge> {
    NATIVE_BRIDGE.get_or_init(|| Mutex::new(NativeBridge::default()))
}

fn bridge_lock() -> std::sync::MutexGuard<'static, NativeBridge> {
    bridge()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn install_bridge(proxy: EventLoopProxy<HostEvent>) {
    let mut bridge = bridge_lock();
    bridge.proxy = Some(proxy);
    bridge.text = ControlledTextSync::default();
    bridge.finish_requested = false;
}

fn clear_bridge() {
    let mut bridge = bridge_lock();
    bridge.proxy = None;
    bridge.text = ControlledTextSync::default();
    bridge.finish_requested = false;
}

fn configured_bundle_index() -> Option<String> {
    bridge_lock().bundle_index.clone()
}

fn configure_bundle_index(index: String) {
    let proxy = {
        let mut bridge = bridge_lock();
        bridge.bundle_index = Some(index.clone());
        bridge.proxy.clone()
    };
    if let Some(proxy) = proxy {
        let _ = proxy.send_event(HostEvent::ConfigureBundleIndex(index));
    }
}

fn send_host_event(event: HostEvent) -> bool {
    let proxy = bridge_lock().proxy.clone();
    proxy.is_some_and(|proxy| proxy.send_event(event).is_ok())
}

fn publish_guest_status(status: &AppStatus) {
    bridge_lock()
        .text
        .publish_guest(status.input_focused, &status.input_value);
}

fn text_snapshot() -> TextInputState {
    bridge_lock().text.snapshot()
}

fn request_activity_finish() {
    bridge_lock().finish_requested = true;
}

fn consume_finish_request() -> bool {
    let mut bridge = bridge_lock();
    let requested = bridge.finish_requested;
    bridge.finish_requested = false;
    requested
}

/// Android's lifecycle entrypoint. It belongs to the Activity instance rather than the process.
#[unsafe(no_mangle)]
pub fn android_main(app: AndroidApp) {
    let mut builder = EventLoop::<HostEvent>::with_user_event();
    builder.with_android_app(app);
    let event_loop = match builder.build() {
        Ok(event_loop) => event_loop,
        Err(error) => {
            eprintln!("tela-target-android: create Android event loop: {error}");
            return;
        }
    };
    let proxy = event_loop.create_proxy();
    install_bridge(proxy.clone());
    let mut host = AndroidHost::new(configured_bundle_index().unwrap_or_default(), proxy);
    host.start_loading();
    if let Err(error) = event_loop.run_app(&mut host) {
        eprintln!("tela-target-android: event loop stopped: {error}");
    }
    clear_bridge();
}

struct AndroidHost {
    bundle_index: String,
    startup_started: bool,
    proxy: EventLoopProxy<HostEvent>,
    window: Option<Arc<Window>>,
    gpu: Option<GpuSession>,
    runtime: Option<GuestRuntime>,
    frame: Option<UiFrame>,
    touch: TouchAdapter,
    failure: Option<String>,
}

impl AndroidHost {
    fn new(bundle_index: String, proxy: EventLoopProxy<HostEvent>) -> Self {
        Self {
            bundle_index,
            startup_started: false,
            proxy,
            window: None,
            gpu: None,
            runtime: None,
            frame: None,
            touch: TouchAdapter::new(),
            failure: None,
        }
    }

    fn start_loading(&mut self) {
        if self.startup_started || self.bundle_index.trim().is_empty() {
            return;
        }
        self.startup_started = true;
        let index_url = self.bundle_index.clone();
        let proxy = self.proxy.clone();
        thread::spawn(move || {
            let result = load_guest(&index_url);
            let _ = proxy.send_event(HostEvent::Startup(result));
        });
    }

    fn configure_bundle_index(&mut self, index: String) {
        if self.startup_started || index.trim().is_empty() {
            return;
        }
        self.bundle_index = index;
        self.start_loading();
        self.request_redraw();
    }

    fn ensure_window_and_gpu(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
        if self.window.is_none() {
            let window = event_loop
                .create_window(Window::default_attributes())
                .map_err(|error| format!("create GameActivity window: {error}"))?;
            self.window = Some(Arc::new(window));
        }
        if self.gpu.is_none() {
            let window = self
                .window
                .as_ref()
                .expect("window is installed before creating GPU")
                .clone();
            self.gpu = Some(GpuSession::new(window)?);
        }
        self.dispatch_viewport()?;
        Ok(())
    }

    fn install_runtime(&mut self, runtime: GuestRuntime) -> Result<(), String> {
        let frame = runtime.frame().map_err(|error| error.to_string())?;
        publish_guest_status(runtime.status());
        self.failure = None;
        self.frame = Some(frame);
        self.runtime = Some(runtime);
        self.dispatch_viewport()?;
        self.request_redraw();
        Ok(())
    }

    fn dispatch_viewport(&mut self) -> Result<(), String> {
        let Some(window) = self.window.as_ref() else {
            return Ok(());
        };
        if self.runtime.is_none() {
            return Ok(());
        }
        let size = window.inner_size();
        if size.width == 0 || size.height == 0 {
            return Ok(());
        }
        let scale = window.scale_factor().max(1.0) as f32;
        self.dispatch_guest(AppEvent::Viewport {
            width: size.width as f32 / scale,
            height: size.height as f32 / scale,
        })?;
        Ok(())
    }

    fn dispatch_guest(&mut self, event: AppEvent) -> Result<bool, String> {
        let Some(runtime) = self.runtime.as_mut() else {
            // Platform events can legally arrive while the strict remote bundle is loading. They
            // have no guest to target yet and must not turn a normal loading state into failure.
            return Ok(false);
        };
        let changed = runtime
            .dispatch(&event)
            .map_err(|error| error.to_string())?;
        let frame = runtime.frame().map_err(|error| error.to_string())?;
        publish_guest_status(runtime.status());
        self.frame = Some(frame);
        self.request_redraw();
        Ok(changed)
    }

    fn handle_system_back(&mut self) {
        if self.runtime.is_none() {
            request_activity_finish();
            return;
        }
        let focused = self
            .runtime
            .as_ref()
            .is_some_and(|runtime| runtime.status().input_focused);
        let result = if focused {
            self.dispatch_guest(AppEvent::InputBlur)
        } else {
            self.dispatch_guest(AppEvent::KeyDown {
                physical_key: 0x29,
                modifier_bits: 0,
                repeat: false,
            })
        };
        match result {
            Ok(changed) if !focused && !changed => request_activity_finish(),
            Ok(_) => {}
            Err(error) => self.fail(error),
        }
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
        // Winit touch locations are physical pixels while Guest layout/input uses logical units.
        // Gesture thresholds are deliberately not interpreted here; the Kernel owns them.
        let x = logical_coordinate(touch.location.x, scale);
        let y = logical_coordinate(touch.location.y, scale);
        let event = self.touch.handle(touch.id, phase, x, y);
        if let Err(error) = self.dispatch_guest(AppEvent::Pointer(event)) {
            self.fail(error);
        }
    }

    fn resize(&mut self) {
        let Some(window) = self.window.as_ref() else {
            return;
        };
        let size = window.inner_size();
        if size.width == 0 || size.height == 0 {
            return;
        }
        if let Some(gpu) = self.gpu.as_mut() {
            gpu.resize(size.width, size.height);
        }
        if let Err(error) = self.dispatch_viewport() {
            self.fail(error);
        }
        self.request_redraw();
    }

    fn redraw(&mut self) {
        let frame = self
            .frame
            .clone()
            .unwrap_or_else(|| self.diagnostic_frame());
        let render_result = match self.gpu.as_mut() {
            Some(gpu) => gpu.render(&frame),
            None => return,
        };
        match render_result {
            RenderOutcome::Presented { suboptimal } => {
                if suboptimal {
                    if let Err(error) = self
                        .gpu
                        .as_mut()
                        .expect("GPU exists while rendering")
                        .reconfigure()
                    {
                        self.fail(format!("reconfigure suboptimal Vulkan surface: {error}"));
                    } else {
                        self.request_redraw();
                    }
                }
            }
            RenderOutcome::Outdated => {
                let result = self
                    .gpu
                    .as_mut()
                    .expect("GPU exists while rendering")
                    .reconfigure();
                if let Err(error) = result {
                    self.fail(format!("reconfigure outdated Vulkan surface: {error}"));
                } else {
                    self.request_redraw();
                }
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
                    Err(error) => self.fail(format!("recreate lost Vulkan surface: {error}")),
                }
            }
            RenderOutcome::Timeout => self.request_redraw(),
            RenderOutcome::Occluded => {}
            RenderOutcome::Validation => {
                self.fail("WGPU surface validation failed while acquiring a frame".to_owned());
            }
        }
    }

    fn diagnostic_frame(&self) -> UiFrame {
        let viewport = self.logical_viewport();
        let (title, detail, accent) = match self.failure.as_deref() {
            Some(error) => (
                "TELA Mobile could not start",
                error.chars().take(160).collect::<String>(),
                Color::rgba(0.78, 0.16, 0.16, 1.0),
            ),
            None if self.bundle_index.trim().is_empty() => (
                "Missing development bundle index",
                "Run tela-android-bootstrap, build the Android package, then serve it".to_owned(),
                Color::rgba(0.78, 0.45, 0.08, 1.0),
            ),
            None => (
                "TELA Mobile",
                "Loading the current development bundle...".to_owned(),
                Color::rgba(0.12, 0.38, 0.92, 1.0),
            ),
        };
        UiFrame {
            viewport,
            commands: vec![
                DrawCommand {
                    geometry: Rect {
                        x: 0.0,
                        y: 0.0,
                        w: viewport.width,
                        h: viewport.height,
                    },
                    clip: None,
                    payload: DrawPayload::Rect {
                        fill: Some(Color::rgba(0.97, 0.98, 1.0, 1.0)),
                        border: None,
                    },
                },
                DrawCommand {
                    geometry: Rect {
                        x: 24.0,
                        y: 72.0,
                        w: (viewport.width - 48.0).max(1.0),
                        h: 30.0,
                    },
                    clip: None,
                    payload: DrawPayload::Text {
                        text: diagnostic_text(title, 22.0, accent),
                        baseline_y: 94.0,
                    },
                },
                DrawCommand {
                    geometry: Rect {
                        x: 24.0,
                        y: 124.0,
                        w: (viewport.width - 48.0).max(1.0),
                        h: 80.0,
                    },
                    clip: None,
                    payload: DrawPayload::Text {
                        text: diagnostic_text(&detail, 15.0, Color::rgba(0.2, 0.24, 0.31, 1.0)),
                        baseline_y: 142.0,
                    },
                },
            ],
            hit_regions: Vec::new(),
            scroll_bounds: Vec::new(),
        }
    }

    fn logical_viewport(&self) -> Viewport {
        let Some(window) = self.window.as_ref() else {
            return Viewport {
                width: 360.0,
                height: 720.0,
            };
        };
        let size = window.inner_size();
        let scale = window.scale_factor().max(1.0) as f32;
        Viewport {
            width: (size.width as f32 / scale).max(1.0),
            height: (size.height as f32 / scale).max(1.0),
        }
    }

    fn fail(&mut self, error: String) {
        eprintln!("tela-target-android: {error}");
        self.failure = Some(error);
        self.runtime = None;
        self.frame = None;
        publish_guest_status(&AppStatus::default());
        self.request_redraw();
    }

    fn request_redraw(&self) {
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }
}

impl ApplicationHandler<HostEvent> for AndroidHost {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if let Err(error) = self.ensure_window_and_gpu(event_loop) {
            self.fail(error);
        }
        self.request_redraw();
    }

    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        // Android invalidates the SurfaceView here; surface and every resource borrowing it must
        // disappear before this callback returns. The portable guest intentionally survives.
        for event in self.touch.cancel_all() {
            if let Err(error) = self.dispatch_guest(AppEvent::Pointer(event)) {
                self.fail(error);
                break;
            }
        }
        self.gpu = None;
        self.window = None;
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: HostEvent) {
        match event {
            HostEvent::ConfigureBundleIndex(index) => self.configure_bundle_index(index),
            HostEvent::Startup(result) => match result {
                Ok(runtime) => {
                    if let Err(error) = self.install_runtime(runtime) {
                        self.fail(error);
                    }
                }
                Err(error) => self.fail(error),
            },
            HostEvent::SetInputValue(value) => {
                if let Err(error) = self.dispatch_guest(AppEvent::SetInputValue(value)) {
                    self.fail(error);
                }
            }
            HostEvent::InputFocus => {
                if let Err(error) = self.dispatch_guest(AppEvent::InputFocus) {
                    self.fail(error);
                }
            }
            HostEvent::InputBlur => {
                if let Err(error) = self.dispatch_guest(AppEvent::InputBlur) {
                    self.fail(error);
                }
            }
            HostEvent::InputEnter => {
                if let Err(error) = self.dispatch_guest(AppEvent::InputEnter) {
                    self.fail(error);
                }
            }
            HostEvent::CompositionStart => {
                if let Err(error) = self.dispatch_guest(AppEvent::InputCompositionStart) {
                    self.fail(error);
                }
            }
            HostEvent::CompositionEnd => {
                if let Err(error) = self.dispatch_guest(AppEvent::InputCompositionEnd) {
                    self.fail(error);
                }
            }
            HostEvent::SystemBack => self.handle_system_back(),
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => self.resize(),
            WindowEvent::RedrawRequested => self.redraw(),
            WindowEvent::Touch(touch) => self.handle_touch(touch),
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                if let Some(physical_key) = map_physical_key(event.physical_key) {
                    if let Err(error) = self.dispatch_guest(AppEvent::KeyDown {
                        physical_key,
                        modifier_bits: 0,
                        repeat: event.repeat,
                    }) {
                        self.fail(error);
                    }
                }
            }
            WindowEvent::Focused(false) => {
                if self
                    .runtime
                    .as_ref()
                    .is_some_and(|runtime| runtime.status().input_focused)
                    && let Err(error) = self.dispatch_guest(AppEvent::InputBlur)
                {
                    self.fail(error);
                }
            }
            WindowEvent::CloseRequested => event_loop.exit(),
            _ => {}
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        self.gpu = None;
        self.window = None;
        clear_bridge();
    }

    fn memory_warning(&mut self, _event_loop: &ActiveEventLoop) {
        // The current renderer retains only the last few submissions. Dropping text caches would
        // require a renderer API; keeping the live guest and surface is safer than ad hoc reset.
        eprintln!("tela-target-android: Android reported low memory");
    }
}

struct GpuSession {
    // Drop order is renderer -> surface -> config -> instance, which keeps Vulkan surface handles
    // valid for every renderer destruction path.
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
            backends: wgpu::Backends::VULKAN,
            flags: wgpu::InstanceFlags::default(),
            memory_budget_thresholds: Default::default(),
            backend_options: Default::default(),
            display: None,
        });
        let surface = instance
            .create_surface(window.clone())
            .map_err(|error| format!("create Vulkan surface: {error}"))?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        }))
        .map_err(|error| format!("request Vulkan adapter: {error}"))?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("tela Android Vulkan device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            experimental_features: Default::default(),
            memory_hints: wgpu::MemoryHints::default(),
            trace: wgpu::Trace::Off,
        }))
        .map_err(|error| format!("create Vulkan device: {error}"))?;
        let size = window.inner_size();
        let config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .ok_or_else(|| "Vulkan surface has no supported default format".to_owned())?;
        surface.configure(&device, &config);
        let renderer = WgpuRenderer::new(
            device,
            queue,
            config.format,
            Color::rgba(0.97, 0.98, 1.0, 1.0),
        );
        Ok(Self {
            renderer,
            surface,
            config,
            _instance: instance,
        })
    }

    fn resize(&mut self, width: u32, height: u32) {
        self.config.width = width.max(1);
        self.config.height = height.max(1);
        self.surface.configure(self.renderer.device(), &self.config);
    }

    fn reconfigure(&mut self) -> Result<(), String> {
        if self.config.width == 0 || self.config.height == 0 {
            return Err("cannot configure a zero-sized Android surface".to_owned());
        }
        self.surface.configure(self.renderer.device(), &self.config);
        Ok(())
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
        let view = texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        self.renderer
            .render_frame(frame, &view, self.config.width, self.config.height);
        self.renderer.present(texture);
        RenderOutcome::Presented { suboptimal }
    }
}

fn load_guest(index_url: &str) -> Result<GuestRuntime, String> {
    let bundle = load_remote_bundle(index_url, fetch_http)?;
    eprintln!(
        "tela-target-android: bundle archive={}KB download={}ms; initializing guest",
        bundle.metrics.archive_bytes / 1024,
        bundle.metrics.download.as_millis(),
    );
    GuestRuntime::new(&bundle.archive.app_wasm).map_err(|error| error.to_string())
}

fn fetch_http(url: &str) -> Result<Vec<u8>, String> {
    let response = ureq::get(url)
        .call()
        .map_err(|error| format!("GET {url}: {error}"))?;
    response
        .into_body()
        .into_with_config()
        .limit(
            u64::try_from(tela_guest_runtime::MAX_ARCHIVE_BYTES)
                .expect("guest archive byte limit fits in u64"),
        )
        .read_to_vec()
        .map_err(|error| format!("read {url}: {error}"))
}

fn touch_phase(phase: WinitTouchPhase) -> Option<TouchPhase> {
    match phase {
        WinitTouchPhase::Started => Some(TouchPhase::Started),
        WinitTouchPhase::Moved => Some(TouchPhase::Moved),
        WinitTouchPhase::Ended => Some(TouchPhase::Ended),
        WinitTouchPhase::Cancelled => Some(TouchPhase::Cancelled),
    }
}

fn map_physical_key(key: WinitPhysicalKey) -> Option<u16> {
    let WinitPhysicalKey::Code(key) = key else {
        return None;
    };
    match key {
        KeyCode::Escape => Some(0x29),
        KeyCode::Enter | KeyCode::NumpadEnter => Some(0x28),
        KeyCode::Backspace => Some(0x2a),
        KeyCode::Tab => Some(0x2b),
        KeyCode::Space => Some(0x2c),
        KeyCode::ArrowRight => Some(0x4f),
        KeyCode::ArrowLeft => Some(0x50),
        KeyCode::ArrowDown => Some(0x51),
        KeyCode::ArrowUp => Some(0x52),
        KeyCode::Home => Some(0x4a),
        KeyCode::End => Some(0x4d),
        KeyCode::PageUp => Some(0x4b),
        KeyCode::PageDown => Some(0x4e),
        KeyCode::Delete => Some(0x4c),
        _ => None,
    }
}

fn diagnostic_text(content: &str, font_size: f32, color: Color) -> TextContent {
    TextContent {
        text: content.to_owned(),
        font: TextStyleRef::body(),
        font_size,
        line_height: font_size * 1.35,
        color,
    }
}

fn java_string<'local>(env: &Env<'local>, value: &JString<'local>) -> jni::errors::Result<String> {
    value.try_to_string(env)
}

/// Receives the Gradle-injected bundle URL before GameActivity creates the native main loop.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_tela_mobile_TelaActivity_nativeConfigureBundleIndex(
    mut env: EnvUnowned<'_>,
    _activity: JObject<'_>,
    value: JString<'_>,
) {
    env.with_env(|env| -> jni::errors::Result<()> {
        configure_bundle_index(java_string(env, &value)?);
        Ok(())
    })
    .resolve::<LogErrorAndDefault>();
}

/// Returns whether Kotlin should attach its hidden controlled `EditText` to the IME.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_tela_mobile_TelaActivity_nativeInputFocused(
    env: EnvUnowned<'_>,
    _activity: JObject<'_>,
) -> jboolean {
    guarded_jni(env, || jboolean::from(text_snapshot().focused))
}

/// Returns the complete controlled value Kotlin must mirror, with its cursor kept at the end.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_tela_mobile_TelaActivity_nativeInputValue(
    mut env: EnvUnowned<'_>,
    _activity: JObject<'_>,
) -> jstring {
    env.with_env(|env| env.new_string(text_snapshot().value).map(JString::into_raw))
        .resolve::<LogErrorAndDefault>()
}

/// Queues a complete native text value for the Guest ABI.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_tela_mobile_TelaActivity_nativeSetInputValue(
    mut env: EnvUnowned<'_>,
    _activity: JObject<'_>,
    value: JString<'_>,
) -> jboolean {
    env.with_env(|env| -> jni::errors::Result<jboolean> {
        let value = java_string(env, &value)?;
        let changed = bridge_lock().text.accept_native_value(value.clone());
        Ok(jboolean::from(
            changed && send_host_event(HostEvent::SetInputValue(value)),
        ))
    })
    .resolve::<LogErrorAndDefault>()
}

/// Forwards native input focus without encoding any Android view details into the guest.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_tela_mobile_TelaActivity_nativeInputFocus(
    env: EnvUnowned<'_>,
    _activity: JObject<'_>,
) -> jboolean {
    guarded_jni(env, || {
        jboolean::from(send_host_event(HostEvent::InputFocus))
    })
}

/// Forwards native input blur without encoding any Android view details into the guest.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_tela_mobile_TelaActivity_nativeInputBlur(
    env: EnvUnowned<'_>,
    _activity: JObject<'_>,
) -> jboolean {
    guarded_jni(env, || {
        jboolean::from(send_host_event(HostEvent::InputBlur))
    })
}

/// Forwards the IME completion action as the guest's platform-neutral Enter event.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_tela_mobile_TelaActivity_nativeInputEnter(
    env: EnvUnowned<'_>,
    _activity: JObject<'_>,
) -> jboolean {
    guarded_jni(env, || {
        jboolean::from(send_host_event(HostEvent::InputEnter))
    })
}

/// Marks the start of a native IME composition segment.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_tela_mobile_TelaActivity_nativeCompositionStart(
    env: EnvUnowned<'_>,
    _activity: JObject<'_>,
) -> jboolean {
    guarded_jni(env, || {
        jboolean::from(
            bridge_lock().text.begin_composition() && send_host_event(HostEvent::CompositionStart),
        )
    })
}

/// Marks the end of a native IME composition segment.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_tela_mobile_TelaActivity_nativeCompositionEnd(
    env: EnvUnowned<'_>,
    _activity: JObject<'_>,
) -> jboolean {
    guarded_jni(env, || {
        jboolean::from(
            bridge_lock().text.end_composition() && send_host_event(HostEvent::CompositionEnd),
        )
    })
}

/// Handles Android system Back: blur the text channel first, otherwise ask the guest to escape.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_tela_mobile_TelaActivity_nativeSystemBack(
    env: EnvUnowned<'_>,
    _activity: JObject<'_>,
) -> jint {
    guarded_jni(env, || {
        let focused = text_snapshot().focused;
        if !send_host_event(HostEvent::SystemBack) {
            return 0;
        }
        if focused {
            BACK_BLURRED_TEXT_INPUT
        } else {
            BACK_DISPATCHED_TO_GUEST
        }
    })
}

/// Lets Kotlin finish the Activity only after root-level guest Escape was explicitly unhandled.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_tela_mobile_TelaActivity_nativeConsumeFinishRequested(
    env: EnvUnowned<'_>,
    _activity: JObject<'_>,
) -> jboolean {
    guarded_jni(env, || jboolean::from(consume_finish_request()))
}

fn guarded_jni<T: Default>(mut env: EnvUnowned<'_>, operation: impl FnOnce() -> T) -> T {
    env.with_env(|_| -> jni::errors::Result<T> { Ok(operation()) })
        .resolve::<LogErrorAndDefault>()
}
