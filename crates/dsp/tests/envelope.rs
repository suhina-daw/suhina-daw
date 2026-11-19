// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use suhina_dsp::envelope::{
    EnvelopeClock, LinearEnvelope, LinearEnvelopeSettings, LoopMode, TimeEnvelope, TimeEnvelopeSettings,
};
use suhina_dsp::smooth::glide_step;

#[test]
fn linear_stages_keep_sample_timing_repeated_release_and_retrigger() {
    let mut e = LinearEnvelope::default();
    let mut s = LinearEnvelopeSettings { attack: 0.5, decay: 0.5, sustain: 0.5, release: 0.5 };
    assert!(e.is_done());
    assert_eq!(e.next(&s, 8.0), 0.0);
    e.start();
    let attack_decay: Vec<_> = (0..8).map(|_| e.next(&s, 8.0)).collect();
    assert_eq!(attack_decay, [0.25, 0.5, 0.75, 1.0, 0.875, 0.75, 0.625, 0.5]);
    assert!(e.is_sustaining());
    s.sustain = 0.9;
    assert_eq!(e.next(&s, 8.0), 0.5, "sustain automation does not replace the held level");
    e.release();
    assert!(e.is_releasing());
    assert_eq!(e.next(&s, 8.0), 0.375);
    e.release();
    assert_eq!(e.next(&s, 8.0), 0.28125);
    for _ in 0..3 {
        e.next(&s, 8.0);
    }
    assert!(e.is_done());
    e.start();
    assert_eq!(e.next(&s, 8.0), 0.25);
    e.release();
    e.next(&s, 8.0);
    e.start();
    assert_eq!(e.next(&s, 8.0), 0.25, "a retrigger starts at zero, even during release");
}

#[test]
fn linear_zero_duration_still_advances_only_one_stage_per_sample() {
    let mut e = LinearEnvelope::default();
    let s = LinearEnvelopeSettings { attack: 0.0, decay: 0.0, sustain: 0.25, release: 0.0 };
    e.start();
    assert_eq!(e.next(&s, 48_000.0), 1.0);
    assert_eq!(e.next(&s, 48_000.0), 0.25);
    e.release();
    assert_eq!(e.next(&s, 48_000.0), 0.0);
    assert!(e.is_done());
}

#[test]
fn linear_public_inputs_remain_finite_and_recover() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, f64::MAX, -f64::MAX, 0.0, f64::MIN_POSITIVE] {
        let mut e = LinearEnvelope::default();
        let bad = LinearEnvelopeSettings { attack: value, decay: value, sustain: value, release: value };
        e.start();
        for i in 0..32 {
            if i == 16 {
                e.release();
            }
            let v = e.next(&bad, value);
            assert!(v.is_finite() && (0.0..=1.0).contains(&v), "{value}: {v}");
        }
        e.start();
        let good = LinearEnvelopeSettings { attack: 0.0, decay: 0.0, sustain: 0.5, release: 0.0 };
        assert_eq!(e.next(&good, 48_000.0), 1.0);
        assert_eq!(e.next(&good, 48_000.0), 0.5);
        e.release();
        assert_eq!(e.next(&good, 48_000.0), 0.0);
        assert!(e.is_done());
    }
    for rate in [f64::NAN, f64::INFINITY, -1.0, 0.0] {
        let mut e = LinearEnvelope::default();
        e.start();
        let s = LinearEnvelopeSettings { attack: 1.0, ..Default::default() };
        assert_eq!(e.next(&s, rate), 1.0 / 48_000.0);
    }
}

fn hostile_time_settings(value: f32, mode: LoopMode) -> TimeEnvelopeSettings {
    TimeEnvelopeSettings {
        attack: value,
        decay: value,
        release: value,
        initial: value,
        peak: value,
        sustain: value,
        end: value,
        mode,
        loop_time: value,
        repeat_beats: value,
        time_by_velocity: value,
        time_by_key: value,
        attack_slope: value,
        decay_slope: value,
        release_slope: value,
    }
}

