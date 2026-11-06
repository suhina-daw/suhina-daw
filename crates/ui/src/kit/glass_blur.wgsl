// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

// Used by the isolated glass_preview example; not wired into the DAW renderer yet.
// Two passes over separate textures: horizontal first, then vertical.
// Smooth mode uses adjacent samples with bounded sigma. Artistic stepped mode keeps
// the sparse nine-tap kernel paired into five bilinear samples.
// The caller supplies a downsampled backdrop, never the current render attachment.
struct BlurParams {
    // UV distance of one pixel along the current pass; the other axis is zero.
    step: vec2<f32>,
    sigma: f32,
    stepped: f32,
};

@group(0) @binding(0) var backdrop: texture_2d<f32>;
@group(0) @binding(1) var linear_clamp_sampler: sampler;
@group(0) @binding(2) var<uniform> params: BlurParams;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vertex(@builtin(vertex_index) index: u32) -> VertexOutput {
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    var output: VertexOutput;
    output.position = vec4<f32>(uv * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0);
    output.uv = uv;
    return output;
}

@fragment
fn fragment(input: VertexOutput) -> @location(0) vec4<f32> {
    if params.stepped < 0.5 {
        // Adjacent texels, not increasingly sparse taps: large radii must not make ghost bands.
        let sigma = clamp(params.sigma, 0.01, 8.0);
        var sum = vec4<f32>(0.0);
        var weight_sum = 0.0;
        for (var i = -24; i <= 24; i += 1) {
            let offset = f32(i);
            let weight = exp(-0.5 * offset * offset / (sigma * sigma));
            sum += textureSampleLevel(backdrop, linear_clamp_sampler, input.uv + params.step * offset, 0.0) * weight;
            weight_sum += weight;
        }
        return sum / weight_sum;
    }
    // Explicit artistic mode, retained separately from the smooth material.
    let near_offset = params.step * params.sigma * 1.3846153846;
    let far_offset = params.step * params.sigma * 3.2307692308;
    var color = textureSample(backdrop, linear_clamp_sampler, input.uv) * 0.2270270270;
    color += textureSample(backdrop, linear_clamp_sampler, input.uv + near_offset) * 0.3162162162;
    color += textureSample(backdrop, linear_clamp_sampler, input.uv - near_offset) * 0.3162162162;
    color += textureSample(backdrop, linear_clamp_sampler, input.uv + far_offset) * 0.0702702703;
    color += textureSample(backdrop, linear_clamp_sampler, input.uv - far_offset) * 0.0702702703;
    return color;
}
