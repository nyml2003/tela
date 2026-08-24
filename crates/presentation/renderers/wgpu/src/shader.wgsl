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
    @location(7) opacity: f32,
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
    @location(7) opacity: f32,
) -> RoundedOut {
    var out: RoundedOut;
    out.clip_position = vec4<f32>(pos, 0.0, 1.0);
    out.local = local;
    out.size = size;
    out.radius = radius;
    out.fill_color = fill_color;
    out.border_color = border_color;
    out.border_width = border_width;
    out.opacity = opacity;
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
    let alpha = color.a * coverage(outer_distance) * in.opacity;
    return vec4<f32>(color.rgb, alpha);
}

struct ImageOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) local: vec2<f32>,
    @location(2) size: vec2<f32>,
    @location(3) radius: vec4<f32>,
    @location(4) opacity: f32,
}

@group(0) @binding(0) var image_texture: texture_2d<f32>;
@group(0) @binding(1) var image_sampler: sampler;

@vertex
fn vs_image(
    @location(0) pos: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) local: vec2<f32>,
    @location(3) size: vec2<f32>,
    @location(4) radius: vec4<f32>,
    @location(5) opacity: f32,
) -> ImageOut {
    var out: ImageOut;
    out.clip_position = vec4<f32>(pos, 0.0, 1.0);
    out.uv = uv;
    out.local = local;
    out.size = size;
    out.radius = radius;
    out.opacity = opacity;
    return out;
}

@fragment
fn fs_image(in: ImageOut) -> @location(0) vec4<f32> {
    let sampled = textureSample(image_texture, image_sampler, in.uv);
    let alpha = sampled.a * coverage(rounded_distance(in.local, in.size, in.radius)) * in.opacity;
    return vec4<f32>(sampled.rgb, alpha);
}

struct GradientOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) size: vec2<f32>,
    @location(2) radius: vec4<f32>,
    @location(3) gradient: vec4<f32>,
    @location(4) gradient_radius: f32,
    @location(5) gradient_kind: f32,
    @location(6) shape_kind: f32,
    @location(7) opacity: f32,
}

@vertex
fn vs_gradient(
    @location(0) pos: vec2<f32>,
    @location(1) local: vec2<f32>,
    @location(2) size: vec2<f32>,
    @location(3) radius: vec4<f32>,
    @location(4) gradient: vec4<f32>,
    @location(5) gradient_radius: f32,
    @location(6) gradient_kind: f32,
    @location(7) shape_kind: f32,
    @location(8) opacity: f32,
) -> GradientOut {
    var out: GradientOut;
    out.clip_position = vec4<f32>(pos, 0.0, 1.0);
    out.local = local;
    out.size = size;
    out.radius = radius;
    out.gradient = gradient;
    out.gradient_radius = gradient_radius;
    out.gradient_kind = gradient_kind;
    out.shape_kind = shape_kind;
    out.opacity = opacity;
    return out;
}

fn ellipse_distance(point: vec2<f32>, size: vec2<f32>) -> f32 {
    let half_size = max(size * 0.5, vec2<f32>(1e-3));
    let normalized = (point - half_size) / half_size;
    return (length(normalized) - 1.0) * min(half_size.x, half_size.y);
}

fn circle_distance(point: vec2<f32>, size: vec2<f32>) -> f32 {
    let center = size * 0.5;
    let radius = max(min(size.x, size.y) * 0.5, 1e-3);
    return distance(point, center) - radius;
}

fn shape_distance(
    point: vec2<f32>,
    size: vec2<f32>,
    radius: vec4<f32>,
    shape_kind: f32,
) -> f32 {
    if shape_kind > 1.5 {
        return circle_distance(point, size);
    }
    if shape_kind > 0.5 {
        return ellipse_distance(point, size);
    }
    return rounded_distance(point, size, radius);
}

@fragment
fn fs_gradient(in: GradientOut) -> @location(0) vec4<f32> {
    var t: f32;
    if in.gradient_kind > 0.5 {
        t = distance(in.local, in.gradient.xy) / max(in.gradient_radius, 1e-3);
    } else {
        let axis = in.gradient.zw - in.gradient.xy;
        let axis_len_sq = dot(axis, axis);
        t = select(
            0.0,
            dot(in.local - in.gradient.xy, axis) / max(axis_len_sq, 1e-6),
            axis_len_sq > 1e-6,
        );
    }
    let color = textureSample(image_texture, image_sampler, vec2<f32>(clamp(t, 0.0, 1.0), 0.5));
    let distance_to_shape = shape_distance(in.local, in.size, in.radius, in.shape_kind);
    return vec4<f32>(color.rgb, color.a * coverage(distance_to_shape) * in.opacity);
}

struct ShadowOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) target_size: vec2<f32>,
    @location(2) radius: vec4<f32>,
    @location(3) color: vec4<f32>,
    @location(4) blur_radius: f32,
    @location(5) inset: f32,
    @location(6) shape_kind: f32,
    @location(7) opacity: f32,
}

@vertex
fn vs_shadow(
    @location(0) pos: vec2<f32>,
    @location(1) local: vec2<f32>,
    @location(2) target_size: vec2<f32>,
    @location(3) radius: vec4<f32>,
    @location(4) color: vec4<f32>,
    @location(5) blur_radius: f32,
    @location(6) inset: f32,
    @location(7) shape_kind: f32,
    @location(8) opacity: f32,
) -> ShadowOut {
    var out: ShadowOut;
    out.clip_position = vec4<f32>(pos, 0.0, 1.0);
    out.local = local;
    out.target_size = target_size;
    out.radius = radius;
    out.color = color;
    out.blur_radius = blur_radius;
    out.inset = inset;
    out.shape_kind = shape_kind;
    out.opacity = opacity;
    return out;
}

@fragment
fn fs_shadow(in: ShadowOut) -> @location(0) vec4<f32> {
    let distance_to_shape = shape_distance(
        in.local,
        in.target_size,
        in.radius,
        in.shape_kind,
    );
    let sigma = max(in.blur_radius * 0.5, 0.5);
    let outer_alpha = 1.0 - smoothstep(-sigma, sigma * 2.0, distance_to_shape);
    let inner_alpha = smoothstep(-sigma * 2.0, sigma, distance_to_shape) * coverage(distance_to_shape);
    let alpha = select(outer_alpha, inner_alpha, in.inset > 0.5);
    return vec4<f32>(in.color.rgb, in.color.a * alpha * in.opacity);
}
