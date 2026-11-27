// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::chain_tests::{SR, blocks, device, project};
use crate::{AudioPool, LaunchCommand, LiveEvent, LiveSource, Renderer};
use suhina_device_api::{NoteExpressionKind, NoteExpressionState, NoteInstanceEvent, NoteInstanceId};
use suhina_model::TrackId;

fn id(serial: u64) -> NoteInstanceId {
    NoteInstanceId::from_parts(99, serial).unwrap()
}
fn instance(serial: u64, event: NoteInstanceEvent) -> LiveEvent {
    LiveEvent::Instance { id: id(serial), event }
}
fn on(key: u8) -> NoteInstanceEvent {
    NoteInstanceEvent::On { key, velocity: 0.7, expression: NoteExpressionState::NEUTRAL }
}
fn setup() -> (Renderer, crate::Controller, crate::LiveSender) {
    let project = project(Vec::new(), 1., vec![device("fm-synth", &[])]);
    let mut renderer = Renderer::stopped(&project, &AudioPool::new(), SR, suhina_device_registry::factory()).unwrap();
    let mut controller = renderer.controller();
    let live = controller.take_live_input().unwrap();
    controller.send(LaunchCommand::SetArmed { track: TrackId(1), armed: true, input: Default::default() }).unwrap();
    (renderer, controller, live)
}

fn render(notes: &[u64], bend: bool, block: usize) -> Vec<f32> {
    let (mut renderer, _controller, mut live) = setup();
    for &serial in notes {
        assert!(live.send_from(
            LiveSource { port: 1, channel: serial as u8 },
            instance(serial, on(if serial == 1 { 60 } else { 67 }))
        ));
    }
    let mut result = Vec::new();
    for (length, change) in [(211usize, false), (813, true)] {
        if change && bend && notes.contains(&1) {
            assert!(live.send_from(
                LiveSource { port: 1, channel: 1 },
                instance(1, NoteInstanceEvent::Expression { kind: NoteExpressionKind::Pitch, value: 12. })
            ));
        }
        let mut remaining = length;
        while remaining > 0 {
            let frames = remaining.min(block);
            let mut buffer = vec![0.; frames * 2];
            renderer.process(&mut buffer);
            result.extend(buffer);
            remaining -= frames;
        }
    }
    result
}

#[test]
fn live_mpe_two_notes_only_one_bends_golden_and_partition_invariant() {
    let mixed = render(&[1, 2], true, 17);
    let first = render(&[1], true, 64);
    let second = render(&[2], false, 512);
    assert!(mixed.iter().zip(first.iter().zip(&second)).all(|(sum, (a, b))| (*sum - (*a + *b)).abs() < 2e-5));
    assert!(mixed.iter().zip(render(&[1, 2], false, 17)).any(|(a, b)| (*a - b).abs() > 1e-4));
    for block in [1, 64, 512] {
        assert_eq!(mixed, render(&[1, 2], true, block));
    }
    suhina_test_support::assert_golden_f32(
        std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden/live-mpe-two-notes.f32")),
        &mixed,
        2e-6,
    );
}

#[test]
fn live_mpe_disarming_chokes_only_the_occurrences_the_track_received() {
    let (mut renderer, mut controller, mut live) = setup();
    assert!(live.send(instance(1, on(60))));
    assert!(blocks(&mut renderer, 4, 64).iter().any(|v| v.abs() > 1e-4));
    controller.send(LaunchCommand::SetArmed { track: TrackId(1), armed: false, input: Default::default() }).unwrap();
    blocks(&mut renderer, 2, 64);
    assert!(blocks(&mut renderer, 2, 64).iter().all(|v| v.abs() < 1e-7));
    controller.send(LaunchCommand::SetArmed { track: TrackId(1), armed: true, input: Default::default() }).unwrap();
    assert!(live.send(instance(2, on(60))));
    assert!(live.send(instance(1, NoteInstanceEvent::Off { key: 60, velocity: 0. })));
    assert!(blocks(&mut renderer, 4, 64).iter().any(|v| v.abs() > 1e-4));
}

