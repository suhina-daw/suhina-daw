// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The picture behind the SDK's `Widget::Strands` (docs/spec/sdk-custom-ui.md › Reusable
//! functional widgets): ribbons of very fine dots that flow across a rectangle, narrowest at its
//! center, swell with a sound's modes and throw sparks at a strike. This module holds what both
//! renderers share: the inputs of a frame, where each dot goes and how it looks, and the host's
//! GPU provider. A host with a GPU draws hundreds of thousands of dots through [`Provider`]
//! (`suhina-ui-gpu`, with the same maths in WGSL); without one, `detail::strands` draws a few
//! thousand of them with [`place`], on the same paths.
//!
//! [`place`] works only with arithmetic and its own [`sin`], [`cos`] and [`exp`], never the
//! system's, so the picture it draws is the same to the last bit on every platform: the painted
//! digests that hold every theme's look are compared on Linux, Windows and macOS alike.

use std::sync::Arc;

use egui::{Context, Id, TextureId};

/// One frame's inputs, read from the widget's value expressions and the host's clock.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    /// The modes' energies, 0 to 1; modes the widget does not name are zero.
    pub modes: [f32; 6],
    /// Seconds since the last strike, and its strength.
    pub since: f32,
    pub velocity: f32,
    /// 0 to 1: the ribbons' spread, width and speed, and how strongly they show.
    pub height: f32,
    pub thickness: f32,
    pub speed: f32,
    pub presence: f32,
    /// The flow's own clock: it advances by the host's time at a rate that grows with the sound.
    pub clock: f32,
    /// Light, mid, shadow and thread, RGBA in 0 to 1 (gamma-encoded, as egui's colors are).
    pub colors: [[f32; 4]; 4],
}

impl Default for Frame {
    fn default() -> Self {
        Self {
            modes: [0.0; 6],
            since: 60.0,
            velocity: 0.0,
            height: 0.5,
            thickness: 0.5,
            speed: 0.5,
            presence: 1.0,
            clock: 0.0,
            colors: [[1.0; 4]; 4],
        }
    }
}

/// Each ribbon's phase, how far its own flow strays from the common one, and its width. The first
/// three carry the first mode and always flow; each further one rises with a mode of its own.
pub const RIBBONS: [(f32, f32, f32); 6] =
    [(0.0, 0.35, 1.0), (2.1, 0.5, 0.8), (4.3, 0.65, 0.6), (1.2, 0.8, 0.7), (3.4, 0.9, 0.55), (5.5, 1.0, 0.45)];

/// How many dots each ribbon has along the rectangle and across it: the GPU's and the lighter
/// version's.
pub const GPU_PLAN: [(u32, u32); 6] = [(2400, 44), (2400, 44), (2400, 44), (1500, 24), (1500, 24), (1500, 24)];
pub const CPU_PLAN: [(u32, u32); 6] = [(360, 6), (360, 6), (360, 6), (240, 4), (240, 4), (240, 4)];

/// The dots of a plan, each `[along 0..1, across -1..1, ribbon, seed 0..1]`, nudged off their grid
/// so no rows show. The same for a plan wherever it is made.
pub fn points(plan: &[(u32, u32); 6]) -> Vec<[f32; 4]> {
    let total: usize = plan.iter().map(|&(along, across)| along as usize * across as usize).sum();
    let mut out = Vec::with_capacity(total);
    let mut id: u32 = 0;
    for (ribbon, &(along, across)) in plan.iter().enumerate() {
        for i in 0..along {
            for f in 0..across {
                let u = ((i as f32 + (hash(id, 1) - 0.5) * 0.8) / (along.max(2) - 1) as f32).clamp(0.0, 1.0);
                let lane = (((f as f32 + hash(id, 2) * 0.7) / (across.max(2) - 1) as f32 - 0.5) * 2.0).clamp(-1.0, 1.0);
                out.push([u, lane, ribbon as f32, hash(id, 3)]);
                id = id.wrapping_add(1);
            }
        }
    }
    out
}

/// A number from 0 to 1 for a dot and a purpose, the same on every machine.
fn hash(id: u32, salt: u32) -> f32 {
    let mut x = id.wrapping_mul(0x9e37_79b9) ^ salt.wrapping_mul(0x85eb_ca6b);
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^= x >> 16;
    (x >> 8) as f32 / 16_777_216.0
}

