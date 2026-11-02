// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Operator waveforms other than the engine's sine: tables with fewer partials at higher pitches,
//! and the drawn waves, whose partials change a few at a time on the audio thread.

use std::sync::LazyLock;

use crate::params::{HARMONICS, WAVES};

/// Samples in a table.
pub const TABLE: usize = 2048;
const TABLE_LG: u32 = 11;
/// One table per partial limit: 64, 32, 16, 8, 4, 2, 1.
const LEVELS: usize = 7;

type Levels = [[f32; TABLE]; LEVELS];

/// Index of the sine in [`WAVES`], which the engine's own table plays.
pub const SINE: usize = 0;
/// Index of white noise in [`WAVES`], which no table holds.
pub const NOISE_WHITE: usize = WAVES.len() - 2;
/// Index of the drawn wave in [`WAVES`].
pub const DRAWN: usize = WAVES.len() - 1;

/// What a built-in wave is made of.
#[derive(Clone, Copy)]
enum Shape {
    Sine,
    Bits(u32),
    Saw(usize),
    SawDigital,
    Square(usize),
    SquareDigital,
    Triangle(usize),
    NoiseLoop,
    None,
}

/// In the order of [`WAVES`].
const SHAPES: [Shape; WAVES.len()] = [
    Shape::Sine,
    Shape::Bits(4),
    Shape::Bits(8),
    Shape::SawDigital,
    Shape::Saw(3),
    Shape::Saw(4),
    Shape::Saw(6),
    Shape::Saw(8),
    Shape::Saw(16),
    Shape::Saw(32),
    Shape::Saw(64),
    Shape::SquareDigital,
    Shape::Square(4),
    Shape::Square(6),
    Shape::Square(8),
    Shape::Square(16),
    Shape::Square(32),
    Shape::Square(64),
    Shape::Triangle(16),
    Shape::Triangle(64),
    Shape::NoiseLoop,
    Shape::None,
    Shape::None,
];

fn additive(partial: impl Fn(usize) -> f64, limit: usize) -> [f32; TABLE] {
    let mut table = [0.0f64; TABLE];
    for h in 1..=limit {
        let a = partial(h);
        if a != 0.0 {
            for (i, v) in table.iter_mut().enumerate() {
                *v += a * (std::f64::consts::TAU * (h * i) as f64 / TABLE as f64).sin();
            }
        }
    }
    let peak = table.iter().fold(0.0f64, |m, v| m.max(v.abs())).max(1e-9);
    table.map(|v| (v / peak) as f32)
}

fn level(shape: Shape, limit: usize) -> [f32; TABLE] {
    let unit = |i: usize| i as f64 / TABLE as f64;
    let mut table = [0.0f32; TABLE];
    match shape {
        Shape::Sine | Shape::None => {}
        Shape::Bits(bits) => {
            let steps = f64::from((1u32 << bits) / 2);
            for (i, v) in table.iter_mut().enumerate() {
                *v = (((std::f64::consts::TAU * unit(i)).sin() * steps).round() / steps) as f32;
            }
        }
        Shape::Saw(count) => {
            return additive(|h| 1.0 / h as f64, count.min(limit));
        }
        Shape::SawDigital => {
            for (i, v) in table.iter_mut().enumerate() {
                *v = (2.0 * unit(i) - 1.0) as f32;
            }
        }
        Shape::Square(count) => {
            return additive(|h| if h % 2 == 1 { 1.0 / h as f64 } else { 0.0 }, count.min(limit));
        }
        Shape::SquareDigital => {
            for (i, v) in table.iter_mut().enumerate() {
                *v = if i < TABLE / 2 { 1.0 } else { -1.0 };
            }
        }
        Shape::Triangle(count) => {
            let partial = |h: usize| {
                if h % 2 == 0 {
                    0.0
                } else if h % 4 == 1 {
                    1.0 / (h * h) as f64
                } else {
                    -1.0 / (h * h) as f64
                }
            };
            return additive(partial, count.min(limit));
        }
        Shape::NoiseLoop => {
            let mut x = 0x2545_f491u32;
            for v in &mut table {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                *v = (x as f64 / f64::from(u32::MAX) * 2.0 - 1.0) as f32;
            }
        }
    }
    table
}