#[test]
fn live_mpe_queue_overflow_has_an_out_of_band_termination_path() {
    let (mut renderer, _controller, mut live) = setup();
    assert!(live.send(instance(1, on(60))));
    // The note sounds before the overflow, so the silence after it is the recovery's doing.
    assert!(blocks(&mut renderer, 4, 64).iter().any(|v| v.abs() > 1e-4));
    for _ in 0..crate::LIVE_QUEUE {
        assert!(live.send(instance(1, NoteInstanceEvent::Expression { kind: NoteExpressionKind::Pitch, value: 0. })));
    }
    assert!(!live.send(instance(1, NoteInstanceEvent::Off { key: 60, velocity: 0. })));
    blocks(&mut renderer, 2, 64);
    assert!(blocks(&mut renderer, 2, 64).iter().all(|v| v.abs() < 1e-7));
    assert!(live.send(instance(2, on(67))));
    assert!(blocks(&mut renderer, 4, 64).iter().any(|v| v.abs() > 1e-4));
}

#[test]
fn live_mpe_expression_flood_cannot_overtake_a_stamped_on_or_lose_its_off() {
    let (mut renderer, _controller, mut live) = setup();
    let start = 1_000_000;
    renderer.set_live_clock(start, 256);
    assert!(live.send_at(Default::default(), instance(1, on(60)), start));
    for value in 0..600 {
        assert!(live.send_at(
            Default::default(),
            instance(
                1,
                NoteInstanceEvent::Expression { kind: NoteExpressionKind::Pressure, value: f64::from(value) / 600. }
            ),
            start
        ));
    }
    assert!(live.send_at(Default::default(), instance(1, NoteInstanceEvent::Off { key: 60, velocity: 0. }), start));
    blocks(&mut renderer, 10, 64);
    let played = &renderer.live.played;
    assert!(matches!(played.first(), Some((256, LiveEvent::Instance { event: NoteInstanceEvent::On { .. }, .. }))));
    assert!(matches!(played.last(), Some((256, LiveEvent::Instance { event: NoteInstanceEvent::Off { .. }, .. }))));
    assert!(played.iter().all(|(at, _)| *at == 256));
    assert!(played.iter().any(|(_, event)| matches!(event,
        LiveEvent::Instance { event: NoteInstanceEvent::Expression { value, .. }, .. } if *value == 599. / 600.)));
}

#[test]
fn live_mpe_dispatch_and_fm_processing_allocate_and_free_nothing() {
    let (mut renderer, _controller, mut live) = setup();
    let mut buffer = [0.; 128];
    renderer.process(&mut buffer);
    // Test-only event evidence is preallocated before measuring the production path.
    renderer.live.played.reserve(8);
    let allocation = allocation_counter::measure(|| {
        live.send(instance(1, on(60)));
        live.send(instance(2, on(67)));
        renderer.process(&mut buffer);
        live.send(instance(1, NoteInstanceEvent::Expression { kind: NoteExpressionKind::Pitch, value: 12. }));
        renderer.process(&mut buffer);
        live.send(instance(1, NoteInstanceEvent::Choke { key: 60 }));
        live.send(instance(2, NoteInstanceEvent::Off { key: 67, velocity: 0. }));
        renderer.process(&mut buffer);
    });
    assert_eq!(allocation.count_total, 0);
    assert_eq!(allocation.count_current, 0);
}

/// A chain whose instrument has no identified-note capability (a MIDI effect in front of the
/// track's implicit tones), so it plays identified input through ordinary-note compatibility.
fn compatibility_setup() -> (Renderer, crate::Controller, crate::LiveSender) {
    let project = project(Vec::new(), 1., vec![device("pitch", &[])]);
    let mut renderer = Renderer::stopped(&project, &AudioPool::new(), SR, suhina_device_registry::factory()).unwrap();
    let mut controller = renderer.controller();
    let live = controller.take_live_input().unwrap();
    controller.send(LaunchCommand::SetArmed { track: TrackId(1), armed: true, input: Default::default() }).unwrap();
    (renderer, controller, live)
}

fn peak(out: &[f32]) -> f32 {
    out.iter().fold(0.0, |m, s| m.max(s.abs()))
}

