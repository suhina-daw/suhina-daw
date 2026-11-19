// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

fn id(serial: u64) -> NoteInstanceId {
    NoteInstanceId::from_parts(23, serial).unwrap()
}
fn on(key: u8) -> NoteInstanceEvent {
    NoteInstanceEvent::On { key, velocity: 0.7, expression: NoteExpressionState::NEUTRAL }
}
fn sampler() -> Sampler {
    let mut sampler = Sampler::with_rate(8000.).unwrap();
    let source: Vec<f32> =
        (0..8000).map(|frame| (std::f64::consts::TAU * 220. * f64::from(frame) / 8000.).sin() as f32).collect();
    sampler.load_sample(8000, 1, Arc::from(source));
    for (name, value) in [("attack", 0.), ("decay", 0.), ("sustain", 1.), ("release", 0.01), ("playback", 2.)] {
        sampler.set_param(PARAMS.iter().position(|p| p.id == name).unwrap(), value);
    }
    sampler
}

fn render(notes: &[u64], bend: bool, block: usize) -> Vec<f32> {
    let mut sampler = sampler();
    for &serial in notes {
        NoteInstanceDevice::event(&mut sampler, id(serial), on(if serial == 1 { 60 } else { 67 })).unwrap();
    }
    let mut result = Vec::new();
    for (length, change) in [(211usize, false), (813, true)] {
        if change && bend && notes.contains(&1) {
            NoteInstanceDevice::event(
                &mut sampler,
                id(1),
                NoteInstanceEvent::Expression { kind: NoteExpressionKind::Pitch, value: 12. },
            )
            .unwrap();
        }
        let mut remaining = length;
        while remaining > 0 {
            let frames = remaining.min(block);
            let mut buffer = vec![0.; frames * 2];
            sampler.process(&ProcessContext::default(), &mut buffer);
            result.extend(buffer);
            remaining -= frames;
        }
    }
    result
}

#[test]
fn mpe_sample_bends_one_voice_at_the_event_sample_independent_of_block_size() {
    let mixed = render(&[1, 2], true, 17);
    let first = render(&[1], true, 64);
    let second = render(&[2], false, 512);
    assert!(mixed.iter().zip(first.iter().zip(&second)).all(|(sum, (a, b))| (*sum - (*a + *b)).abs() < 1e-6));
    let plain = render(&[1, 2], false, 17);
    assert_eq!(
        &mixed[..424],
        &plain[..424],
        "step changes the next read position, not the sample already at the playhead"
    );
    assert!(mixed[424..430].iter().zip(&plain[424..430]).any(|(a, b)| (*a - *b).abs() > 1e-3));
    for block in [1, 64, 512] {
        assert_eq!(mixed, render(&[1, 2], true, block));
    }
}

#[test]
fn mpe_sample_same_key_release_keeps_other_occurrence_and_tail_identity() {
    let mut sampler = sampler();
    for serial in [1, 2] {
        NoteInstanceDevice::event(&mut sampler, id(serial), on(60)).unwrap();
    }
    sampler.process(&ProcessContext::default(), &mut [0.; 64]);
    NoteInstanceDevice::event(&mut sampler, id(1), NoteInstanceEvent::Off { key: 60, velocity: 0. }).unwrap();
    assert_eq!(sampler.drain_finished(&mut [None; 4]), 0);
    sampler.process(&ProcessContext::default(), &mut [0.; 256]);
    let mut finished = [None; 4];
    assert_eq!(sampler.drain_finished(&mut finished), 1);
    assert_eq!(finished[0], Some(id(1)));
    assert!(sampler.voices.iter().any(|v| v.instance == Some(id(2)) && v.active));
    assert_eq!(
        NoteInstanceDevice::event(
            &mut sampler,
            id(1),
            NoteInstanceEvent::Expression { kind: NoteExpressionKind::Pitch, value: 12. }
        ),
        Err(NoteInstanceDispatchError::UnknownInstance)
    );
}

#[test]
fn mpe_sample_pressure_and_filter_timbre_are_audible_without_cross_voice_changes() {
    let mut sampler = sampler();
    sampler.set_param(15, 1.);
    sampler.set_param(16, 300.);
    NoteInstanceDevice::event(&mut sampler, id(1), on(60)).unwrap();
    let mut before = [0.; 1024];
    sampler.process(&ProcessContext::default(), &mut before);
    NoteInstanceDevice::event(
        &mut sampler,
        id(1),
        NoteInstanceEvent::Expression { kind: NoteExpressionKind::Pressure, value: 1. },
    )
    .unwrap();
    NoteInstanceDevice::event(
        &mut sampler,
        id(1),
        NoteInstanceEvent::Expression { kind: NoteExpressionKind::Timbre, value: 1. },
    )
    .unwrap();
    let mut after = [0.; 1024];
    sampler.process(&ProcessContext::default(), &mut after);
    let energy = |buffer: &[f32]| buffer.iter().map(|s| f64::from(*s).powi(2)).sum::<f64>();
    assert!(energy(&after) > energy(&before) * 1.5);
}

#[test]
fn mpe_sample_native_zero_velocity_retains_identity_and_initial_expression() {
    let mut sampler = sampler();
    NoteInstanceDevice::event(
        &mut sampler,
        id(1),
        NoteInstanceEvent::On { key: 60, velocity: 0., expression: NoteExpressionState::new(3., 0.2, 0.8).unwrap() },
    )
    .unwrap();
    let voice = sampler.voices.iter().find(|v| v.instance == Some(id(1))).unwrap();
    assert!(voice.active);
    assert_eq!(voice.velocity, 0.);
    assert_eq!(voice.expression.value(NoteExpressionKind::Pitch), 3.);
}

#[test]
fn mpe_sample_events_processing_and_retirement_allocate_and_free_nothing() {
    let mut sampler = sampler();
    let mut buffer = [0.; 256];
    let counted = allocation_counter::measure(|| {
        NoteInstanceDevice::event(&mut sampler, id(1), on(60)).unwrap();
        sampler.process(&ProcessContext::default(), &mut buffer);
        NoteInstanceDevice::event(&mut sampler, id(1), NoteInstanceEvent::Choke { key: 60 }).unwrap();
        sampler.drain_finished(&mut [None; 64]);
    });
    assert_eq!(counted.count_total, 0);
    assert_eq!(counted.bytes_total, 0);
}
