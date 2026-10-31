// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Waveshapers: memoryless curves that add harmonics. Run them oversampled
//! ([`Oversampler2x`](crate::Oversampler2x)) to keep the harmonics from aliasing.

/// How a shaper limits large values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Curve {
    /// `tanh`: rounds off gradually, mostly low odd harmonics.
    Soft,
    /// Clips flat at ±1: sharper, more and higher harmonics.
    Hard,
}

impl Curve {
    /// The curve at `v`: about `v` for small values, never beyond ±1. Non-finite input gives 0.
    #[inline]
    pub fn apply(self, v: f64) -> f64 {
        if !v.is_finite() {
            return 0.0;
        }
        match self {
            Self::Soft => v.tanh(),
            Self::Hard => v.clamp(-1.0, 1.0),
        }
    }

    /// An asymmetric version: the negative half limits earlier, at `-1 / (1 + bias)`, which
    /// adds even harmonics (and a DC offset the caller removes). Both halves start with slope
    /// 1 and the curve always rises. `bias` from 0 (symmetric) to 1; other values are limited
    /// to that range.
    #[inline]
    pub fn asymmetric(self, v: f64, bias: f64) -> f64 {
        let k = 1.0 + if bias.is_finite() { bias.clamp(0.0, 1.0) } else { 0.0 };
        if v >= 0.0 { self.apply(v) } else { self.apply(k * v) / k }
    }
}

/// Linear up to `knee`, then bending towards ±1: past the knee the distance to 1 shrinks
/// exponentially, starting with slope 1, so the curve has no corner. `knee` 0 is `1 - e^-v`
/// from the origin, 1 is a hard clip; it is limited to 0 … 1. Non-finite input gives 0.
#[inline]
pub fn knee_clip(v: f64, knee: f64) -> f64 {
    if !v.is_finite() {
        return 0.0;
    }
    let knee = if knee.is_finite() { knee.clamp(0.0, 1.0) } else { 1.0 };
    let a = v.abs();
    if a <= knee {
        return v;
    }
    let room = 1.0 - knee;
    if room <= 0.0 {
        return v.signum();
    }
    (1.0 - room * (-(a - knee) / room).exp()).copysign(v)
}

/// A quarter sine: `sin(π/2 · v)` up to ±1, flat beyond. Slope π/2 at the origin.
#[inline]
pub fn soft_sine(v: f64) -> f64 {
    if !v.is_finite() {
        return 0.0;
    }
    (std::f64::consts::FRAC_PI_2 * v.clamp(-1.0, 1.0)).sin()
}

/// A sine fold: `sin(π/2 · v)` without a limit, so louder input folds back (peaks at odd
/// values, zero at even ones).
#[inline]
pub fn sine_fold(v: f64) -> f64 {
    if !v.is_finite() {
        return 0.0;
    }
    (std::f64::consts::FRAC_PI_2 * v).sin()
}

/// Linear up to `knee`, then a parabola that meets ±1 with slope 0 at `2 - knee`, then flat:
/// `v - (v - knee)² / (4 (1 - knee))` between the two. Unlike [`knee_clip`] it reaches the
/// ceiling at a finite value. `knee` is limited to 0 … 1 (1 is a hard clip). Non-finite input
/// gives 0.
#[inline]
pub fn quadratic_clip(v: f64, knee: f64) -> f64 {
    if !v.is_finite() {
        return 0.0;
    }
    let knee = if knee.is_finite() { knee.clamp(0.0, 1.0) } else { 1.0 };
    let a = v.abs();
    if a <= knee {
        return v;
    }
    let room = 1.0 - knee;
    if room <= 0.0 || a >= 2.0 - knee {
        return v.signum();
    }
    let over = a - knee;
    (a - over * over / (4.0 * room)).copysign(v)
}

/// An arctangent normalised to 1 at `v` = 1: `atan(g·v) / atan(g)`, slope `g / atan(g)` at the
/// origin, still rising slowly past 1 towards `π/2 / atan(g)`.
#[inline]
pub fn arctan_curve(v: f64, gain: f64) -> f64 {
    let scale = gain.atan();
    if !v.is_finite() || !gain.is_finite() || scale <= 0.0 {
        return 0.0;
    }
    (gain * v).atan() / scale
}

