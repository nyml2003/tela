//! Browser WebView development SDK host.
//!
//! This crate owns the Rust half of the browser shell: development-bundle validation, Tela ABI
//! packet codecs, `UiFrame` decoding, and WGPU presentation. The DOM event loop, browser fetches
//! and ordinary `WebAssembly` guest instantiation stay in `products/webview/src/webview-sdk`, where browser
//! lifetime and IME APIs can remain explicit.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

#[cfg(target_arch = "wasm32")]
use std::cell::RefCell;

#[cfg(target_arch = "wasm32")]
use tela_app_abi::decode_frame;
use tela_app_abi::{
    ABI_VERSION, AppEvent, AppFrameInput, AppFrameToken, AppPointerEvent, AppPointerKind,
    AppPointerPhase, AppStatus, decode_status, encode_event,
};
use tela_bundle::{BundleArchive, DevelopmentManifest, read_archive, sha256_hex};
use wasm_bindgen::prelude::*;
#[cfg(target_arch = "wasm32")]
use web_sys::HtmlCanvasElement;

const MAX_ARCHIVE_BYTES: usize = 64 * 1024 * 1024;

#[cfg(target_arch = "wasm32")]
thread_local! {
    static GPU: RefCell<Option<GpuSession>> = const { RefCell::new(None) };
}

/// A validated development index which the JavaScript loader can use to resolve the archive URL.
#[wasm_bindgen]
pub struct DevelopmentBundleIndex {
    manifest: DevelopmentManifest,
}

/// A development bundle whose archive and internal manifest have both been validated.
#[wasm_bindgen]
pub struct ValidatedBundle {
    archive: BundleArchive,
}

/// Browser-visible non-drawing state decoded from an application ABI status packet.
#[wasm_bindgen]
pub struct WebAppStatus {
    status: AppStatus,
}

#[cfg(target_arch = "wasm32")]
struct GpuSession {
    renderer: tela_render_wgpu::WgpuRenderer,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    canvas: HtmlCanvasElement,
    last_status: String,
}

/// Returns the application ABI version implemented by this host.
#[wasm_bindgen]
pub fn host_app_abi_version() -> u32 {
    ABI_VERSION
}

/// Parses and validates the one development index fetched at browser-shell startup.
///
/// The caller still resolves `bundle_url` with the browser URL API, because that is the platform
/// authority for relative URLs. This function owns every format, ABI and size validation.
#[wasm_bindgen]
pub fn parse_development_index(bytes: &[u8]) -> Result<DevelopmentBundleIndex, JsValue> {
    let manifest: DevelopmentManifest = serde_json::from_slice(bytes)
        .map_err(|error| js_error(format!("invalid development index: {error}")))?;
    validate_development_index(&manifest).map_err(js_error)?;
    Ok(DevelopmentBundleIndex { manifest })
}

/// Validates one downloaded archive against its already-validated development index.
#[wasm_bindgen]
pub fn validate_development_bundle(
    index: &DevelopmentBundleIndex,
    bytes: &[u8],
) -> Result<ValidatedBundle, JsValue> {
    validate_development_bundle_impl(index, bytes).map_err(js_error)
}

#[wasm_bindgen]
impl DevelopmentBundleIndex {
    /// The archive URL declared by the validated development index.
    #[wasm_bindgen(getter)]
    pub fn bundle_url(&self) -> String {
        self.manifest.bundle_url.clone()
    }

    /// The content-addressed archive identifier used for diagnostics.
    #[wasm_bindgen(getter)]
    pub fn bundle_id(&self) -> String {
        self.manifest.bundle_id.clone()
    }
}

#[wasm_bindgen]
impl ValidatedBundle {
    /// Returns a fresh copy of the validated guest application module.
    pub fn app_wasm(&self) -> Vec<u8> {
        self.archive.app_wasm.clone()
    }

    /// Returns the internal bundle content identifier for diagnostics.
    #[wasm_bindgen(getter)]
    pub fn bundle_id(&self) -> String {
        self.archive.manifest.bundle_id.clone()
    }
}

