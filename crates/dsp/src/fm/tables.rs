// SPDX-License-Identifier: Apache-2.0

// Ported from MSFA (music-synthesizer-for-android) as found in Dexed: sin.cc, exp2.cc,
// freqlut.cc, lfo.cc, pitchenv.cc, env.cc and porta.cpp.
// Copyright 2012-2013 Google Inc.; Copyright 2017 Pascal Gauthier; Copyright 2019 Jean Pierre
// Cimalando.
// Licensed under the Apache License, Version 2.0 (see LICENSE-APACHE in this crate).
// Changes: translated to Rust; the global tables became one struct per synth, built off the
// audio thread; arithmetic that overflowed in C at low sample rates is done in 64 bits.
// Moved to shared DSP; invalid and sub-1-kHz rates use the shared 48-kHz fallback.

//! Lookup tables and the constants that depend on the sample rate.
//!
//! Number formats follow the engine: a phase is a 32-bit integer whose low 24 bits are one
//! cycle, a level or a frequency is "Q24 log", where 1 << 24 is one doubling (an octave or
//! 6 dB), and a gain is linear Q24 (1 << 24 is unity).

/// Frames per engine block: envelopes, the LFO and modulation advance once per block, and
/// gains ramp across it.
pub const LG_N: u32 = 6;
pub const N: usize = 1 << LG_N;

const SIN_LG: u32 = 10;
const SIN_SIZE: usize = 1 << SIN_LG;
const EXP2_LG: u32 = 10;
const EXP2_SIZE: usize = 1 << EXP2_LG;
const FREQ_LG: u32 = 10;
const FREQ_SIZE: usize = 1 << FREQ_LG;
const MAX_LOGFREQ_INT: i32 = 20;

/// Portamento steps per CC value 0..=127.
pub const PORTA_STEPS: usize = 128;

/// Tables and per-sample-rate constants for one synth.
pub struct Tables {
    /// Pairs of (slope, value) for linear interpolation.
    sin: [i32; SIN_SIZE * 2],
    exp2: [i32; EXP2_SIZE * 2],
    freq: [i64; FREQ_SIZE + 1],
    /// LFO delay step per block.
    pub lfo_unit: u32,
    /// LFO phase step per block for 1 Hz, scaled as the speed table expects.
    pub lfo_ratio: u32,
    /// Pitch envelope step per block for a rate of 1.
    pub pitch_unit: i32,
    /// 44.1 kHz over the sample rate, Q24: envelope rates were measured at 44.1 kHz.
    pub env_rate: u32,
    pub porta: [i32; PORTA_STEPS],
    pub porta_glissando: [i32; PORTA_STEPS],
}

impl Tables {
    /// Prepare lookup data off the audio thread. Non-finite rates and rates below
    /// 1 kHz use 48 kHz, matching the shared DSP sample-rate policy.
    pub fn new(sample_rate: f64) -> Self {
        let sample_rate = crate::usable_sample_rate(sample_rate);
        let block = N as f64;
        let mut porta = [0; PORTA_STEPS];
        let mut porta_glissando = [0; PORTA_STEPS];
        let step = f64::from((1i32 << 24) / 12);
        for (i, (rate, gliss)) in porta.iter_mut().zip(porta_glissando.iter_mut()).enumerate() {
            // Semitones travelled per second, then per block, in pitch units.
            let per_second = 2_100.0 * 2f64.powf(-0.062 * i as f64);
            *rate = (0.5 + step * per_second / sample_rate * block) as i32;
            let per_second = 1_300.0 * 2f64.powf(-0.062 * i as f64);
            *gliss = (0.5 + step * per_second / sample_rate * block) as i32;
        }
        Self {
            sin: sin_table(),
            exp2: exp2_table(),
            freq: freq_table(sample_rate),
            // 1 << 32 / 15.5 s / 11
            lfo_unit: (block * 25_190_424.0 / sample_rate + 0.5) as u32,
            lfo_ratio: (4_437_500_000.0 * block / sample_rate) as u32,
            pitch_unit: (block * f64::from(1 << 24) / (21.3 * sample_rate) + 0.5) as i32,
            env_rate: (44_100.0 / sample_rate * f64::from(1 << 24)) as u32,
            porta,
            porta_glissando,
        }
    }

    /// Sine of a phase (1 << 24 is one cycle), Q24.
    #[inline]
    pub fn sin(&self, phase: i32) -> i32 {
        const SHIFT: u32 = 24 - SIN_LG;
        let low = i64::from(phase & ((1 << SHIFT) - 1));
        let index = ((phase >> (SHIFT - 1)) & (((SIN_SIZE - 1) << 1) as i32)) as usize;
        let slope = self.sin.get(index).copied().unwrap_or(0);
        let value = self.sin.get(index + 1).copied().unwrap_or(0);
        value.wrapping_add(((i64::from(slope) * low) >> SHIFT) as i32)
    }

