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
    assert_eq!(s.voices[0].envelope.value(), 1.0);
    assert_eq!(s.voices[0].filter_env, 0.5);
    for (event, amplitude, filter) in [
        (NoteEvent::Off { key: 60, velocity: 0.0 }, 0.75, 0.4375),
        (NoteEvent::Off { key: 60, velocity: 0.0 }, 0.5625, 0.3828125),
        (NoteEvent::AllNotesOff, 0.421875, 0.3349609375),
    ] {
        s.event(event);
        advance(&mut s, 2);
        assert!((s.voices[0].envelope.value() - amplitude).abs() < 1e-12);
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
    assert!((s.voices[0].envelope.value() - 0.65625).abs() < 1e-12, "reuse must not start a new attack");
}

#[test]
fn pedal_up_does_not_restart_an_already_releasing_amplitude_or_filter_tail() {
    let mut actual = sampler();
    let mut expected = sampler();
    for s in [&mut actual, &mut expected] {
        advance(s, 4);
        s.event(NoteEvent::Off { key: 60, velocity: 0.0 });
        advance(s, 2);
    }
    actual.event(NoteEvent::Controller { number: 64, value: 1.0 });
    actual.event(NoteEvent::Off { key: 60, velocity: 0.0 });
    actual.event(NoteEvent::Controller { number: 64, value: 0.0 });
    for _ in 0..3 {
        advance(&mut actual, 2);
        advance(&mut expected, 2);
        assert_eq!(actual.voices[0].envelope.value(), expected.voices[0].envelope.value());
        assert_eq!(actual.voices[0].filter_env, expected.voices[0].filter_env);
        assert_eq!(actual.voices[0].active, expected.voices[0].active);
    }
}

#[test]
fn reused_and_stolen_voices_do_not_inherit_a_deferred_pedal_release() {
    for retrigger in [0.0, 1.0] {
        let mut s = sampler();
        s.set_param(INFO.param_index("retrigger").unwrap(), retrigger);
        advance(&mut s, 4);
        s.event(NoteEvent::Controller { number: 64, value: 1.0 });
        s.event(NoteEvent::Off { key: 60, velocity: 0.0 });
        s.event(NoteEvent::On { key: 62, velocity: 0.8 });
        advance(&mut s, 4);
        s.event(NoteEvent::Controller { number: 64, value: 0.0 });
        advance(&mut s, 8);
        assert!(s.voices[0].active && !s.voices[0].envelope.is_releasing());
        assert_eq!(s.voices[0].key, 62);
        s.event(NoteEvent::Off { key: 62, velocity: 0.0 });
        advance(&mut s, 8);
        assert!(!s.voices[0].active);
    }
}

#[test]
fn one_shots_ignore_note_release_with_or_without_the_pedal() {
    for pedal in [0.0, 1.0] {
        let mut actual = sampler();
        let mut expected = sampler();
        for s in [&mut actual, &mut expected] {
            s.set_param(INFO.param_index("playback").unwrap(), 1.0);
            s.event(NoteEvent::On { key: 60, velocity: 1.0 });
            advance(s, 4);
        }
        actual.event(NoteEvent::Controller { number: 64, value: pedal });
        actual.event(NoteEvent::Off { key: 60, velocity: 0.0 });
        actual.event(NoteEvent::Controller { number: 64, value: 0.0 });
        for _ in 0..16 {
            advance(&mut actual, 8);
            advance(&mut expected, 8);
            assert_eq!(actual.voices[0].envelope.value(), expected.voices[0].envelope.value());
        }
        assert!(actual.voices[0].active && expected.voices[0].active);
        actual.event(NoteEvent::AllNotesOff);
        advance(&mut actual, 8);
        assert!(!actual.voices[0].active, "All Notes Off also releases one-shots");
    }
}

#[test]
fn reset_and_all_notes_off_clear_pedal_state_for_the_next_note() {
    for reset in [false, true] {
        let mut s = sampler();
        advance(&mut s, 4);
        s.event(NoteEvent::Controller { number: 64, value: 1.0 });
        s.event(NoteEvent::Off { key: 60, velocity: 0.0 });
        if reset {
            s.reset();
        } else {
            s.event(NoteEvent::AllNotesOff);
            advance(&mut s, 8);
            assert!(!s.voices[0].active);
        }
        s.event(NoteEvent::On { key: 62, velocity: 1.0 });
        advance(&mut s, 4);
        s.event(NoteEvent::Off { key: 62, velocity: 0.0 });
        advance(&mut s, 8);
        assert!(!s.voices[0].active);
    }
}