#[wasm_bindgen]
impl WebAppStatus {
    /// Frame identity eligible for input after the browser has actually presented it.
    ///
    /// `undefined` on the JavaScript side means the guest has not published an interactive frame.
    #[wasm_bindgen(getter)]
    pub fn frame_token(&self) -> Option<u64> {
        self.status.frame_token.map(AppFrameToken::get)
    }

    /// Cursor request encoded as `0 = default`, `1 = text`, `2 = pointer`.
    #[wasm_bindgen(getter)]
    pub fn cursor(&self) -> u8 {
        self.status.cursor as u8
    }

    /// Whether the guest currently requests the platform text-input channel.
    #[wasm_bindgen(getter)]
    pub fn input_focused(&self) -> bool {
        self.status.input_focused
    }

    /// Controlled visible value for the active text input.
    #[wasm_bindgen(getter)]
    pub fn input_value(&self) -> String {
        self.status.input_value.clone()
    }
}

/// Decodes guest status after its bytes crossed the ordinary WebAssembly boundary.
#[wasm_bindgen]
pub fn decode_app_status(bytes: &[u8]) -> Result<WebAppStatus, JsValue> {
    decode_status(bytes)
        .map(|status| WebAppStatus { status })
        .map_err(|error| js_error(format!("invalid guest status packet: {error}")))
}

/// Encodes a logical viewport event for the guest.
#[wasm_bindgen]
pub fn event_viewport(width: f32, height: f32) -> Result<Vec<u8>, JsValue> {
    encode_host_event(AppEvent::Viewport { width, height })
}

/// Encodes one complete raw browser pointer packet for the guest.
///
/// `kind` uses `0 = mouse`, `1 = touch`, `2 = pen`; `phase` uses `0 = down`, `1 = move`,
/// `2 = up`, `3 = cancel`, `4 = scroll`. JavaScript must forward browser `pointerId`, `buttons`
/// and `timeStamp` unchanged after coordinate conversion. It must not infer click or drag.
#[wasm_bindgen]
#[allow(clippy::too_many_arguments)]
pub fn event_pointer(
    source_frame_token: u64,
    pointer_id: u64,
    kind: u8,
    phase: u8,
    x: f32,
    y: f32,
    buttons: u16,
    timestamp_micros: u64,
    delta_x: f32,
    delta_y: f32,
) -> Result<Vec<u8>, JsValue> {
    let kind = match kind {
        0 => AppPointerKind::Mouse,
        1 => AppPointerKind::Touch,
        2 => AppPointerKind::Pen,
        value => {
            return Err(js_error(format!(
                "unsupported browser pointer kind: {value}"
            )));
        }
    };
    let phase = match phase {
        0 => AppPointerPhase::Down,
        1 => AppPointerPhase::Move,
        2 => AppPointerPhase::Up,
        3 => AppPointerPhase::Cancel,
        4 => AppPointerPhase::Scroll,
        value => {
            return Err(js_error(format!(
                "unsupported browser pointer phase: {value}"
            )));
        }
    };
    encode_frame_input(
        source_frame_token,
        AppFrameInput::Pointer(AppPointerEvent::new(
            pointer_id,
            kind,
            phase,
            x,
            y,
            buttons,
            timestamp_micros,
            delta_x,
            delta_y,
        )),
    )
}

/// Encodes a normalized physical-key event for the guest keymap.
#[wasm_bindgen]
pub fn event_key_down(
    source_frame_token: u64,
    physical_key: u16,
    modifier_bits: u8,
    repeat: bool,
) -> Result<Vec<u8>, JsValue> {
    encode_frame_input(
        source_frame_token,
        AppFrameInput::KeyDown {
            physical_key,
            modifier_bits,
            repeat,
        },
    )
}

/// Encodes a controlled text-input value replacement for the guest.
#[wasm_bindgen]
pub fn event_set_input_value(source_frame_token: u64, value: String) -> Result<Vec<u8>, JsValue> {
    encode_frame_input(source_frame_token, AppFrameInput::SetInputValue(value))
}