#[test]
fn time_public_settings_clock_and_controls_are_bounded_and_recover() {
    let values = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, f32::MAX, -f32::MAX, 0.0, f32::MIN_POSITIVE];
    for amp in [false, true] {
        for mode in [LoopMode::Off, LoopMode::Loop, LoopMode::Beat, LoopMode::Sync, LoopMode::Trigger] {
            for value in values {
                let s = hostile_time_settings(value, mode);
                let clock = EnvelopeClock { tempo: f64::from(value), beat: f64::from(value), playing: true };
                let mut e = TimeEnvelope::default();
                e.start(&s, value, value, amp, &clock);
                for i in 0..64 {
                    // Reach moving stages with valid elapsed time as well as exercising bad time.
                    let seconds = if i % 2 == 0 { value } else { 0.02 };
                    let scale = if i % 3 == 0 { value } else { 1.0 };
                    let v = e.next(&s, seconds, scale, i < 48, &clock);
                    assert!(v.is_finite() && v.abs() <= 1e6, "{amp} {mode:?} {value}: {v}");
                }
                let good = TimeEnvelopeSettings {
                    attack: 0.001,
                    decay: 0.001,
                    release: 0.001,
                    peak: 1.0,
                    sustain: 0.5,
                    end: 0.25,
                    ..Default::default()
                };
                let clock = EnvelopeClock::default();
                e.start(&good, 0.5, 60.0, false, &clock);
                assert_eq!(e.next(&good, 0.01, 1.0, true, &clock), 1.0);
                assert_eq!(e.next(&good, 0.01, 1.0, true, &clock), 0.5);
                assert_eq!(e.next(&good, 0.01, 1.0, false, &clock), 0.25);
                assert!(e.is_done());
            }
        }
    }
}

#[test]
fn time_negative_beats_and_large_transport_jumps_remain_single_step() {
    let s = TimeEnvelopeSettings {
        attack: 0.001,
        decay: 0.001,
        peak: 1.0,
        sustain: 0.5,
        mode: LoopMode::Sync,
        repeat_beats: 1.0,
        ..Default::default()
    };
    let mut clock = EnvelopeClock { beat: -2.0, playing: true, ..Default::default() };
    let mut e = TimeEnvelope::default();
    e.start(&s, 0.5, 60.0, false, &clock);
    assert_eq!(e.next(&s, 0.01, 1.0, true, &clock), 1.0);
    assert_eq!(e.next(&s, 0.01, 1.0, true, &clock), 0.5);
    clock.beat = -1.0;
    assert_eq!(e.next(&s, 0.01, 1.0, true, &clock), 1.0, "repeat must start before beat zero");
    clock.beat = 1000.0;
    for _ in 0..8 {
        assert_eq!(e.next(&s, 0.01, 1.0, true, &clock), 1.0, "one repeat per call; no catch-up loop");
    }
}

#[test]
fn control_glide_keeps_normal_rounding_and_handles_extreme_inputs() {
    assert_eq!(glide_step(0.25, 0.75, 0.125), 0.3125);
    assert_eq!(glide_step(0.49995, 0.5, 0.25), 0.5);
    assert_eq!(glide_step(0.25, 0.75, 10.0), 0.75);
    assert_eq!(glide_step(0.25, 0.75, -10.0), 0.25);
    assert_eq!(glide_step(0.25, f32::NAN, 0.5), 0.25);
    assert_eq!(glide_step(f32::NAN, 1.0, 0.5), 0.5);
    assert_eq!(glide_step(-f32::MAX, f32::MAX, 0.5), 0.0);
    for now in [f32::MIN, f32::MAX, f32::NAN, f32::INFINITY, 0.0] {
        for target in [f32::MIN, f32::MAX, f32::NAN, f32::NEG_INFINITY, 0.0] {
            for share in [f32::NAN, f32::MAX, -f32::MAX, 0.0, 0.5, 1.0] {
                assert!(glide_step(now, target, share).is_finite());
            }
        }
    }
}
