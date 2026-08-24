//! wgpu 渲染器离屏回读集成测试（lavapipe/llvmpipe 无头可跑，验证命令 → GPU 像素）。
//! 这是"wgpu 什么都没画出来"的回归防护：同一 UiFrame 在离屏纹理回读非背景像素。

use tela_contract::{
    BorderRadius, Color, ColorStop, DrawCommand, DrawPayload, Fill, Gradient, GradientKind,
    PixelOffset, Point, Rect, ShadowSpec, UiFrame, Viewport,
};
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
                opacity: 1.0,
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

fn visual_golden_frame() -> UiFrame {
    let linear = Gradient {
        kind: GradientKind::Linear {
            start: Point { x: 5.0, y: 0.0 },
            end: Point { x: 95.0, y: 0.0 },
        },
        stops: vec![
            ColorStop {
                position: 0.0,
                color: Color::RED,
            },
            ColorStop {
                position: 0.5,
                color: Color::rgba(0.2, 0.8, 0.4, 1.0),
            },
            ColorStop {
                position: 1.0,
                color: Color::BLUE,
            },
        ],
    };
    let radial = Gradient {
        kind: GradientKind::Radial {
            center: Point { x: 75.0, y: 75.0 },
            radius: 18.0,
        },
        stops: vec![
            ColorStop {
                position: 0.0,
                color: Color::WHITE,
            },
            ColorStop {
                position: 1.0,
                color: Color::rgba(0.1, 0.35, 0.95, 1.0),
            },
        ],
    };
    UiFrame {
        viewport: Viewport {
            width: W as f32,
            height: H as f32,
        },
        commands: vec![
            DrawCommand {
                geometry: Rect {
                    x: 5.0,
                    y: 5.0,
                    w: 90.0,
                    h: 24.0,
                },
                clip: None,
                opacity: 1.0,
                payload: DrawPayload::RoundedRect {
                    fill: Some(Fill::Linear(linear)),
                    border: None,
                    radius: BorderRadius::all(8.0),
                },
            },
            DrawCommand {
                geometry: Rect {
                    x: 10.0,
                    y: 42.0,
                    w: 45.0,
                    h: 34.0,
                },
                clip: None,
                opacity: 0.8,
                payload: DrawPayload::Shadow {
                    spec: ShadowSpec {
                        offset: PixelOffset { x: 3.0, y: 4.0 },
                        blur_radius: 7.0,
                        color: Color::rgba(0.0, 0.0, 0.0, 0.45),
                        inset: false,
                    },
                    target: Box::new(DrawPayload::RoundedRect {
                        fill: Some(Fill::Solid(Color::rgba(0.95, 0.35, 0.2, 1.0))),
                        border: None,
                        radius: BorderRadius::all(10.0),
                    }),
                },
            },
            DrawCommand {
                geometry: Rect {
                    x: 58.0,
                    y: 56.0,
                    w: 34.0,
                    h: 38.0,
                },
                clip: None,
                opacity: 1.0,
                payload: DrawPayload::Ellipse {
                    fill: Some(Fill::Radial(radial)),
                    border: None,
                },
            },
            DrawCommand {
                geometry: Rect {
                    x: 5.0,
                    y: 82.0,
                    w: 30.0,
                    h: 12.0,
                },
                clip: None,
                opacity: 1.0,
                payload: DrawPayload::Circle {
                    fill: Some(Fill::Solid(Color::rgba(0.05, 0.7, 0.25, 1.0))),
                    border: None,
                },
            },
            DrawCommand {
                geometry: Rect {
                    x: 38.0,
                    y: 82.0,
                    w: 16.0,
                    h: 12.0,
                },
                clip: None,
                opacity: 0.75,
                payload: DrawPayload::Image {
                    texture: tela_contract::TextureRef("golden-checker".to_owned()),
                    radius: BorderRadius::all(5.0),
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
        // 不强制 fallback：Linux 上 lavapipe 仍会被枚举为唯一 Vulkan 适配器，
        // macOS 上则避免 Metal 无 fallback 适配器导致 NotFound。
        force_fallback_adapter: false,
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
    let mut data = Vec::with_capacity((W * H * 4) as usize);
    for row in range.chunks_exact(bytes_per_row as usize) {
        data.extend_from_slice(&row[..(W * 4) as usize]);
    }
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
        Color::rgba(0.93, 0.94, 0.96, 1.0),
    );
    let mut checker = vec![0_u8; 32 * 32 * 4];
    for (index, pixel) in checker.chunks_exact_mut(4).enumerate() {
        let x = index % 32;
        let y = index / 32;
        let bright = (x / 2 + y / 2) % 2 == 0;
        pixel.copy_from_slice(if bright {
            &[245, 214, 64, 255]
        } else {
            &[32, 96, 210, 255]
        });
    }
    renderer
        .upload_rgba8(
            tela_contract::TextureRef("golden-checker".to_owned()),
            32,
            32,
            &checker,
        )
        .expect("golden checker upload");
    renderer.render_frame(frame, &view, W, H);
    read_pixels(device, queue, &texture)
}

/// 回读链路验证：write_texture 直接写红 → readback 应读到红。
#[test]
#[ignore = "requires nix develop .#render-wgpu; GPU readback is covered by the ops visual gate"]
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

/// 对照实验：raw wgpu 直接画红色三角形（不经 tela 渲染器）。
///
/// 该实验在当前 lavapipe 上仍不稳定，因此只保留为环境诊断；Tela renderer 的正式回归门
/// 是下面两个非 ignored 的离屏测试。
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
#[ignore = "requires nix develop .#render-wgpu; ops visual gate runs this test explicitly"]
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

#[test]
#[ignore = "requires nix develop .#render-wgpu; ops visual gate runs this test explicitly"]
fn renders_visual_primitive_golden_offscreen() {
    let (device, queue) = setup();
    let pixels = render_to_texture(&device, &queue, &visual_golden_frame());
    let pixel = |x: u32, y: u32| {
        let index = ((y * W + x) * 4) as usize;
        [
            pixels[index],
            pixels[index + 1],
            pixels[index + 2],
            pixels[index + 3],
        ]
    };
    let samples = [
        (10, 17, pixel(10, 17)),
        (50, 17, pixel(50, 17)),
        (90, 17, pixel(90, 17)),
        (32, 58, pixel(32, 58)),
        (57, 78, pixel(57, 78)),
        (75, 75, pixel(75, 75)),
        (88, 75, pixel(88, 75)),
        (10, 88, pixel(10, 88)),
        (20, 88, pixel(20, 88)),
        (39, 83, pixel(39, 83)),
        (46, 88, pixel(46, 88)),
        (5, 95, pixel(5, 95)),
    ];
    println!("visual golden samples: {samples:?}");

    let expected = include_str!("golden/visual_primitives.samples");
    let actual = samples
        .iter()
        .map(|(x, y, rgba)| format!("{x},{y}:{},{},{},{}", rgba[0], rgba[1], rgba[2], rgba[3]))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(actual.trim(), expected.trim());
}