/// Encodes a platform text-channel focus event for the guest.
#[wasm_bindgen]
pub fn event_input_focus(source_frame_token: u64) -> Result<Vec<u8>, JsValue> {
    encode_frame_input(source_frame_token, AppFrameInput::InputFocus)
}

/// Encodes a platform text-channel blur event for the guest.
#[wasm_bindgen]
pub fn event_input_blur(source_frame_token: u64) -> Result<Vec<u8>, JsValue> {
    encode_frame_input(source_frame_token, AppFrameInput::InputBlur)
}

/// Encodes an explicit text-input confirmation for the guest.
#[wasm_bindgen]
pub fn event_input_enter(source_frame_token: u64) -> Result<Vec<u8>, JsValue> {
    encode_frame_input(source_frame_token, AppFrameInput::InputEnter)
}

/// Encodes an explicit text-input cancellation for the guest.
#[wasm_bindgen]
pub fn event_input_cancel(source_frame_token: u64) -> Result<Vec<u8>, JsValue> {
    encode_frame_input(source_frame_token, AppFrameInput::InputCancel)
}

/// Encodes an IME composition-start marker for the guest.
#[wasm_bindgen]
pub fn event_input_composition_start(source_frame_token: u64) -> Result<Vec<u8>, JsValue> {
    encode_frame_input(source_frame_token, AppFrameInput::InputCompositionStart)
}

/// Encodes an IME composition-end marker for the guest.
#[wasm_bindgen]
pub fn event_input_composition_end(source_frame_token: u64) -> Result<Vec<u8>, JsValue> {
    encode_frame_input(source_frame_token, AppFrameInput::InputCompositionEnd)
}

/// Encodes a validated-at-guest runtime keymap replacement request.
#[wasm_bindgen]
pub fn event_replace_keymap_json(json: String) -> Result<Vec<u8>, JsValue> {
    encode_host_event(AppEvent::ReplaceKeymapJson(json))
}

/// Initializes the one WGPU browser surface used by the current WebView session.
#[wasm_bindgen]
#[cfg(target_arch = "wasm32")]
pub async fn start_gpu(canvas: HtmlCanvasElement) -> Result<(), JsValue> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::BROWSER_WEBGPU,
        flags: wgpu::InstanceFlags::default(),
        memory_budget_thresholds: Default::default(),
        backend_options: Default::default(),
        display: None,
    });
    let surface = instance
        .create_surface(wgpu::SurfaceTarget::Canvas(canvas.clone()))
        .map_err(|error| js_error(format!("create WebGPU surface: {error}")))?;
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        })
        .await
        .map_err(|error| js_error(format!("request WebGPU adapter: {error}")))?;
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("tela WebView device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            experimental_features: Default::default(),
            memory_hints: wgpu::MemoryHints::default(),
            trace: wgpu::Trace::Off,
        })
        .await
        .map_err(|error| js_error(format!("create WebGPU device: {error}")))?;
    device.on_uncaptured_error(std::sync::Arc::new(|error| {
        web_sys::console::error_1(&JsValue::from_str(&format!("tela WebView WGPU: {error}")));
    }));
    let config = surface
        .get_default_config(&adapter, canvas.width().max(1), canvas.height().max(1))
        .ok_or_else(|| js_error("WebGPU surface has no default configuration"))?;
    let format = config.format;
    surface.configure(&device, &config);
    let renderer = tela_render_wgpu::WgpuRenderer::new(
        device,
        queue,
        format,
        tela_contract::Color::rgba(1.0, 1.0, 1.0, 1.0),
    );
    GPU.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_some() {
            return Err(js_error("WebGPU session is already initialized"));
        }
        *slot = Some(GpuSession {
            renderer,
            surface,
            config,
            canvas,
            last_status: "initialized".to_owned(),
        });
        Ok(())
    })
}

