// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

fn input(mode: MpeMode) -> MpeInput {
    MpeInput::new(MpeConfig { mode, ..MpeConfig::default() }, 9).unwrap()
}

fn send(input: &mut MpeInput, bytes: &[u8]) -> Vec<(u8, LiveEvent)> {
    let mut events = Vec::new();
    input.process(bytes, &mut |channel, event| {
        events.push((channel, event));
        true
    });
    events
}

fn rpn(input: &mut MpeInput, channel: u8, number: u8, value: u8) -> Vec<(u8, LiveEvent)> {
    send(input, &[0xb0 | channel, 100, number]);
    send(input, &[0xb0 | channel, 101, 0]);
    send(input, &[0xb0 | channel, 6, value])
}

fn on(events: &[(u8, LiveEvent)]) -> (NoteInstanceId, NoteExpressionState) {
    match events.first().unwrap().1 {
        LiveEvent::Instance { id, event: Event::On { expression, .. } } => (id, expression),
        other => panic!("expected note on, got {other:?}"),
    }
}

#[test]
fn lower_and_upper_configuration_resolve_overlap_in_both_orders() {
    for (first, second) in [(0, 15), (15, 0)] {
        let mut input = input(MpeMode::Automatic);
        rpn(&mut input, first, 6, 15);
        assert!((0..16).all(|channel| input.zone(channel) == Some(usize::from(first != 0))));
        rpn(&mut input, second, 6, 15);
        assert!((0..16).all(|channel| input.zone(channel) == Some(usize::from(second != 0))));
        rpn(&mut input, first, 6, 6);
        assert_eq!((input.config.lower_members, input.config.upper_members), if first == 0 { (6, 8) } else { (8, 6) });
    }
}

#[test]
fn manual_configuration_ignores_rpn_and_rejects_invalid_configuration() {
    let mut input = input(MpeMode::Manual);
    rpn(&mut input, 15, 6, 15);
    rpn(&mut input, 1, 0, 12);
    assert_eq!(input.config.lower_members, 15);
    assert_eq!(input.config.lower_member_bend, 48);
    assert!(MpeInput::new(MpeConfig { lower_members: 8, upper_members: 7, ..MpeConfig::default() }, 1).is_none());
    assert!(MpeInput::new(MpeConfig::default(), 0).is_none());
}

#[test]
fn initial_controls_are_atomic_and_one_member_bend_does_not_move_another() {
    let mut input = input(MpeMode::Manual);
    send(&mut input, &[0xe1, 0, 96]);
    send(&mut input, &[0xd1, 90]);
    send(&mut input, &[0xb1, 74, 127]);
    let (a, expression) = on(&send(&mut input, &[0x91, 60, 100]));
    let (b, neutral) = on(&send(&mut input, &[0x92, 64, 100]));
    assert_eq!(expression.value(Kind::Pitch), 24.);
    assert_eq!(expression.value(Kind::Pressure), 90. / 127.);
    assert_eq!(expression.value(Kind::Timbre), 1.);
    assert_eq!(neutral, NoteExpressionState::NEUTRAL);
    let moved = send(&mut input, &[0xe1, 0, 0]);
    assert_eq!(
        moved,
        [(1, LiveEvent::Instance { id: a, event: Event::Expression { kind: Kind::Pitch, value: -48. } })]
    );
    assert_ne!(a, b);
}

#[test]
fn manager_controls_compose_and_only_manager_bend_follows_released_tails() {
    let mut input = input(MpeMode::Manual);
    send(&mut input, &[0xe1, 0, 96]);
    let (old, _) = on(&send(&mut input, &[0x91, 60, 100]));
    send(&mut input, &[0xb0, 64, 127]);
    assert!(send(&mut input, &[0x81, 60, 64]).is_empty(), "pedal defers audible off");
    let (new, expression) = on(&send(&mut input, &[0x91, 62, 100]));
    assert_eq!(expression.value(Kind::Pitch), 24., "recycled member uses its latest control latch");
    let member = send(&mut input, &[0xe1, 0, 0]);
    assert_eq!(member.len(), 1);
    assert!(matches!(member[0].1, LiveEvent::Instance { id, .. } if id == new));
    let manager = send(&mut input, &[0xe0, 0, 96]);
    assert!(
        manager.contains(&(
            1,
            LiveEvent::Instance { id: old, event: Event::Expression { kind: Kind::Pitch, value: 25. } }
        ))
    );
    assert!(
        manager.contains(&(
            1,
            LiveEvent::Instance { id: new, event: Event::Expression { kind: Kind::Pitch, value: -47. } }
        ))
    );
    let pressure = send(&mut input, &[0xd0, 100]);
    assert_eq!(pressure.len(), 1);
    assert!(matches!(pressure[0].1, LiveEvent::Instance { id, .. } if id == new));
    let released = send(&mut input, &[0xb0, 64, 0]);
    assert_eq!(released, [(1, LiveEvent::Instance { id: old, event: Event::Off { key: 60, velocity: 64. / 127. } })]);
}