    /// 2 to the power of `x` (Q24), Q24.
    #[inline]
    pub fn exp2(&self, x: i32) -> i32 {
        const SHIFT: u32 = 24 - EXP2_LG;
        let low = i64::from(x & ((1 << SHIFT) - 1));
        let index = ((x >> (SHIFT - 1)) & (((EXP2_SIZE - 1) << 1) as i32)) as usize;
        let slope = self.exp2.get(index).copied().unwrap_or(0);
        let value = self.exp2.get(index + 1).copied().unwrap_or(0);
        let y = value.wrapping_add(((i64::from(slope) * low) >> SHIFT) as i32);
        y >> (6 - (x >> 24)).clamp(0, 31)
    }

    /// Phase step per sample for a Q24 log frequency (1 << 24 is one octave).
    #[inline]
    pub fn freq(&self, logfreq: i32) -> i32 {
        const SHIFT: u32 = 24 - FREQ_LG;
        let index = ((logfreq & 0xff_ffff) >> SHIFT) as usize;
        let y0 = self.freq.get(index).copied().unwrap_or(0);
        let y1 = self.freq.get(index + 1).copied().unwrap_or(0);
        let low = i64::from(logfreq & ((1 << SHIFT) - 1));
        let y = y0 + (((y1 - y0) * low) >> SHIFT);
        (y >> (MAX_LOGFREQ_INT - (logfreq >> 24)).clamp(0, 63)) as i32
    }
}

fn sin_table() -> [i32; SIN_SIZE * 2] {
    const R: i64 = 1 << 29;
    let mut table = [0i32; SIN_SIZE * 2];
    let dphase = std::f64::consts::TAU / SIN_SIZE as f64;
    let c = (dphase.cos() * f64::from(1 << 30) + 0.5).floor() as i64;
    let s = (dphase.sin() * f64::from(1 << 30) + 0.5).floor() as i64;
    let (mut u, mut v): (i64, i64) = (1 << 30, 0);
    for i in 0..SIN_SIZE / 2 {
        let value = ((v + 32) >> 6) as i32;
        if let Some(slot) = table.get_mut((i << 1) + 1) {
            *slot = value;
        }
        if let Some(slot) = table.get_mut(((i + SIN_SIZE / 2) << 1) + 1) {
            *slot = -value;
        }
        let t = (u * s + v * c + R) >> 30;
        u = (u * c - v * s + R) >> 30;
        v = t;
    }
    slopes(&mut table, |last| -last);
    table
}

fn exp2_table() -> [i32; EXP2_SIZE * 2] {
    let mut table = [0i32; EXP2_SIZE * 2];
    let inc = 2f64.powf(1.0 / EXP2_SIZE as f64);
    let mut y = f64::from(1 << 30);
    for i in 0..EXP2_SIZE {
        if let Some(slot) = table.get_mut((i << 1) + 1) {
            *slot = (y + 0.5).floor() as i32;
        }
        y *= inc;
    }
    slopes(&mut table, |last| ((1i64 << 31) - i64::from(last)) as i32);
    table
}

/// Fills the even slots of a (slope, value) table with the difference to the next value; the
/// last slope comes from `last_slope(last value)`.
fn slopes<const S: usize>(table: &mut [i32; S], last_slope: impl Fn(i32) -> i32) {
    let pairs = S / 2;
    for i in 0..pairs.saturating_sub(1) {
        let next = table.get((i << 1) + 3).copied().unwrap_or(0);
        let this = table.get((i << 1) + 1).copied().unwrap_or(0);
        if let Some(slot) = table.get_mut(i << 1) {
            *slot = next.wrapping_sub(this);
        }
    }
    let last = table.get(S - 1).copied().unwrap_or(0);
    if let Some(slot) = table.get_mut(S - 2) {
        *slot = last_slope(last);
    }
}

fn freq_table(sample_rate: f64) -> [i64; FREQ_SIZE + 1] {
    let mut table = [0i64; FREQ_SIZE + 1];
    let mut y = (1i64 << (24 + MAX_LOGFREQ_INT)) as f64 / sample_rate;
    let inc = 2f64.powf(1.0 / FREQ_SIZE as f64);
    for slot in &mut table {
        *slot = (y + 0.5).floor() as i64;
        y *= inc;
    }
    table
}
