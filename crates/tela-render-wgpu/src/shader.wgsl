// tela-render-wgpu 当前最小能力：UiFrame 的纯色矩形。

struct SolidOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
}

@vertex
fn vs_solid(
    @location(0) pos: vec2<f32>,
    @location(1) color: vec4<f32>,
) -> SolidOut {
    var out: SolidOut;
    out.clip_position = vec4<f32>(pos, 0.0, 1.0);
    out.color = color;
    return out;
}

@fragment
fn fs_solid(in: SolidOut) -> @location(0) vec4<f32> {
    return in.color;
}
