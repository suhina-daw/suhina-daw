// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

const RATE: f64 = 48000.0;

fn one_band(level: f32, other: f32, decay: f32) -> Tail {
    let mut tail = Tail::NONE;
    tail.level[6] = level;
    tail.other[6] = other;
    tail.decay[6] = decay;
    tail
}

fn rms(x: &[f32]) -> f64 {
    (x.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / x.len().max(1) as f64).sqrt()
}

fn play(player: &mut TailPlayer, frames: usize) -> (Vec<f32>, Vec<f32>) {
    let (mut left, mut right) = (Vec::with_capacity(frames), Vec::with_capacity(frames));
    for _ in 0..frames {
        let [a, b] = player.next();
        left.push(a);
        right.push(b);
    }
    (left, right)
}

/// A band falls 60 dB per T60, its two channels are independent noise at their own levels, and
/// a seed repeats exactly.
#[test]
fn a_band_decays_by_its_t60_in_two_independent_channels() {
    let mut player = TailPlayer::default();
    player.start(&one_band(0.01, 0.005, 0.5), 1.0, RATE, (0, 0), true, 7);
    assert!(player.is_active());
    let (left, right) = play(&mut player, 48000);
    let early = rms(&left[2400..7200]);
    let late = rms(&left[14400..19200]);
    // 0.25 s apart: 30 dB down.
    let drop = 20.0 * (early / late).log10();
    assert!((drop - 30.0).abs() < 3.0, "{drop}");
    let ratio = 20.0 * (rms(&left[2400..7200]) / rms(&right[2400..7200])).log10();
    assert!((ratio - 6.02).abs() < 1.0, "{ratio}");
    let correlation = left.iter().zip(&right).map(|(a, b)| f64::from(*a) * f64::from(*b)).sum::<f64>()
        / (left.iter().map(|a| f64::from(*a).powi(2)).sum::<f64>()
            * right.iter().map(|b| f64::from(*b).powi(2)).sum::<f64>())
        .sqrt();
    assert!(correlation.abs() < 0.1, "{correlation}");
    let mut again = TailPlayer::default();
    again.start(&one_band(0.01, 0.005, 0.5), 1.0, RATE, (0, 0), true, 7);
    assert_eq!(play(&mut again, 48000), (left, right));
    // A mono recording's tail is the same noise in both channels.
    let mut mono = TailPlayer::default();
    mono.start(&one_band(0.01, 0.01, 0.5), 1.0, RATE, (0, 0), false, 7);
    let (a, b) = play(&mut mono, 4800);
    assert_eq!(a, b);
}

/// The tail is silent until its fade starts and in full once it ends; the damper caps its decay;
/// an empty tail never sounds.
#[test]
fn the_tail_fades_in_and_the_damper_caps_it() {
    let mut player = TailPlayer::default();
    player.start(&one_band(0.01, 0.01, 2.0), 1.0, RATE, (4800, 9600), true, 3);
    let (left, _) = play(&mut player, 14400);
    assert!(left[..4800].iter().all(|v| *v == 0.0));
    assert!(rms(&left[9600..14400]) > 4.0 * rms(&left[4800..6000]));
    let mut damped = TailPlayer::default();
    damped.start(&one_band(0.01, 0.01, 2.0), 1.0, RATE, (0, 0), true, 3);
    damped.cap_decay(RATE, 0.1);
    let (left, _) = play(&mut damped, 24000);
    assert!(rms(&left[19200..24000]) < 1e-3 * rms(&left[0..4800]));
    let mut empty = TailPlayer::default();
    empty.start(&Tail::NONE, 1.0, RATE, (0, 0), true, 1);
    assert!(!empty.is_active());
    assert_eq!(empty.next(), [0.0, 0.0]);
}

#[test]
fn tails_round_trip_their_fields_and_validate_bounds() {
    let tail = one_band(0.2, 0.1, 1.5);
    assert_eq!(Tail::from_fields(&tail.fields()), tail);
    assert!(tail.validate().is_ok());
    assert!(Tail::NONE.is_empty() && !tail.is_empty());
    for broken in
        [one_band(1.5, 0.0, 1.0), one_band(0.1, -0.1, 1.0), one_band(0.1, 0.1, 0.01), one_band(f32::NAN, 0.0, 1.0)]
    {
        assert!(broken.validate().is_err(), "{broken:?}");
    }
}
