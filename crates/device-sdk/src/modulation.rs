// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use suhina_dsp::sync::RateMode;

/// An LFO's rate in hertz from its rate, cycle `time` in ms, `division` (a note of `1/division`, or
/// a number of sixteenths), rate `mode` and `tempo`, each clamped to the node's range.
pub(crate) fn lfo_hz(rate: f64, time: f64, division: f64, mode: f64, tempo: f64) -> f64 {
    let division = division.round().clamp(1., 64.);
    let bpm = tempo.clamp(1., 1000.);
    let mode = RateMode::from_value(mode);
    match mode.cycle_beats(4. / division, division) {
        Some(beats) => bpm / 60. / beats,
        None if mode == RateMode::Time => 1000. / time.clamp(1., 20000.),
        None => rate.clamp(0.01, 20.),
    }
}

pub(crate) fn coefficient(seconds: f64, rate: f64) -> f64 {
    if seconds <= 0. { 0. } else { (-1. / (seconds.min(60.) * rate)).exp() }
}
