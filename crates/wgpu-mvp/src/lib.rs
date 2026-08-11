//! wgpu 26 官方链路最小 MVP（wasm）：验证 wgpu crate 在本环境（Blackwell/Chrome）
//! 能否画出东西——**零 tela 自定义逻辑**，等价 wgpu 官方 hello-triangle 的 wasm 版。
//!
//! 背景：wgpu 26/29/30 的 WebGPU 桥接层在 tela 集成中曾出现 draw 无输出
//! （跨版本一致）；本 MVP 排除所有自定义因素（批次/文字/双后端），只跑
//! instance → surface → adapter → device → pipeline → pass → submit → present
//! 官方链路。判读：三角形显示 = wgpu 官方链路通（tela 集成有 bug，对齐修复）；
//! 无输出 = wgpu crate 在此环境不可用坐实（web-sys 直调是唯一路）。
//!
//! 入口页面 demo/mvp.html（构建见 ops/README 或手工命令）。
//! 本 crate 仅面向 wasm32 目标（native 空编译，供 workspace test/clippy 通过）。

#![cfg(target_arch = "wasm32")]

use std::cell::RefCell;

use wasm_bindgen::prelude::*;
use web_sys::HtmlCanvasElement;

/// 三角形 shader：vs_vb（顶点缓冲三角）——两个全等三角形组合成平行四边形。
const SHADER: &str = "
struct VsOut {
    @builtin(position) p: vec4<f32>,
    @location(0) color: vec4<f32>,
}

@vertex
fn vs_vb(@location(0) pos: vec2<f32>, @location(1) color: vec4<f32>) -> VsOut {
    var o: VsOut;
    o.p = vec4<f32>(pos, 0.0, 1.0);
    o.color = color;
    return o;
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    return in.color;
}
";

/// VB 测试三角顶点（蓝色，位置偏左与红色基线错开）。
#[repr(C)]
struct TriVertex {
    pos: [f32; 2],
    color: [f32; 4],
}

/// 两个全等三角形（蓝 + 青）组合成平行四边形（沿对角线 AD 分割，两三角全等）。
/// 平行四边形四角（顺序 A-B-D-C）：A(-0.7,0.25) B(0.5,0.25) D(0.9,-0.25) C(-0.3,-0.25)
/// ——对边 AB∥DC（|AB|=|DC|=1.2）、BD∥CA（|BD|=|CA|=√0.41）。
/// 三角1（蓝）：A,B,D；三角2（青）：A,C,D——共享对角线 AD，两三角全等（SSS：边集相同）。
const TRI_VERTS: [TriVertex; 6] = [
    // 三角1（蓝）：A B D
    TriVertex {
        pos: [-0.7, 0.25],
        color: [0.2, 0.5, 1.0, 1.0],
    },
    TriVertex {
        pos: [0.5, 0.25],
        color: [0.2, 0.5, 1.0, 1.0],
    },
    TriVertex {
        pos: [0.9, -0.25],
        color: [0.2, 0.5, 1.0, 1.0],
    },
    // 三角2（青）：A C D
    TriVertex {
        pos: [-0.7, 0.25],
        color: [0.2, 0.9, 0.9, 1.0],
    },
    TriVertex {
        pos: [-0.3, -0.25],
        color: [0.2, 0.9, 0.9, 1.0],
    },
    TriVertex {
        pos: [0.9, -0.25],
        color: [0.2, 0.9, 0.9, 1.0],
    },
];

/// 顶点 → 字节（f32 LE，无 padding：2+4 个 f32 = 24 字节）。
fn tri_vertex_bytes() -> Vec<u8> {
    let mut out = Vec::with_capacity(6 * 24);
    for v in &TRI_VERTS {
        for f in [
            v.pos[0], v.pos[1], v.color[0], v.color[1], v.color[2], v.color[3],
        ] {
            out.extend_from_slice(&f.to_le_bytes());
        }
    }
    out
}