#[test]
fn live_mpe_reclaiming_a_released_occurrence_keeps_a_newer_ordinary_note_on_that_key() {
    // The MIDI input chokes its oldest released occurrence before reusing its slot. On an
    // ordinary-compatibility chain that occurrence's off was already sent; the choke must not
    // become a second key-wide off that ends the note now holding the same key.
    for (control, choke_released) in [(false, true), (true, false)] {
        let (mut renderer, _controller, mut live) = compatibility_setup();
        assert!(live.send(instance(1, on(60))));
        blocks(&mut renderer, 2, 256);
        assert!(live.send(instance(1, NoteInstanceEvent::Off { key: 60, velocity: 0. })));
        blocks(&mut renderer, 2, 256);
        assert!(live.send(instance(2, on(60))));
        let held = peak(&blocks(&mut renderer, 4, 256));
        if choke_released {
            assert!(live.send(instance(1, NoteInstanceEvent::Choke { key: 60 })));
        }
        if control {
            // Control: the held note's own off still reaches the compatibility instrument.
            assert!(live.send(instance(2, NoteInstanceEvent::Off { key: 60, velocity: 0. })));
        }
        blocks(&mut renderer, 40, 256);
        let after = peak(&blocks(&mut renderer, 4, 256));
        println!("control={control} held={held} after={after}");
        assert!(held > 1e-3, "the second note sounds before the choke");
        if control {
            assert!(after < 1e-6, "its own off releases the held note");
        } else {
            assert!(after > held * 0.5, "a stale choke left the held note sounding");
        }
    }
}

#[test]
fn live_mpe_records_one_release_per_occurrence() {
    let (mut renderer, mut controller, mut live) = setup();
    let mut recorder = controller.take_recorder().unwrap();
    controller.send(LaunchCommand::Play).unwrap();
    controller.send(LaunchCommand::SetRecording { on: true }).unwrap();
    blocks(&mut renderer, 2, 256);
    assert!(live.send(instance(1, on(60))));
    assert!(live.send(instance(1, NoteInstanceEvent::Off { key: 60, velocity: 0. })));
    assert!(live.send(instance(2, on(60))));
    // Reclaiming the released occurrence records nothing; a choke of a held one is its release.
    assert!(live.send(instance(1, NoteInstanceEvent::Choke { key: 60 })));
    assert!(live.send(instance(3, on(64))));
    assert!(live.send(instance(3, NoteInstanceEvent::Choke { key: 64 })));
    blocks(&mut renderer, 4, 256);
    let mut recorded = Vec::new();
    while let Some(note) = recorder.next_note() {
        recorded.push((note.key, note.velocity.is_some()));
    }
    println!("recorded={recorded:?}");
    assert_eq!(recorded, [(60, true), (60, false), (60, true), (64, true), (64, false)]);
}

#[test]
fn live_mpe_a_replaced_queue_does_not_inherit_the_old_queues_overflow() {
    let (mut renderer, _controller, mut live) = setup();
    for _ in 0..crate::LIVE_QUEUE {
        assert!(live.send(instance(1, NoteInstanceEvent::Expression { kind: NoteExpressionKind::Pitch, value: 0. })));
    }
    assert!(!live.send(instance(1, on(60))), "the old queue overflows");
    // A new controller replaces the queue before the renderer saw the overflow.
    let mut controller = renderer.controller();
    let mut fresh = controller.take_live_input().unwrap();
    assert!(fresh.send(instance(2, on(67))));
    blocks(&mut renderer, 2, 64);
    let played: Vec<_> = renderer.live.played.iter().map(|(_, e)| *e).collect();
    println!("played={played:?}");
    assert!(played.contains(&instance(2, on(67))), "the new queue's note is played, not discarded");
}

