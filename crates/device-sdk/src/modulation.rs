// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

pub(crate) fn lfo_hz(rate: f64, time: f64, division: f64, mode: f64, tempo: f64) -> f64 {
    let division = division.round().clamp(1., 64.);
    let bpm = tempo.clamp(1., 1000.);
    match mode.round().clamp(0., 5.) as u8 {
        0 => rate.clamp(0.01, 20.),
        1 => 1000. / time.clamp(1., 20000.),
        2 => bpm * division / 240.,
        3 => bpm * division / 160.,
        4 => bpm * division / 360.,
        _ => bpm / (15. * division),
    }
}

pub(crate) fn coefficient(seconds: f64, rate: f64) -> f64 {
    if seconds <= 0. { 0. } else { (-1. / (seconds.min(60.) * rate)).exp() }
}