/// A curve shaped by six amounts, each 0 … 1, fitted to a measured original: the slope of the
/// straight part (the amplitude gain of the linear path), a soft dead zone around zero, an even
/// term `sin(2π · curve · x²)` that starts as a square and folds back once it reaches 1, and a
/// sine ripple on top. Its [`drive`](Self::drive) blends it with the straight line `v`. The even
/// term has even harmonics and a DC offset, which the caller takes out after the curve.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Waveshaper {
    /// 0: the straight line, 1: the shaped curve.
    pub drive: f64,
    /// Size of the even term.
    pub curve: f64,
    /// Height of the sine ripple, as the square of this.
    pub depth: f64,
    /// Gain of the straight part: 0 is silence, ½ is half, 1 is unity.
    pub linear: f64,
    /// Strength of the dead zone around zero.
    pub damp: f64,
    /// Frequency of the ripple: 0 is one period over ±1, 1 is thirty.
    pub period: f64,
}

impl Default for Waveshaper {
    /// Half the input: Linear at ½ and nothing else on.
    fn default() -> Self {
        Self { drive: 1.0, curve: 0.0, depth: 0.0, linear: 0.5, damp: 0.0, period: 0.0 }
    }
}

/// The even term is `sin(2π · curve · x²)`: against the fundamental of a small tone the second
/// harmonic is `2π · curve · A`, as measured to 2 % up to −12 dBFS, and it folds back past 1
/// (at `x` = ½ for Curve 100 %) as the measured transfer curves do.
const CURVE_GAIN: f64 = std::f64::consts::TAU;
/// Below this `|x|` (−46 dBFS) the even term is `CURVE_FLOOR · |x|` instead of `x²`: the
/// original's second harmonic stops falling with level there (−31.7 dB against the fundamental
/// at Curve 100 %, the fourth 14 dB under it, as a rectified sine has).
const CURVE_FLOOR: f64 = 0.0049;
/// The ripple's height at full depth.
const RIPPLE_HEIGHT: f64 = 0.5;
/// The ripple's angular frequency per unit of `x` at `period` 0, and the added per unit of period.
const RIPPLE_BASE: f64 = std::f64::consts::PI;
const RIPPLE_SPAN: f64 = 94.1;
/// The dead zone's slope at the origin is `1 / (1 + DAMP_STEEPNESS · damp)` of the straight
/// part's; it is half open at `|x|` = `DAMP_WIDTH + DAMP_GROWTH · damp - DAMP_BEND · damp²`
/// (0.204, 0.240 and 0.264 at 25, 50 and 75 %, as measured), opening as `x⁴ / (x⁴ + width⁴)`.
const DAMP_STEEPNESS: f64 = 64.2;
const DAMP_WIDTH: f64 = 0.156;
const DAMP_GROWTH: f64 = 0.216;
const DAMP_BEND: f64 = 0.096;

impl Waveshaper {
    /// The curve at `v`. Non-finite input gives 0.
    #[inline]
    pub fn apply(&self, v: f64) -> f64 {
        if !v.is_finite() {
            return 0.0;
        }
        if self.drive.is_nan() || self.drive <= 0.0 {
            return v;
        }
        // Keeps `v⁴` finite: no curve changes this far out, where every audio signal is long gone.
        let v = v.clamp(-1e30, 1e30);
        let unit = |x: f64| if x.is_finite() { x.clamp(0.0, 1.0) } else { 0.0 };
        let (damp, depth) = (unit(self.damp), unit(self.depth));
        let x = if damp > 0.0 {
            let floor = 1.0 / (1.0 + DAMP_STEEPNESS * damp);
            let width = DAMP_WIDTH + (DAMP_GROWTH - DAMP_BEND * damp) * damp;
            let (v2, w2) = (v * v, width * width);
            let (v4, w4) = (v2 * v2, w2 * w2);
            let opened = v4 / (v4 + w4);
            v * (floor + (1.0 - floor) * opened)
        } else {
            v
        };
        let squared = CURVE_GAIN * unit(self.curve) * (x * x).max(CURVE_FLOOR * x.abs());
        let mut shaped = unit(self.linear) * x + squared.sin();
        if depth > 0.0 {
            shaped += RIPPLE_HEIGHT * depth * depth * ((RIPPLE_BASE + RIPPLE_SPAN * unit(self.period)) * x).sin();
        }
        v + unit(self.drive) * (shaped - v)
    }
}

#[cfg(test)]
#[path = "shape_tests.rs"]
mod tests;
