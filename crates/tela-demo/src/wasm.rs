//! WebGPU 浏览器入口。
//!
//! 它只负责把共享的 `crate::scene_frame()` 提交到 WGPU surface；场景构建不在此处复制。

use std::cell::RefCell;

use wasm_bindgen::prelude::*;
use web_sys::HtmlCanvasElement;

use crate::{VIEWPORT, with_app};

thread_local! {
    static GPU: RefCell<Option<GpuSession>> = const { RefCell::new(None) };
}

struct GpuSession {
    renderer: tela_render_wgpu::WgpuRenderer,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    canvas: HtmlCanvasElement,
    last_status: &'static str,
}

/// 供页面识别 WASM 构建的版本号。
#[wasm_bindgen]
pub fn wasm_version() -> String {
    format!(
        "tela-demo {} (build {})",
        env!("CARGO_PKG_VERSION"),
        option_env!("TELA_BUILD_TS").unwrap_or("dev")
    )
}

/// 初始化 WebGPU surface 与 tela WGPU renderer。
#[wasm_bindgen]
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
        .map_err(|error| JsValue::from_str(&format!("surface 创建失败: {error}")))?;
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        })
        .await
        .map_err(|error| JsValue::from_str(&format!("无可用 WebGPU 适配器: {error}")))?;
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("tela minimal device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            experimental_features: Default::default(),
            memory_hints: wgpu::MemoryHints::default(),
            trace: wgpu::Trace::Off,
        })
        .await
        .map_err(|error| JsValue::from_str(&format!("设备创建失败: {error}")))?;
    device.on_uncaptured_error(std::sync::Arc::new(|error| {
        web_sys::console::error_1(&JsValue::from_str(&format!("tela wgpu: {error}")));
    }));
    let config = surface
        .get_default_config(&adapter, canvas.width().max(1), canvas.height().max(1))
        .ok_or_else(|| JsValue::from_str("surface 无默认配置"))?;
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
            return Err(JsValue::from_str("GPU 会话重复初始化"));
        }
        *slot = Some(GpuSession {
            renderer,
            surface,
            config,
            canvas,
            last_status: "initialized",
        });
        Ok(())
    })
}

/// 将缓存的共享 tela 帧提交到 surface。浏览器 canvas 必须持续 present，即使场景未更新。
/// 返回 1 表示实际提交，0 表示 surface 尚不可用；详情见 `gpu_diagnostics`。
#[wasm_bindgen]
pub fn tick_gpu() -> u32 {
    let submitted = GPU.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(session) = slot.as_mut() else {
            return false;
        };
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
            wgpu::CurrentSurfaceTexture::Success(texture) => {
                session.last_status = "submitted";
                texture
            }
            wgpu::CurrentSurfaceTexture::Suboptimal(texture) => {
                session.last_status = "submitted (suboptimal)";
                texture
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                session.last_status = "surface outdated; reconfigured";
                session
                    .surface
                    .configure(session.renderer.device(), &session.config);
                return false;
            }
            wgpu::CurrentSurfaceTexture::Lost => {
                session.last_status = "surface lost; reconfigured";
                session
                    .surface
                    .configure(session.renderer.device(), &session.config);
                return false;
            }
            wgpu::CurrentSurfaceTexture::Timeout => {
                session.last_status = "surface timeout";
                return false;
            }
            wgpu::CurrentSurfaceTexture::Occluded => {
                session.last_status = "surface occluded";
                return false;
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                session.last_status = "surface validation error";
                return false;
            }
        };
        let view = texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        with_app(|app| {
            app.ensure_frame();
            session
                .renderer
                .render_frame(app.frame(), &view, width, height);
        });
        session.renderer.present(texture);
        true
    });
    u32::from(submitted)
}

/// 逻辑画布对应的浏览器 canvas 尺寸。
#[wasm_bindgen]
pub fn frame_size() -> u32 {
    VIEWPORT.width as u32 | ((VIEWPORT.height as u32) << 16)
}

/// 从缓存的共享 `UiFrame` 读取结构化 JSON；不重新构建场景。
#[wasm_bindgen]
pub fn frame_trace() -> String {
    with_app(|app| {
        app.ensure_frame();
        std::str::from_utf8(app.frame_trace())
            .expect("frame trace 必须是 UTF-8")
            .to_owned()
    })
}

