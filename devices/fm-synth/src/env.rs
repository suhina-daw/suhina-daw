// SPDX-License-Identifier: Apache-2.0

// Ported from MSFA (music-synthesizer-for-android) as found in Dexed: env.cc and pitchenv.cc.
// Copyright 2012-2013 Google Inc.; Copyright 2017 Pascal Gauthier.
// Licensed under the Apache License, Version 2.0 (see LICENSE-APACHE in this crate).
// Changes: translated to Rust; the rising step is computed in 64 bits (it overflowed in C below
// about 16 kHz); `retune` replaces `update`, keeping the stage an envelope is in when a
// parameter changes instead of jumping to stage 3; a released level 4 above zero falls away after
// a second, where the original held it for as long as the voice was not taken.

//! The operator envelope and the pitch envelope: four rates and four levels (0 to 99) each,
//! advanced once per engine block.

use crate::tables::{LG_N, N, Tables};

/// How long a released envelope holds a level 4 above zero, in samples at 44.1 kHz.
const TAIL_HOLD: i32 = 44_100;
/// How far it then falls per block at 44.1 kHz: from full to silence in about two seconds.
const TAIL_FALL: i32 = 191_000;
/// The level an envelope is silent at: what `advance` never goes under.
const SILENT: i32 = 16 << 16;

const LEVEL_LUT: [i32; 20] = [0, 5, 9, 13, 17, 20, 23, 25, 27, 29, 31, 33, 35, 37, 39, 41, 42, 43, 45, 46];

/// Samples at 44.1 kHz a stage takes when its level does not change, measured on hardware up
/// to rate 76; faster rates use `20 * (99 - rate)`.
const STATICS: [i32; 77] = [
    1_764_000, 1_764_000, 1_411_200, 1_411_200, 1_190_700, 1_014_300, 992_250, 882_000, 705_600, 705_600, 584_325,
    507_150, 502_740, 441_000, 418_950, 352_800, 308_700, 286_650, 253_575, 220_500, 220_500, 176_400, 145_530,
    145_530, 125_685, 110_250, 110_250, 88_200, 88_200, 74_970, 61_740, 61_740, 55_125, 48_510, 44_100, 37_485, 31_311,
    30_870, 27_562, 27_562, 22_050, 18_522, 17_640, 15_435, 14_112, 13_230, 11_025, 9_261, 9_261, 7_717, 6_615, 6_615,
    5_512, 5_512, 4_410, 3_969, 3_969, 3_439, 2_866, 2_690, 2_249, 1_984, 1_896, 1_808, 1_411, 1_367, 1_234, 1_146,
    926, 837, 837, 705, 573, 573, 529, 441, 441,
];

/// An output level 0..=99 in the engine's 0..=127 steps (about 0.75 dB each).
pub fn scale_out_level(level: i32) -> i32 {
    if level >= 20 { 28 + level } else { LEVEL_LUT.get(level.max(0) as usize).copied().unwrap_or(0) }
}

/// The operator envelope. Its level is Q24 log (1 << 24 per doubling), with 16 more bits of
/// fraction than the hardware kept.
#[derive(Debug, Clone, Copy, Default)]
pub struct Env {
    initialised: bool,
    rates: [i32; 4],
    levels: [i32; 4],
    out_level: i32,
    rate_scaling: i32,
    level: i32,
    target: i32,
    rising: bool,
    stage: usize,
    inc: i32,
    static_count: i32,
    down: bool,
    /// Samples at 44.1 kHz since it reached a level 4 above zero.
    tail: i32,
}

impl Env {
    /// Starts the envelope for a key going down. `out_level` is in microsteps (99 * 32 is
    /// full scale), `rate_scaling` in quarter-rate steps (0 to 63).
    pub fn init(&mut self, rates: [i32; 4], levels: [i32; 4], out_level: i32, rate_scaling: i32, t: &Tables) {
        self.initialised = true;
        self.rates = rates;
        self.levels = levels;
        self.out_level = out_level;
        self.rate_scaling = rate_scaling;
        self.level = 0;
        self.down = true;
        self.tail = 0;
        self.advance(0, t);
    }

    /// Takes new rates and levels while it runs: the stage it is in heads for its new level at
    /// its new rate, and a stage that was waiting keeps waiting.
    pub fn retune(&mut self, rates: [i32; 4], levels: [i32; 4], out_level: i32, rate_scaling: i32, t: &Tables) {
        self.rates = rates;
        self.levels = levels;
        self.out_level = out_level;
        self.rate_scaling = rate_scaling;
        if self.stage < 4 {
            let waiting = self.static_count;
            self.advance(self.stage, t);
            self.static_count = waiting;
        }
    }

    /// The level for the next block, Q24 log.
    pub fn next(&mut self, t: &Tables) -> i32 {
        if self.static_count != 0 {
            self.static_count -= N as i32;
            if self.static_count <= 0 {
                self.static_count = 0;
                self.advance(self.stage + 1, t);
            }
        }
        if (self.stage < 3 || (self.stage < 4 && !self.down)) && self.static_count == 0 {
            if self.rising {
                const JUMP_TARGET: i32 = 1_716;
                if self.level < (JUMP_TARGET << 16) {
                    self.level = JUMP_TARGET << 16;
                }
                let step = (i64::from((17 << 24) - self.level) >> 24) * i64::from(self.inc);
                self.level = (i64::from(self.level) + step).clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
                if self.level >= self.target {
                    self.level = self.target;
                    self.advance(self.stage + 1, t);
                }
            } else {
                self.level = self.level.saturating_sub(self.inc);
                if self.level <= self.target {
                    self.level = self.target;
                    self.advance(self.stage + 1, t);
                }
            }
        } else if self.stage >= 4 && self.levels[3] > 0 {
            // Past its release: a note ends however high its level 4 is.
            let rate = i64::from(t.env_rate);
            self.tail = self.tail.saturating_add(((N as i64 * rate) >> 24) as i32);
            if self.tail >= TAIL_HOLD {
                let fall = ((i64::from(TAIL_FALL) * rate) >> 24) as i32;
                self.level = self.level.saturating_sub(fall).max(SILENT);
            }
        }
        self.level
    }