#[test]
fn repeated_keys_pair_oldest_first_and_zero_velocity_is_midi_note_off() {
    let mut input = input(MpeMode::Manual);
    let (first, _) = on(&send(&mut input, &[0x91, 60, 100]));
    let (second, _) = on(&send(&mut input, &[0x91, 60, 100]));
    assert_eq!(
        send(&mut input, &[0x91, 60, 0]),
        [(1, LiveEvent::Instance { id: first, event: Event::Off { key: 60, velocity: 0. } })]
    );
    assert_eq!(
        send(&mut input, &[0x81, 60, 0]),
        [(1, LiveEvent::Instance { id: second, event: Event::Off { key: 60, velocity: 0. } })]
    );
}

#[test]
fn poly_key_pressure_targets_only_ordinary_or_manager_keys() {
    for mode in [MpeMode::Automatic, MpeMode::Manual] {
        let mut input = input(mode);
        let (manager, _) = on(&send(&mut input, &[0x90, 60, 100]));
        send(&mut input, &[0x90, 64, 100]);
        send(&mut input, &[0x91, 60, 100]);
        assert_eq!(
            send(&mut input, &[0xa0, 60, 127]),
            [(0, LiveEvent::Instance { id: manager, event: Event::Expression { kind: Kind::Pressure, value: 1. } })]
        );
        assert_eq!(send(&mut input, &[0xa1, 60, 127]).len(), usize::from(mode == MpeMode::Automatic));
    }
}

#[test]
fn rpn_null_nrpn_and_invalid_data_do_not_reconfigure() {
    let mut input = input(MpeMode::Automatic);
    rpn(&mut input, 0, 6, 4);
    let saved = input.config();
    for selection in [[101, 127], [100, 127], [99, 0], [98, 0]] {
        send(&mut input, &[0xb0, 101, 0]);
        send(&mut input, &[0xb0, 100, 6]);
        send(&mut input, &[0xb0, selection[0], selection[1]]);
        send(&mut input, &[0xb0, 6, 10]);
        assert_eq!(input.config(), saved);
    }
    rpn(&mut input, 0, 6, 16);
    rpn(&mut input, 1, 6, 15);
    assert_eq!(input.config(), saved);
    rpn(&mut input, 1, 0, 12);
    assert_eq!(input.config().lower_member_bend, 12);
    rpn(&mut input, 1, 0, 97);
    assert_eq!(input.config().lower_member_bend, 12);
}

#[test]
fn all_notes_off_respects_sustain_but_all_sound_off_chokes_tails() {
    let mut input = input(MpeMode::Manual);
    let (id, _) = on(&send(&mut input, &[0x91, 60, 100]));
    send(&mut input, &[0xb0, 64, 127]);
    assert!(send(&mut input, &[0xb0, 123, 0]).is_empty());
    assert_eq!(send(&mut input, &[0xb0, 120, 0]), [(1, LiveEvent::Instance { id, event: Event::Choke { key: 60 } })]);
    assert!(send(&mut input, &[0xb0, 64, 0]).is_empty());
}

#[test]
fn fixed_capacity_rejects_held_notes_and_reclaims_only_released_occurrences() {
    let mut input = input(MpeMode::Manual);
    let (first, _) = on(&send(&mut input, &[0x91, 60, 100]));
    for _ in 1..MAX_OCCURRENCES {
        assert_eq!(send(&mut input, &[0x91, 60, 100]).len(), 1);
    }
    assert!(send(&mut input, &[0x92, 64, 100]).is_empty());
    // The refused press's own release ends nothing and frees the key for the next press.
    assert!(send(&mut input, &[0x82, 64, 0]).is_empty());
    send(&mut input, &[0x81, 60, 0]);
    let replaced = send(&mut input, &[0x92, 64, 100]);
    assert_eq!(replaced[0], (1, LiveEvent::Instance { id: first, event: Event::Choke { key: 60 } }));
    let (new, _) = on(&replaced[1..]);
    assert_ne!(new, first);
}