/// Decodes one guest frame packet, renders it, and presents it to the WebView canvas.
///
/// Returns `true` only when a texture was acquired and submitted. Recoverable surface states are
/// retained in [`gpu_diagnostics`] and return `false` so the JavaScript animation loop can retry.
#[wasm_bindgen]
#[cfg(target_arch = "wasm32")]
pub fn render_gpu(frame_packet: &[u8]) -> Result<bool, JsValue> {
    let frame = decode_frame(frame_packet)
        .map_err(|error| js_error(format!("invalid guest frame packet: {error}")))?;
    GPU.with(|slot| {
        let mut slot = slot.borrow_mut();
        let session = slot
            .as_mut()
            .ok_or_else(|| js_error("WebGPU session is not initialized"))?;
        let width = session.canvas.width().max(1);
        let height = session.canvas.height().max(1);
        if session.config.width != width || session.config.height != height {
            session.config.width = width;
            session.config.height = height;
            session
                .surface
                .configure(session.renderer.device(), &session.config);
        }
        let texture = match session.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture) => texture,
            wgpu::CurrentSurfaceTexture::Suboptimal(texture) => {
                session.last_status = "submitted (suboptimal)".to_owned();
                texture
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                session.last_status = "surface outdated; reconfigured".to_owned();
                session
                    .surface
                    .configure(session.renderer.device(), &session.config);
                return Ok(false);
            }
            wgpu::CurrentSurfaceTexture::Lost => {
                session.last_status = "surface lost; reconfigured".to_owned();
                session
                    .surface
                    .configure(session.renderer.device(), &session.config);
                return Ok(false);
            }
            wgpu::CurrentSurfaceTexture::Timeout => {
                session.last_status = "surface timeout".to_owned();
                return Ok(false);
            }
            wgpu::CurrentSurfaceTexture::Occluded => {
                session.last_status = "surface occluded".to_owned();
                return Ok(false);
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                session.last_status = "surface validation error".to_owned();
                return Ok(false);
            }
        };
        let view = texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        session.renderer.render_frame(&frame, &view, width, height);
        session.renderer.present(texture);
        session.last_status = "submitted".to_owned();
        Ok(true)
    })
}

/// Releases the WGPU surface and renderer for a closing browser WebView session.
#[wasm_bindgen]
#[cfg(target_arch = "wasm32")]
pub fn shutdown_gpu() {
    GPU.with(|slot| {
        slot.borrow_mut().take();
    });
}

/// Returns the latest renderer and surface diagnostic string for developer-visible failures.
#[wasm_bindgen]
#[cfg(target_arch = "wasm32")]
pub fn gpu_diagnostics() -> String {
    GPU.with(|slot| {
        let slot = slot.borrow();
        let Some(session) = slot.as_ref() else {
            return "WebGPU session is not initialized".to_owned();
        };
        let stats = session.renderer.last_stats();
        format!(
            "status={} commands={} batches={} draw_calls={} vertices={} indices={} unsupported={} missing_images={} ignored_borders={}; {}",
            session.last_status,
            stats.commands,
            stats.batches,
            stats.draw_calls,
            stats.vertices,
            stats.indices,
            stats.unsupported_commands,
            stats.missing_images,
            stats.ignored_borders,
            session.renderer.last_diagnostics(),
        )
    })
}

fn validate_development_index(manifest: &DevelopmentManifest) -> Result<(), String> {
    manifest.validate().map_err(|error| error.to_string())?;
    if manifest.app_abi != ABI_VERSION {
        return Err(format!(
            "app ABI mismatch: host={ABI_VERSION}, bundle={}",
            manifest.app_abi
        ));
    }
    if manifest.bytes > MAX_ARCHIVE_BYTES as u64 {
        return Err(format!(
            "bundle exceeds {} MiB limit",
            MAX_ARCHIVE_BYTES / 1024 / 1024
        ));
    }
    Ok(())
}

fn encode_host_event(event: AppEvent) -> Result<Vec<u8>, JsValue> {
    encode_event(&event).map_err(|error| js_error(format!("encode host event: {error}")))
}

fn encode_frame_input(
    raw_source_frame_token: u64,
    input: AppFrameInput,
) -> Result<Vec<u8>, JsValue> {
    let event = frame_input_event(raw_source_frame_token, input).map_err(js_error)?;
    encode_host_event(event)
}

