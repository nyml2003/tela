// tela-render-wgpu 的最小图元：纯色矩形与圆角矩形。

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

struct RoundedOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) size: vec2<f32>,
    @location(2) radius: vec4<f32>,
    @location(4) fill_color: vec4<f32>,
    @location(5) border_color: vec4<f32>,
    @location(6) border_width: f32,
}

@vertex
fn vs_rounded(
    @location(0) pos: vec2<f32>,
    @location(1) local: vec2<f32>,
    @location(2) size: vec2<f32>,
    @location(3) radius: vec4<f32>,
    @location(4) fill_color: vec4<f32>,
    @location(5) border_color: vec4<f32>,
    @location(6) border_width: f32,
) -> RoundedOut {
    var out: RoundedOut;
    out.clip_position = vec4<f32>(pos, 0.0, 1.0);
    out.local = local;
    out.size = size;
    out.radius = radius;
    out.fill_color = fill_color;
    out.border_color = border_color;
    out.border_width = border_width;
    return out;
}

fn rounded_distance(point: vec2<f32>, size: vec2<f32>, radii: vec4<f32>) -> f32 {
    let half_size = size * 0.5;
    let centered = point - half_size;
    let left = centered.x < 0.0;
    let top = centered.y < 0.0;
    let top_radius = select(radii.y, radii.x, left);
    let bottom_radius = select(radii.z, radii.w, left);
    let radius = select(bottom_radius, top_radius, top);
    let clamped_radius = clamp(radius, 0.0, min(half_size.x, half_size.y));
    let q = abs(centered) - (half_size - vec2<f32>(clamped_radius));
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - clamped_radius;
}

fn coverage(distance: f32) -> f32 {
    let pixel_width = max(fwidth(distance), 1e-3);
    return clamp(0.5 - distance / pixel_width, 0.0, 1.0);
}

@fragment
fn fs_rounded(in: RoundedOut) -> @location(0) vec4<f32> {
    let outer_distance = rounded_distance(in.local, in.size, in.radius);
    let width = min(in.border_width, min(in.size.x, in.size.y) * 0.5);
    let inner_size = max(in.size - vec2<f32>(2.0 * width), vec2<f32>(1e-3));
    let inner_radius = max(in.radius - vec4<f32>(width), vec4<f32>(0.0));
    let inner_distance = rounded_distance(
        in.local - vec2<f32>(width),
        inner_size,
        inner_radius,
    );
    let color = mix(in.border_color, in.fill_color, coverage(inner_distance));
    let alpha = color.a * coverage(outer_distance);
    return vec4<f32>(color.rgb, alpha);
}
