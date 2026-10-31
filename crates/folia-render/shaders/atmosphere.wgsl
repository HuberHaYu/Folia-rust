// Vignette, integer-hash grain, and the subtitle radial glow.

struct Uniforms {
    resolution: vec2<f32>,
    time: f32,
    vignette: f32,
    glow_center: vec2<f32>,
    glow_radius: vec2<f32>,
    glow_rgb: vec3<f32>,
    glow_alpha: f32,
    grain: f32,
    frame: f32,
    _pad0: f32,
    _pad1: f32,
}

@group(0) @binding(0) var<uniform> u: Uniforms;

fn grain_of(uv: vec2<f32>) -> f32 {
    let px = vec2<u32>(u32(uv.x * u.resolution.x), u32(uv.y * u.resolution.y));
    return abs(hash_px(px, u32(u.frame)) - 0.5) * u.grain;
}

// Same curve as `folia_ui::glow_coverage`: stops 0.96, 0.78 at 0.62, then a smooth edge
// that runs 40px past the ellipse, standing in for `blur-2xl`.
fn subtitle_alpha(uv: vec2<f32>) -> f32 {
    if (u.glow_alpha <= 0.0) {
        return 0.0;
    }
    let axes = max(u.glow_radius * u.resolution, vec2<f32>(1.0, 1.0));
    let t = length((uv - u.glow_center) * u.resolution / axes);
    let blur_t = 40.0 / min(axes.x, axes.y);
    let end_t = 1.0 + blur_t;
    var stop: f32;
    if (t <= 0.62) {
        stop = mix(0.96, 0.78, t / 0.62);
    } else {
        let x = clamp((t - 0.62) / max(end_t - 0.62, 0.001), 0.0, 1.0);
        let s = x * x * (3.0 - 2.0 * x);
        stop = mix(0.78, 0.0, s);
    }
    return stop * u.glow_alpha;
}

@fragment
fn fs_overlay(input: VsOut) -> @location(0) vec4<f32> {
    let uv = input.uv;
    let edge = length((uv - vec2<f32>(0.5, 0.45)) * vec2<f32>(1.15, 1.0));
    let shade = smoothstep(0.28, 0.98, edge) * u.vignette;
    let n = grain_of(uv);
    let shade_a = clamp(shade + n, 0.0, 0.92);
    let shade_rgb = mix(vec3<f32>(0.0, 0.0, 0.0), vec3<f32>(1.0, 1.0, 1.0), n / max(shade_a, 0.001));
    let glow_a = subtitle_alpha(uv);
    let a = glow_a + shade_a * (1.0 - glow_a);
    let rgb = (u.glow_rgb * glow_a + shade_rgb * shade_a * (1.0 - glow_a)) / max(a, 0.001);
    return vec4<f32>(rgb, a);
}
