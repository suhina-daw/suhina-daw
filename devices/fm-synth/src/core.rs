// SPDX-License-Identifier: Apache-2.0

// Ported from MSFA (music-synthesizer-for-android) as found in Dexed: fm_core.cc and
// fm_op_kernel.cc.
// Copyright 2012 Google Inc.
// Licensed under the Apache License, Version 2.0 (see LICENSE-APACHE in this crate).
// Changes: translated to Rust; the experimental and NEON kernels were left out; sums wrap as
// the C code did on every platform it ran on; eleven four-operator algorithms added after the 32;
// helpers opened to Suhina's kernel (`kernel.rs`).

//! Six sine operators connected by one of 32 algorithms, rendered one engine block at a time.

use crate::tables::{LG_N, N, Tables};

/// One operator's state between blocks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OpParams {
    /// The level asked for this block, Q24 log; turned into `gain_out`.
    pub level_in: i32,
    /// The gain at the end of the last block, linear Q24; this block ramps from it.
    pub gain_out: i32,
    /// Phase step per sample.
    pub freq: i32,
    pub phase: i32,
}

pub const OUT_BUS_ADD: u8 = 1 << 2;
pub const FB_IN: u8 = 1 << 6;
pub const FB_OUT: u8 = 1 << 7;

/// Per algorithm, per operator (operator 6 first): bits 0-1 the bus written (0 is the
/// output), bit 2 add to it rather than replace, bits 4-5 the bus read (0 is none), bits 6-7
/// the feedback loop. Algorithms 33 to 43 connect operators 1 to 4; 5 and 6 are heard directly
/// when they are given a level.
const ALGORITHMS: [[u8; 6]; 43] = [
    [0xc1, 0x11, 0x11, 0x14, 0x01, 0x14], // 1
    [0x01, 0x11, 0x11, 0x14, 0xc1, 0x14], // 2
    [0xc1, 0x11, 0x14, 0x01, 0x11, 0x14], // 3
    [0xc1, 0x11, 0x94, 0x01, 0x11, 0x14], // 4
    [0xc1, 0x14, 0x01, 0x14, 0x01, 0x14], // 5
    [0xc1, 0x94, 0x01, 0x14, 0x01, 0x14], // 6
    [0xc1, 0x11, 0x05, 0x14, 0x01, 0x14], // 7
    [0x01, 0x11, 0xc5, 0x14, 0x01, 0x14], // 8
    [0x01, 0x11, 0x05, 0x14, 0xc1, 0x14], // 9
    [0x01, 0x05, 0x14, 0xc1, 0x11, 0x14], // 10
    [0xc1, 0x05, 0x14, 0x01, 0x11, 0x14], // 11
    [0x01, 0x05, 0x05, 0x14, 0xc1, 0x14], // 12
    [0xc1, 0x05, 0x05, 0x14, 0x01, 0x14], // 13
    [0xc1, 0x05, 0x11, 0x14, 0x01, 0x14], // 14
    [0x01, 0x05, 0x11, 0x14, 0xc1, 0x14], // 15
    [0xc1, 0x11, 0x02, 0x25, 0x05, 0x14], // 16
    [0x01, 0x11, 0x02, 0x25, 0xc5, 0x14], // 17
    [0x01, 0x11, 0x11, 0xc5, 0x05, 0x14], // 18
    [0xc1, 0x14, 0x14, 0x01, 0x11, 0x14], // 19
    [0x01, 0x05, 0x14, 0xc1, 0x14, 0x14], // 20
    [0x01, 0x14, 0x14, 0xc1, 0x14, 0x14], // 21
    [0xc1, 0x14, 0x14, 0x14, 0x01, 0x14], // 22
    [0xc1, 0x14, 0x14, 0x01, 0x14, 0x04], // 23
    [0xc1, 0x14, 0x14, 0x14, 0x04, 0x04], // 24
    [0xc1, 0x14, 0x14, 0x04, 0x04, 0x04], // 25
    [0xc1, 0x05, 0x14, 0x01, 0x14, 0x04], // 26
    [0x01, 0x05, 0x14, 0xc1, 0x14, 0x04], // 27
    [0x04, 0xc1, 0x11, 0x14, 0x01, 0x14], // 28
    [0xc1, 0x14, 0x01, 0x14, 0x04, 0x04], // 29
    [0x04, 0xc1, 0x11, 0x14, 0x04, 0x04], // 30
    [0xc1, 0x14, 0x04, 0x04, 0x04, 0x04], // 31
    [0xc4, 0x04, 0x04, 0x04, 0x04, 0x04], // 32
    [0x04, 0x04, 0x01, 0x11, 0x11, 0x14], // 33: 4 > 3 > 2 > 1
    [0x04, 0x04, 0x01, 0x05, 0x11, 0x14], // 34: (3 + 4) > 2 > 1
    [0x04, 0x04, 0x01, 0x11, 0x05, 0x14], // 35: (4 > 3 + 2) > 1
    [0x04, 0x04, 0x01, 0x05, 0x05, 0x14], // 36: (2 + 3 + 4) > 1
    [0x04, 0x04, 0x01, 0x11, 0x14, 0x04], // 37: 4 > 3 > 2, 1
    [0x04, 0x04, 0x01, 0x14, 0x01, 0x14], // 38: 4 > 3, 2 > 1
    [0x04, 0x04, 0x01, 0x14, 0x14, 0x14], // 39: 4 > (3, 2, 1)
    [0x04, 0x04, 0x01, 0x14, 0x04, 0x04], // 40: 4 > 3, 2, 1
    [0x04, 0x04, 0x01, 0x05, 0x14, 0x04], // 41: (3 + 4) > 2, 1
    [0x04, 0x04, 0x01, 0x14, 0x14, 0x04], // 42: 4 > (3, 2), 1
    [0x04, 0x04, 0x04, 0x04, 0x04, 0x04], // 43: all heard
];

