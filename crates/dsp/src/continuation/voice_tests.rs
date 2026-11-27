// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use crate::continuation::Partial;
use crate::continuation::tests::sample;

const RATE: f64 = 48000.0;

fn note(continuation: &Continuation, ratio: f64) -> NoteModel {
    let mut model = NoteModel::default();
    model.transposed(continuation, ratio, RATE);
    model
}

fn render(generator: &mut Generator, frames: usize) -> Vec<[f32; 2]> {
    (0..frames).map(|_| generator.next()).collect()
}

fn rms(values: impl Iterator<Item = f32>) -> f64 {
    let (sum, count) = values.fold((0.0, 0usize), |(s, c), v| (s + f64::from(v) * f64::from(v), c + 1));
    (sum / count.max(1) as f64).sqrt()
}

#[test]
fn a_note_renders_the_same_however_far_ahead_it_was_generated() {
    let continuation = sample(24, true);
    let prepared = Prepared::new(&continuation, RATE);
    let model = note(&continuation, 1.0);
    let mut plain = Generator::new(&prepared, None);
    plain.start(&model, 0.0, 7);
    let expected = render(&mut plain, 24000);
    for budget in [1_usize, 5, 400] {
        let mut spread = Generator::new(&prepared, None);
        spread.start(&model, 0.0, 7);
        let mut got = Vec::new();
        while got.len() < 24000 {
            spread.produce(budget);
            got.extend(render(&mut spread, 333.min(24000 - got.len())));
        }
        assert_eq!(got, expected, "budget {budget}");
    }
    // Another seed is another performance.
    let mut other = Generator::new(&prepared, None);
    other.start(&model, 0.0, 8);
    assert_ne!(render(&mut other, 24000), expected);
}

#[test]
fn a_note_is_finite_bounded_and_near_its_expected_level() {
    for stereo in [false, true] {
        let continuation = sample(40, stereo);
        let prepared = Prepared::new(&continuation, RATE);
        let mut generator = Generator::new(&prepared, None);
        generator.start(&note(&continuation, 1.0), 0.0, 3);
        let out = render(&mut generator, 3 * RATE as usize);
        assert!(out.iter().flatten().all(|v| v.is_finite() && v.abs() < 4.0));
        let level = rms(out[RATE as usize..].iter().map(|p| p[0]));
        let expected = continuation.expected_rms();
        let ratio_db = 20.0 * (level / expected).log10();
        assert!(ratio_db.abs() < 3.0, "stereo {stereo}: {level} against {expected} ({ratio_db} dB)");
        let side = rms(out[RATE as usize..].iter().map(|p| p[1]));
        if stereo {
            assert!(side > 0.05 * level && side < level, "side {side}, mid {level}");
        } else {
            assert_eq!(side, 0.0);
        }
    }
}

#[test]
fn a_transposed_note_warms_up_and_plays() {
    let continuation = sample(40, true);
    let prepared = Prepared::new(&continuation, RATE);
    let mut generator = Generator::new(&prepared, None);
    generator.start(&note(&continuation, 2f64.powf(5.0 / 12.0)), 0.0, 11);
    let out = render(&mut generator, RATE as usize * 2);
    assert!(out.iter().flatten().all(|v| v.is_finite()));
    assert!(rms(out[RATE as usize..].iter().map(|p| p[0])) > 0.01);
}

/// A lone partial with neither diffuse part nor noise, whose pitch is read back from its zero
/// crossings: the 3..97 % range of its deviation in cents over `from..` seconds.
fn pitch_swing(depth: f64, rate: f64) -> (f64, usize) {
    let mut continuation = sample(1, false);
    continuation.noise_rms = 0.0;
    continuation.jitter_innovation = 0.0;
    continuation.jitter_cents = 0.0;
    continuation.partials = vec![Partial { carrier: [0.3, 0.0], mean_log: 0.3_f32.ln(), ..Partial::NONE }];
    let prepared = Prepared::new(&continuation, RATE);
    let mut generator = Generator::new(&prepared, None);
    generator.start(&note(&continuation, 1.0), 0.0, 5);
    generator.set_vibrato(depth, rate);
    let out: Vec<f64> = render(&mut generator, 5 * RATE as usize).iter().map(|p| f64::from(p[0])).collect();
    let mut crossings = Vec::new();
    for (i, pair) in out.windows(2).enumerate() {
        if pair[0] < 0.0 && pair[1] >= 0.0 {
            crossings.push(i as f64 + pair[0] / (pair[0] - pair[1]));
        }
    }
    let cents: Vec<f64> = crossings
        .windows(2)
        .filter(|w| w[0] > 3.0 * RATE)
        .map(|w| 1200.0 * (RATE / (w[1] - w[0]) / 200.0).log2())
        .collect();
    let mut sorted = cents.clone();
    sorted.sort_by(f64::total_cmp);
    let pick = |q: f64| sorted[((sorted.len() - 1) as f64 * q) as usize];
    // Vibrato cycles: the deviation's upward crossings of its median.
    let median = pick(0.5);
    let cycles = cents.windows(2).filter(|w| w[0] < median && w[1] >= median).count();
    (pick(0.97) - pick(0.03), cycles)
}

#[test]
fn the_vibrato_depth_and_rate_controls_move_the_pitch() {
    let (recorded, recorded_cycles) = pitch_swing(1.0, 0.0);
    let (none, _) = pitch_swing(0.0, 0.0);
    let (deep, _) = pitch_swing(3.0, 0.0);
    // A 20-cent vibrato swings about 40 cents.
    assert!(recorded > 25.0 && recorded < 60.0, "{recorded}");
    assert!(none < 8.0, "{none}");
    assert!(deep > 2.5 * recorded, "{deep} against {recorded}");
    // Twice the recorded 5.5 Hz: about twice the cycles in the two seconds read.
    let (_, fast_cycles) = pitch_swing(1.0, 11.0);
    assert!(fast_cycles as f64 > 1.6 * recorded_cycles as f64, "{fast_cycles} cycles against {recorded_cycles}");
}
