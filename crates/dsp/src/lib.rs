// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Signal-processing building blocks for Suhina's built-in effects and instruments (ADR 0004): gain
//! conversions, the pan law, parameter smoothing, attack/release followers, filters, crossovers,
//! LFO waveforms and tempo sync, waveshapers, oversampling, delay lines and allpasses.
//!
//! Nothing here depends on the engine, the model or a device: every block is plain state plus a
//! per-sample or per-block `process`, so it can be tested on its own and reused by any device.
//! Constructors and coefficient calculations may run anywhere; every `process`, `next` and
//! `reset` method is real-time safe (no allocation, locking or I/O) and never panics, whatever
//! numbers it is given.

pub mod biquad;
pub mod convolver;
pub mod crossover;
pub mod dc;
pub mod delay;
pub mod first_order;
pub mod follower;
pub mod gain;
pub mod lfo;
pub mod oversample;
pub mod pan;
pub mod resampler;
pub mod shape;
pub mod smooth;
pub mod svf;
pub mod sync;
pub mod trim_delay;

pub use biquad::{Biquad, Coefficients};
pub use convolver::{Convolver, Ir, MAX_FRAMES as IR_MAX_FRAMES, PARTITION};
pub use crossover::{LinkwitzRiley, Slope};
pub use dc::DcBlocker;
pub use delay::{Allpass, DelayLine};
pub use first_order::{FirstOrder, FirstOrderCoefficients};
pub use follower::AttackRelease;
pub use oversample::Oversampler2x;
pub use resampler::{HALF_TAPS as RESAMPLER_HALF_TAPS, Resampler};
pub use smooth::{Glide, OnePole, Smoother, time_constant};
pub use svf::{Svf, SvfCoefficients};
pub use trim_delay::TrimDelay;

/// Highest filter frequency as a fraction of the sample rate; keeps coefficients stable just
/// below Nyquist.
pub(crate) const MAX_FREQUENCY_RATIO: f64 = 0.49;

/// `frequency` limited to what a filter at `sample_rate` can use, with non-finite input mapped
/// to 1 kHz (or the highest usable frequency, if lower).
pub(crate) fn usable_frequency(frequency: f64, sample_rate: f64) -> f64 {
    let top = usable_sample_rate(sample_rate) * MAX_FREQUENCY_RATIO;
    if frequency.is_finite() { frequency.clamp(1.0, top) } else { 1_000.0f64.min(top) }
}

/// `sample_rate`, or 48 kHz when it is not a positive finite number.
pub(crate) fn usable_sample_rate(sample_rate: f64) -> f64 {
    if sample_rate.is_finite() && sample_rate >= 1_000.0 { sample_rate } else { 48_000.0 }
}

/// Filter state below this is set to zero, so a decaying tail never reaches subnormal numbers
/// (which are slow on most CPUs).
pub(crate) const DENORMAL_FLOOR: f64 = 1e-30;

/// Filter state above this (+400 dB) can only come from broken input; it is cleared.
pub(crate) const STATE_CEILING: f64 = 1e20;

/// Filter state made safe to keep: tiny values become zero, and so do NaN, infinities and
/// absurdly large values, so bad input cannot leave a filter producing garbage forever.
#[inline]
pub(crate) fn flush(value: f64) -> f64 {
    if (DENORMAL_FLOOR..STATE_CEILING).contains(&value.abs()) { value } else { 0.0 }
}