#[test]
fn live_mpe_overflow_recovery_closes_the_notes_it_ends_in_the_take() {
    let (mut renderer, mut controller, mut live) = setup();
    let mut recorder = controller.take_recorder().unwrap();
    controller.send(LaunchCommand::Play).unwrap();
    controller.send(LaunchCommand::SetRecording { on: true }).unwrap();
    blocks(&mut renderer, 2, 256);
    assert!(live.send(instance(1, on(60))));
    assert!(live.send(instance(2, on(64))));
    assert!(live.send(instance(2, NoteInstanceEvent::Off { key: 64, velocity: 0. })));
    blocks(&mut renderer, 2, 256);
    for _ in 0..crate::LIVE_QUEUE {
        assert!(live.send(instance(1, NoteInstanceEvent::Expression { kind: NoteExpressionKind::Pitch, value: 0. })));
    }
    assert!(!live.send(instance(1, NoteInstanceEvent::Off { key: 60, velocity: 0. })), "the off is lost");
    blocks(&mut renderer, 2, 256);
    let mut recorded = Vec::new();
    while let Some(note) = recorder.next_note() {
        recorded.push((note.key, note.velocity.is_some()));
    }
    println!("recorded={recorded:?}");
    // The held note is closed by the recovery; the note already let go is not closed twice.
    assert_eq!(recorded, [(60, true), (64, true), (64, false), (60, false)]);
}

#[test]
fn live_mpe_an_expression_with_no_room_on_a_full_timeline_is_dropped_not_a_panic() {
    let (mut renderer, _controller, mut live) = setup();
    let start = 1_000_000;
    renderer.set_live_clock(start, 256);
    // 256 stamped note events fill the waiting timeline; none of them is a control.
    for serial in 1..=128 {
        assert!(live.send_at(Default::default(), instance(serial, on(40 + (serial % 40) as u8)), start));
        assert!(live.send_at(
            Default::default(),
            instance(serial, NoteInstanceEvent::Off { key: 40 + (serial % 40) as u8, velocity: 0. }),
            start
        ));
    }
    assert!(live.send_at(
        Default::default(),
        instance(1, NoteInstanceEvent::Expression { kind: NoteExpressionKind::Pressure, value: 0.5 }),
        start
    ));
    blocks(&mut renderer, 10, 64);
    let played = &renderer.live.played;
    let ons = played
        .iter()
        .filter(|(_, e)| matches!(e, LiveEvent::Instance { event: NoteInstanceEvent::On { .. }, .. }))
        .count();
    let offs = played
        .iter()
        .filter(|(_, e)| matches!(e, LiveEvent::Instance { event: NoteInstanceEvent::Off { .. }, .. }))
        .count();
    println!("ons={ons} offs={offs} played={}", played.len());
    assert_eq!((ons, offs), (128, 128), "every waiting note still plays at its frame");
    assert!(
        !played
            .iter()
            .any(|(_, e)| matches!(e, LiveEvent::Instance { event: NoteInstanceEvent::Expression { .. }, .. }))
    );
}

#[test]
fn live_mpe_a_held_note_ends_when_an_edit_changes_its_route_and_plays_on_when_it_does_not() {
    // Inserting Pitch before the instrument changes the route the note's on took: the new
    // Pitch never saw it and would drop its off, so the edit ends it. A utility after the
    // instrument leaves the route alone: the note plays on and its own off releases it.
    for (route_changes, at) in [(true, 0), (false, 1)] {
        let mut p = project(Vec::new(), 1., vec![device("fm-synth", &[])]);
        p.assign_ids().unwrap();
        let mut edited = p.clone();
        edited.channels[0].devices.insert(at, device(if route_changes { "pitch" } else { "utility" }, &[]));
        edited.assign_ids().unwrap();
        let mut renderer = Renderer::stopped(&p, &AudioPool::new(), SR, suhina_device_registry::factory()).unwrap();
        let mut controller = renderer.controller();
        let mut live = controller.take_live_input().unwrap();
        controller.send(LaunchCommand::SetArmed { track: TrackId(1), armed: true, input: Default::default() }).unwrap();
        assert!(live.send(instance(1, on(60))));
        assert!(peak(&blocks(&mut renderer, 4, 256)) > 1e-3, "the note sounds before the edit");
        controller.update(&edited).unwrap();
        let after_edit = peak(&blocks(&mut renderer, 40, 256));
        assert!(live.send(instance(1, NoteInstanceEvent::Off { key: 60, velocity: 0. })));
        blocks(&mut renderer, 200, 256);
        let after_off = peak(&blocks(&mut renderer, 4, 256));
        println!("route_changes={route_changes} after_edit={after_edit} after_off={after_off}");
        if !route_changes {
            assert!(after_edit > 1e-3, "an edit that keeps the route keeps the note playing");
        }
        assert!(after_off < 1e-6, "the note is silent after its key went up");
    }
}
