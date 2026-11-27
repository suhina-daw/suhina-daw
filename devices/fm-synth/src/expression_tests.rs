// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

fn id(serial: u64) -> NoteInstanceId {
    NoteInstanceId::from_parts(17, serial).unwrap()
}
fn on(key: u8) -> NoteInstanceEvent {
    NoteInstanceEvent::On { key, velocity: 0.7, expression: NoteExpressionState::NEUTRAL }
}

fn render(notes: &[u64], bend: bool, block: usize) -> Vec<f32> {
    let mut synth = FmSynth::new();
    for &serial in notes {
        NoteInstanceDevice::event(&mut synth, id(serial), on(if serial == 1 { 60 } else { 67 })).unwrap();
    }
    let mut output = Vec::new();
    for (length, change) in [(211usize, false), (813, true)] {
        if change && bend && notes.contains(&1) {
            NoteInstanceDevice::event(
                &mut synth,
                id(1),
                NoteInstanceEvent::Expression { kind: NoteExpressionKind::Pitch, value: 12. },
            )
            .unwrap();
        }
        let mut remaining = length;
        while remaining > 0 {
            let frames = remaining.min(block);
            let mut buffer = vec![0.; frames * 2];
            synth.process(&ProcessContext::default(), &mut buffer);
            output.extend(buffer);
            remaining -= frames;
        }
    }
    output
}

#[test]
fn mpe_one_bent_note_leaves_the_other_voice_unchanged_and_is_partition_invariant() {
    let mixed = render(&[1, 2], true, 17);
    let first = render(&[1], true, 64);
    let second = render(&[2], false, 512);
    assert!(mixed.iter().zip(first.iter().zip(&second)).all(|(sum, (a, b))| (*sum - (*a + *b)).abs() < 2e-5));
    assert!(mixed.iter().zip(render(&[1, 2], false, 17)).any(|(a, b)| (*a - b).abs() > 1e-4));
    for block in [1, 64, 512] {
        assert_eq!(mixed, render(&[1, 2], true, block));
    }
}

#[test]
fn mpe_same_key_occurrences_release_and_retire_independently() {
    let mut synth = FmSynth::new();
    for serial in [1, 2] {
        NoteInstanceDevice::event(&mut synth, id(serial), on(60)).unwrap();
    }
    NoteInstanceDevice::event(&mut synth, id(1), NoteInstanceEvent::Off { key: 60, velocity: 0. }).unwrap();
    assert!(!synth.slots.iter().find(|s| s.instance == Some(id(1))).unwrap().down);
    assert!(synth.slots.iter().find(|s| s.instance == Some(id(2))).unwrap().down);
    assert_eq!(synth.drain_finished(&mut [None; 4]), 0);
    NoteInstanceDevice::event(&mut synth, id(1), NoteInstanceEvent::Choke { key: 60 }).unwrap();
    let mut finished = [None; 4];
    assert_eq!(synth.drain_finished(&mut finished), 1);
    assert_eq!(finished[0], Some(id(1)));
    assert!(synth.slots.iter().any(|s| s.instance == Some(id(2)) && s.live));
}

#[test]
fn mpe_native_zero_velocity_remains_an_on_and_a_second_note_takes_the_only_voice() {
    let mut synth = FmSynth::new();
    synth.set_param(params::VOICE_COUNT, 1.);
    let event =
        NoteInstanceEvent::On { key: 60, velocity: 0., expression: NoteExpressionState::new(12., 0.4, 0.8).unwrap() };
    NoteInstanceDevice::event(&mut synth, id(1), event).unwrap();
    assert!(synth.slots[0].down);
    assert_eq!(synth.slots[0].expression.value(NoteExpressionKind::Pitch), 12.);
    // One voice: the second note takes it with its own expression, and the first occurrence
    // keeps its identity as the voice's retiring tail until its completion is reported.
    NoteInstanceDevice::event(&mut synth, id(2), on(67)).unwrap();
    assert_eq!(synth.slots[0].instance, Some(id(2)));
    assert_eq!(synth.slots[0].retiring_instance, Some(id(1)));
    assert_eq!(synth.slots[0].expression.value(NoteExpressionKind::Pitch), 0.);
    assert!(synth.lifetimes.contains(id(1)) && synth.lifetimes.contains(id(2)));
}

#[test]
fn mpe_pressure_is_audible_without_muting_neutral_notes() {
    let mut plain = FmSynth::new();
    let mut pressed = FmSynth::new();
    NoteInstanceDevice::event(&mut plain, id(1), on(60)).unwrap();
    NoteInstanceDevice::event(&mut pressed, id(1), on(60)).unwrap();
    NoteInstanceDevice::event(
        &mut pressed,
        id(1),
        NoteInstanceEvent::Expression { kind: NoteExpressionKind::Pressure, value: 1. },
    )
    .unwrap();
    let mut a = vec![0.; 2048];
    let mut b = vec![0.; 2048];
    plain.process(&ProcessContext::default(), &mut a);
    pressed.process(&ProcessContext::default(), &mut b);
    let energy = |values: &[f32]| values.iter().map(|s| f64::from(*s).powi(2)).sum::<f64>();
    assert!(energy(&a) > 1e-6);
    assert!(energy(&b) > energy(&a) * 1.2);
}

#[test]
fn mpe_a_full_voice_pool_takes_the_oldest_voice_and_still_retires_every_id() {
    let mut synth = FmSynth::new();
    synth.set_param(params::VOICE_COUNT, 2.);
    for serial in 1..=2 {
        NoteInstanceDevice::event(&mut synth, id(serial), on(60 + serial as u8)).unwrap();
    }
    // Both voices are held: the third note takes the one that started first, as an ordinary
    // note does, instead of being refused.
    NoteInstanceDevice::event(&mut synth, id(3), on(67)).unwrap();
    let instances: Vec<_> = synth.slots.iter().take(2).map(|s| s.instance).collect();
    println!("instances={instances:?}");
    assert_eq!(instances, [Some(id(3)), Some(id(2))]);
    let mut buffer = vec![0.; 256];
    synth.process(&ProcessContext::default(), &mut buffer);
    assert!(buffer.iter().any(|s| s.abs() > 1e-4), "the new note sounds");
    let mut finished = [None; 8];
    let first = NoteInstanceDevice::drain_finished(&mut synth, &mut finished);
    assert_eq!(&finished[..first], &[Some(id(1))], "the taken occurrence retires once its quantum played");
    for serial in [2, 3] {
        NoteInstanceDevice::event(&mut synth, id(serial), NoteInstanceEvent::Choke { key: 60 }).unwrap();
    }
    synth.process(&ProcessContext::default(), &mut buffer);
    let rest = NoteInstanceDevice::drain_finished(&mut synth, &mut finished);
    let mut rest: Vec<_> = finished[..rest].to_vec();
    rest.sort_by_key(|id| id.map(|id| id.serial()));
    assert_eq!(rest, [Some(id(2)), Some(id(3))]);
}
