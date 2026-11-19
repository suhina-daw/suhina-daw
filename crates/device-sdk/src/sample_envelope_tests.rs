// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

fn sampler() -> Sampler {
    let mut s = Sampler::with_rate(48_000.0).unwrap();
    s.load_sample(48_000, 1, vec![0.5; 4096].into());
    for (id, value) in [
        ("attack", 4.0 / 48_000.0),
        ("decay", 4.0 / 48_000.0),
        ("sustain", 0.5),
        ("release", 8.0 / 48_000.0),
        ("filter_attack", 8.0 / 48_000.0),
        ("filter_release", 16.0 / 48_000.0),
        ("voice_mode", 1.0),
        ("retrigger", 0.0),
    ] {
        s.set_param(INFO.param_index(id).unwrap(), value);
    }
    s.event(NoteEvent::On { key: 60, velocity: 1.0 });
    s
}

fn advance(s: &mut Sampler, frames: usize) {
    let mut audio = [0.0; 16];
    s.process(&ProcessContext::default(), &mut audio[..frames * 2]);
    assert!(audio.iter().all(|v| v.is_finite()));
}

#[test]
fn repeated_releases_recapture_amplitude_and_filter_together() {
    let mut s = sampler();
    advance(&mut s, 4);
    assert_eq!(s.voices[0].env, 1.0);
    assert_eq!(s.voices[0].filter_env, 0.5);
    for (event, amplitude, filter) in [
        (NoteEvent::Off { key: 60, velocity: 0.0 }, 0.75, 0.4375),
        (NoteEvent::Off { key: 60, velocity: 0.0 }, 0.5625, 0.3828125),
        (NoteEvent::AllNotesOff, 0.421875, 0.3349609375),
    ] {
        s.event(event);
        advance(&mut s, 2);
        assert!((s.voices[0].env - amplitude).abs() < 1e-12);
        assert!((s.voices[0].filter_env - filter).abs() < 1e-12);
    }
    let filter = s.voices[0].filter_env;
    let age = s.voices[0].age;
    s.set_param(INFO.param_index("release").unwrap(), 0.0);
    advance(&mut s, 1);
    assert!(!s.voices[0].active);
    assert_eq!(s.voices[0].filter_env, filter, "finished amplitude skips the filter step");
    assert_eq!(s.voices[0].age, age);
}

#[test]
fn note_reuse_preserves_envelope_and_rendering_does_not_allocate() {
    let mut s = sampler();
    advance(&mut s, 4);
    let allocations = allocation_counter::measure(|| {
        s.event(NoteEvent::On { key: 62, velocity: 0.75 });
        advance(&mut s, 1);
        s.event(NoteEvent::Off { key: 62, velocity: 0.0 });
        s.event(NoteEvent::AllNotesOff);
        advance(&mut s, 2);
    });
    assert_eq!(allocations.count_total, 0);
    assert_eq!(allocations.bytes_total, 0);
    assert_eq!(s.voices[0].key, 62);
    assert!((s.voices[0].env - 0.65625).abs() < 1e-12, "reuse must not start a new attack");
}
