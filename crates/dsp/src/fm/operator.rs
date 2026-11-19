// SPDX-License-Identifier: Apache-2.0

// Ported from MSFA (music-synthesizer-for-android) as found in Dexed: fm_core.cc and
// fm_op_kernel.cc.
// Copyright 2012 Google Inc.
// Licensed under the Apache License, Version 2.0 (see LICENSE-APACHE in this crate).
// Changes: translated to Rust; the experimental and NEON kernels were left out; sums wrap as
// the C code did on every platform it ran on. Extracted from the FM device for reuse;
// feedback-shift addition saturates before clamping to avoid overflow on external input.

//! Fixed-point sine operators with caller-owned buffers and feedback history.

use super::{LG_N, N, Tables};

/// Gain step per sample, so a block ramps from `gain1` to `gain2`.
pub fn gain_step(gain1: i32, gain2: i32) -> i32 {
    gain2.wrapping_sub(gain1).wrapping_add(N as i32 >> 1) >> LG_N
}

/// Multiply a Q24 sample by a linear Q24 gain, retaining the low 32 output bits.
pub fn scale(y: i32, gain: i32) -> i32 {
    ((i64::from(y) * i64::from(gain)) >> 24) as i32
}

/// Replace a sample, or mix it with wrapping addition.
pub fn write(slot: &mut i32, y: i32, add: bool) {
    *slot = if add { slot.wrapping_add(y) } else { y };
}

/// An operator modulated by `input`, or by what `output` holds when `input` is `None`.
#[allow(clippy::too_many_arguments)]
pub fn render_modulated(
    output: &mut [i32; N],
    input: Option<&[i32; N]>,
    phase: i32,
    freq: i32,
    gain1: i32,
    gain2: i32,
    add: bool,
    t: &Tables,
) {
    let dgain = gain_step(gain1, gain2);
    let (mut gain, mut phase) = (gain1, phase);
    for (i, slot) in output.iter_mut().enumerate() {
        gain = gain.wrapping_add(dgain);
        let modulation = input.map_or(*slot, |input| input.get(i).copied().unwrap_or(0));
        let y = t.sin(phase.wrapping_add(modulation));
        write(slot, scale(y, gain), add);
        phase = phase.wrapping_add(freq);
    }
}

/// An unmodulated sine.
#[allow(clippy::too_many_arguments)]
pub fn render_sine(output: &mut [i32; N], phase: i32, freq: i32, gain1: i32, gain2: i32, add: bool, t: &Tables) {
    let dgain = gain_step(gain1, gain2);
    let (mut gain, mut phase) = (gain1, phase);
    for slot in output.iter_mut() {
        gain = gain.wrapping_add(dgain);
        write(slot, scale(t.sin(phase), gain), add);
        phase = phase.wrapping_add(freq);
    }
}

/// An operator modulated by its own last two outputs.
///
/// `fb` stores the preceding two scaled outputs, oldest first, and is updated in place.
/// The effective shift is `fb_shift.saturating_add(1).clamp(0, 31)`. To disable feedback,
/// call [`render_sine`] instead; no shift value switches it off.
#[allow(clippy::too_many_arguments)]
pub fn render_feedback(
    output: &mut [i32; N],
    phase: i32,
    freq: i32,
    gain1: i32,
    gain2: i32,
    fb: &mut [i32; 2],
    fb_shift: i32,
    add: bool,
    t: &Tables,
) {
    let dgain = gain_step(gain1, gain2);
    let (mut gain, mut phase) = (gain1, phase);
    let [mut y0, mut y] = *fb;
    let shift = fb_shift.saturating_add(1).clamp(0, 31);
    for slot in output.iter_mut() {
        gain = gain.wrapping_add(dgain);
        let scaled = y0.wrapping_add(y) >> shift;
        y0 = y;
        y = scale(t.sin(phase.wrapping_add(scaled)), gain);
        write(slot, y, add);
        phase = phase.wrapping_add(freq);
    }
    *fb = [y0, y];
}