#[test]
fn rejected_queue_admission_does_not_leave_a_held_protocol_occurrence() {
    let mut input = input(MpeMode::Manual);
    input.process(&[0x91, 60, 100], &mut |_, _| false);
    assert!(input.notes.iter().all(Option::is_none));
    assert!(send(&mut input, &[0x81, 60, 0]).is_empty());
}

#[test]
fn malformed_bytes_are_ignored_without_changing_protocol_state() {
    let mut input = input(MpeMode::Automatic);
    for message in [
        &[][..],
        &[0x90],
        &[0x90, 60],
        &[0x90, 128, 20],
        &[0x90, 60, 128],
        &[0xd0, 128],
        &[0xb0, 6, 200],
        &[0x40, 60, 100],
        &[0xf0, 0, 0],
        &[0xff, 0, 0],
    ] {
        assert!(send(&mut input, message).is_empty(), "{message:?}");
    }
    assert!(input.notes.iter().all(Option::is_none));
    assert_eq!(input.config.lower_members, 0);
}

#[test]
fn off_mode_preserves_ordinary_note_and_control_routing() {
    let mut input = input(MpeMode::Off);
    for message in [
        [0x91, 60, 100],
        [0x91, 60, 100],
        [0x91, 60, 0],
        [0x81, 60, 20],
        [0xb1, 64, 127],
        [0xb1, 64, 0],
        [0xb1, 101, 0],
        [0xb1, 100, 6],
        [0xb1, 6, 15],
        [0xe1, 0, 96],
        [0xb1, 74, 100],
        [0xb1, 123, 0],
    ] {
        assert_eq!(send(&mut input, &message), crate::parse(&message).into_iter().collect::<Vec<_>>());
    }
    assert!(send(&mut input, &[0xa1, 60, 127]).is_empty());
    assert!(input.notes.iter().all(Option::is_none));
    assert_eq!(input.config.lower_members, 0);
}

#[test]
fn reused_member_with_fresh_neutral_controls_does_not_inherit_the_released_voice() {
    let mut input = input(MpeMode::Manual);
    send(&mut input, &[0xe1, 0, 96]);
    send(&mut input, &[0xd1, 127]);
    send(&mut input, &[0xb1, 74, 127]);
    let (old, _) = on(&send(&mut input, &[0x91, 60, 100]));
    send(&mut input, &[0x81, 60, 0]);
    for message in [&[0xe1, 0, 64][..], &[0xd1, 0], &[0xb1, 74, 64]] {
        assert!(send(&mut input, message).is_empty(), "released tail is detached");
    }
    let (new, expression) = on(&send(&mut input, &[0x91, 62, 100]));
    assert_ne!(old, new);
    assert_eq!(expression, NoteExpressionState::NEUTRAL);
}

#[test]
fn independent_ports_and_two_zones_keep_pedal_expression_and_resets_separate() {
    let config = MpeConfig { mode: MpeMode::Manual, lower_members: 4, upper_members: 4, ..MpeConfig::default() };
    let mut a = MpeInput::new(config, 1).unwrap();
    let mut b = MpeInput::new(config, 2).unwrap();
    let (lower, _) = on(&send(&mut a, &[0x91, 60, 100]));
    let (upper, _) = on(&send(&mut a, &[0x9e, 60, 100]));
    let (other, _) = on(&send(&mut b, &[0x91, 60, 100]));
    assert_ne!(lower, other);
    send(&mut a, &[0xb0, 64, 127]);
    assert!(send(&mut a, &[0x81, 60, 0]).is_empty());
    assert_eq!(
        send(&mut a, &[0x8e, 60, 0]),
        [(14, LiveEvent::Instance { id: upper, event: Event::Off { key: 60, velocity: 0. } })]
    );
    assert_eq!(
        send(&mut b, &[0x81, 60, 0]),
        [(1, LiveEvent::Instance { id: other, event: Event::Off { key: 60, velocity: 0. } })]
    );
    assert_eq!(
        send(&mut a, &[0xb0, 121, 0]),
        [
            (1, LiveEvent::Instance { id: lower, event: Event::Off { key: 60, velocity: 0. } }),
            (1, LiveEvent::Instance { id: lower, event: Event::Expression { kind: Kind::Pitch, value: 0. } })
        ]
    );
    assert_eq!(
        send(&mut a, &[0xbf, 120, 0]),
        [(14, LiveEvent::Instance { id: upper, event: Event::Choke { key: 60 } })]
    );
    assert!(b.notes.iter().flatten().any(|note| note.id == other));
}