/// 索引 → 字节（两个三角形，走 draw_indexed 链路——tela 渲染器共用环节）。
fn tri_index_bytes() -> Vec<u8> {
    let mut out = Vec::with_capacity(12);
    for i in [0u16, 1, 2, 3, 4, 5] {
        out.extend_from_slice(&i.to_le_bytes());
    }
    out
}

/// 会话状态（wasm 单线程：wgpu 对象非 Send/Sync，thread_local 持有）。
struct State {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    vb: wgpu::Buffer,
    /// VB 测试索引缓冲（draw_indexed 链路）。
    ib: wgpu::Buffer,
    config: wgpu::SurfaceConfiguration,
    /// 帧状态日志节流（1Hz）。
    last_log: f64,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn now_ms() -> f64 {
    web_sys::window()
        .and_then(|w| w.performance())
        .map(|p| p.now())
        .unwrap_or(0.0)
}

/// MVP 版本标识（页面显示用）。
#[wasm_bindgen]
pub fn wasm_version() -> String {
    // wgpu 无公开 VERSION 常量；依赖版本在 Cargo.toml（wgpu = "30"）。
    format!("wgpu-mvp {} (wgpu 30)", env!("CARGO_PKG_VERSION"))
}

/// 启动：instance → surface → adapter → device → 管线 → 存会话。
#[wasm_bindgen]
pub async fn start(canvas: HtmlCanvasElement) -> Result<(), JsValue> {
    console_log("mvp: instance 创建");
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::BROWSER_WEBGPU,
        flags: wgpu::InstanceFlags::default(),
        memory_budget_thresholds: Default::default(),
        backend_options: Default::default(),
        display: None,
    });
    let surface = instance
        .create_surface(wgpu::SurfaceTarget::Canvas(canvas.clone()))
        .map_err(|e| JsValue::from_str(&format!("surface 创建失败: {e}")))?;
    console_log("mvp: surface 创建成功");
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        })
        .await
        .map_err(|e| JsValue::from_str(&format!("无可用 WebGPU 适配器: {e}")))?;
    {
        let info = adapter.get_info();
        console_log(&format!(
            "mvp: adapter {} vendor=0x{:x} device=0x{:x} type={:?}",
            info.name, info.vendor, info.device, info.device_type
        ));
    }
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("wgpu-mvp device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            experimental_features: Default::default(),
            memory_hints: wgpu::MemoryHints::default(),
            trace: wgpu::Trace::Off,
        })
        .await
        .map_err(|e| JsValue::from_str(&format!("设备创建失败: {e}")))?;
    console_log("mvp: device 创建成功");
    // GPU 错误显性化（wgpu 30：Arc<dyn Fn(Error) + Send + Sync>）。
    device.on_uncaptured_error(std::sync::Arc::new(|err: wgpu::Error| {
        console_error(&format!("mvp: 未捕获 GPU 错误: {err}"));
    }));
    // 用 surface 默认配置（preferred format/alpha；手动取 formats[0] 可能非 preferred）。
    let mut config = surface
        .get_default_config(&adapter, canvas.width().max(1), canvas.height().max(1))
        .ok_or_else(|| JsValue::from_str("surface 无默认配置"))?;
    config.width = canvas.width().max(1);
    config.height = canvas.height().max(1);
    let format = config.format;
    surface.configure(&device, &config);
    console_log(&format!(
        "mvp: configure format={format:?} size={}x{}",
        config.width, config.height
    ));

    // 注：不用 error scope——wgpu 30 WebGPU 后端的 pop() 在无错误时收到 Chrome 的
    // null（JsOption 只认 undefined 为空）→ Error::from_js(null) panic → wasm abort
    // （wasm panic=abort 无法捕获）。validation error 经
    // on_uncaptured_error → console（真错误时可见，成功路径不崩）。
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("mvp shader"),
        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
    });
    // ---- 管线（顶点缓冲三角，两个全等三角形组合成平行四边形） ----
    use wgpu::util::DeviceExt;
    let vb = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("mvp tri vb"),
        contents: &tri_vertex_bytes(),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let ib = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("mvp tri ib"),
        contents: &tri_index_bytes(),
        usage: wgpu::BufferUsages::INDEX,
    });
    let vb_layout = wgpu::VertexBufferLayout {
        array_stride: 24,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4],
    };
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("mvp pipeline"),
        layout: None,
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_vb"),
            compilation_options: Default::default(),
            buffers: &[Some(vb_layout)],
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
    console_log("mvp: 管线创建成功（内置+VB 双管线）");

    STATE.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.is_some() {
            return Err(JsValue::from_str("mvp: 会话重复初始化"));
        }
        *slot = Some(State {
            surface,
            device,
            queue,
            pipeline,
            vb,
            ib,
            config,
            last_log: 0.0,
        });
        Ok(())
    })
}