/// Every built-in wave's tables, shared by every synth; built the first time a synth is made,
/// never on the audio thread.
static BANK: LazyLock<Vec<Levels>> = LazyLock::new(|| {
    SHAPES
        .iter()
        .map(|&shape| {
            let mut levels = [[0.0f32; TABLE]; LEVELS];
            for (k, slot) in levels.iter_mut().enumerate() {
                *slot = level(shape, 64 >> k);
            }
            levels
        })
        .collect()
});

/// Builds the shared tables now, so the audio thread never does.
pub fn prepare_bank() {
    LazyLock::force(&BANK);
}

/// The table of `levels` for a phase step of `freq` per sample: the fullest whose partials stay
/// below half the sample rate (a whole cycle is 1 << 24).
#[inline]
fn pick(levels: &Levels, freq: i32, antialias: bool) -> &[f32; TABLE] {
    let k = if antialias {
        let room = (1i64 << 23) / i64::from(freq.max(1));
        (0..LEVELS - 1).find(|&k| (64i64 >> k) <= room).unwrap_or(LEVELS - 1)
    } else {
        0
    };
    levels.get(k).unwrap_or(&levels[0])
}

/// A value of `table` at `phase` (1 << 24 is one cycle).
#[inline]
pub fn read(table: &[f32; TABLE], phase: i32, interpolate: bool) -> f32 {
    const SHIFT: u32 = 24 - TABLE_LG;
    let phase = phase & 0xff_ffff;
    let index = (phase >> SHIFT) as usize;
    let a = table.get(index).copied().unwrap_or(0.0);
    if !interpolate {
        return a;
    }
    let b = table.get((index + 1) & (TABLE - 1)).copied().unwrap_or(0.0);
    let t = (phase & ((1 << SHIFT) - 1)) as f32 / (1u32 << SHIFT) as f32;
    a + (b - a) * t
}

/// The table to play built-in wave `wave` from, or `None` for the sine and white noise.
#[inline]
pub fn builtin(wave: usize, freq: i32, antialias: bool) -> Option<&'static [f32; TABLE]> {
    if wave == SINE || wave >= NOISE_WHITE {
        return None;
    }
    BANK.get(wave).map(|levels| pick(levels, freq, antialias))
}

/// How the drawn partials become the partials played.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DrawnSettings {
    /// How many of the partials count: 16, 32 or 64.
    pub partials: usize,
    /// 0 cuts everything above them; more repeats them above, fading faster the higher.
    pub repeat: f32,
    pub normalize: bool,
    /// 0 all, 1 odd partials only, 2 even only.
    pub odd_even: usize,
}

impl Default for DrawnSettings {
    fn default() -> Self {
        Self { partials: 16, repeat: 0.0, normalize: true, odd_even: 0 }
    }
}

/// The amplitude each of the [`HARMONICS`] partials plays at.
pub fn played(drawn: &[f32; HARMONICS], s: &DrawnSettings) -> [f32; HARMONICS] {
    let count = s.partials.clamp(1, HARMONICS);
    let mut out = [0.0f32; HARMONICS];
    for (i, slot) in out.iter_mut().enumerate() {
        let number = i + 1;
        let keep = match s.odd_even {
            1 => number % 2 == 1,
            2 => number % 2 == 0,
            _ => true,
        };
        if !keep {
            continue;
        }
        let cycle = i / count;
        let amount = drawn.get(i % count).copied().unwrap_or(0.0);
        *slot = if cycle == 0 {
            amount
        } else if s.repeat > 0.0 {
            amount * 0.5f32.powf(s.repeat * cycle as f32)
        } else {
            0.0
        };
    }
    out
}

/// One operator's drawn wave as it is built, moving towards what is wanted by a few partials per
/// block: changing a partial adds its difference to the tables.
#[derive(Debug, Clone)]
pub struct DrawnWave {
    levels: Box<Levels>,
    applied: [f32; HARMONICS],
    gain: f32,
}

/// Partials a drawn wave may change per engine block.
const PARTIALS_PER_BLOCK: usize = 4;

impl DrawnWave {
    pub fn new() -> Self {
        Self { levels: Box::new([[0.0; TABLE]; LEVELS]), applied: [0.0; HARMONICS], gain: 1.0 }
    }

    fn apply(&mut self, partial: usize, amount: f32) {
        let difference = amount - self.applied.get(partial).copied().unwrap_or(0.0);
        if let Some(slot) = self.applied.get_mut(partial) {
            *slot = amount;
        }
        let number = partial + 1;
        for (k, table) in self.levels.iter_mut().enumerate() {
            if number <= 64 >> k {
                for (i, v) in table.iter_mut().enumerate() {
                    let phase = (number * i) & (TABLE - 1);
                    *v += difference * sine(phase);
                }
            }
        }
    }

