// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

// The dots of `Widget::Strands`, one instance each: the same maths as `kit::strands::place`
// (suhina-ui), with the GPU's own sin, cos and exp. Each dot is a quad of two triangles; its
// fragment is a soft disc. Colors are gamma-encoded and the output premultiplied, as egui's
// images are.

struct Uniforms {
    // Width and height in pixels, the middle, how far the ribbons spread.
    size: vec4<f32>,
    // Ribbon width, the flow's clock, the swell, the sparks' strength.
    a: vec4<f32>,
    // How far the sparks have flown, pixels per point, 1 for the front pass, unused.
    b: vec4<f32>,
    strength_low: vec4<f32>,
    strength_high: vec4<f32>,
    light: vec4<f32>,
    mid: vec4<f32>,
    shadow: vec4<f32>,
    thread: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: Uniforms;

struct Out {
    @builtin(position) position: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) color: vec4<f32>,
};

fn flow(x: f32, c: f32, p: f32) -> f32 {
    return 0.55 * sin(x * 5.3 + c * 0.61 + p) + 0.30 * sin(x * 9.7 - c * 0.93 + p * 1.7)
        + 0.15 * sin(x * 17.1 + c * 1.37 + p * 2.3);
}

fn hidden() -> Out {
    var out: Out;
    out.position = vec4<f32>(2.0, 2.0, 2.0, 1.0);
    out.local = vec2<f32>(0.0, 0.0);
    out.color = vec4<f32>(0.0, 0.0, 0.0, 0.0);
    return out;
}

@vertex
fn vertex(@builtin(vertex_index) corner_index: u32, @location(0) point: vec4<f32>) -> Out {
    var phases = array<f32, 6>(0.0, 2.1, 4.3, 1.2, 3.4, 5.5);
    var amps = array<f32, 6>(0.35, 0.5, 0.65, 0.8, 0.9, 1.0);
    var wides = array<f32, 6>(1.0, 0.8, 0.6, 0.7, 0.55, 0.45);
    var strengths = array<f32, 6>(
        u.strength_low.x, u.strength_low.y, u.strength_low.z, u.strength_low.w, u.strength_high.x, u.strength_high.y);
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(-1.0, 1.0),
        vec2<f32>(-1.0, 1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0));

    let uu = point.x;
    let lane = point.y;
    let r = min(u32(point.z + 0.5), 5u);
    let seed = point.w;
    let strength = strengths[r];
    if (strength < 0.03) {
        return hidden();
    }
    let width = u.size.x;
    let mid = u.size.z;
    let reach = u.size.w;
    let c = u.a.y;
    let swell = u.a.z;
    let d = (uu - 0.5) / 0.19;
    let body = 0.08 + 0.92 * exp(-d * d);
    let own = seed * 6.2831853 + lane * 1.7;
    let center = flow(uu, c, phases[r]) * (0.7 + 0.5 * swell) + 0.55 * (0.6 + swell) * flow(uu * 1.31 + 0.2, c * 1.21, own);
    let z = cos(uu * 4.1 - c * 0.53 + own * 0.6) * 0.9;
    let front = z >= 0.0;
    if (front != (u.b.z > 0.5)) {
        return hidden();
    }
    let persp = 1.0 + 0.25 * z;
    var x = width * uu;
    var y = mid + (reach * body * center * (0.6 + 0.4 * amps[r]) + lane * u.a.x * wides[r] * body * 0.35) * persp;
    var fade = 1.0;
    if (seed > 0.8) {
        let from_x = x - width * 0.5;
        let from_y = y - mid;
        let dist = sqrt(from_x * from_x + from_y * from_y) + 1.0;
        let spark_reach = u.b.x * reach * 2.2 * (0.5 + (seed - 0.8) * 2.5);
        let push = spark_reach * exp(-dist / max(reach * 2.5, 1.0)) / dist;
        x = x + from_x * push;
        y = y + from_y * push;
        if (seed > 0.9) {
            fade = u.a.w;
        }
    }
    if (fade < 0.02) {
        return hidden();
    }
    let near = (z + 1.0) * 0.5;
    let blur = max(-z - 0.25, 0.0) * 1.4;
    let face = sin(uu * 9.0 + c * 0.8 + own);
    let light = clamp(0.5 + 0.35 * face + 0.25 * z, 0.0, 1.0);
    var color = select(mix(u.mid, u.light, (light - 0.5) * 2.0), mix(u.shadow, u.mid, light * 2.0), light < 0.5);
    var cover = 0.08 + 0.3 * near;
    if (seed < 0.14) {
        color = mix(mix(u.thread, u.shadow, 0.45), u.thread, light);
        cover = cover * 1.3;
    }
    if (seed > 0.9) {
        color = mix(u.thread, u.light, 0.5);
    }
    let alpha = clamp(strength * fade * (1.0 - abs(lane) * 0.45) * cover * (0.7 + 0.5 * body) / (1.0 + blur * 1.4), 0.0, 1.0)
        * color.w;
    if (alpha < 0.002) {
        return hidden();
    }
    let size = (1.0 + 0.6 * near) * (1.0 + blur) * u.b.y;
    let corner = corners[corner_index % 6u];
    let px = x + corner.x * size * 0.5;
    let py = y + corner.y * size * 0.5;
    var out: Out;
    out.position = vec4<f32>(px / width * 2.0 - 1.0, 1.0 - py / u.size.y * 2.0, 0.0, 1.0);
    out.local = corner;
    out.color = vec4<f32>(color.xyz, alpha);
    return out;
}

@fragment
fn fragment(input: Out) -> @location(0) vec4<f32> {
    let r2 = dot(input.local, input.local);
    if (r2 > 1.0) {
        discard;
    }
    let a = exp(-r2 * 3.0) * input.color.w;
    return vec4<f32>(input.color.xyz * a, a);
}