/// 一帧推进（宿主 rAF 调用）：get_current_texture → pass(clear+draw) → submit → present。
/// 返回 1 = 已提交，0 = 跳过（surface 错误）。
#[wasm_bindgen]
pub fn tick() -> u32 {
    let Some(mut state) = STATE.with(|cell| cell.borrow_mut().take()) else {
        return 0;
    };
    let result = draw_frame(&mut state);
    STATE.with(|cell| *cell.borrow_mut() = Some(state));
    result
}

fn draw_frame(state: &mut State) -> u32 {
    // surface 获取（wgpu 30：CurrentSurfaceTexture 枚举；Lost/Outdated → 重配置重试）。
    let surface_texture = match state.surface.get_current_texture() {
        wgpu::CurrentSurfaceTexture::Success(t) | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
        wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
            state.surface.configure(&state.device, &state.config);
            console_log("mvp: surface Outdated/Lost，已重配置");
            return 0;
        }
        wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => return 0,
        wgpu::CurrentSurfaceTexture::Validation => {
            console_error("mvp: surface Validation 错误（见 on_uncaptured_error）");
            return 0;
        }
    };
    let view = surface_texture
        .texture
        .create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = state
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("mvp pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.05,
                        g: 0.08,
                        b: 0.1,
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
        // 两个全等三角形（平行四边形，对角线分割）：VB + IB + draw_indexed
        //（index/draw_indexed 是 tela 渲染器共用环节）。
        pass.set_pipeline(&state.pipeline);
        pass.set_vertex_buffer(0, state.vb.slice(..));
        pass.set_index_buffer(state.ib.slice(..), wgpu::IndexFormat::Uint16);
        pass.draw_indexed(0..6, 0, 0..1);
    }
    state.queue.submit(Some(encoder.finish()));
    // wgpu 30：present 移到 Queue::present（SurfaceTexture::present 已移除）。
    state.queue.present(surface_texture);
    // 帧状态 1Hz 节流日志。
    let now = now_ms();
    if now - state.last_log > 1000.0 {
        console_log(&format!(
            "mvp: tick ✓ 已提交+present（{}x{}）",
            state.config.width, state.config.height
        ));
        state.last_log = now;
    }
    1
}

/// canvas 像素尺寸变化（页面调用）：surface 重配置。
#[wasm_bindgen]
pub fn set_size(w: u32, h: u32) {
    STATE.with(|cell| {
        if let Some(state) = cell.borrow_mut().as_mut() {
            state.config.width = w.max(1);
            state.config.height = h.max(1);
            state.surface.configure(&state.device, &state.config);
            console_log(&format!(
                "mvp: reconfigure {}x{}",
                state.config.width, state.config.height
            ));
        }
    });
}

fn console_log(msg: &str) {
    web_sys::console::log_1(&JsValue::from_str(msg));
}

fn console_error(msg: &str) {
    web_sys::console::error_1(&JsValue::from_str(msg));
}