#[test]
fn a_refused_press_never_releases_a_later_note_on_its_key() {
    let mut input = input(MpeMode::Manual);
    // Fill the table with held notes, A first on channel 2/key 60.
    let (a, _) = on(&send(&mut input, &[0x91, 60, 100]));
    for key in 0..(MAX_OCCURRENCES - 1) {
        assert_eq!(send(&mut input, &[0x93, (key % 128) as u8, 100]).len(), 1);
    }
    // B, a second press on A's key, is refused: the table holds only held notes.
    assert!(send(&mut input, &[0x91, 60, 100]).is_empty());
    // A's release pairs with A, the oldest press.
    let released = send(&mut input, &[0x81, 60, 0]);
    assert_eq!(released, [(1, LiveEvent::Instance { id: a, event: Event::Off { key: 60, velocity: 0. } })]);
    // C on the same key waits until B's release has come, so B's release cannot end C.
    assert!(send(&mut input, &[0x91, 60, 100]).is_empty(), "a press is refused while one is owed");
    assert!(send(&mut input, &[0x81, 60, 0]).is_empty(), "B's release ends nothing");
    assert!(send(&mut input, &[0x81, 60, 0]).is_empty(), "the refused C's release ends nothing");
    let replaced = send(&mut input, &[0x91, 60, 100]);
    let (c, _) = on(&replaced[1..]);
    assert_eq!(replaced[0], (1, LiveEvent::Instance { id: a, event: Event::Choke { key: 60 } }));
    let held = send(&mut input, &[0xB1, 74, 100]);
    println!("after the owed releases: {held:?}");
    assert!(held.iter().any(|(_, e)| matches!(e, LiveEvent::Instance { id, .. } if *id == c)), "C is held");
}

fn pitch_values(events: &[(u8, LiveEvent)]) -> Vec<f64> {
    events
        .iter()
        .filter_map(|(_, event)| match event {
            LiveEvent::Instance { event: Event::Expression { kind: Kind::Pitch, value }, .. } => Some(*value),
            _ => None,
        })
        .collect()
}

#[test]
fn automatic_mode_uses_the_default_ranges_whatever_manual_mode_saved() {
    let saved =
        MpeConfig { mode: MpeMode::Automatic, lower_member_bend: 24, lower_manager_bend: 7, ..MpeConfig::default() };
    let mut input = MpeInput::new(saved, 9).unwrap();
    rpn(&mut input, 0, 6, 15);
    assert_eq!((input.config().lower_member_bend, input.config().lower_manager_bend), (48, 2));
    on(&send(&mut input, &[0x91, 60, 100]));
    let bent = pitch_values(&send(&mut input, &[0xe1, 0, 96]));
    println!("half-scale member bend: {bent:?}");
    assert_eq!(bent, [24.]);
}

#[test]
fn a_configuration_message_restores_its_zones_default_ranges_and_retunes_held_notes() {
    let mut input = input(MpeMode::Automatic);
    rpn(&mut input, 0, 6, 12);
    rpn(&mut input, 15, 6, 2);
    rpn(&mut input, 14, 0, 7);
    rpn(&mut input, 1, 0, 12);
    on(&send(&mut input, &[0x91, 60, 100]));
    assert_eq!(pitch_values(&send(&mut input, &[0xe1, 0, 96])), [6.]);
    // The same lower-zone configuration again: the held note stays, at the default range.
    let retuned = rpn(&mut input, 0, 6, 12);
    println!("after RPN 6: {retuned:?}");
    assert!(!retuned.iter().any(|(_, e)| matches!(e, LiveEvent::Instance { event: Event::Choke { .. }, .. })));
    assert_eq!(pitch_values(&retuned), [24.]);
    assert_eq!(input.config().lower_member_bend, 48);
    assert_eq!(input.config().upper_member_bend, 7, "the other zone keeps its range");
}