    /// Moves towards `wanted`: all the way, or by a few partials when `all` is false. Returns
    /// whether it is there.
    pub fn follow(&mut self, wanted: &[f32; HARMONICS], normalize: bool, all: bool) -> bool {
        let mut left = if all { HARMONICS } else { PARTIALS_PER_BLOCK };
        let mut settled = true;
        for (partial, &amount) in wanted.iter().enumerate() {
            if self.applied.get(partial).copied() != Some(amount) {
                if left == 0 {
                    settled = false;
                    break;
                }
                self.apply(partial, amount);
                left -= 1;
            }
        }
        let power: f32 = self.applied.iter().map(|a| a * a).sum();
        self.gain = if normalize && power > 1e-9 { 1.0 / power.sqrt() } else { 1.0 };
        settled
    }

    /// The table to play for a phase step of `freq`, and the gain to play it at.
    #[inline]
    pub fn table(&self, freq: i32, antialias: bool) -> (&[f32; TABLE], f32) {
        (pick(&self.levels, freq, antialias), self.gain)
    }
}

impl Default for DrawnWave {
    fn default() -> Self {
        Self::new()
    }
}

/// A sine of one table step per unit, kept apart from the wave tables.
fn sine(index: usize) -> f32 {
    static SINE_TABLE: LazyLock<Vec<f32>> =
        LazyLock::new(|| (0..TABLE).map(|i| (std::f64::consts::TAU * i as f64 / TABLE as f64).sin() as f32).collect());
    SINE_TABLE.get(index).copied().unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn higher_pitches_play_fewer_partials() {
        prepare_bank();
        let saw = WAVES.iter().position(|w| *w == "Saw 64").unwrap_or(0);
        let low = builtin(saw, 1 << 12, true);
        let high = builtin(saw, 1 << 21, true);
        assert!(low.is_some() && high.is_some());
        let roughness = |t: &[f32; TABLE]| t.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f32>();
        assert!(roughness(high.unwrap_or(&[0.0; TABLE])) < roughness(low.unwrap_or(&[0.0; TABLE])));
    }

    #[test]
    fn every_builtin_table_stays_in_range() {
        for wave in 0..WAVES.len() {
            if let Some(table) = builtin(wave, 1 << 14, true) {
                assert!(table.iter().all(|v| v.abs() <= 1.0 + 1e-3), "{}", WAVES[wave]);
            }
        }
    }

    #[test]
    fn a_drawn_wave_gets_there_a_few_partials_at_a_time() {
        let mut wanted = [0.0f32; HARMONICS];
        wanted[..10].fill(0.5);
        let mut wave = DrawnWave::new();
        assert!(!wave.follow(&wanted, true, false));
        assert!(wave.follow(&wanted, true, false) || wave.follow(&wanted, true, false));
        let (table, _) = wave.table(1 << 10, true);
        assert!(table.iter().any(|v| v.abs() > 0.1));
    }

    #[test]
    fn a_change_in_small_steps_ends_at_the_same_table_as_all_at_once() {
        let mut wanted = [0.0f32; HARMONICS];
        wanted[0] = 1.0;
        wanted[2] = 0.3;
        wanted[7] = 0.2;
        let mut direct = DrawnWave::new();
        direct.follow(&wanted, false, true);
        let mut stepped = DrawnWave::new();
        stepped.follow(&[0.4; HARMONICS], false, true);
        while !stepped.follow(&wanted, false, false) {}
        let (a, _) = direct.table(1, false);
        let (b, _) = stepped.table(1, false);
        assert!(a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-2));
    }

    #[test]
    fn repeat_continues_the_drawn_partials_and_fades() {
        let mut drawn = [0.0f32; HARMONICS];
        drawn[0] = 1.0;
        let off = played(&drawn, &DrawnSettings { partials: 16, repeat: 0.0, normalize: true, odd_even: 0 });
        assert_eq!(off[16], 0.0);
        let on = played(&drawn, &DrawnSettings { partials: 16, repeat: 1.0, normalize: true, odd_even: 0 });
        assert!(on[16] > 0.0 && on[32] < on[16]);
        let odd = played(&drawn, &DrawnSettings { partials: 16, repeat: 0.0, normalize: true, odd_even: 2 });
        assert_eq!(odd[0], 0.0);
    }
}
