//! wgpu 渲染器离屏回读集成测试（lavapipe/llvmpipe 无头可跑，验证命令 → GPU 像素）。
//! 这是"wgpu 什么都没画出来"的回归防护：同一 UiFrame 在离屏纹理回读非背景像素。

use tela_contract::{Color, DrawCommand, DrawPayload, Rect, UiFrame, Viewport};
use tela_render_wgpu::WgpuRenderer;

const W: u32 = 100;
const H: u32 = 100;
fn make_frame() -> UiFrame {
    UiFrame {
        viewport: Viewport {
            width: W as f32,
            height: H as f32,
        },
        commands: vec![
            // 红色矩形（居中 80×80）。
            DrawCommand {
                geometry: Rect {
                    x: 10.0,
                    y: 10.0,
                    w: 80.0,
                    h: 80.0,
                },
                clip: None,
                payload: DrawPayload::Rect {
                    fill: Some(Color::rgba(1.0, 0.0, 0.0, 1.0)),
                    border: None,
                },
            },
        ],
        hit_regions: vec![],
        scroll_bounds: vec![],
    }
}

fn setup() -> (wgpu::Device, wgpu::Queue) {
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
        force_fallback_adapter: true,
        apply_limit_buckets: false,
    }))
    .expect("无可用适配器（需要 lavapipe/llvmpipe 或真实 GPU）");
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("tela test device"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        experimental_features: Default::default(),
        memory_hints: wgpu::MemoryHints::default(),
        trace: wgpu::Trace::Off,
    }))
    .expect("设备创建失败");
    (device, queue)
}

fn read_pixels(device: &wgpu::Device, queue: &wgpu::Queue, texture: &wgpu::Texture) -> Vec<u8> {
    // COPY_BYTES_PER_ROW_ALIGNMENT = 256：bytes_per_row 必须 256 对齐。
    let bytes_per_row = (W * 4).div_ceil(256) * 256;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("tela readback"),
        size: (bytes_per_row * H) as u64,
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
                rows_per_image: Some(H),
            },
        },
        wgpu::Extent3d {
            width: W,
            height: H,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(Some(encoder.finish()));
    buffer.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("poll 等待失败");
    let range = buffer
        .slice(..)
        .get_mapped_range()
        .expect("mapped range 获取失败");
    let data = range.to_vec();
    drop(range);
    buffer.unmap();
    data
}

fn render_to_texture(device: &wgpu::Device, queue: &wgpu::Queue, frame: &UiFrame) -> Vec<u8> {
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("tela offscreen"),
        size: wgpu::Extent3d {
            width: W,
            height: H,
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
    let mut renderer = WgpuRenderer::new(
        device.clone(),
        queue.clone(),
        format,
        Color::rgba(0.0, 0.0, 0.0, 1.0),
    );
    renderer.render_frame(frame, &view, W, H);
    read_pixels(device, queue, &texture)
}

/// 回读链路验证：write_texture 直接写红 → readback 应读到红。
#[test]
fn write_texture_readback_works() {
    let (device, queue) = setup();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("wtex"),
        size: wgpu::Extent3d {
            width: W,
            height: H,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let mut data = vec![0u8; (W * H * 4) as usize];
    for px in data.chunks_mut(4) {
        px[0] = 200;
        px[1] = 0;
        px[2] = 0;
        px[3] = 255;
    }
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &data,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(W * 4),
            rows_per_image: Some(H),
        },
        wgpu::Extent3d {
            width: W,
            height: H,
            depth_or_array_layers: 1,
        },
    );
    let pixels = read_pixels(&device, &queue, &texture);
    let i = ((H / 2 * W + W / 2) * 4) as usize;
    let (r, g, b) = (pixels[i], pixels[i + 1], pixels[i + 2]);
    assert!(
        r > 150 && g < 60 && b < 60,
        "write_texture 回读应红色，实际 ({r},{g},{b})"
    );
}

