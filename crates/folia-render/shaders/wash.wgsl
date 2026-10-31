// The wall's ground (opaque background with CSS-style radial glows) and veil (edge vignette and grain over the tiles).

struct Glow {
    center: vec2<f32>,
    reach: f32,
    _pad: f32,
    color: vec4<f32>,
}

struct Uniforms {
    resolution: vec2<f32>,
    encode: f32,
    vignette: f32,
    grain: f32,
    _pad0: f32,
    _pad1: f32,
    _pad2: f32,
    base: vec4<f32>,
    glows: array<Glow, 2>,
}

@group(0) @binding(0) var<uniform> u: Uniforms;

// `radial-gradient(circle at center, color, transparent reach)` sized to the farthest corner.
fn glow_alpha(glow: Glow, px: vec2<f32>) -> f32 {
    let center = glow.center * u.resolution;
    let corner = max(center, u.resolution - center);
    let t = length(px - center) / max(length(corner) * glow.reach, 1.0);
    return glow.color.a * (1.0 - clamp(t, 0.0, 1.0));
}

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3<f32>(2.4)), c / 12.92, c <= vec3<f32>(0.04045));
}

@fragment
fn fs_wash(input: VsOut) -> @location(0) vec4<f32> {
    let px = input.uv * u.resolution;
    var rgb = u.base.rgb;
    for (var i = 1; i >= 0; i--) {
        rgb = mix(rgb, u.glows[i].color.rgb, glow_alpha(u.glows[i], px));
    }
    if (u.encode > 0.5) {
        rgb = to_linear(rgb);
    }
    return vec4<f32>(rgb, 1.0);
}

// `radial-gradient(ellipse at center, transparent 78%, black 100%)` plus a static white/black grain.
@fragment
fn fs_veil(input: VsOut) -> @location(0) vec4<f32> {
    let t = length((input.uv - vec2<f32>(0.5)) * 2.0) / sqrt(2.0);
    var shade = clamp((t - 0.78) / 0.22, 0.0, 1.0) * u.vignette;
    if (u.encode > 0.5) {
        shade = 1.0 - pow(1.0 - shade, 2.2);
    }
    let px = vec2<u32>(input.uv * u.resolution);
    let n = hash_px(px, 0u) - 0.5;
    let grain = abs(n) * 2.0 * u.grain;
    let a = grain + shade * (1.0 - grain);
    let white = select(0.0, 1.0, n > 0.0);
    return vec4<f32>(vec3<f32>(white * grain / max(a, 0.001)), a);
}