/// A frame's values every dot shares, for a rectangle `width` × `height` (in the renderer's
/// units).
#[derive(Clone, Copy, Debug)]
pub struct Shared {
    pub width: f32,
    pub height: f32,
    pub mid: f32,
    /// How far the ribbons spread from the middle, and their width.
    pub reach: f32,
    pub ribbon_width: f32,
    pub clock: f32,
    pub swell: f32,
    /// The sparks' strength and how far they have flown, 0 to 1.
    pub burst: f32,
    pub burst_age: f32,
    pub strength: [f32; 6],
}

impl Frame {
    /// What every dot of this frame shares in a `width` × `height` rectangle.
    pub fn shared(&self, width: f32, height: f32) -> Shared {
        let m = |n: usize| unit(self.modes.get(n).copied().unwrap_or(0.0));
        let presence = unit(self.presence);
        let main = (0.8 + 0.2 * (m(0) * 1.6).min(1.0)) * presence;
        let rise = |n: usize| (m(n) * 3.0).min(1.0) * presence;
        let since = if self.since.is_finite() { self.since.max(0.0) } else { 60.0 };
        let velocity = unit(self.velocity);
        Shared {
            width,
            height,
            mid: height * 0.5,
            reach: height * 0.42 * (0.6 + 0.8 * unit(self.height)),
            ribbon_width: height * 0.19 * (0.7 + 0.8 * unit(self.thickness)),
            clock: if self.clock.is_finite() { self.clock } else { 0.0 },
            swell: (m(0) * 1.2 + m(1)).min(1.5),
            burst: velocity * exp(-since * 1.4) * (0.6 + 0.6 * unit(self.speed)),
            burst_age: 1.0 - exp(-since * 2.2),
            strength: [main, main * 0.8, main * 0.6, rise(1), rise(2), rise(3)],
        }
    }
}

/// `v` within 0 to 1, and 0 for a value that is not finite.
pub fn unit(v: f32) -> f32 {
    if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 }
}

/// How fast the flow's clock runs for these energies and Speed: slow at rest, faster as it sounds.
pub fn clock_rate(modes: &[f32; 6], speed: f32) -> f32 {
    (0.35 + 1.4 * (unit(modes[0]) + 0.5 * unit(modes[1]))) * (0.8 + 0.6 * unit(speed))
}

/// One dot as drawn: its center, its size (in the rectangle's units, at one unit a point), its
/// color (gamma-encoded, premultiplied by its cover) and whether it lies in front.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dot {
    pub x: f32,
    pub y: f32,
    pub size: f32,
    pub color: [f32; 4],
    pub front: bool,
}

/// Three waves whose lengths do not divide each other: a flow that never repeats.
fn flow(u: f32, c: f32, p: f32) -> f32 {
    0.55 * sin(u * 5.3 + c * 0.61 + p)
        + 0.30 * sin(u * 9.7 - c * 0.93 + p * 1.7)
        + 0.15 * sin(u * 17.1 + c * 1.37 + p * 2.3)
}

fn mix(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}

/// Where a dot goes in this frame and how it looks; `None` when it does not show.
pub fn place(point: [f32; 4], shared: &Shared, colors: &[[f32; 4]; 4]) -> Option<Dot> {
    let [u, lane, ribbon, seed] = point;
    let r = (ribbon as usize).min(5);
    let (phase, amp, wide) = RIBBONS[r];
    let strength = shared.strength[r];
    if strength < 0.03 {
        return None;
    }
    let c = shared.clock;
    let d = (u - 0.5) / 0.19;
    let body = 0.08 + 0.92 * exp(-d * d);
    let own = seed * std::f32::consts::TAU + lane * 1.7;
    let center = flow(u, c, phase) * (0.7 + 0.5 * shared.swell)
        + 0.55 * (0.6 + shared.swell) * flow(u * 1.31 + 0.2, c * 1.21, own);
    let z = cos(u * 4.1 - c * 0.53 + own * 0.6) * 0.9;
    let persp = 1.0 + 0.25 * z;
    let mut x = shared.width * u;
    let mut y = shared.mid
        + (shared.reach * body * center * (0.6 + 0.4 * amp) + lane * shared.ribbon_width * wide * body * 0.35) * persp;
    let mut fade = 1.0;
    // A strike throws a share of the dots out of the center as a spreading shell of sparks.
    if seed > 0.8 {
        let from_x = x - shared.width * 0.5;
        let from_y = y - shared.mid;
        let dist = (from_x * from_x + from_y * from_y).sqrt() + 1.0;
        let reach = shared.burst_age * shared.reach * 2.2 * (0.5 + (seed - 0.8) * 2.5);
        let push = reach * exp(-dist / (shared.reach * 2.5).max(1.0)) / dist;
        x += from_x * push;
        y += from_y * push;
        if seed > 0.9 {
            fade = shared.burst;
        }
    }
    if fade < 0.02 {
        return None;
    }
    let near = (z + 1.0) * 0.5;
    let blur = (-z - 0.25).max(0.0) * 1.4;
    let face = sin(u * 9.0 + c * 0.8 + own);
    let light = (0.5 + 0.35 * face + 0.25 * z).clamp(0.0, 1.0);
    let [lit, midtone, shadow, thread] = *colors;
    let mut color =
        if light < 0.5 { mix(shadow, midtone, light * 2.0) } else { mix(midtone, lit, (light - 0.5) * 2.0) };
    let mut cover = 0.08 + 0.3 * near;
    if seed < 0.14 {
        color = mix(mix(thread, shadow, 0.45), thread, light);
        cover *= 1.3;
    }
    if seed > 0.9 {
        color = mix(thread, lit, 0.5);
    }
    let alpha = (strength * fade * (1.0 - lane.abs() * 0.45) * cover * (0.7 + 0.5 * body) / (1.0 + blur * 1.4))
        .clamp(0.0, 1.0)
        * color[3];
    if alpha < 0.002 {
        return None;
    }
    Some(Dot {
        x,
        y,
        size: (1.0 + 0.6 * near) * (1.0 + blur),
        color: [color[0] * alpha, color[1] * alpha, color[2] * alpha, alpha],
        front: z >= 0.0,
    })
}