pub fn flags(algorithm: usize, op: usize) -> u8 {
    ALGORITHMS.get(algorithm).and_then(|a| a.get(op)).copied().unwrap_or(0)
}

/// Whether operator `op` (0 is operator 6) is heard directly in `algorithm` (0 to 42).
pub fn is_carrier(algorithm: usize, op: usize) -> bool {
    flags(algorithm, op) & OUT_BUS_ADD != 0
}

/// Gains below this are silent: the operator is skipped.
pub const LEVEL_THRESHOLD: i32 = 1_120;

/// Scratch buses for one block, shared by every voice of a synth.
#[derive(Debug, Clone, Copy)]
pub struct Buses(pub [[i32; N]; 2]);

impl Default for Buses {
    fn default() -> Self {
        Self([[0; N]; 2])
    }
}

/// Renders one block of a voice into `output` (bus 0), adding where the algorithm says.
/// `fb_shift` 16 or more turns feedback off.
pub fn render(
    output: &mut [i32; N],
    params: &mut [OpParams; 6],
    algorithm: usize,
    fb: &mut [i32; 2],
    fb_shift: i32,
    buses: &mut Buses,
    t: &Tables,
) {
    let mut has_contents = [true, false, false];
    for (op, param) in params.iter_mut().enumerate() {
        let flags = flags(algorithm, op);
        let mut add = flags & OUT_BUS_ADD != 0;
        let in_bus = usize::from((flags >> 4) & 3);
        let out_bus = usize::from(flags & 3);
        let gain1 = param.gain_out;
        let gain2 = t.exp2(param.level_in.wrapping_sub(14 << 24));
        param.gain_out = gain2;

        if gain1 >= LEVEL_THRESHOLD || gain2 >= LEVEL_THRESHOLD {
            if !has_contents.get(out_bus).copied().unwrap_or(false) {
                add = false;
            }
            let modulated = in_bus != 0 && has_contents.get(in_bus).copied().unwrap_or(false);
            let [bus1, bus2] = &mut buses.0;
            let (out, input): (&mut [i32; N], Input) = match (out_bus, in_bus) {
                _ if !modulated => (pick(out_bus, output, bus1, bus2), Input::None),
                (0, 1) => (&mut *output, Input::Bus(&*bus1)),
                (0, _) => (&mut *output, Input::Bus(&*bus2)),
                (1, 2) => (bus1, Input::Bus(&*bus2)),
                (2, 1) => (bus2, Input::Bus(&*bus1)),
                _ => (pick(out_bus, output, bus1, bus2), Input::Itself),
            };
            let (phase, freq) = (param.phase, param.freq);
            match input {
                Input::Bus(input) => compute(out, Some(input), phase, freq, gain1, gain2, add, t),
                Input::Itself => compute(out, None, phase, freq, gain1, gain2, add, t),
                Input::None if flags & (FB_IN | FB_OUT) == FB_IN | FB_OUT && fb_shift < 16 => {
                    compute_fb(out, phase, freq, gain1, gain2, fb, fb_shift, add, t);
                }
                Input::None => compute_pure(out, phase, freq, gain1, gain2, add, t),
            }
            if let Some(slot) = has_contents.get_mut(out_bus) {
                *slot = true;
            }
        } else if !add && let Some(slot) = has_contents.get_mut(out_bus) {
            *slot = false;
        }
        param.phase = param.phase.wrapping_add(param.freq.wrapping_shl(LG_N));
    }
}

/// Where an operator's modulation comes from.
enum Input<'a> {
    None,
    Bus(&'a [i32; N]),
    /// The bus it writes to: each sample is read before it is written.
    Itself,
}

pub fn pick<'a>(
    bus: usize,
    output: &'a mut [i32; N],
    bus1: &'a mut [i32; N],
    bus2: &'a mut [i32; N],
) -> &'a mut [i32; N] {
    match bus {
        0 => output,
        1 => bus1,
        _ => bus2,
    }
}

/// Gain step per sample, so a block ramps from `gain1` to `gain2`.
pub fn gain_step(gain1: i32, gain2: i32) -> i32 {
    gain2.wrapping_sub(gain1).wrapping_add(N as i32 >> 1) >> LG_N
}

pub fn scale(y: i32, gain: i32) -> i32 {
    ((i64::from(y) * i64::from(gain)) >> 24) as i32
}

pub fn write(slot: &mut i32, y: i32, add: bool) {
    *slot = if add { slot.wrapping_add(y) } else { y };
}

/// An operator modulated by `input`, or by what `output` holds when `input` is `None`.
#[allow(clippy::too_many_arguments)]
fn compute(
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
fn compute_pure(output: &mut [i32; N], phase: i32, freq: i32, gain1: i32, gain2: i32, add: bool, t: &Tables) {
    let dgain = gain_step(gain1, gain2);
    let (mut gain, mut phase) = (gain1, phase);
    for slot in output.iter_mut() {
        gain = gain.wrapping_add(dgain);
        write(slot, scale(t.sin(phase), gain), add);
        phase = phase.wrapping_add(freq);
    }
}

/// An operator modulated by its own last two outputs.
#[allow(clippy::too_many_arguments)]
fn compute_fb(
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
    let shift = (fb_shift + 1).clamp(0, 31);
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