/// 最近一次真实场景帧的 WGPU 编码统计。
#[wasm_bindgen]
pub fn gpu_diagnostics() -> String {
    GPU.with(|slot| {
        let slot = slot.borrow();
        let Some(session) = slot.as_ref() else {
            return "GPU 会话未启动".to_owned();
        };
        let stats = session.renderer.last_stats();
        format!(
            "status={} commands={} batches={} draw_calls={} vertices={} indices={} unsupported={} ignored_borders={}; {}",
            session.last_status,
            stats.commands,
            stats.batches,
            stats.draw_calls,
            stats.vertices,
            stats.indices,
            stats.unsupported_commands,
            stats.ignored_borders,
            session.renderer.last_diagnostics(),
        )
    })
}

/// 离屏 probe：读取 main 左上圆角外侧像素，验证共享 RoundedRect 已由 WGPU 呈现。
#[wasm_bindgen]
pub async fn gpu_probe() -> Result<u32, JsValue> {
    const PROBE_WIDTH: u32 = 512;
    const PROBE_HEIGHT: u32 = 384;
    // 物理像素约对应逻辑坐标 (242, 57)，位于半径 18 的左上圆角外侧。
    const ROUNDED_CORNER_PROBE_X: u32 = 258;
    const ROUNDED_CORNER_PROBE_Y: u32 = 61;
    let (device, queue, format) = GPU.with(|slot| {
        let slot = slot.borrow();
        let session = slot
            .as_ref()
            .ok_or_else(|| JsValue::from_str("GPU 会话未启动"))?;
        Ok::<_, JsValue>((
            session.renderer.device().clone(),
            session.renderer.queue().clone(),
            session.renderer.surface_format(),
        ))
    })?;
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("tela shared-scene probe"),
        size: wgpu::Extent3d {
            width: PROBE_WIDTH,
            height: PROBE_HEIGHT,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    // probe 覆盖完整逻辑画布；512 像素宽也保证 RGBA 行满足 256 字节对齐。
    let frame = with_app(|app| {
        app.ensure_frame();
        app.frame().clone()
    });
    GPU.with(|slot| {
        let mut slot = slot.borrow_mut();
        let session = slot
            .as_mut()
            .ok_or_else(|| JsValue::from_str("GPU 会话未启动"))?;
        session
            .renderer
            .render_frame(&frame, &view, PROBE_WIDTH, PROBE_HEIGHT);
        Ok::<_, JsValue>(())
    })?;

    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("tela probe readback"),
        size: (PROBE_WIDTH * PROBE_HEIGHT * 4) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(PROBE_WIDTH * 4),
                rows_per_image: Some(PROBE_HEIGHT),
            },
        },
        wgpu::Extent3d {
            width: PROBE_WIDTH,
            height: PROBE_HEIGHT,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(Some(encoder.finish()));
    let slice = buffer.slice(..);
    let promise = js_sys::Promise::new(&mut |resolve, reject| {
        let resolve = resolve.clone();
        let reject = reject.clone();
        slice.map_async(wgpu::MapMode::Read, move |result| match result {
            Ok(()) => {
                let _ = resolve.call0(&JsValue::UNDEFINED);
            }
            Err(error) => {
                let _ = reject.call1(
                    &JsValue::UNDEFINED,
                    &JsValue::from_str(&format!("probe map 失败: {error:?}")),
                );
            }
        });
    });
    wasm_bindgen_futures::JsFuture::from(promise)
        .await
        .map_err(|error| JsValue::from_str(&format!("probe 回读失败: {error:?}")))?;
    let range = slice
        .get_mapped_range()
        .map_err(|error| JsValue::from_str(&format!("probe 读取失败: {error:?}")))?;
    let index = ((ROUNDED_CORNER_PROBE_Y * PROBE_WIDTH + ROUNDED_CORNER_PROBE_X) * 4) as usize;
    // WebGPU 常用的 canvas 格式是 BGRA；readback 的内存顺序跟随 texture format，
    // 这里统一向页面导出 RGB，保证 probe 与 tela_contract::Color 一致。
    let rgb = match format {
        wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb => {
            (range[index + 2], range[index + 1], range[index])
        }
        _ => (range[index], range[index + 1], range[index + 2]),
    };
    drop(range);
    buffer.unmap();
    Ok(((rgb.0 as u32) << 16) | ((rgb.1 as u32) << 8) | rgb.2 as u32)
}