const PI: f32 = std::f32::consts::PI;

/// `sin`, from arithmetic alone: the same result on every machine.
pub fn sin(x: f32) -> f32 {
    if !x.is_finite() {
        return 0.0;
    }
    // 2π in two parts, the first exact in few bits, so taking whole turns off loses nothing.
    const TURN_HIGH: f32 = 6.281_25;
    const TURN_LOW: f32 = 0.001_935_307_2;
    let turns = (x / (2.0 * PI)).round();
    let mut t = (x - turns * TURN_HIGH) - turns * TURN_LOW;
    if t > PI / 2.0 {
        t = PI - t;
    } else if t < -PI / 2.0 {
        t = -PI - t;
    }
    let t2 = t * t;
    t * (1.0 - t2 / 6.0 * (1.0 - t2 / 20.0 * (1.0 - t2 / 42.0 * (1.0 - t2 / 72.0 * (1.0 - t2 / 110.0)))))
}

/// `cos`, from arithmetic alone.
pub fn cos(x: f32) -> f32 {
    sin(x + PI / 2.0)
}

/// `e^x` for the dots' falls (`x` at most a little above zero), from arithmetic alone.
pub fn exp(x: f32) -> f32 {
    if x.is_nan() {
        return 0.0;
    }
    let t = (x * std::f32::consts::LOG2_E).clamp(-126.0, 126.0);
    let whole = t.floor();
    let f = t - whole;
    // 2^f on 0..1, to about 1e-7.
    let ln2 = std::f32::consts::LN_2;
    let y = f * ln2;
    let fraction = 1.0 + y * (1.0 + y / 2.0 * (1.0 + y / 3.0 * (1.0 + y / 4.0 * (1.0 + y / 5.0 * (1.0 + y / 6.0)))));
    f32::from_bits((((whole as i32) + 127) as u32) << 23) * fraction
}

/// The host side that draws a frame on the GPU into a texture egui can show.
pub trait Provider: Send + Sync {
    /// Draws `frame` into a `size` (physical pixels) texture for the widget `id`, transparent
    /// where no dot is, and returns it; `None` leaves the lighter version to the caller.
    fn draw(&self, ctx: &Context, id: Id, size: [u32; 2], frame: &Frame) -> Option<TextureId>;
}

#[derive(Clone)]
struct Host(Arc<dyn Provider>);

const HOST: &str = "strands-host";

/// Installs the GPU host for every `Strands` the window draws.
pub fn install(ctx: &Context, provider: Arc<dyn Provider>) {
    ctx.data_mut(|data| data.insert_temp(Id::unique(HOST), Host(provider)));
}

/// The installed GPU host, if any.
pub fn provider(ctx: &Context) -> Option<Arc<dyn Provider>> {
    ctx.data(|data| data.get_temp::<Host>(Id::unique(HOST))).map(|host| host.0)
}

#[cfg(test)]
#[path = "strands_tests.rs"]
mod tests;