fn frame_input_event(
    raw_source_frame_token: u64,
    input: AppFrameInput,
) -> Result<AppEvent, String> {
    let source_frame_token = AppFrameToken::new(raw_source_frame_token)
        .ok_or_else(|| "frame-owned input requires a non-zero presented frame token".to_owned())?;
    Ok(AppEvent::FrameInput {
        source_frame_token,
        input,
    })
}

fn js_error(message: impl AsRef<str>) -> JsValue {
    JsValue::from_str(message.as_ref())
}

fn validate_development_bundle_impl(
    index: &DevelopmentBundleIndex,
    bytes: &[u8],
) -> Result<ValidatedBundle, String> {
    if bytes.len() > MAX_ARCHIVE_BYTES {
        return Err(format!(
            "bundle exceeds {} MiB limit",
            MAX_ARCHIVE_BYTES / 1024 / 1024
        ));
    }
    if bytes.len() as u64 != index.manifest.bytes {
        return Err(format!(
            "archive size mismatch: expected {}, got {}",
            index.manifest.bytes,
            bytes.len()
        ));
    }
    let actual_hash = sha256_hex(bytes);
    if actual_hash != index.manifest.sha256 {
        return Err(format!(
            "archive checksum mismatch: expected {}, got {actual_hash}",
            index.manifest.sha256
        ));
    }
    let archive = read_archive(bytes).map_err(|error| error.to_string())?;
    if archive.manifest.app_abi != ABI_VERSION {
        return Err(format!(
            "app ABI mismatch: host={ABI_VERSION}, archive={}",
            archive.manifest.app_abi
        ));
    }
    if archive.manifest.app_abi != index.manifest.app_abi {
        return Err(format!(
            "development index/archive ABI mismatch: index={}, archive={}",
            index.manifest.app_abi, archive.manifest.app_abi
        ));
    }
    Ok(ValidatedBundle { archive })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use tela_app_abi::decode_event;
    use tela_bundle::{BUNDLE_FORMAT_VERSION, BundleInput, build_archive, sha256_hex};

    use super::*;

    fn index_for(archive: &[u8]) -> DevelopmentManifest {
        DevelopmentManifest {
            format_version: BUNDLE_FORMAT_VERSION,
            bundle_id: sha256_hex(archive),
            bundle_url: "/tela-dev/app.tela".to_owned(),
            sha256: sha256_hex(archive),
            bytes: archive.len() as u64,
            app_abi: ABI_VERSION,
        }
    }

    #[test]
    fn validates_index_and_matching_archive_before_exposing_guest_bytes() {
        let archive = build_archive(&BundleInput {
            app_abi: ABI_VERSION,
            app_wasm: b"guest".to_vec(),
            assets: BTreeMap::new(),
        })
        .expect("archive");
        let index = index_for(&archive);
        validate_development_index(&index).expect("index");
        let index = DevelopmentBundleIndex { manifest: index };
        let bundle = validate_development_bundle_impl(&index, &archive).expect("bundle");
        assert_eq!(bundle.archive.app_wasm, b"guest");
    }

    #[test]
    fn rejects_archive_with_a_different_hash() {
        let archive = build_archive(&BundleInput {
            app_abi: ABI_VERSION,
            app_wasm: b"guest".to_vec(),
            assets: BTreeMap::new(),
        })
        .expect("archive");
        let index = DevelopmentBundleIndex {
            manifest: index_for(&archive),
        };
        let mut corrupt = archive;
        corrupt.push(0);
        assert!(validate_development_bundle_impl(&index, &corrupt).is_err());
    }

    #[test]
    fn frame_input_encoder_requires_a_non_zero_presented_token() {
        let event = frame_input_event(7, AppFrameInput::InputCancel).expect("construct input");
        let packet = tela_app_abi::encode_event(&event).expect("encode input");
        assert_eq!(
            decode_event(&packet).expect("decode input"),
            AppEvent::FrameInput {
                source_frame_token: AppFrameToken::new(7).expect("non-zero token"),
                input: AppFrameInput::InputCancel,
            }
        );
        assert!(frame_input_event(0, AppFrameInput::InputCancel).is_err());
    }
}
