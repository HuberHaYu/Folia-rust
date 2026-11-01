// Rounded fill with a CSS box-shadow (Gaussian, offset, spread) and an optional 1px outer ring.

struct PlateUniform {
    resolution: vec2<f32>,
    encode: f32,
    radius: f32,
    origin: vec2<f32>,
    size: vec2<f32>,
    fill: vec4<f32>,
    shadow: vec4<f32>,
    shadow_blur: f32,
    shadow_y: f32,
    shadow_spread: f32,
    _pad: f32,
    ring: vec4<f32>,
}

@group(0) @binding(0) var<uniform> u: PlateUniform;

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    return pow(c, vec3<f32>(2.2));
}

fn sd_round(p: vec2<f32>, half: vec2<f32>, radius: f32) -> f32 {
    let q = abs(p) - half + vec2<f32>(radius);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - radius;
}

fn erf2(x: vec2<f32>) -> vec2<f32> {
    let s = sign(x);
    let a = abs(x);
    var r = 1.0 + (0.278393 + (0.230389 + 0.078108 * a * a) * a) * a;
    r = r * r;
    return s - s / (r * r);
}

fn gaussian(x: f32, sigma: f32) -> f32 {
    return exp(-(x * x) / (2.0 * sigma * sigma)) / (2.5066283 * sigma);
}

// Blurred coverage of one horizontal slice of a rounded box at height `y`.
fn slice(x: f32, y: f32, sigma: f32, corner: f32, half: vec2<f32>) -> f32 {
    let delta = min(half.y - corner - abs(y), 0.0);
    let curved = half.x - corner + sqrt(max(0.0, corner * corner - delta * delta));
    let integral = 0.5 + 0.5 * erf2((vec2<f32>(-curved, curved) + x) * (0.70710678 / sigma));
    return integral.y - integral.x;
}

// Coverage of a rounded box blurred by a Gaussian of `sigma`, after Evan Wallace's fast box shadows.
fn shadow_coverage(p: vec2<f32>, half: vec2<f32>, corner: f32, sigma: f32) -> f32 {
    let low = p.y - half.y;
    let high = p.y + half.y;
    let start = clamp(-3.0 * sigma, low, high);
    let end = clamp(3.0 * sigma, low, high);
    let step = (end - start) / 4.0;
    var y = start + step * 0.5;
    var value = 0.0;
    for (var i = 0; i < 4; i = i + 1) {
        value = value + slice(p.x, p.y - y, sigma, corner, half) * gaussian(y, sigma) * step;
        y = y + step;
    }
    return value;
}

@fragment
fn fs_plate(input: VsOut) -> @location(0) vec4<f32> {
    let px = input.uv * u.resolution;
    let radius = min(u.radius, min(u.size.x, u.size.y) * 0.5);
    let half = max(u.size * 0.5, vec2<f32>(0.0));
    let center = u.origin + half;
    let fill_d = sd_round(px - center, half, radius);
    let inside = clamp(0.5 - fill_d, 0.0, 1.0);

    let shadow_half = max(half + vec2<f32>(u.shadow_spread), vec2<f32>(0.0));
    let shadow_corner = max(radius + u.shadow_spread, 0.0);
    let shadow_p = px - (center + vec2<f32>(0.0, u.shadow_y));
    var cover = 0.0;
    if (u.shadow_blur > 0.01) {
        cover = shadow_coverage(shadow_p, shadow_half, min(shadow_corner, min(shadow_half.x, shadow_half.y)), u.shadow_blur * 0.5);
    } else {
        cover = clamp(0.5 - sd_round(shadow_p, shadow_half, shadow_corner), 0.0, 1.0);
    }
    // Like box-shadow, nothing under the box; the shadow alone fades over the widget's last 1.5px.
    let edge = min(min(px.x, u.resolution.x - px.x), min(px.y, u.resolution.y - px.y));
    let shadow_a = u.shadow.a * cover * (1.0 - inside) * smoothstep(0.0, 1.5, edge);
    let ring_a = u.ring.a * clamp(1.5 - fill_d, 0.0, 1.0) * (1.0 - inside);
    let fill_a = u.fill.a * inside;

    // Back to front: shadow, ring, fill.
    var rgb = u.shadow.rgb;
    var a = shadow_a;
    rgb = mix(rgb * a, u.ring.rgb, ring_a);
    a = ring_a + a * (1.0 - ring_a);
    rgb = mix(rgb, u.fill.rgb, fill_a);
    a = fill_a + a * (1.0 - fill_a);
    rgb = rgb / max(a, 0.0001);

    if (u.encode > 0.5) {
        rgb = to_linear(rgb);
    }
    return vec4<f32>(rgb, a);
}