/// 对照实验：raw wgpu 直接画红色三角形（不经 tela 渲染器），
/// 验证环境渲染链路。本机 lavapipe（nix mesa 26.1.x）draw 不输出像素，
/// 已确认是驱动/环境问题（write_texture 回读链路正常、无 validation error）；
/// 在正常 Vulkan 环境可运行。
#[test]
#[ignore = "lavapipe draw 不可靠（环境），浏览器 gpu_probe 为准"]
fn raw_wgpu_draws() {
    let (device, queue) = setup();
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("raw"),
        size: wgpu::Extent3d {
            width: W,
            height: H,
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
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("raw shader"),
        source: wgpu::ShaderSource::Wgsl(
            "struct VsOut { @builtin(position) p: vec4<f32>, @location(0) c: vec4<f32> }
             @vertex fn vs(@builtin(vertex_index) i: u32) -> VsOut {
                 var o: VsOut;
                 let pos = array<vec2<f32>, 3>(vec2(0.0, 0.5), vec2(-0.5, -0.5), vec2(0.5, -0.5));
                 o.p = vec4(pos[i], 0.0, 1.0);
                 o.c = vec4(1.0, 0.0, 0.0, 1.0);
                 return o;
             }
             @fragment fn fs(in: VsOut) -> @location(0) vec4<f32> { return in.c; }"
                .into(),
        ),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("raw layout"),
        bind_group_layouts: &[],
        immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("raw pipeline"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    });
    // 26 的 push_error_scope 返回 ()，无 guard pop——跳过（原生测试仅做环境对照）。
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("raw pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.0,
                        g: 0.0,
                        b: 0.0,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            multiview_mask: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_viewport(0.0, 0.0, W as f32, H as f32, 0.0, 1.0);
        pass.set_scissor_rect(0, 0, W, H);
        pass.draw(0..3, 0..1);
    }
    queue.submit(Some(encoder.finish()));
    // 诊断 2：draw 后用 clear 色纹理 + Load 验证（先写蓝背景再 Load 渲染）。
    let bg = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("bg"),
        size: wgpu::Extent3d {
            width: W,
            height: H,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let mut blue = vec![0u8; (W * H * 4) as usize];
    for px in blue.chunks_mut(4) {
        px[0] = 0;
        px[1] = 0;
        px[2] = 200;
        px[3] = 255;
    }
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &bg,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &blue,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(W * 4),
            rows_per_image: Some(H),
        },
        wgpu::Extent3d {
            width: W,
            height: H,
            depth_or_array_layers: 1,
        },
    );
    let bg_view = bg.create_view(&wgpu::TextureViewDescriptor::default());
    let mut enc2 = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    {
        let mut pass = enc2.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("load pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &bg_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            multiview_mask: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_viewport(0.0, 0.0, W as f32, H as f32, 0.0, 1.0);
        pass.set_scissor_rect(0, 0, W, H);
        pass.draw(0..3, 0..1);
    }
    queue.submit(Some(enc2.finish()));
    let bp = read_pixels(&device, &queue, &bg);
    let bi = ((H / 2 * W + W / 2) * 4) as usize;
    let (br, bgc, bb) = (bp[bi], bp[bi + 1], bp[bi + 2]);
    println!("Load 实验中心像素: ({br},{bgc},{bb})（蓝=200 说明 draw 未执行；红说明 draw 执行）");

    let pixels = read_pixels(&device, &queue, &texture);
    let i = ((H / 2 * W + W / 2) * 4) as usize;
    let (r, g, b) = (pixels[i], pixels[i + 1], pixels[i + 2]);
    assert!(
        r > 200 && g < 60 && b < 60,
        "raw 三角形中心应红色，实际 ({r},{g},{b})"
    );
}

#[test]
#[ignore = "lavapipe draw 不可靠（环境），浏览器 gpu_probe 为准"]
fn renders_solid_rect_offscreen() {
    let (device, queue) = setup();
    let pixels = render_to_texture(&device, &queue, &make_frame());
    // 调试：全图非背景像素统计。
    let mut non_bg = 0usize;
    let mut min_x = u32::MAX;
    let mut max_x = 0u32;
    let mut min_y = u32::MAX;
    let mut max_y = 0u32;
    for y in 0..H {
        for x in 0..W {
            let i = ((y * W + x) * 4) as usize;
            if pixels[i] != 0 || pixels[i + 1] != 0 || pixels[i + 2] != 0 {
                non_bg += 1;
                min_x = min_x.min(x);
                max_x = max_x.max(x);
                min_y = min_y.min(y);
                max_y = max_y.max(y);
            }
        }
    }
    println!(
        "非背景像素 {non_bg}，包围盒 x[{min_x},{max_x}] y[{min_y},{max_y}]，首像素 {:?}",
        &pixels[0..12]
    );
    let px = |x: u32, y: u32| {
        let i = ((y * W + x) * 4) as usize;
        (pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3])
    };
    // 中心 (50,50)：红色矩形内部（圆角蓝色在左上角 10..40，不冲突）。
    let c = px(50, 50);
    assert!(c.0 > 200 && c.1 < 60 && c.2 < 60, "中心应红色，实际 {c:?}");
}