    pub fn key_down(&mut self, down: bool, t: &Tables) {
        if self.down != down {
            self.down = down;
            self.advance(if down { 0 } else { 3 }, t);
        }
    }

    /// Whether it still sounds: started, and either before the end of stage 4 or falling from a
    /// level 4 above zero.
    pub fn is_active(&self) -> bool {
        self.initialised && (self.stage < 4 || (self.levels[3] > 0 && self.level > SILENT))
    }

    fn advance(&mut self, stage: usize, t: &Tables) {
        self.stage = stage;
        let (Some(&new_level), Some(&rate)) = (self.levels.get(stage), self.rates.get(stage)) else {
            return;
        };
        let actual = scale_out_level(new_level) >> 1;
        let actual = ((actual << 6) + self.out_level - 4_256).max(16);
        self.target = actual << 16;
        self.rising = self.target > self.level;

        let qrate = (((rate * 41) >> 6) + self.rate_scaling).clamp(0, 63);
        if self.target == self.level || (stage == 0 && new_level == 0) {
            // A stage that does not move still takes time.
            let static_rate = (rate + self.rate_scaling).clamp(0, 99);
            let mut count = match STATICS.get(static_rate as usize) {
                Some(&samples) => samples,
                None => 20 * (99 - static_rate),
            };
            if static_rate < 77 && stage == 0 && new_level == 0 {
                count /= 20; // attack is scaled faster
            }
            self.static_count = ((i64::from(count) * i64::from(t.env_rate)) >> 24) as i32;
        } else {
            self.static_count = 0;
        }
        let inc = (4 + (qrate & 3)) << (2 + LG_N as i32 + (qrate >> 2));
        self.inc = ((i64::from(inc) * i64::from(t.env_rate)) >> 24) as i32;
    }
}

const PITCH_RATE: [u8; 100] = [
    1, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13, 14, 14, 15, 16, 16, 17, 18, 18, 19,
    20, 21, 22, 23, 24, 25, 26, 27, 28, 30, 31, 33, 34, 36, 37, 38, 39, 41, 42, 44, 46, 47, 49, 51, 53, 54, 56, 58, 60,
    62, 64, 66, 68, 70, 72, 74, 76, 79, 82, 85, 88, 91, 94, 98, 102, 106, 110, 115, 120, 125, 130, 135, 141, 147, 153,
    159, 165, 171, 178, 185, 193, 202, 211, 232, 243, 254, 255,
];

const PITCH_LEVEL: [i8; 100] = [
    -128, -116, -104, -95, -85, -76, -68, -61, -56, -52, -49, -46, -43, -41, -39, -37, -35, -33, -32, -31, -30, -29,
    -28, -27, -26, -25, -24, -23, -22, -21, -20, -19, -18, -17, -16, -15, -14, -13, -12, -11, -10, -9, -8, -7, -6, -5,
    -4, -3, -2, -1, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26,
    27, 28, 29, 30, 31, 32, 33, 34, 35, 38, 40, 43, 46, 49, 53, 58, 65, 73, 82, 92, 103, 115, 127,
];

fn pitch_level(level: i32) -> i32 {
    i32::from(PITCH_LEVEL.get(level.clamp(0, 99) as usize).copied().unwrap_or(0)) << 19
}

/// The pitch envelope; its output is Q24 per octave.
#[derive(Debug, Clone, Copy, Default)]
pub struct PitchEnv {
    rates: [i32; 4],
    levels: [i32; 4],
    level: i32,
    target: i32,
    rising: bool,
    stage: usize,
    inc: i32,
    unit: i32,
    down: bool,
}

impl PitchEnv {
    pub fn set(&mut self, rates: [i32; 4], levels: [i32; 4], t: &Tables) {
        self.rates = rates;
        self.levels = levels;
        self.level = pitch_level(levels[3]);
        self.down = true;
        self.unit = t.pitch_unit;
        self.advance(0);
    }

    /// Takes new rates and levels while it runs, keeping its stage.
    pub fn retune(&mut self, rates: [i32; 4], levels: [i32; 4]) {
        self.rates = rates;
        self.levels = levels;
        if self.stage < 4 {
            self.advance(self.stage);
        }
    }

    pub fn next(&mut self) -> i32 {
        if self.stage < 3 || (self.stage < 4 && !self.down) {
            if self.rising {
                self.level = self.level.saturating_add(self.inc);
                if self.level >= self.target {
                    self.level = self.target;
                    self.advance(self.stage + 1);
                }
            } else {
                self.level = self.level.saturating_sub(self.inc);
                if self.level <= self.target {
                    self.level = self.target;
                    self.advance(self.stage + 1);
                }
            }
        }
        self.level
    }

    pub fn key_down(&mut self, down: bool) {
        if self.down != down {
            self.down = down;
            self.advance(if down { 0 } else { 3 });
        }
    }

    fn advance(&mut self, stage: usize) {
        self.stage = stage;
        let (Some(&level), Some(&rate)) = (self.levels.get(stage), self.rates.get(stage)) else {
            return;
        };
        self.target = pitch_level(level);
        self.rising = self.target > self.level;
        let rate = i32::from(PITCH_RATE.get(rate.clamp(0, 99) as usize).copied().unwrap_or(0));
        self.inc = rate.saturating_mul(self.unit);
    }
}
