// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

struct Params { step: vec2<f32>, sigma: f32, padding: f32 };
@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var clamp_sampler: sampler;
@group(0) @binding(2) var<uniform> params: Params;
struct Vertex { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex fn vertex(@builtin(vertex_index) index: u32) -> Vertex {
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    var out: Vertex;
    out.position = vec4<f32>(uv * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0);
    out.uv = uv;
    return out;
}
fn blur(uv: vec2<f32>) -> vec4<f32> {
    var sum = vec4<f32>(0.0);
    var weights = 0.0;
    for (var i = -9; i <= 9; i += 1) {
        let x = f32(i);
        let w = exp(-0.5 * x * x / (params.sigma * params.sigma));
        sum += textureSampleLevel(source, clamp_sampler, uv + params.step * x, 0.0) * w;
        weights += w;
    }
    return sum / weights;
}
@fragment fn linear_blur(v: Vertex) -> @location(0) vec4<f32> { return blur(v.uv); }
@fragment fn encoded_blur(v: Vertex) -> @location(0) vec4<f32> {
    let c = blur(v.uv);
    // The egui native texture contract is gamma-encoded, premultiplied RGBA8Unorm.
    let alpha = c.a;
    let rgb = c.rgb / max(alpha, 0.000001);
    let encoded = select(1.055 * pow(max(rgb, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.4)) - 0.055,
        rgb * 12.92, rgb <= vec3<f32>(0.0031308));
    return vec4<f32>(encoded * alpha, alpha);
}
