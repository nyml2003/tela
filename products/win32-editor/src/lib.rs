//! Win32 静态文本编辑器产品装配根。
//!
//! 明确选择编辑器应用、受控文本/Material 图标资源与 Win32 静态壳；不引入 bundle、WASM
//! ABI 或 guest executor。桥按静态路径语义（进程内 dispatcher）为关于页提供构建信息。
//! 壳协议由 `tela_app_runtime::Application` 一次性实现，产品只装配资源、
//! 控制器与配置。

#![warn(missing_docs)]

#[cfg(target_os = "windows")]
use tela_app_runtime::{Application, ApplicationConfig};
#[cfg(target_os = "windows")]
use tela_contract::UiResourceSet;
#[cfg(target_os = "windows")]
use tela_desktop_runtime::bridge::common::BuildConstants;
#[cfg(target_os = "windows")]
use tela_icon_resources::MaterialIconFontProvider;
#[cfg(target_os = "windows")]
use tela_target_win32::{NativeWindowOptions, WindowMetrics, build_dispatcher, run_native_window};
#[cfg(target_os = "windows")]
use tela_text_resources::{CONTROLLED_FONT_CATALOG, ControlledTextMeasurer};
#[cfg(target_os = "windows")]
use tela_win32_editor::{EditorController, FOCUS_APPEARANCE};

#[cfg(target_os = "windows")]
const APP_NAME: &str = "Tela 文本编辑器";
#[cfg(target_os = "windows")]
const APP_VERSION: &str = "0.1.0";
#[cfg(target_os = "windows")]
const BUNDLE_VERSION: &str = "0.1.0";

#[cfg(target_os = "windows")]
fn app_build_id() -> u32 {
    option_env!("TELA_APP_BUILD_ID")
        .and_then(|value| value.parse().ok())
        .unwrap_or(1)
}

#[cfg(target_os = "windows")]
fn bundle_build_id() -> u32 {
    option_env!("TELA_BUNDLE_BUILD_ID")
        .and_then(|value| value.parse().ok())
        .unwrap_or(1)
}

#[cfg(target_os = "windows")]
static RESOURCES: UiResourceSet<ControlledTextMeasurer, MaterialIconFontProvider> =
    UiResourceSet::new(ControlledTextMeasurer, MaterialIconFontProvider)
        .with_fonts(CONTROLLED_FONT_CATALOG);

/// 启动 Win32 静态编辑器窗口（阻塞至窗口关闭）。
#[cfg(target_os = "windows")]
pub fn run() -> Result<(), String> {
    eprintln!(
        "tela-win32-editor: build app_name=\"{}\" app_version={} app_build_id={} bundle_version={} bundle_build_id={}",
        APP_NAME,
        APP_VERSION,
        app_build_id(),
        BUNDLE_VERSION,
        bundle_build_id()
    );
    let dispatcher = build_dispatcher(
        std::rc::Rc::new(std::cell::RefCell::new(WindowMetrics::default())),
        &BuildConstants {
            app_name: APP_NAME.to_owned(),
            app_version: tela_utils::Version::new(0, 1, 0),
            app_build_id: app_build_id(),
            bundle_version: tela_utils::Version::new(0, 1, 0),
            bundle_build_id: bundle_build_id(),
        },
        vec![],
    );
    let controller = EditorController::new(&RESOURCES, dispatcher);
    let application = Application::new(
        &RESOURCES,
        controller,
        ApplicationConfig {
            focus_appearance: Some(FOCUS_APPEARANCE),
            ..ApplicationConfig::default()
        },
    );
    run_native_window(
        Box::new(application),
        NativeWindowOptions::new(APP_NAME).size(960, 640),
    )
}

#[cfg(test)]
mod tests {
    use tela_app_runtime::{Application, ApplicationConfig};
    use tela_app_session::ApplicationSession;
    use tela_bridge::BridgeDispatcher;
    use tela_contract::{Color, UiResourceSet, Viewport};
    use tela_icon_resources::MaterialIconFontProvider;
    use tela_render_wgpu::WgpuRenderer;
    use tela_text_resources::{CONTROLLED_FONT_CATALOG, ControlledTextMeasurer};
    use tela_win32_editor::{EditorController, FOCUS_APPEARANCE};

    const WIDTH: u32 = 320;
    const HEIGHT: u32 = 180;

    static TEST_RESOURCES: UiResourceSet<ControlledTextMeasurer, MaterialIconFontProvider> =
        UiResourceSet::new(ControlledTextMeasurer, MaterialIconFontProvider)
            .with_fonts(CONTROLLED_FONT_CATALOG);

    fn device_and_queue() -> (wgpu::Device, wgpu::Queue) {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            flags: wgpu::InstanceFlags::default(),
            memory_budget_thresholds: Default::default(),
            backend_options: Default::default(),
            display: None,
        });
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            compatible_surface: None,
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        }))
        .expect("an offscreen WGPU adapter is required for the visual regression");
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("tela win32 editor visual regression"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            experimental_features: Default::default(),
            memory_hints: wgpu::MemoryHints::default(),
            trace: wgpu::Trace::Off,
        }))
        .expect("create offscreen WGPU device")
    }

    fn pixels(device: &wgpu::Device, queue: &wgpu::Queue, texture: &wgpu::Texture) -> Vec<u8> {
        let bytes_per_row = WIDTH * 4;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("tela win32 editor visual readback"),
            size: (bytes_per_row * HEIGHT) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(bytes_per_row),
                    rows_per_image: Some(HEIGHT),
                },
            },
            wgpu::Extent3d {
                width: WIDTH,
                height: HEIGHT,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(Some(encoder.finish()));
        buffer.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("wait for visual readback");
        let mapped = buffer
            .slice(..)
            .get_mapped_range()
            .expect("map visual readback");
        let result = mapped.to_vec();
        drop(mapped);
        buffer.unmap();
        result
    }

    #[test]
    #[ignore = "requires nix develop .#render-wgpu; runs the assembled editor through WGPU"]
    fn editor_first_frame_reaches_a_direct_wgpu_target() {
        let mut application = Application::new(
            &TEST_RESOURCES,
            EditorController::new(&TEST_RESOURCES, BridgeDispatcher::new()),
            ApplicationConfig {
                initial_viewport: Viewport {
                    width: WIDTH as f32,
                    height: HEIGHT as f32,
                },
                focus_appearance: Some(FOCUS_APPEARANCE),
                ..ApplicationConfig::default()
            },
        );
        let publication = ApplicationSession::publish(&mut application)
            .expect("assemble the editor's first published frame");

        let (device, queue) = device_and_queue();
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("tela win32 editor direct target"),
            size: wgpu::Extent3d {
                width: WIDTH,
                height: HEIGHT,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut renderer = WgpuRenderer::new(
            device.clone(),
            queue.clone(),
            wgpu::TextureFormat::Rgba8Unorm,
            Color::WHITE,
        );
        renderer.render_frame(&publication.frame, &view, WIDTH, HEIGHT);
        assert!(
            renderer.last_stats().draw_calls > 0,
            "the assembled editor frame must encode draw calls"
        );

        let pixels = pixels(&device, &queue, &texture);
        let top_band_has_non_white_pixel = (0..34).any(|y| {
            (0..WIDTH).any(|x| {
                let index = ((y * WIDTH + x) * 4) as usize;
                pixels[index] < 250 || pixels[index + 1] < 250 || pixels[index + 2] < 250
            })
        });
        assert!(
            top_band_has_non_white_pixel,
            "the editor title bar must produce visible pixels on a direct WGPU target"
        );
    }
}
